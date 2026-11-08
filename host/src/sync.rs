//! Synthetic end-to-end test of encode → permission-scoped handoff → sync gate → decode.
//!
//! This uses `videotestsrc` and in-memory GStreamer pipelines only. It never
//! touches PipeWire, the desktop portal, user pixels or a network endpoint.
//! Reported latency is the in-process encode/handoff/decode traversal time, not
//! capture latency and not network round-trip time.

use crate::{
    decoder_sync::{H264SyncEvent, H264SyncGate, SyncError},
    media::{H264Encoder, decoder_candidates, probe_encoders},
    session::{FrameGeometry, H264AccessUnit, H264Discontinuity, SessionToken, StreamSession},
    transport::{Direction, RecordCipher},
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const FRAME_COUNT: usize = 12;
/// A deliberate loss after this many frames must be followed by a fresh IDR.
const INJECT_AFTER: usize = 4;
const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const WAIT: Duration = Duration::from_secs(10);
const FRAME_PTS_NS: u64 = 1_000_000_000 / 30;

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct LatencySummary {
    pub samples: usize,
    pub min_ms: f64,
    pub median_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct SyncRecoveryReport {
    pub encoder: H264Encoder,
    pub decoder: String,
    pub input_frames: usize,
    pub encoded_buffers: usize,
    pub decoded_frames: usize,
    pub discontinuities: usize,
    pub discarded_access_units: u64,
    pub decoded_with_pts: usize,
    pub aead_records: usize,
    pub aead_bytes: usize,
    pub recovered_after_injected_loss: bool,
    pub latency: LatencySummary,
    pub elapsed_ms: u128,
    /// True only when recovery, decoding and latency sampling all succeeded.
    pub verified: bool,
}

#[derive(Default)]
struct LoopStats {
    encoded_buffers: usize,
    decoded_frames: usize,
    discontinuities: usize,
    decoded_with_pts: usize,
    injected: bool,
    awaiting_recovery: bool,
    recovered: bool,
    sent_at: HashMap<u64, Instant>,
    latencies_ms: Vec<f64>,
    /// Test-only AEAD pair proving each emitted access unit survives the
    /// encrypted media-record boundary before decoding.
    aead_seal: Option<RecordCipher>,
    aead_open: Option<RecordCipher>,
    aead_records: usize,
    aead_bytes: usize,
}

fn deadline_remaining(deadline: Instant) -> gst::ClockTime {
    gst::ClockTime::from_nseconds(
        u64::try_from(
            deadline
                .saturating_duration_since(Instant::now())
                .as_nanos(),
        )
        .unwrap_or(u64::MAX),
    )
}

fn build_pipeline(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|e| format!("cannot construct pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned an unexpected element".to_string())
}

fn encoder_description(encoder: H264Encoder) -> String {
    // The short GOP guarantees a fresh IDR after the injected loss. Recovery is
    // still decided by the gate inspecting Annex-B NAL headers, not by flags.
    let settings = match encoder {
        H264Encoder::Nvidia => {
            "preset=p1 bframes=0 bitrate=3000 gop-size=4 repeat-sequence-header=true"
        }
        H264Encoder::X264 => {
            "tune=zerolatency speed-preset=ultrafast bitrate=3000 bframes=0 key-int-max=4"
        }
        H264Encoder::OpenH264 => "bitrate=3000000 gop-size=4",
    };
    format!(
        "appsrc name=raw_source is-live=false format=time \
         caps=video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT},framerate=30/1 ! \
         queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} {} ! \
         h264parse config-interval=-1 ! \
         video/x-h264,stream-format=byte-stream,alignment=au ! \
         appsink name=encoded_sink sync=false max-buffers=2 drop=false",
        encoder.element_name(),
        settings
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

fn raw_description() -> String {
    format!(
        "videotestsrc num-buffers={FRAME_COUNT} is-live=true ! \
         video/x-raw,width={WIDTH},height={HEIGHT},framerate=30/1 ! videoconvert ! \
         video/x-raw,format=RGBA ! \
         appsink name=raw_sink sync=false max-buffers=2 drop=false"
    )
}

fn validate_raw(sample: &gst::Sample, expected: usize) -> Result<Vec<u8>, String> {
    let caps = sample.caps().ok_or("raw sample missing caps")?;
    let structure = caps.structure(0).ok_or("raw caps missing structure")?;
    if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA") {
        return Err("raw sample is not RGBA video".into());
    }
    let width = structure
        .get::<i32>("width")
        .map_err(|_| "raw caps missing width")?;
    let height = structure
        .get::<i32>("height")
        .map_err(|_| "raw caps missing height")?;
    if width != WIDTH as i32 || height != HEIGHT as i32 {
        return Err(format!("unexpected raw dimensions: {width}x{height}"));
    }
    let buffer = sample.buffer().ok_or("raw sample missing buffer")?;
    let readable = buffer.map_readable().map_err(|_| "cannot map raw buffer")?;
    if readable.size() != expected {
        return Err(format!(
            "raw buffer size {} does not match tightly packed {expected}",
            readable.size()
        ));
    }
    Ok(readable.as_slice().to_vec())
}

fn drain_encoded(
    encoded_sink: &gst_app::AppSink,
    session: &mut StreamSession,
    gate: &mut H264SyncGate,
    token: SessionToken,
    decoder_source: &gst_app::AppSrc,
    stats: &mut LoopStats,
) -> Result<(), String> {
    while let Some(sample) = encoded_sink.try_pull_sample(gst::ClockTime::ZERO) {
        let caps = sample.caps().ok_or("encoded sample missing caps")?;
        if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
            return Err("encoded sample is not H.264".into());
        }
        let buffer = sample.buffer().ok_or("encoded sample missing buffer")?;
        let len = buffer.size();
        if len == 0 || len > 16 * 1024 * 1024 {
            return Err(format!("invalid encoded buffer size: {len}"));
        }
        let bytes = buffer
            .map_readable()
            .map_err(|_| "cannot map encoded buffer")?
            .as_slice()
            .to_vec();
        let is_keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        // GStreamer may report a segment-relative offset for this synthetic
        // pipeline; normalize it so downstream PTS comparison stays meaningful.
        let segment_start = sample
            .segment()
            .map(|segment| u64::try_from(segment.start().value()).unwrap_or(0))
            .unwrap_or(0);
        let pts_ns = buffer
            .pts()
            .map(|pts| pts.nseconds().saturating_sub(segment_start));
        session
            .enqueue_h264(
                token,
                H264AccessUnit {
                    pts_ns,
                    is_keyframe,
                    bytes,
                },
            )
            .map_err(|e| format!("session rejected access unit: {e:?}"))?;
        stats.encoded_buffers += 1;
    }

    while let Some(event) = gate.next(session, token).map_err(|e| match e {
        SyncError::InvalidAnnexB => "sync gate rejected Annex-B input".to_string(),
        SyncError::Session(error) => format!("sync gate session error: {error:?}"),
    })? {
        match event {
            H264SyncEvent::Discontinuity(reason) => {
                stats.discontinuities += 1;
                if stats.injected && !stats.recovered {
                    stats.awaiting_recovery = true;
                }
                let _ = reason;
            }
            H264SyncEvent::AccessUnit(unit) => {
                if stats.awaiting_recovery {
                    stats.recovered = true;
                    stats.awaiting_recovery = false;
                }
                let plaintext = if let (Some(seal), Some(open)) =
                    (stats.aead_seal.as_mut(), stats.aead_open.as_mut())
                {
                    let sealed = seal
                        .seal(&unit.bytes)
                        .map_err(|e| format!("AEAD media seal failed: {e}"))?;
                    let opened = open
                        .open(&sealed)
                        .map_err(|e| format!("AEAD media open failed: {e}"))?;
                    stats.aead_records += 1;
                    stats.aead_bytes += opened.len();
                    opened
                } else {
                    unit.bytes
                };
                let mut buffer = gst::Buffer::from_mut_slice(plaintext);
                if let Some(writable) = buffer.get_mut() {
                    if let Some(pts_ns) = unit.pts_ns {
                        writable.set_pts(gst::ClockTime::from_nseconds(pts_ns));
                    }
                    writable.set_duration(gst::ClockTime::from_nseconds(FRAME_PTS_NS));
                }
                decoder_source
                    .push_buffer(buffer)
                    .map_err(|e| format!("decoder input: {e}"))?;
            }
        }
    }
    Ok(())
}

fn drain_decoded(
    decoded_sink: &gst_app::AppSink,
    stats: &mut LoopStats,
    timeout: gst::ClockTime,
) -> Result<(), String> {
    while let Some(sample) = decoded_sink.try_pull_sample(timeout) {
        let caps = sample.caps().ok_or("decoded sample missing caps")?;
        let structure = caps.structure(0).ok_or("decoded caps missing structure")?;
        if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA")
        {
            return Err("decoded sample is not RGBA video".into());
        }
        let width = structure
            .get::<i32>("width")
            .map_err(|_| "decoded caps missing width")?;
        let height = structure
            .get::<i32>("height")
            .map_err(|_| "decoded caps missing height")?;
        if width != WIDTH as i32 || height != HEIGHT as i32 {
            return Err(format!("unexpected decoded dimensions: {width}x{height}"));
        }
        let buffer = sample.buffer().ok_or("decoded sample missing buffer")?;
        let expected = (WIDTH as usize) * (HEIGHT as usize) * 4;
        if buffer.size() < expected {
            return Err("decoded buffer is truncated".into());
        }
        stats.decoded_frames += 1;
        let segment_start = sample
            .segment()
            .map(|segment| u64::try_from(segment.start().value()).unwrap_or(0))
            .unwrap_or(0);
        if let Some(pts_ns) = buffer
            .pts()
            .map(|pts| pts.nseconds().saturating_sub(segment_start))
        {
            stats.decoded_with_pts += 1;
            if let Some(sent) = stats.sent_at.get(&pts_ns) {
                stats
                    .latencies_ms
                    .push(sent.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    Ok(())
}

fn summarize(mut samples: Vec<f64>) -> LatencySummary {
    if samples.is_empty() {
        return LatencySummary::default();
    }
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    let median_ms = if samples.len().is_multiple_of(2) {
        (samples[middle - 1] + samples[middle]) / 2.0
    } else {
        samples[middle]
    };
    LatencySummary {
        samples: samples.len(),
        min_ms: samples[0],
        median_ms,
        max_ms: samples[samples.len() - 1],
    }
}

fn run_pipeline(encoder: H264Encoder, decoder: &str) -> Result<SyncRecoveryReport, String> {
    let raw = build_pipeline(&raw_description())?;
    let encoded = build_pipeline(&encoder_description(encoder))?;
    let decoded = build_pipeline(&decoder_description(decoder))?;

    let result = (|| {
        let raw_sink = raw
            .by_name("raw_sink")
            .ok_or("missing raw_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "invalid raw_sink")?;
        let encoded_source = encoded
            .by_name("raw_source")
            .ok_or("missing raw_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "invalid raw_source")?;
        let encoded_sink = encoded
            .by_name("encoded_sink")
            .ok_or("missing encoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "invalid encoded_sink")?;
        let decoder_source = decoded
            .by_name("h264_source")
            .ok_or("missing h264_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "invalid h264_source")?;
        let decoded_sink = decoded
            .by_name("decoded_sink")
            .ok_or("missing decoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "invalid decoded_sink")?;

        // Synthetic grant only. Real producers must authorize after a portal
        // window-only selection and revoke before pipeline teardown.
        let mut session = StreamSession::new();
        let token = session.authorize().map_err(|e| format!("{e:?}"))?;
        session.start(token).map_err(|e| format!("{e:?}"))?;
        let mut gate = H264SyncGate::new();

        encoded
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start encoder: {e}"))?;
        decoded
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start decoder: {e}"))?;
        raw.set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start source: {e}"))?;

        let started = Instant::now();
        let deadline = started + WAIT;
        let geometry = FrameGeometry {
            width: WIDTH,
            height: HEIGHT,
        };
        let expected = geometry.rgba_size().ok_or("invalid geometry")?;
        // Fixed test key/session: this proves the media record boundary, not a
        // real pairing or network transport.
        let media_key = [0x5au8; 32];
        let media_session = "00112233445566778899aabbccddeeff";
        let mut stats = LoopStats {
            aead_seal: Some(
                RecordCipher::new(&media_key, media_session, Direction::ClientToHost)
                    .map_err(|e| format!("cannot create AEAD media cipher: {e}"))?,
            ),
            aead_open: Some(
                RecordCipher::new(&media_key, media_session, Direction::ClientToHost)
                    .map_err(|e| format!("cannot create AEAD media cipher: {e}"))?,
            ),
            ..LoopStats::default()
        };

        for index in 0..FRAME_COUNT {
            let sample = raw_sink
                .try_pull_sample(deadline_remaining(deadline))
                .ok_or("synthetic source ended before the expected frame count")?;
            let bytes = validate_raw(&sample, expected)?;
            let pts_ns = u64::try_from(index).unwrap_or(u64::MAX) * FRAME_PTS_NS;
            stats.sent_at.insert(pts_ns, Instant::now());
            session
                .enqueue_rgba(token, geometry, bytes)
                .map_err(|e| format!("session rejected raw frame: {e:?}"))?;
            if let Some(bytes) = session
                .dequeue(token)
                .map_err(|e| format!("session dequeue: {e:?}"))?
            {
                let mut buffer = gst::Buffer::from_mut_slice(bytes);
                if let Some(writable) = buffer.get_mut() {
                    writable.set_pts(gst::ClockTime::from_nseconds(pts_ns));
                    writable.set_duration(gst::ClockTime::from_nseconds(FRAME_PTS_NS));
                }
                encoded_source
                    .push_buffer(buffer)
                    .map_err(|e| format!("encoder input: {e}"))?;
            }

            if index == INJECT_AFTER {
                session
                    .mark_h264_discontinuity(token, H264Discontinuity::Pipeline)
                    .map_err(|e| format!("session rejected discontinuity: {e:?}"))?;
                stats.injected = true;
            }

            drain_encoded(
                &encoded_sink,
                &mut session,
                &mut gate,
                token,
                &decoder_source,
                &mut stats,
            )?;
            drain_decoded(&decoded_sink, &mut stats, gst::ClockTime::ZERO)?;
            if deadline <= Instant::now() {
                return Err("encode/decode deadline reached".into());
            }
        }

        encoded_source
            .end_of_stream()
            .map_err(|e| format!("cannot signal encoder EOS: {e}"))?;
        while !encoded_sink.is_eos() && deadline > Instant::now() {
            drain_encoded(
                &encoded_sink,
                &mut session,
                &mut gate,
                token,
                &decoder_source,
                &mut stats,
            )?;
            drain_decoded(&decoded_sink, &mut stats, gst::ClockTime::from_mseconds(20))?;
        }
        if !encoded_sink.is_eos() {
            return Err("encoder did not reach EOS before deadline".into());
        }

        decoder_source
            .end_of_stream()
            .map_err(|e| format!("cannot signal decoder EOS: {e}"))?;
        while !decoded_sink.is_eos() && deadline > Instant::now() {
            drain_decoded(&decoded_sink, &mut stats, gst::ClockTime::from_mseconds(50))?;
        }

        let discarded_access_units = gate.discarded_access_units();
        let latency = summarize(std::mem::take(&mut stats.latencies_ms));
        let verified = stats.decoded_frames > 0
            && stats.discontinuities >= 2
            && stats.recovered
            && latency.samples > 0
            && stats.aead_records > 0
            && stats.aead_bytes > 0;
        let report = SyncRecoveryReport {
            encoder,
            decoder: decoder.to_string(),
            input_frames: FRAME_COUNT,
            encoded_buffers: stats.encoded_buffers,
            decoded_frames: stats.decoded_frames,
            discontinuities: stats.discontinuities,
            discarded_access_units,
            decoded_with_pts: stats.decoded_with_pts,
            aead_records: stats.aead_records,
            aead_bytes: stats.aead_bytes,
            recovered_after_injected_loss: stats.recovered,
            latency,
            elapsed_ms: started.elapsed().as_millis(),
            verified,
        };
        session.close();
        Ok(report)
    })();

    let raw_stop = raw.set_state(gst::State::Null);
    let encoded_stop = encoded.set_state(gst::State::Null);
    let decoded_stop = decoded.set_state(gst::State::Null);
    match (result, raw_stop, encoded_stop, decoded_stop) {
        (Ok(report), Ok(_), Ok(_), Ok(_)) => Ok(report),
        (Err(error), Ok(_), Ok(_), Ok(_)) => Err(error),
        (result, a, b, c) => Err(format!(
            "pipeline cleanup failed (source={a:?}, encoder={b:?}, decoder={c:?}); prior result={result:?}"
        )),
    }
}

/// Exercises the full in-memory encode → handoff → recovery-gate → decode path,
/// including one deliberate discontinuity, and measures traversal latency.
pub fn synthetic_sync_recovery() -> Result<SyncRecoveryReport, String> {
    let caps = probe_encoders()?;
    if !caps.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoders = [
        (caps.nvidia, H264Encoder::Nvidia),
        (caps.x264, H264Encoder::X264),
        (caps.openh264, H264Encoder::OpenH264),
    ];
    let decoders: Vec<_> = decoder_candidates()
        .iter()
        .copied()
        .filter(|name| gst::ElementFactory::find(name).is_some())
        .collect();
    if decoders.is_empty() {
        return Err("no usable H.264 decoder factories are installed".into());
    }
    let mut failures = Vec::new();
    for (present, encoder) in encoders {
        if !present {
            continue;
        }
        for decoder in &decoders {
            match run_pipeline(encoder, decoder) {
                Ok(report) if report.verified => return Ok(report),
                Ok(report) => failures.push(format!(
                    "{} -> {decoder}: not verified ({} decoded, {} discontinuities, recovered={}, {} latency samples)",
                    encoder.element_name(),
                    report.decoded_frames,
                    report.discontinuities,
                    report.recovered_after_injected_loss,
                    report.latency.samples
                )),
                Err(error) => {
                    failures.push(format!("{} -> {decoder}: {error}", encoder.element_name()))
                }
            }
        }
    }
    Err(format!(
        "no encoder/decoder path recovered after loss: {}",
        failures.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_summary_is_ordered_and_handles_even_samples() {
        let summary = summarize(vec![3.0, 1.0, 2.0, 4.0]);
        assert_eq!(summary.samples, 4);
        assert_eq!(summary.min_ms, 1.0);
        assert_eq!(summary.median_ms, 2.5);
        assert_eq!(summary.max_ms, 4.0);
        assert_eq!(summarize(Vec::new()), LatencySummary::default());
    }

    #[test]
    fn synthetic_pipelines_are_bounded_and_never_capture_desktop() {
        for encoder in [
            H264Encoder::Nvidia,
            H264Encoder::X264,
            H264Encoder::OpenH264,
        ] {
            let description = encoder_description(encoder);
            assert!(description.contains(encoder.element_name()));
            assert!(description.contains("h264parse config-interval=-1"));
            assert!(description.contains("max-buffers=2 drop=false"));
            assert!(!description.contains("pipewiresrc"));
            assert!(!description.contains("portal"));
        }
        let decoder = decoder_description("nvh264dec");
        assert!(decoder.contains("video/x-h264,stream-format=byte-stream,alignment=au"));
        assert!(!decoder.contains("pipewiresrc"));
    }

    #[test]
    #[ignore = "requires functioning H.264 encoder and decoder plugins"]
    fn sync_recovery_integration_on_supported_host() {
        let report = synthetic_sync_recovery().expect("in-memory sync recovery");
        assert!(report.verified);
        assert!(report.decoded_with_pts > 0);
        assert!(report.recovered_after_injected_loss);
        assert!(report.decoded_frames > 0);
        assert!(report.latency.samples > 0);
        assert!(report.aead_records > 0);
        assert!(report.aead_bytes > 0);
    }
}
