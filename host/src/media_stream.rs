//! Two-process synthetic H.264 media stream over an authenticated encrypted TCP record.
//!
//! The media connection first runs the versioned Vista control handshake with
//! a 256-bit out-of-band pairing secret, opens a video-only session, and then
//! derives the media record key from the authenticated session keys. The client
//! sends an authenticated `KeyframeRequest` over the reverse record direction;
//! the host rate-limits it and restarts the encoder to emit a fresh IDR.
//!
//! This is a localhost transport diagnostic with a synthetic `videotestsrc`.
//! It does not capture desktop pixels, use the portal, or send media to iOS.

use crate::{
    media::{H264Encoder, decoder_candidates, probe_encoders},
    media_transport::{MediaError, MediaRecord, media_cipher, read_record, write_record},
    protocol::{
        ClientSession, Envelope, HostSession, HostState, MAX_CONTROL_FRAME_BYTES, PairingSecret,
        PermissionSet, SessionKeys, unix_now,
    },
    transport::{Direction, MAX_RECORD_BYTES, RecordCipher},
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
pub const DEFAULT_FRAMES: usize = 12;
const KEYFRAME_AFTER: usize = 4;
const WAIT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize)]
pub struct MediaOffer {
    pub protocol: String,
    pub address: String,
    pub pairing_id: String,
    /// Out-of-band test material. A real deployment uses a QR/code; this is
    /// not the media key, which is derived from the authenticated session.
    pub pairing_secret_hex: String,
    pub expires_at: u64,
    pub frames: usize,
}

#[derive(Debug, Serialize)]
pub struct ProducerStats {
    pub frames_sent: usize,
    pub bytes_sent: usize,
    pub keyframe_requests: u64,
    pub encoder_restarts: usize,
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
    pub keyframe_requests_sent: usize,
    pub latency: Latency,
    pub elapsed_ms: u128,
    pub verified: bool,
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

fn write_plain(stream: &mut TcpStream, envelope: &Envelope) -> Result<(), String> {
    let bytes = envelope
        .encode()
        .map_err(|error| format!("control encode: {error}"))?;
    stream
        .write_all(&bytes)
        .map_err(|error| format!("control write: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("control flush: {error}"))
}

fn read_plain(stream: &mut TcpStream) -> Result<Envelope, String> {
    let mut prefix = [0u8; 4];
    stream
        .read_exact(&mut prefix)
        .map_err(|error| format!("control read: {error}"))?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_CONTROL_FRAME_BYTES {
        return Err(format!("invalid control frame length: {length}"));
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("control read: {error}"))?;
    let mut framed = Vec::with_capacity(4 + length);
    framed.extend_from_slice(&(length as u32).to_be_bytes());
    framed.extend_from_slice(&body);
    Envelope::decode(&framed).map_err(|error| format!("control decode: {error}"))
}

fn host_handshake(
    stream: &mut TcpStream,
    host: &mut HostSession,
    now_secs: u64,
) -> Result<(SessionKeys, String), String> {
    for _ in 0..3 {
        let envelope = read_plain(stream)?;
        let response = host
            .handle(&envelope, now_secs)
            .map_err(|error| format!("control handshake: {error}"))?;
        if let Some(response) = response {
            write_plain(stream, &response)?;
        }
    }
    if host.state() != HostState::Active {
        return Err("control session did not become active".into());
    }
    let keys = host
        .session_keys()
        .ok_or("authenticated session keys are unavailable")?
        .clone();
    let session_id = host
        .session_id()
        .ok_or("authenticated session id is unavailable")?
        .to_string();
    Ok((keys, session_id))
}

fn client_handshake(
    stream: &mut TcpStream,
    secret: PairingSecret,
) -> Result<(ClientSession, SessionKeys, String), String> {
    let mut client = ClientSession::new(secret, "vista-media-client")
        .map_err(|error| format!("control client: {error}"))?;
    let hello = client
        .hello()
        .map_err(|error| format!("control hello: {error}"))?;
    write_plain(stream, &hello)?;
    let server_hello = read_plain(stream)?;
    let proof = client
        .accept_server_hello(&server_hello)
        .map_err(|error| format!("control handshake: {error}"))?;
    write_plain(stream, &proof)?;
    let accepted = read_plain(stream)?;
    client
        .accept_pairing(&accepted)
        .map_err(|error| format!("pairing rejected: {error}"))?;
    let open = client
        .open_session(PermissionSet::video_only())
        .map_err(|error| format!("session open: {error}"))?;
    write_plain(stream, &open)?;
    let opened = read_plain(stream)?;
    client
        .accept_session_opened(&opened)
        .map_err(|error| format!("session open rejected: {error}"))?;
    let keys = client
        .session_keys()
        .ok_or("authenticated session keys are unavailable")?
        .clone();
    let session_id = client
        .session_id()
        .ok_or("authenticated session id is unavailable")?
        .to_string();
    Ok((client, keys, session_id))
}

fn read_control_requests(
    mut stream: TcpStream,
    mut host: HostSession,
    keys: &SessionKeys,
    session_id: &str,
    request: Arc<AtomicBool>,
) -> Result<u64, String> {
    stream
        .set_read_timeout(Some(CONTROL_READ_TIMEOUT))
        .map_err(|error| format!("cannot set control timeout: {error}"))?;
    let mut cipher = RecordCipher::new(keys.client_to_host(), session_id, Direction::ClientToHost)
        .map_err(|error| format!("control cipher: {error}"))?;
    let mut accepted = 0;
    loop {
        let mut prefix = [0u8; 4];
        match stream.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(format!("control read failed: {error}")),
        }
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 || length > MAX_RECORD_BYTES {
            return Err(format!("invalid control record length: {length}"));
        }
        let mut ciphertext = vec![0u8; length];
        stream
            .read_exact(&mut ciphertext)
            .map_err(|error| format!("control read failed: {error}"))?;
        let plaintext = cipher
            .open(&ciphertext)
            .map_err(|error| format!("control open failed: {error}"))?;
        let envelope: Envelope = serde_json::from_slice(&plaintext)
            .map_err(|error| format!("control decode failed: {error}"))?;
        let before = host.keyframe_requests();
        host.handle(&envelope, unix_now())
            .map_err(|error| format!("control request rejected: {error}"))?;
        if host.keyframe_requests() > before {
            accepted += 1;
            request.store(true, Ordering::SeqCst);
        }
    }
    Ok(accepted)
}

fn send_frames(
    stream: &mut TcpStream,
    key: &[u8; 32],
    session_id: &str,
    frames: usize,
    request: &AtomicBool,
) -> Result<ProducerStats, String> {
    let capabilities = probe_encoders()?;
    if !capabilities.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoder = capabilities.selected.ok_or("no usable H.264 encoder")?;
    let mut pipeline = build(&producer_description(encoder, frames))?;
    let started = Instant::now();
    let result = (|| {
        let mut sink = pipeline
            .by_name("encoded_sink")
            .ok_or("missing encoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "encoded_sink is not an AppSink")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start producer: {error}"))?;
        let mut cipher = media_cipher(key, session_id, Direction::HostToClient)
            .map_err(|error| format!("media cipher: {error}"))?;
        let deadline = started + WAIT;
        let mut sent = 0;
        let mut bytes_sent = 0;
        let mut encoder_restarts = 0;
        let mut pts_offset = 0u64;
        while sent < frames {
            if request.swap(false, Ordering::SeqCst) {
                pipeline.set_state(gst::State::Null).map_err(|error| {
                    format!("cannot stop encoder for keyframe request: {error}")
                })?;
                pipeline = build(&producer_description(encoder, frames))?;
                sink = pipeline
                    .by_name("encoded_sink")
                    .ok_or("missing encoded_sink after restart")?
                    .downcast::<gst_app::AppSink>()
                    .map_err(|_| "encoded_sink is not an AppSink after restart")?;
                pipeline
                    .set_state(gst::State::Playing)
                    .map_err(|error| format!("cannot restart encoder: {error}"))?;
                encoder_restarts += 1;
                pts_offset = pts_offset.saturating_add(1_000_000_000);
            }
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
            let pts = buffer
                .pts()
                .map(|pts| pts.nseconds())
                .unwrap_or(0)
                .saturating_add(pts_offset);
            let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
            let record = MediaRecord::new(pts, keyframe, false, data.as_slice())
                .map_err(|error| format!("media record: {error}"))?;
            bytes_sent += write_record(stream, &mut cipher, &record)
                .map_err(|error| format!("media write failed: {error}"))?;
            sent += 1;
        }
        if sent == 0 {
            return Err("producer emitted no access units".into());
        }
        Ok(ProducerStats {
            frames_sent: sent,
            bytes_sent,
            keyframe_requests: 0,
            encoder_restarts,
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
    mut control_writer: TcpStream,
    client: &mut ClientSession,
    keys: &SessionKeys,
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
        let mut cipher = media_cipher(keys.host_to_client(), session_id, Direction::HostToClient)
            .map_err(|error| format!("media cipher: {error}"))?;
        let mut control_cipher =
            RecordCipher::new(keys.client_to_host(), session_id, Direction::ClientToHost)
                .map_err(|error| format!("control cipher: {error}"))?;
        let mut received = HashMap::new();
        let mut latency = Vec::new();
        let mut decoded_frames = 0;
        let mut decoded_bytes = 0;
        let mut frames = 0;
        let mut keyframe_requests_sent = 0;
        loop {
            if frames == KEYFRAME_AFTER && keyframe_requests_sent == 0 {
                let request = client
                    .keyframe_request()
                    .map_err(|error| format!("keyframe request: {error}"))?;
                let json = serde_json::to_vec(&request)
                    .map_err(|error| format!("keyframe encode: {error}"))?;
                let sealed = control_cipher
                    .seal(&json)
                    .map_err(|error| format!("keyframe seal: {error}"))?;
                let length = u32::try_from(sealed.len())
                    .map_err(|_| "control record too large".to_string())?;
                control_writer
                    .write_all(&length.to_be_bytes())
                    .map_err(|error| format!("keyframe write: {error}"))?;
                control_writer
                    .write_all(&sealed)
                    .map_err(|error| format!("keyframe write: {error}"))?;
                control_writer
                    .flush()
                    .map_err(|error| format!("keyframe flush: {error}"))?;
                control_writer
                    .shutdown(Shutdown::Write)
                    .map_err(|error| format!("keyframe shutdown: {error}"))?;
                keyframe_requests_sent = 1;
            }
            let record = match read_record(&mut stream, &mut cipher) {
                Ok(record) => record,
                Err(MediaError::Truncated) => break,
                Err(error) => return Err(format!("media read failed: {error}")),
            };
            let pts = record.pts_ns;
            received.insert(pts, Instant::now());
            let mut buffer = gst::Buffer::from_mut_slice(record.bytes);
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
            keyframe_requests_sent,
            latency,
            elapsed_ms: started.elapsed().as_millis(),
            verified: frames > 0 && decoded_frames > 0 && keyframe_requests_sent == 1,
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
    host: HostSession,
    now_secs: u64,
    frames: usize,
}

impl MediaServer {
    pub fn bind(address: SocketAddr, frames: usize) -> Result<(Self, MediaOffer), String> {
        let listener = TcpListener::bind(address)
            .map_err(|error| format!("cannot bind media listener: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read media address: {error}"))?;
        let secret =
            PairingSecret::generate().map_err(|error| format!("pairing secret: {error}"))?;
        let now_secs = unix_now();
        let host = HostSession::new(
            "vista-media-host",
            "Vista media host",
            secret.clone(),
            PermissionSet::video_only(),
            now_secs,
        )
        .map_err(|error| format!("control host: {error}"))?;
        let offer = MediaOffer {
            protocol: "media-auth-keyframe/1".into(),
            address: address.to_string(),
            pairing_id: host.pairing_id().to_string(),
            pairing_secret_hex: secret.to_hex(),
            expires_at: host.expires_at(),
            frames,
        };
        Ok((
            Self {
                listener,
                host,
                now_secs,
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
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("cannot set media timeout: {error}"))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("cannot set media timeout: {error}"))?;
        let mut host = self.host;
        let (keys, session_id) = host_handshake(&mut stream, &mut host, self.now_secs)?;
        let control_stream = stream
            .try_clone()
            .map_err(|error| format!("cannot clone media stream: {error}"))?;
        let request = Arc::new(AtomicBool::new(false));
        let control_request = request.clone();
        let control_keys = keys.clone();
        let control_session = session_id.clone();
        let control_thread = thread::spawn(move || {
            read_control_requests(
                control_stream,
                host,
                &control_keys,
                &control_session,
                control_request,
            )
        });
        let mut stats = send_frames(
            &mut stream,
            keys.host_to_client(),
            &session_id,
            self.frames,
            &request,
        )?;
        stats.keyframe_requests = control_thread
            .join()
            .map_err(|_| "control thread panicked".to_string())??;
        Ok(stats)
    }
}

pub fn connect(address: SocketAddr, secret_hex: &str) -> Result<ConsumerStats, String> {
    let secret = PairingSecret::from_hex(secret_hex)
        .map_err(|error| format!("invalid pairing secret: {error}"))?;
    let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
        .map_err(|error| format!("media connect failed: {error}"))?;
    let mut client_stream = stream;
    let (mut client, keys, session_id) = client_handshake(&mut client_stream, secret)?;
    let control_writer = client_stream
        .try_clone()
        .map_err(|error| format!("cannot clone media stream: {error}"))?;
    receive_frames(
        client_stream,
        control_writer,
        &mut client,
        &keys,
        &session_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_secret_must_be_256_bits_of_hex() {
        assert!(PairingSecret::from_hex(&"00".repeat(32)).is_ok());
        assert!(PairingSecret::from_hex("00").is_err());
        assert!(PairingSecret::from_hex(&"zz".repeat(32)).is_err());
    }

    #[test]
    #[ignore = "requires H.264 GStreamer plugins and localhost networking"]
    fn authenticated_media_stream_decodes_and_requests_a_keyframe() {
        let (server, offer) =
            MediaServer::bind("127.0.0.1:0".parse().expect("loopback"), 12).unwrap();
        let handle = std::thread::spawn(move || server.accept_and_send());
        let consumer = connect(offer.address.parse().unwrap(), &offer.pairing_secret_hex).unwrap();
        let producer = handle.join().unwrap().unwrap();
        assert!(producer.frames_sent > 0);
        assert_eq!(producer.keyframe_requests, 1);
        assert!(producer.encoder_restarts >= 1);
        assert_eq!(consumer.keyframe_requests_sent, 1);
        assert!(consumer.verified);
        assert!(consumer.decoded_frames > 0);
    }

    #[test]
    #[ignore = "requires localhost networking and control protocol"]
    fn wrong_pairing_secret_is_rejected() {
        let (server, offer) =
            MediaServer::bind("127.0.0.1:0".parse().expect("loopback"), 12).unwrap();
        let handle = std::thread::spawn(move || server.accept_and_send());
        let wrong = "11".repeat(32);
        let client = connect(offer.address.parse().unwrap(), &wrong);
        assert!(client.is_err());
        assert!(handle.join().unwrap().is_err());
    }
}
