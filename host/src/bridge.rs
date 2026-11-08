//! Permission-free synthetic raw frame to H.264 encoder bridge.
//!
//! Exercises GStreamer AppSink -> permission-scoped StreamSession -> AppSrc
//! -> H.264. It does not use PipeWire, desktop pixels or network access.
//! This is deliberately bounded and does not claim live portal enforcement.

use crate::media::{H264Encoder, probe_encoders};
use crate::session::{FrameGeometry, StreamSession};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::time::{Duration, Instant};

const FRAME_COUNT: usize = 8;
const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const WAIT: Duration = Duration::from_secs(8);

#[derive(Debug, Serialize)]
pub struct SyntheticBridgeReport {
    pub encoder: H264Encoder,
    pub input_frames: usize,
    pub encoded_buffers: usize,
    pub encoded_bytes: usize,
    pub dropped_frames: u64,
    pub elapsed_ms: u128,
    pub verified: bool,
}

fn build_pipeline(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|e| format!("cannot construct pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer pipeline returned unexpected element".to_string())
}

fn run_encoder(encoder: H264Encoder) -> Result<SyntheticBridgeReport, String> {
    let source = build_pipeline(
        "videotestsrc num-buffers=8 is-live=true ! video/x-raw,width=320,height=180,framerate=30/1 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=raw_sink sync=false max-buffers=2 drop=false",
    )?;
    let encoded_description = format!(
        "appsrc name=raw_source is-live=false format=time caps=video/x-raw,format=RGBA,width=320,height=180,framerate=30/1 ! queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} {} ! h264parse ! video/x-h264,alignment=au ! appsink name=encoded_sink sync=false max-buffers=2 drop=true",
        encoder.element_name(),
        match encoder {
            H264Encoder::Nvidia => "preset=p1 bframes=0 bitrate=3000",
            H264Encoder::X264 => "tune=zerolatency speed-preset=ultrafast bitrate=3000 bframes=0",
            H264Encoder::OpenH264 => "bitrate=3000000",
        }
    );
    let target = build_pipeline(&encoded_description)?;

    let result = (|| {
        let raw_sink = source
            .by_name("raw_sink")
            .ok_or("missing raw_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "invalid raw_sink")?;
        let appsrc = target
            .by_name("raw_source")
            .ok_or("missing raw_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "invalid raw_source")?;
        let encoded_sink = target
            .by_name("encoded_sink")
            .ok_or("missing encoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "invalid encoded_sink")?;

        let started = Instant::now();
        let deadline = started + WAIT;
        let remaining = || {
            gst::ClockTime::from_nseconds(
                u64::try_from(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_nanos(),
                )
                .unwrap_or(u64::MAX),
            )
        };

        // This grant is *synthetic only*. The real worker must authorize only
        // following a successful portal window-only selection.
        let mut session = StreamSession::new();
        let token = session.authorize().map_err(|e| format!("{e:?}"))?;
        session.start(token).map_err(|e| format!("{e:?}"))?;
        target
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start encoder: {e}"))?;
        source
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start source: {e}"))?;

        let mut input_frames = 0;
        let mut encoded_buffers = 0;
        let mut encoded_bytes = 0;
        let frame_geometry = FrameGeometry {
            width: WIDTH,
            height: HEIGHT,
        };
        let expected = frame_geometry.rgba_size().ok_or("invalid geometry")?;

        while input_frames < FRAME_COUNT {
            if deadline <= Instant::now() {
                return Err("source frame deadline reached".into());
            }
            let sample = raw_sink
                .try_pull_sample(remaining())
                .ok_or("synthetic source ended before eight frames")?;
            let caps = sample.caps().ok_or("raw sample missing caps")?;
            let fmt = caps.structure(0).and_then(|s| s.get::<&str>("format").ok());
            if fmt != Some("RGBA") {
                return Err("raw frame format is not RGBA".into());
            }
            let buffer = sample.buffer().ok_or("raw frame missing buffer")?;
            let readable = buffer.map_readable().map_err(|_| "cannot map raw buffer")?;
            if readable.size() < expected {
                return Err("raw buffer truncated".into());
            }
            // The fixture uses tightly packed RGBA; reject unanticipated padding.
            if readable.size() != expected {
                return Err("raw buffer has unsupported row padding".into());
            }
            session
                .enqueue_rgba(token, frame_geometry, readable.as_slice().to_vec())
                .map_err(|e| format!("session rejected frame: {e:?}"))?;
            input_frames += 1;
            if let Some(bytes) = session
                .dequeue(token)
                .map_err(|e| format!("session dequeue: {e:?}"))?
            {
                let mut buffer = gst::Buffer::from_mut_slice(bytes);
                {
                    let writable = buffer.get_mut().ok_or("buffer not writable")?;
                    let index = u64::try_from(input_frames - 1).unwrap_or(u64::MAX);
                    writable.set_pts(gst::ClockTime::from_nseconds(index * 1_000_000_000 / 30));
                    writable.set_duration(gst::ClockTime::from_nseconds(1_000_000_000 / 30));
                }
                appsrc
                    .push_buffer(buffer)
                    .map_err(|e| format!("encoder input: {e}"))?;
            }
            // Do not accumulate encoded samples in memory; drain promptly.
            while let Some(sample) = encoded_sink.try_pull_sample(gst::ClockTime::ZERO) {
                let caps = sample.caps().ok_or("encoded caps missing")?;
                if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
                    return Err("encoded sample is not H.264".into());
                }
                let len = sample.buffer().ok_or("encoded buffer missing")?.size();
                if len == 0 || len > 16 * 1024 * 1024 {
                    return Err("encoded sample size invalid".into());
                }
                encoded_buffers += 1;
                encoded_bytes += len;
            }
        }
        appsrc
            .end_of_stream()
            .map_err(|e| format!("cannot signal EOS: {e}"))?;
        while let Some(sample) = encoded_sink.try_pull_sample(remaining()) {
            let caps = sample.caps().ok_or("encoded caps missing")?;
            if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
                return Err("encoded sample is not H.264".into());
            }
            let len = sample.buffer().ok_or("encoded buffer missing")?.size();
            if len == 0 || len > 16 * 1024 * 1024 {
                return Err("encoded sample size invalid".into());
            }
            encoded_buffers += 1;
            encoded_bytes += len;
            if encoded_sink.is_eos() || deadline <= Instant::now() {
                break;
            }
        }
        if !encoded_sink.is_eos() {
            return Err("encoder did not reach EOS before deadline".into());
        }
        let dropped_frames = session.dropped_frames();
        session.close();
        if encoded_buffers == 0 {
            return Err("encoder did not produce H.264 buffers".into());
        }
        Ok(SyntheticBridgeReport {
            encoder,
            input_frames,
            encoded_buffers,
            encoded_bytes,
            dropped_frames,
            elapsed_ms: started.elapsed().as_millis(),
            verified: true,
        })
    })();
    // Both pipelines must be stopped on *all* paths, even after source or
    // encoder initialization fails. Hold neither samples nor session after return.
    let source_stop = source.set_state(gst::State::Null);
    let target_stop = target.set_state(gst::State::Null);
    match (result, source_stop, target_stop) {
        (Ok(report), Ok(_), Ok(_)) => Ok(report),
        (Err(error), Ok(_), Ok(_)) => Err(error),
        (result, a, b) => Err(format!(
            "pipeline cleanup failed (source={a:?}, encoder={b:?}); prior result={result:?}"
        )),
    }
}

pub fn synthetic_frame_bridge() -> Result<SyntheticBridgeReport, String> {
    let caps = probe_encoders()?;
    if !caps.h264_parser {
        return Err("GStreamer h264parse unavailable".into());
    }
    let choices = [
        (caps.nvidia, H264Encoder::Nvidia),
        (caps.x264, H264Encoder::X264),
        (caps.openh264, H264Encoder::OpenH264),
    ];
    let mut errors = Vec::new();
    for (present, encoder) in choices {
        if present {
            match run_encoder(encoder) {
                Ok(report) => return Ok(report),
                Err(error) => errors.push(format!("{}: {error}", encoder.element_name())),
            }
        }
    }
    Err(format!(
        "no synthetic encoder bridge worked: {}",
        errors.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires functioning H.264 encoder plugins"]
    fn synthetic_frame_bridge_hardware_integration() {
        let result = synthetic_frame_bridge().unwrap();
        assert_eq!(result.input_frames, FRAME_COUNT);
        assert!(result.encoded_buffers > 0);
        assert!(result.encoded_bytes > 0);
        assert!(result.verified);
    }
}
