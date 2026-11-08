//! Two-process synthetic H.264 media stream over an encrypted TCP record.
//!
//! One process encodes a synthetic `videotestsrc` and sends each access unit
//! as a ChaCha20-Poly1305 record; a second process decrypts and decodes the
//! access units. This is a localhost transport diagnostic with a synthetic
//! source. It does **not** capture desktop pixels, use the portal, or send
//! media to iOS.

use crate::{
    media::{H264Encoder, decoder_candidates, probe_encoders},
    transport::{Direction, MAX_RECORD_BYTES, RecordCipher},
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
pub const DEFAULT_FRAMES: usize = 12;
const WAIT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize)]
pub struct MediaOffer {
    pub protocol: String,
    pub address: String,
    pub key_hex: String,
    pub session_id: String,
    pub frames: usize,
}

#[derive(Debug, Serialize)]
pub struct ProducerStats {
    pub frames_sent: usize,
    pub bytes_sent: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct Latency {
    pub samples: usize,
    pub min_ms: f64,
    pub median_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct ConsumerStats {
    pub frames_received: usize,
    pub decoded_frames: usize,
    pub decoded_bytes: usize,
    pub latency: Latency,
    pub elapsed_ms: u128,
    pub verified: bool,
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut value = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| "secure randomness unavailable".to_string())?;
    Ok(hex::encode(value))
}

fn key_bytes(hex_value: &str) -> Result<[u8; 32], String> {
    let decoded = hex::decode(hex_value).map_err(|_| "key is not hex".to_string())?;
    decoded
        .try_into()
        .map_err(|_| "key must be 32 bytes".to_string())
}

fn build(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|error| format!("cannot construct pipeline: {error}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned an unexpected element".to_string())
}

fn producer_description(encoder: H264Encoder, frames: usize) -> String {
    format!(
        "videotestsrc is-live=true num-buffers={frames} ! \
         video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT},framerate=30/1 ! \
         videoconvert ! {} {} ! h264parse config-interval=-1 ! \
         video/x-h264,stream-format=byte-stream,alignment=au ! \
         appsink name=encoded_sink sync=false max-buffers=2 drop=false",
        encoder.element_name(),
        encoder.settings()
    )
}

fn decoder_description(decoder: &str) -> String {
    format!(
        "appsrc name=h264_source is-live=false format=time \
         caps=video/x-h264,stream-format=byte-stream,alignment=au ! \
         h264parse ! {decoder} ! videoconvert ! video/x-raw,format=RGBA ! \
         appsink name=decoded_sink sync=false max-buffers=2 drop=false"
    )
}

fn write_record(
    stream: &mut TcpStream,
    cipher: &mut RecordCipher,
    pts: u64,
    keyframe: bool,
    bytes: &[u8],
) -> Result<usize, String> {
    let mut plaintext = Vec::with_capacity(9 + bytes.len());
    plaintext.extend_from_slice(&pts.to_be_bytes());
    plaintext.push(u8::from(keyframe));
    plaintext.extend_from_slice(bytes);
    let sealed = cipher
        .seal(&plaintext)
        .map_err(|error| format!("media seal failed: {error}"))?;
    let length = u32::try_from(sealed.len()).map_err(|_| "record too large".to_string())?;
    stream
        .write_all(&length.to_be_bytes())
        .map_err(|error| format!("media write failed: {error}"))?;
    stream
        .write_all(&sealed)
        .map_err(|error| format!("media write failed: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("media flush failed: {error}"))?;
    Ok(4 + sealed.len())
}

fn send_frames(
    stream: &mut TcpStream,
    key: &[u8; 32],
    session_id: &str,
    frames: usize,
) -> Result<ProducerStats, String> {
    let capabilities = probe_encoders()?;
    if !capabilities.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoder = capabilities.selected.ok_or("no usable H.264 encoder")?;
    let pipeline = build(&producer_description(encoder, frames))?;
    let started = Instant::now();
    let result = (|| {
        let sink = pipeline
            .by_name("encoded_sink")
            .ok_or("missing encoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "encoded_sink is not an AppSink")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start producer: {error}"))?;
        let mut cipher = RecordCipher::new(key, session_id, Direction::HostToClient)
            .map_err(|error| format!("media cipher: {error}"))?;
        let deadline = started + WAIT;
        let mut sent = 0;
        let mut bytes_sent = 0;
        while sent < frames {
            let remaining = gst::ClockTime::from_nseconds(
                u64::try_from(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_nanos(),
                )
                .unwrap_or(u64::MAX),
            );
            let Some(sample) = sink.try_pull_sample(remaining) else {
                break;
            };
            let caps = sample.caps().ok_or("encoded sample missing caps")?;
            if caps.structure(0).map(|structure| structure.name().as_str()) != Some("video/x-h264")
            {
                return Err("encoded sample is not H.264".into());
            }
            let buffer = sample.buffer().ok_or("encoded sample missing buffer")?;
            if buffer.size() == 0 || buffer.size() > 16 * 1024 * 1024 {
                return Err("invalid encoded sample size".into());
            }
            let data = buffer
                .map_readable()
                .map_err(|_| "cannot map encoded buffer")?;
            let pts = buffer.pts().map(|pts| pts.nseconds()).unwrap_or(0);
            let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
            bytes_sent += write_record(stream, &mut cipher, pts, keyframe, data.as_slice())?;
            sent += 1;
        }
        if sent == 0 {
            return Err("producer emitted no access units".into());
        }
        Ok(ProducerStats {
            frames_sent: sent,
            bytes_sent,
            elapsed_ms: started.elapsed().as_millis(),
        })
    })();
    let stop = pipeline.set_state(gst::State::Null);
    match (result, stop) {
        (Ok(report), Ok(_)) => Ok(report),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!(
            "producer cleanup failed: {error}; prior={result:?}"
        )),
    }
}

fn drain_decoded(
    sink: &gst_app::AppSink,
    received: &HashMap<u64, Instant>,
    latency: &mut Vec<f64>,
    decoded_frames: &mut usize,
    decoded_bytes: &mut usize,
    timeout: gst::ClockTime,
) -> Result<(), String> {
    while let Some(sample) = sink.try_pull_sample(timeout) {
        let caps = sample.caps().ok_or("decoded sample missing caps")?;
        let structure = caps.structure(0).ok_or("decoded caps missing structure")?;
        if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA")
        {
            return Err("decoded sample is not RGBA".into());
        }
        let width = structure
            .get::<i32>("width")
            .map_err(|_| "decoded width missing")?;
        let height = structure
            .get::<i32>("height")
            .map_err(|_| "decoded height missing")?;
        if width != WIDTH as i32 || height != HEIGHT as i32 {
            return Err(format!("unexpected decoded dimensions: {width}x{height}"));
        }
        let buffer = sample.buffer().ok_or("decoded sample missing buffer")?;
        let expected = (WIDTH as usize) * (HEIGHT as usize) * 4;
        if buffer.size() < expected {
            return Err("decoded buffer is truncated".into());
        }
        *decoded_frames += 1;
        *decoded_bytes += buffer.size();
        if let Some(pts) = buffer.pts().map(|pts| pts.nseconds())
            && let Some(at) = received.get(&pts)
        {
            latency.push(at.elapsed().as_secs_f64() * 1000.0);
        }
    }
    Ok(())
}

pub fn receive_frames(
    mut stream: TcpStream,
    key: &[u8; 32],
    session_id: &str,
) -> Result<ConsumerStats, String> {
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| format!("cannot set media timeout: {error}"))?;
    let capabilities = probe_encoders()?;
    if !capabilities.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let decoder = decoder_candidates()
        .iter()
        .copied()
        .find(|name| gst::ElementFactory::find(name).is_some())
        .ok_or("no usable H.264 decoder")?;
    let pipeline = build(&decoder_description(decoder))?;
    let started = Instant::now();
    let result = (|| {
        let source = pipeline
            .by_name("h264_source")
            .ok_or("missing h264_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "h264_source is not an AppSrc")?;
        let sink = pipeline
            .by_name("decoded_sink")
            .ok_or("missing decoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "decoded_sink is not an AppSink")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start consumer: {error}"))?;
        let mut cipher = RecordCipher::new(key, session_id, Direction::HostToClient)
            .map_err(|error| format!("media cipher: {error}"))?;
        let mut received = HashMap::new();
        let mut latency = Vec::new();
        let mut decoded_frames = 0;
        let mut decoded_bytes = 0;
        let mut frames = 0;
        loop {
            let mut length = [0u8; 4];
            match stream.read_exact(&mut length) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(format!("media read failed: {error}")),
            }
            let length = u32::from_be_bytes(length) as usize;
            if length == 0 || length > MAX_RECORD_BYTES {
                return Err(format!("invalid media record length: {length}"));
            }
            let mut ciphertext = vec![0u8; length];
            stream
                .read_exact(&mut ciphertext)
                .map_err(|error| format!("media read failed: {error}"))?;
            let plaintext = cipher
                .open(&ciphertext)
                .map_err(|error| format!("media open failed: {error}"))?;
            if plaintext.len() < 9 {
                return Err("media record is too short".into());
            }
            let pts = u64::from_be_bytes(plaintext[..8].try_into().expect("eight bytes"));
            let data = &plaintext[9..];
            received.insert(pts, Instant::now());
            let mut buffer = gst::Buffer::from_mut_slice(data.to_vec());
            if let Some(writable) = buffer.get_mut() {
                writable.set_pts(gst::ClockTime::from_nseconds(pts));
                writable.set_duration(gst::ClockTime::from_nseconds(1_000_000_000 / 30));
            }
            source
                .push_buffer(buffer)
                .map_err(|error| format!("decoder input: {error}"))?;
            frames += 1;
            drain_decoded(
                &sink,
                &received,
                &mut latency,
                &mut decoded_frames,
                &mut decoded_bytes,
                gst::ClockTime::ZERO,
            )?;
        }
        source
            .end_of_stream()
            .map_err(|error| format!("decoder EOS: {error}"))?;
        let deadline = started + WAIT;
        while Instant::now() < deadline {
            let before = decoded_frames;
            drain_decoded(
                &sink,
                &received,
                &mut latency,
                &mut decoded_frames,
                &mut decoded_bytes,
                gst::ClockTime::from_mseconds(50),
            )?;
            if decoded_frames == before && sink.is_eos() {
                break;
            }
        }
        latency.sort_by(f64::total_cmp);
        let latency = if latency.is_empty() {
            Latency {
                samples: 0,
                min_ms: 0.0,
                median_ms: 0.0,
                max_ms: 0.0,
            }
        } else {
            let middle = latency.len() / 2;
            let median = if latency.len().is_multiple_of(2) {
                (latency[middle - 1] + latency[middle]) / 2.0
            } else {
                latency[middle]
            };
            Latency {
                samples: latency.len(),
                min_ms: latency[0],
                median_ms: median,
                max_ms: latency[latency.len() - 1],
            }
        };
        Ok(ConsumerStats {
            frames_received: frames,
            decoded_frames,
            decoded_bytes,
            latency,
            elapsed_ms: started.elapsed().as_millis(),
            verified: frames > 0 && decoded_frames > 0,
        })
    })();
    let stop = pipeline.set_state(gst::State::Null);
    match (result, stop) {
        (Ok(report), Ok(_)) => Ok(report),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!(
            "consumer cleanup failed: {error}; prior={result:?}"
        )),
    }
}

pub struct MediaServer {
    listener: TcpListener,
    key: [u8; 32],
    session_id: String,
    frames: usize,
}

impl MediaServer {
    pub fn bind(address: SocketAddr, frames: usize) -> Result<(Self, MediaOffer), String> {
        let listener = TcpListener::bind(address)
            .map_err(|error| format!("cannot bind media listener: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read media address: {error}"))?;
        let key_hex = random_hex(32)?;
        let key = key_bytes(&key_hex)?;
        let session_id = random_hex(16)?;
        let offer = MediaOffer {
            protocol: "media-test/1".into(),
            address: address.to_string(),
            key_hex,
            session_id: session_id.clone(),
            frames,
        };
        Ok((
            Self {
                listener,
                key,
                session_id,
                frames,
            },
            offer,
        ))
    }

    pub fn accept_and_send(self) -> Result<ProducerStats, String> {
        let (mut stream, _) = self
            .listener
            .accept()
            .map_err(|error| format!("media accept failed: {error}"))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("cannot set media timeout: {error}"))?;
        send_frames(&mut stream, &self.key, &self.session_id, self.frames)
    }
}

pub fn connect(
    address: SocketAddr,
    key_hex: &str,
    session_id: &str,
) -> Result<ConsumerStats, String> {
    let key = key_bytes(key_hex)?;
    let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
        .map_err(|error| format!("media connect failed: {error}"))?;
    receive_frames(stream, &key, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_material_is_bounded() {
        assert!(key_bytes(&"00".repeat(32)).is_ok());
        assert!(key_bytes("00").is_err());
        assert!(key_bytes(&"zz".repeat(32)).is_err());
    }

    #[test]
    #[ignore = "requires H.264 GStreamer plugins and localhost networking"]
    fn two_process_style_media_stream_decodes() {
        let (server, offer) =
            MediaServer::bind("127.0.0.1:0".parse().expect("loopback"), 12).unwrap();
        let handle = std::thread::spawn(move || server.accept_and_send());
        let consumer = connect(
            offer.address.parse().unwrap(),
            &offer.key_hex,
            &offer.session_id,
        )
        .unwrap();
        let producer = handle.join().unwrap().unwrap();
        assert!(producer.frames_sent > 0);
        assert!(consumer.verified);
        assert!(consumer.decoded_frames > 0);
    }
}
