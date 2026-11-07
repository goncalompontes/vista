//! Bounded, explicitly consented PipeWire window → H.264 diagnostic.
//! Never persists pixels or streams content to a network endpoint.
use crate::{
    media::{H264Encoder, probe_encoders},
    session::{FrameGeometry, StreamSession},
};
use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use futures_util::StreamExt;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use serde::Serialize;
use std::{
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_FRAMES: usize = 8;
const MAX_DURATION: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);
// A fixed-size encoder input keeps memory use bounded and prevents mixed caps on resize.
// The source is explicitly scaled; this is not a quality or native-resolution benchmark.
const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

#[derive(Debug, Serialize)]
pub struct LiveBridgeReport {
    pub portal_session_started: bool,
    pub pipewire_node_id: u32,
    pub encoder: H264Encoder,
    pub input_frames: usize,
    pub encoded_buffers: usize,
    pub encoded_bytes: usize,
    pub dropped_frames: u64,
    pub elapsed_ms: u128,
    pub verified: bool,
    pub arbitrary_window_capture_verified: bool,
}

fn pipeline(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|e| format!("cannot construct pipeline: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer did not construct a pipeline".to_string())
}

fn encode_settings(encoder: H264Encoder) -> &'static str {
    match encoder {
        H264Encoder::Nvidia => "preset=p1 bframes=0 bitrate=3000",
        H264Encoder::X264 => "tune=zerolatency speed-preset=ultrafast bitrate=3000 bframes=0",
        H264Encoder::OpenH264 => "bitrate=3000000",
    }
}

fn pack_rgba_rows(plane: &[u8], stride: usize, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let row_bytes = usize::try_from(width)
        .map_err(|_| "width overflow")?
        .checked_mul(4)
        .ok_or("row size overflow")?;
    let rows = usize::try_from(height).map_err(|_| "height overflow")?;
    let total = row_bytes.checked_mul(rows).ok_or("frame size overflow")?;
    if row_bytes == 0 || rows == 0 || total > 256 * 1024 * 1024 || stride < row_bytes {
        return Err("invalid RGBA geometry or row stride".into());
    }
    let required = (rows - 1)
        .checked_mul(stride)
        .and_then(|v| v.checked_add(row_bytes))
        .ok_or("RGBA layout overflow")?;
    if plane.len() < required {
        return Err("truncated RGBA plane".into());
    }
    let mut packed = vec![0; total];
    for (row, output) in packed.chunks_exact_mut(row_bytes).enumerate() {
        let start = row * stride;
        output.copy_from_slice(&plane[start..start + row_bytes]);
    }
    Ok(packed)
}

fn validate_rgba(sample: &gst::Sample) -> Result<Vec<u8>, String> {
    let caps = sample.caps().ok_or("raw sample missing caps")?;
    let structure = caps.structure(0).ok_or("raw caps missing structure")?;
    if structure.name() != "video/x-raw"
        || structure.get::<&str>("format").ok() != Some("RGBA")
        || structure.get::<i32>("width").ok() != Some(WIDTH as i32)
        || structure.get::<i32>("height").ok() != Some(HEIGHT as i32)
    {
        return Err("unexpected raw sample format or geometry".into());
    }
    let buffer = sample.buffer().ok_or("raw sample missing buffer")?;
    let info = gst_video::VideoInfo::from_caps(caps)
        .map_err(|e| format!("invalid RGBA video layout: {e}"))?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
        .map_err(|e| format!("cannot map RGBA video frame: {e}"))?;
    let stride =
        usize::try_from(info.stride()[0]).map_err(|_| "negative RGBA row stride is unsupported")?;
    let plane = frame
        .plane_data(0)
        .map_err(|e| format!("cannot access RGBA plane: {e}"))?;
    pack_rgba_rows(plane, stride, WIDTH, HEIGHT)
}

fn drain_encoded(
    sink: &gst_app::AppSink,
    buffers: &mut usize,
    bytes: &mut usize,
) -> Result<(), String> {
    while let Some(sample) = sink.try_pull_sample(gst::ClockTime::ZERO) {
        let caps = sample.caps().ok_or("encoded sample missing caps")?;
        if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
            return Err("encoded sample is not H.264".into());
        }
        let len = sample
            .buffer()
            .ok_or("encoded sample missing buffer")?
            .size();
        if len == 0 || len > 16 * 1024 * 1024 {
            return Err("invalid H.264 sample length".into());
        }
        *buffers += 1;
        *bytes += len;
    }
    Ok(())
}

/// Testable worker boundary: the caller must own a *live* window-only portal
/// grant before calling. The source is never started before session authorization.
fn bridge_pipeline(
    source: &gst::Pipeline,
    encoder: H264Encoder,
    node: u32,
    cancelled: &AtomicBool,
) -> Result<LiveBridgeReport, String> {
    let target = pipeline(&format!(
        "appsrc name=raw_source is-live=true format=time caps=video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT},framerate=30/1 ! queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} {} ! h264parse ! video/x-h264,alignment=au ! appsink name=encoded_sink sync=false max-buffers=2 drop=true",
        encoder.element_name(),
        encode_settings(encoder)
    ))?;
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
        let mut session = StreamSession::new();
        // Safe only because the caller has already negotiated exactly one portal
        // window source and opened its restricted PipeWire remote.
        let token = session.authorize().map_err(|e| format!("{e:?}"))?;
        session.start(token).map_err(|e| format!("{e:?}"))?;
        let start = Instant::now();
        let deadline = start + MAX_DURATION;
        target
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start encoder: {e}"))?;
        source
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start source: {e}"))?;
        let mut input_frames = 0;
        let mut encoded_buffers = 0;
        let mut encoded_bytes = 0;
        let geometry = FrameGeometry {
            width: WIDTH,
            height: HEIGHT,
        };
        while input_frames < MAX_FRAMES && Instant::now() < deadline {
            if cancelled.load(Ordering::Acquire) {
                session.revoke();
                return Err("portal session closed during capture".into());
            }
            let wait = deadline.saturating_duration_since(Instant::now()).min(POLL);
            let ns = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            if let Some(sample) = raw_sink.try_pull_sample(gst::ClockTime::from_nseconds(ns)) {
                let frame = validate_rgba(&sample)?;
                session
                    .enqueue_rgba(token, geometry, frame)
                    .map_err(|e| format!("{e:?}"))?;
                input_frames += 1;
                if cancelled.load(Ordering::Acquire) {
                    session.revoke();
                    return Err("portal session closed during capture".into());
                }
                if let Some(bytes) = session.dequeue(token).map_err(|e| format!("{e:?}"))? {
                    let mut buffer = gst::Buffer::from_mut_slice(bytes);
                    let writable = buffer.get_mut().ok_or("unwritable raw buffer")?;
                    let pts = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    writable.set_pts(gst::ClockTime::from_nseconds(pts));
                    appsrc
                        .push_buffer(buffer)
                        .map_err(|e| format!("encoder rejected frame: {e}"))?;
                }
                drain_encoded(&encoded_sink, &mut encoded_buffers, &mut encoded_bytes)?;
            } else if raw_sink.is_eos() {
                break;
            }
        }
        if cancelled.load(Ordering::Acquire) {
            session.revoke();
            return Err("portal session closed during capture".into());
        }
        appsrc
            .end_of_stream()
            .map_err(|e| format!("cannot signal encoder EOS: {e}"))?;
        // The overall deadline also bounds encoder draining.
        while Instant::now() < deadline && !encoded_sink.is_eos() {
            if cancelled.load(Ordering::Acquire) {
                session.revoke();
                return Err("portal session closed while encoding".into());
            }
            let wait = deadline.saturating_duration_since(Instant::now()).min(POLL);
            let ns = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            if let Some(sample) = encoded_sink.try_pull_sample(gst::ClockTime::from_nseconds(ns)) {
                let caps = sample.caps().ok_or("encoded sample missing caps")?;
                if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
                    return Err("encoded sample is not H.264".into());
                }
                let len = sample.buffer().ok_or("encoded buffer missing")?.size();
                if len == 0 || len > 16 * 1024 * 1024 {
                    return Err("invalid encoded H.264 size".into());
                }
                encoded_buffers += 1;
                encoded_bytes += len;
            }
        }
        drain_encoded(&encoded_sink, &mut encoded_buffers, &mut encoded_bytes)?;
        if input_frames == 0 || encoded_buffers == 0 {
            return Err("no frames encoded from selected source".into());
        }
        if !encoded_sink.is_eos() {
            return Err("encoder did not finish before capture deadline".into());
        }
        let report = LiveBridgeReport {
            portal_session_started: true,
            pipewire_node_id: node,
            encoder,
            input_frames,
            encoded_buffers,
            encoded_bytes,
            dropped_frames: session.dropped_frames(),
            elapsed_ms: start.elapsed().as_millis(),
            verified: true,
            arbitrary_window_capture_verified: false,
        };
        session.close();
        Ok(report)
    })();
    let stop_source = source.set_state(gst::State::Null);
    let stop_target = target.set_state(gst::State::Null);
    match (result, stop_source, stop_target) {
        (Ok(report), Ok(_), Ok(_)) => Ok(report),
        (Err(err), Ok(_), Ok(_)) => Err(err),
        (old, src, enc) => Err(format!(
            "pipeline shutdown failure: source={src:?}, encoder={enc:?}; result={old:?}"
        )),
    }
}

fn run_worker(fd: OwnedFd, node: u32, cancelled: &AtomicBool) -> Result<LiveBridgeReport, String> {
    gst::init().map_err(|e| format!("GStreamer initialization: {e}"))?;
    let caps = probe_encoders()?;
    if !caps.h264_parser {
        return Err("h264parse plugin unavailable".into());
    }
    // The PipeWire FD remains open for the complete lifetime of the source pipeline.
    let source = pipeline(&format!(
        "pipewiresrc fd={} target-object={node} do-timestamp=true ! videoconvert ! videoscale ! video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT} ! appsink name=raw_sink sync=false max-buffers=2 drop=true",
        fd.as_raw_fd()
    ))?;
    let mut failures = Vec::new();
    for (present, encoder) in [
        (caps.nvidia, H264Encoder::Nvidia),
        (caps.x264, H264Encoder::X264),
        (caps.openh264, H264Encoder::OpenH264),
    ] {
        if !present {
            continue;
        }
        if cancelled.load(Ordering::Acquire) {
            return Err("portal session closed before encoding".into());
        }
        match bridge_pipeline(&source, encoder, node, cancelled) {
            Ok(report) => return Ok(report),
            Err(err) if cancelled.load(Ordering::Acquire) => return Err(err),
            Err(err) => failures.push(format!("{}: {err}", encoder.element_name())),
        }
    }
    Err(format!(
        "no live encoder path succeeded: {}",
        failures.join("; ")
    ))
}

/// The caller must explicitly choose this operation (CLI requires --confirm).
/// The portal chooser selects exactly one window; permission is never persisted.
/// No encoded video bytes leave this process.
pub async fn capture_h264_probe() -> Result<LiveBridgeReport, String> {
    let portal = Screencast::new().await.map_err(|e| e.to_string())?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|e| e.to_string())?;
    let result = async {
        portal
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(enumflags2::BitFlags::from(SourceType::Window))
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await
            .map_err(|e| e.to_string())?;
        let response = portal
            .start(&session, None, Default::default())
            .await
            .map_err(|e| e.to_string())?
            .response()
            .map_err(|e| e.to_string())?;
        let streams = response.streams();
        if streams.len() != 1 || streams[0].source_type() != Some(SourceType::Window) {
            return Err("portal did not grant exactly one window".into());
        }
        let node = streams[0].pipe_wire_node_id();
        let fd = portal
            .open_pipe_wire_remote(&session, Default::default())
            .await
            .map_err(|e| e.to_string())?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_flag = cancelled.clone();
        // Subscribe before starting capture so permission revocation is observed.
        let closed = session.receive_closed().await.map_err(|e| e.to_string())?;
        let worker = tokio::task::spawn_blocking(move || run_worker(fd, node, &worker_flag));
        tokio::pin!(closed);
        tokio::pin!(worker);
        tokio::select! {
            result = &mut worker => result.map_err(|e| format!("capture worker panicked: {e}"))?,
            _ = closed.next() => {
                cancelled.store(true, Ordering::Release);
                let _ = worker.await;
                Err("portal closed the authorized window session".into())
            }
        }
    }
    .await;
    let close = session.close().await.map_err(|e| e.to_string());
    match (result, close) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(format!("could not close portal session: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_packing_preserves_pixels_and_discards_padding() {
        let input = [
            1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 88, 88,
        ];
        assert_eq!(
            pack_rgba_rows(&input, 10, 2, 2).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(
            pack_rgba_rows(&input[..18], 10, 2, 2).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }
    #[test]
    fn row_packing_rejects_truncation_overflow_and_invalid_stride() {
        assert!(pack_rgba_rows(&[0; 15], 8, 2, 2).is_err());
        assert!(pack_rgba_rows(&[0; 16], 7, 2, 2).is_err());
        assert!(pack_rgba_rows(&[], usize::MAX, 2, 2).is_err());
        assert!(pack_rgba_rows(&[], 0, 0, 1).is_err());
        assert!(pack_rgba_rows(&[], 0, u32::MAX, u32::MAX).is_err());
    }
    #[test]
    fn rejects_unexpected_raw_geometry() {
        gst::init().unwrap();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", 64i32)
            .field("height", 64i32)
            .build();
        let sample = gst::Sample::builder()
            .buffer(&gst::Buffer::with_size(64 * 64 * 4).unwrap())
            .caps(&caps)
            .build();
        assert!(validate_rgba(&sample).is_err());
    }
    #[test]
    #[ignore = "requires functioning H.264 encoder plugins"]
    fn synthetic_source_uses_same_worker_boundary() {
        gst::init().unwrap();
        let caps = probe_encoders().unwrap();
        let encoder = [
            (caps.nvidia, H264Encoder::Nvidia),
            (caps.x264, H264Encoder::X264),
            (caps.openh264, H264Encoder::OpenH264),
        ]
        .into_iter()
        .find_map(|(available, e)| available.then_some(e))
        .unwrap();
        let source = pipeline(
            "videotestsrc num-buffers=8 is-live=true ! videoconvert ! video/x-raw,format=RGBA,width=320,height=180 ! appsink name=raw_sink sync=false max-buffers=2 drop=true"
        ).unwrap();
        let cancelled = AtomicBool::new(false);
        let result = bridge_pipeline(&source, encoder, 0, &cancelled).unwrap();
        assert_eq!(result.input_frames, 8);
        assert!(result.encoded_buffers > 0);
    }
    #[test]
    fn cancelled_worker_does_not_produce_frames() {
        gst::init().unwrap();
        let caps = probe_encoders().unwrap();
        let Some(encoder) = [
            (caps.nvidia, H264Encoder::Nvidia),
            (caps.x264, H264Encoder::X264),
            (caps.openh264, H264Encoder::OpenH264),
        ]
        .into_iter()
        .find_map(|(available, e)| available.then_some(e)) else {
            return;
        };
        let source = pipeline(
            "videotestsrc num-buffers=8 is-live=true ! videoconvert ! video/x-raw,format=RGBA,width=320,height=180 ! appsink name=raw_sink sync=false max-buffers=2 drop=true"
        ).unwrap();
        let cancelled = AtomicBool::new(true);
        let error = bridge_pipeline(&source, encoder, 0, &cancelled).unwrap_err();
        assert!(error.contains("portal session closed"), "{error}");
        assert_eq!(source.current_state(), gst::State::Null);
    }
}
