//! Local synthetic PipeWire video-node loopback probe.
//!
//! This exercises the same `pipewiresrc` element, caps negotiation and RGBA
//! frame validation used by the capture path, but against a **local synthetic
//! node** created by this process. It does **not** request portal consent,
//! read user windows, or prove that per-window portal capture works.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const FRAME_COUNT: usize = 5;
const PRODUCER_BUFFERS: usize = 150;
const WAIT: Duration = Duration::from_secs(5);
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
static NEXT_NODE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize)]
pub struct PipeWireLoopbackReport {
    pub node_name: String,
    pub frames: usize,
    pub width: i32,
    pub height: i32,
    pub bytes: usize,
    pub first_frame_ms: u128,
    pub elapsed_ms: u128,
    /// Always false: this diagnostic never requests the desktop portal.
    pub portal_consent: bool,
    pub verified: bool,
}

fn build_pipeline(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|error| format!("cannot construct pipeline: {error}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned an unexpected element".to_string())
}

fn node_name() -> String {
    format!(
        "vista-pw-loopback-{}-{}",
        std::process::id(),
        NEXT_NODE.fetch_add(1, Ordering::Relaxed)
    )
}

fn remaining(deadline: Instant) -> gst::ClockTime {
    gst::ClockTime::from_nseconds(
        u64::try_from(
            deadline
                .saturating_duration_since(Instant::now())
                .as_nanos(),
        )
        .unwrap_or(u64::MAX),
    )
}

fn validate_frame(sample: &gst::Sample) -> Result<(i32, i32, usize), String> {
    let caps = sample.caps().ok_or("PipeWire sample missing caps")?;
    let structure = caps.structure(0).ok_or("PipeWire caps missing structure")?;
    if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA") {
        return Err("PipeWire sample is not RGBA video".into());
    }
    let width = structure
        .get::<i32>("width")
        .map_err(|_| "PipeWire frame missing width")?;
    let height = structure
        .get::<i32>("height")
        .map_err(|_| "PipeWire frame missing height")?;
    if width != WIDTH as i32 || height != HEIGHT as i32 {
        return Err(format!("unexpected PipeWire dimensions: {width}x{height}"));
    }
    let bytes = sample
        .buffer()
        .ok_or("PipeWire frame missing buffer")?
        .size();
    let expected = (WIDTH as usize) * (HEIGHT as usize) * 4;
    if bytes < expected || bytes > MAX_FRAME_BYTES {
        return Err(format!("invalid PipeWire frame size: {bytes}"));
    }
    Ok((width, height, bytes))
}

/// Creates a local `videotestsrc` → `pipewiresink` node and reads it back with
/// `pipewiresrc`, validating RGBA caps and frame bounds. No portal, no real
/// window content and no network access.
pub fn pipewire_loopback() -> Result<PipeWireLoopbackReport, String> {
    gst::init().map_err(|error| format!("GStreamer initialization: {error}"))?;
    for element in [
        "videotestsrc",
        "videoconvert",
        "pipewiresink",
        "pipewiresrc",
        "appsink",
    ] {
        if gst::ElementFactory::find(element).is_none() {
            return Err(format!(
                "required GStreamer element is unavailable: {element}"
            ));
        }
    }

    let node = node_name();
    let producer = build_pipeline(&format!(
        "videotestsrc is-live=true num-buffers={PRODUCER_BUFFERS} ! \
         video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT},framerate=30/1 ! \
         pipewiresink mode=provide stream-properties=\"props,node.name={node}\""
    ))?;
    let consumer = build_pipeline(&format!(
        "pipewiresrc target-object={node} num-buffers={FRAME_COUNT} do-timestamp=true ! \
         videoconvert ! video/x-raw,format=RGBA ! \
         appsink name=loopback_sink sync=false max-buffers=2 drop=false"
    ))?;

    let result = (|| {
        producer
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start PipeWire test producer: {error}"))?;
        // Give the sink time to publish the node before the source connects.
        std::thread::sleep(Duration::from_millis(500));
        consumer
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start PipeWire test consumer: {error}"))?;
        let sink = consumer
            .by_name("loopback_sink")
            .ok_or("missing loopback_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "loopback_sink is not an AppSink")?;

        let started = Instant::now();
        let deadline = started + WAIT;
        let mut frames = 0;
        let mut bytes = 0;
        let mut first_frame_ms = 0;
        while frames < FRAME_COUNT && Instant::now() < deadline {
            let sample = sink
                .try_pull_sample(remaining(deadline))
                .ok_or("PipeWire loopback produced no frame before the deadline")?;
            let (_, _, frame_bytes) = validate_frame(&sample)?;
            if frames == 0 {
                first_frame_ms = started.elapsed().as_millis();
            }
            frames += 1;
            bytes += frame_bytes;
        }
        if frames != FRAME_COUNT {
            return Err(format!(
                "PipeWire loopback delivered {frames}/{FRAME_COUNT} frames"
            ));
        }
        Ok(PipeWireLoopbackReport {
            node_name: node.clone(),
            frames,
            width: WIDTH as i32,
            height: HEIGHT as i32,
            bytes,
            first_frame_ms,
            elapsed_ms: started.elapsed().as_millis(),
            portal_consent: false,
            verified: true,
        })
    })();

    let consumer_stop = consumer.set_state(gst::State::Null);
    let producer_stop = producer.set_state(gst::State::Null);
    match (result, consumer_stop, producer_stop) {
        (Ok(report), Ok(_), Ok(_)) => Ok(report),
        (Err(error), Ok(_), Ok(_)) => Err(error),
        (result, consumer, producer) => Err(format!(
            "PipeWire loopback cleanup failed (consumer={consumer:?}, producer={producer:?}); prior result={result:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PipeWire and the GStreamer pipewiresrc/pipewiresink elements"]
    fn local_pipewire_loopback_delivers_frames() {
        let report = pipewire_loopback().unwrap();
        assert!(report.verified);
        assert_eq!(report.frames, FRAME_COUNT);
        assert_eq!((report.width, report.height), (WIDTH as i32, HEIGHT as i32));
        assert!(report.bytes > 0);
        assert!(!report.portal_consent);
    }
}
