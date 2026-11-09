//! Bounded, explicitly consented PipeWire window → H.264 diagnostic.
//! Never persists pixels or streams content to a network endpoint.
use crate::{
    lifecycle::WindowWatcher,
    media::{H264Encoder, probe_encoders},
    session::{FrameGeometry, H264AccessUnit, H264Discontinuity, SessionToken, StreamSession},
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

const POLL: Duration = Duration::from_millis(50);
pub const DEFAULT_SECONDS: u64 = 5;
pub const MAX_SECONDS: u64 = 120;
const FRAME_RATE: u64 = 30;
const MAX_FRAMES: usize = 3_600;
const MAX_FRAME_DIMENSION: i32 = 8192;

/// Bounded live-capture run configuration. The frame cap and deadline are
/// independent so a stalled source cannot exceed either bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureRun {
    pub seconds: u64,
    pub keyframe_at: Option<usize>,
}

impl Default for CaptureRun {
    fn default() -> Self {
        Self {
            seconds: DEFAULT_SECONDS,
            keyframe_at: None,
        }
    }
}

impl CaptureRun {
    pub fn new(seconds: u64, keyframe_at: Option<usize>) -> Result<Self, String> {
        if seconds == 0 || seconds > MAX_SECONDS {
            return Err(format!("--seconds must be 1..={MAX_SECONDS}"));
        }
        let max_frames = Self::max_frames_for(seconds);
        if let Some(frame) = keyframe_at
            && (frame == 0 || frame >= max_frames)
        {
            return Err(format!(
                "--keyframe-at must be 1..{} for a {seconds}-second run",
                max_frames.saturating_sub(1)
            ));
        }
        Ok(Self {
            seconds,
            keyframe_at,
        })
    }

    pub fn max_frames(&self) -> usize {
        Self::max_frames_for(self.seconds)
    }

    fn max_frames_for(seconds: u64) -> usize {
        usize::try_from(seconds.saturating_mul(FRAME_RATE))
            .unwrap_or(MAX_FRAMES)
            .min(MAX_FRAMES)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LatencySummary {
    pub samples: usize,
    pub min_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl LatencySummary {
    pub fn from_samples(mut samples: Vec<f64>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort_by(f64::total_cmp);
        let index = |percent: f64| {
            let last = samples.len() - 1;
            let position = ((samples.len() as f64 - 1.0) * percent).round() as usize;
            samples[position.min(last)]
        };
        Self {
            samples: samples.len(),
            min_ms: samples[0],
            p50_ms: index(0.50),
            p95_ms: index(0.95),
            max_ms: samples[samples.len() - 1],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LiveBridgeReport {
    pub portal_session_started: bool,
    pub pipewire_node_id: u32,
    pub encoder: H264Encoder,
    pub input_frames: usize,
    pub input_width: u32,
    pub input_height: u32,
    pub encoded_buffers: usize,
    pub encoded_bytes: usize,
    pub dropped_frames: u64,
    pub dropped_access_units: u64,
    pub elapsed_ms: u128,
    pub seconds: u64,
    pub max_frames: usize,
    pub keyframe_requests: usize,
    pub keyframes_emitted: usize,
    /// Raw-sample pull to encoded-buffer availability, including pipeline
    /// startup in the first samples. This is not network or decoder latency.
    pub capture_to_encode_ms: LatencySummary,
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

struct RgbaFrame {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn validate_rgba(sample: &gst::Sample) -> Result<RgbaFrame, String> {
    let caps = sample.caps().ok_or("raw sample missing caps")?;
    let structure = caps.structure(0).ok_or("raw caps missing structure")?;
    let width = structure
        .get::<i32>("width")
        .map_err(|_| "raw sample missing width")?;
    let height = structure
        .get::<i32>("height")
        .map_err(|_| "raw sample missing height")?;
    if structure.name() != "video/x-raw"
        || structure.get::<&str>("format").ok() != Some("RGBA")
        || width <= 0
        || height <= 0
        || width > MAX_FRAME_DIMENSION
        || height > MAX_FRAME_DIMENSION
    {
        return Err("unexpected raw sample format or geometry".into());
    }
    let width = u32::try_from(width).map_err(|_| "invalid raw width")?;
    let height = u32::try_from(height).map_err(|_| "invalid raw height")?;
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
    Ok(RgbaFrame {
        bytes: pack_rgba_rows(plane, stride, width, height)?,
        width,
        height,
    })
}

fn raw_caps(width: u32, height: u32) -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", "RGBA")
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(FRAME_RATE as i32, 1))
        .build()
}

/// GStreamer often reports source/codec failures asynchronously on the bus
/// rather than from set_state or AppSink. Drain pending diagnostics before
/// treating an empty source as timeout or returning a successful report.
fn check_pipeline_error(pipeline: &gst::Pipeline, label: &str) -> Result<(), String> {
    let bus = pipeline
        .bus()
        .ok_or_else(|| format!("{label} pipeline has no message bus"))?;
    while let Some(message) = bus.pop() {
        if let gst::MessageView::Error(error) = message.view() {
            let details = error
                .debug()
                .map(|debug| format!("; {debug}"))
                .unwrap_or_default();
            return Err(format!(
                "{label} pipeline error: {}{details}",
                error.error()
            ));
        }
    }
    Ok(())
}

fn check_capture_errors(source: &gst::Pipeline, target: &gst::Pipeline) -> Result<(), String> {
    check_pipeline_error(source, "capture source")?;
    check_pipeline_error(target, "H.264 encoder")
}

fn collect_encoded(
    sample: &gst::Sample,
    session: &mut StreamSession,
    token: SessionToken,
    buffers: &mut usize,
    bytes: &mut usize,
    keyframes: &mut usize,
) -> Result<(), String> {
    let caps = sample.caps().ok_or("encoded sample missing caps")?;
    let structure = caps
        .structure(0)
        .ok_or("encoded sample missing structure")?;
    if structure.name() != "video/x-h264"
        || structure.get::<&str>("stream-format").ok() != Some("byte-stream")
        || structure.get::<&str>("alignment").ok() != Some("au")
    {
        return Err("encoded sample is not byte-stream, AU-aligned H.264".into());
    }
    let buffer = sample.buffer().ok_or("encoded sample missing buffer")?;
    let mapped = buffer
        .map_readable()
        .map_err(|_| "cannot map H.264 sample")?;
    let len = mapped.size();
    if len == 0 || len > 16 * 1024 * 1024 {
        return Err("invalid H.264 sample length".into());
    }
    if buffer.flags().contains(gst::BufferFlags::DISCONT) {
        session
            .mark_h264_discontinuity(token, H264Discontinuity::Pipeline)
            .map_err(|e| format!("encoded session rejected discontinuity: {e:?}"))?;
    }
    let is_keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
    session
        .enqueue_h264(
            token,
            H264AccessUnit {
                pts_ns: buffer.pts().map(|pts| pts.nseconds()),
                is_keyframe,
                bytes: mapped.as_slice().to_vec(),
            },
        )
        .map_err(|e| format!("encoded session rejected access unit: {e:?}"))?;
    if is_keyframe {
        *keyframes += 1;
    }
    *buffers += 1;
    *bytes += len;
    Ok(())
}

fn drain_encoded(
    sink: &gst_app::AppSink,
    session: &mut StreamSession,
    token: SessionToken,
    buffers: &mut usize,
    bytes: &mut usize,
    keyframes: &mut usize,
) -> Result<(), String> {
    while let Some(sample) = sink.try_pull_sample(gst::ClockTime::ZERO) {
        collect_encoded(&sample, session, token, buffers, bytes, keyframes)?;
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
    watcher: Option<&WindowWatcher>,
    run: CaptureRun,
) -> Result<LiveBridgeReport, String> {
    let target = pipeline(&format!(
        "appsrc name=raw_source is-live=true format=time caps=video/x-raw,format=RGBA,framerate=30/1 ! queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} {} ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au ! appsink name=encoded_sink sync=false max-buffers=2 drop=false",
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
        let deadline = start + Duration::from_secs(run.seconds);
        source
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start source: {e}"))?;
        let mut input_frames = 0;
        let mut encoded_buffers = 0;
        let mut encoded_bytes = 0;
        let mut keyframe_requests = 0;
        let mut keyframes_emitted = 0;
        let mut capture_latency = Vec::new();
        let mut geometry: Option<FrameGeometry> = None;
        let mut input_width = 0u32;
        let mut input_height = 0u32;
        while input_frames < run.max_frames() && Instant::now() < deadline {
            if let Some(reason) = interruption(cancelled, watcher) {
                session.revoke();
                return Err(reason);
            }
            if run.keyframe_at == Some(input_frames) && keyframe_requests == 0 {
                let event = gst_video::DownstreamForceKeyUnitEvent::builder()
                    .all_headers(true)
                    .build();
                if !appsrc.send_event(event) {
                    return Err("encoder rejected force-key-unit event".into());
                }
                session
                    .mark_h264_discontinuity(token, H264Discontinuity::ReceiverReset)
                    .map_err(|e| format!("session rejected keyframe request: {e:?}"))?;
                keyframe_requests += 1;
            }
            check_capture_errors(source, &target)?;
            let wait = deadline.saturating_duration_since(Instant::now()).min(POLL);
            let ns = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            if let Some(sample) = raw_sink.try_pull_sample(gst::ClockTime::from_nseconds(ns)) {
                let pulled_at = Instant::now();
                let frame = validate_rgba(&sample)?;
                let frame_geometry = FrameGeometry {
                    width: frame.width,
                    height: frame.height,
                };
                if geometry != Some(frame_geometry) {
                    if geometry.is_some() {
                        session
                            .resize(token, frame_geometry)
                            .map_err(|e| format!("{e:?}"))?;
                    } else {
                        target
                            .set_state(gst::State::Playing)
                            .map_err(|e| format!("cannot start encoder: {e}"))?;
                    }
                    appsrc.set_caps(Some(&raw_caps(frame.width, frame.height)));
                    geometry = Some(frame_geometry);
                    input_width = frame.width;
                    input_height = frame.height;
                }
                session
                    .enqueue_rgba(token, frame_geometry, frame.bytes)
                    .map_err(|e| format!("{e:?}"))?;
                input_frames += 1;
                if let Some(reason) = interruption(cancelled, watcher) {
                    session.revoke();
                    return Err(reason);
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
                let before = encoded_buffers;
                drain_encoded(
                    &encoded_sink,
                    &mut session,
                    token,
                    &mut encoded_buffers,
                    &mut encoded_bytes,
                    &mut keyframes_emitted,
                )?;
                if encoded_buffers > before {
                    capture_latency.push(pulled_at.elapsed().as_secs_f64() * 1000.0);
                }
            } else if raw_sink.is_eos() {
                break;
            }
            check_capture_errors(source, &target)?;
        }
        if let Some(reason) = interruption(cancelled, watcher) {
            session.revoke();
            return Err(reason);
        }
        check_capture_errors(source, &target)?;
        appsrc
            .end_of_stream()
            .map_err(|e| format!("cannot signal encoder EOS: {e}"))?;
        // Encoding may lag the capture deadline; give bounded time to drain.
        let drain_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < drain_deadline && !encoded_sink.is_eos() {
            if let Some(reason) = interruption(cancelled, watcher) {
                session.revoke();
                return Err(reason);
            }
            check_capture_errors(source, &target)?;
            let wait = drain_deadline
                .saturating_duration_since(Instant::now())
                .min(POLL);
            let ns = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            if let Some(sample) = encoded_sink.try_pull_sample(gst::ClockTime::from_nseconds(ns)) {
                collect_encoded(
                    &sample,
                    &mut session,
                    token,
                    &mut encoded_buffers,
                    &mut encoded_bytes,
                    &mut keyframes_emitted,
                )?;
            }
        }
        drain_encoded(
            &encoded_sink,
            &mut session,
            token,
            &mut encoded_buffers,
            &mut encoded_bytes,
            &mut keyframes_emitted,
        )?;
        check_capture_errors(source, &target)?;
        if input_frames == 0 || encoded_buffers == 0 {
            return Err("no frames encoded from selected source".into());
        }
        if !encoded_sink.is_eos() {
            return Err("encoder did not finish before capture deadline".into());
        }
        // This diagnostic does not have a transport consumer. Release all
        // transient encoded buffers before returning the aggregate report.
        while session
            .dequeue_h264(token)
            .map_err(|e| format!("{e:?}"))?
            .is_some()
        {}
        let report = LiveBridgeReport {
            portal_session_started: true,
            pipewire_node_id: node,
            encoder,
            input_frames,
            input_width,
            input_height,
            encoded_buffers,
            encoded_bytes,
            dropped_frames: session.dropped_frames(),
            dropped_access_units: session.dropped_access_units(),
            elapsed_ms: start.elapsed().as_millis(),
            seconds: run.seconds,
            max_frames: run.max_frames(),
            keyframe_requests,
            keyframes_emitted,
            capture_to_encode_ms: LatencySummary::from_samples(capture_latency),
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

fn interruption(cancelled: &AtomicBool, watcher: Option<&WindowWatcher>) -> Option<String> {
    if cancelled.load(Ordering::Acquire) {
        return Some("portal session closed during capture".into());
    }
    if let Some(watcher) = watcher
        && watcher.is_cancelled()
    {
        return Some(format!(
            "target window lifecycle ended ({})",
            watcher.reason().unwrap_or("unknown")
        ));
    }
    None
}

fn run_worker(
    fd: OwnedFd,
    node: u32,
    cancelled: &AtomicBool,
    window_id: Option<u64>,
    run: CaptureRun,
) -> Result<LiveBridgeReport, String> {
    gst::init().map_err(|e| format!("GStreamer initialization: {e}"))?;
    let caps = probe_encoders()?;
    if !caps.h264_parser {
        return Err("h264parse plugin unavailable".into());
    }
    // The watcher is opt-in; without a Niri window id this diagnostic keeps its
    // previous portal-closure-only behavior.
    let watcher = match window_id {
        Some(id) => Some(WindowWatcher::start(id)?),
        None => None,
    };
    // The PipeWire FD remains open for the complete lifetime of the source pipeline.
    let source = pipeline(&format!(
        "pipewiresrc fd={} target-object={node} do-timestamp=true ! videoconvert ! video/x-raw,format=RGBA ! appsink name=raw_sink sync=false max-buffers=2 drop=true",
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
        if let Some(watcher) = watcher.as_ref()
            && watcher.is_cancelled()
        {
            return Err(format!(
                "target window lifecycle ended ({})",
                watcher.reason().unwrap_or("unknown")
            ));
        }
        match bridge_pipeline(&source, encoder, node, cancelled, watcher.as_ref(), run) {
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
    capture_h264_probe_for_window(None).await
}

/// Same diagnostic, optionally watching one Niri window id so removal or a
/// compositor restart terminates capture instead of waiting for a timeout.
pub async fn capture_h264_probe_for_window(
    window_id: Option<u64>,
) -> Result<LiveBridgeReport, String> {
    capture_h264_probe_with(window_id, CaptureRun::default()).await
}

/// Bounded run with explicit duration and an optional keyframe request at a
/// given input-frame index.
pub async fn capture_h264_probe_with(
    window_id: Option<u64>,
    run: CaptureRun,
) -> Result<LiveBridgeReport, String> {
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
        // Some portal implementations do not answer the signal subscription; a
        // bounded wait keeps capture usable while reporting the reduced coverage.
        let closed = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            session.receive_closed(),
        )
        .await
        {
            Ok(Ok(stream)) => Some(stream),
            Ok(Err(error)) => {
                return Err(format!("cannot subscribe to portal session closure: {error}"));
            }
            Err(_) => {
                eprintln!(
                    "portal did not answer the Closed signal subscription within 2 s; continuing without portal-revocation monitoring for this run"
                );
                None
            }
        };
        let worker =
            tokio::task::spawn_blocking(move || run_worker(fd, node, &worker_flag, window_id, run));
        tokio::pin!(worker);
        match closed {
            Some(closed) => {
                tokio::pin!(closed);
                tokio::select! {
                    // If a frame result and revocation become ready together, never
                    // claim successful capture after the portal has closed the grant.
                    biased;
                    _ = closed.next() => {
                        cancelled.store(true, Ordering::Release);
                        let _ = worker.await;
                        Err("portal closed the authorized window session".into())
                    }
                    result = &mut worker => result.map_err(|e| format!("capture worker panicked: {e}"))?,
                }
            }
            None => worker
                .await
                .map_err(|e| format!("capture worker panicked: {e}"))?,
        }
    }
    .await;
    let close = match tokio::time::timeout(std::time::Duration::from_secs(2), session.close()).await
    {
        Ok(result) => Some(result.map_err(|e| e.to_string())),
        Err(_) => {
            eprintln!(
                "portal did not acknowledge session close within 2 s; the grant may be released by the portal after this diagnostic exits"
            );
            None
        }
    };
    match (result, close) {
        (Ok(report), None) => Ok(report),
        (Ok(report), Some(Ok(()))) => Ok(report),
        (Err(err), _) => Err(err),
        (Ok(_), Some(Err(err))) => Err(format!("could not close portal session: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_discont_is_propagated_to_stream_continuity() {
        gst::init().unwrap();
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        let (before, _) = session.h264_continuity(token).unwrap();
        let caps = gst::Caps::builder("video/x-h264")
            .field("stream-format", "byte-stream")
            .field("alignment", "au")
            .build();
        let mut buffer = gst::Buffer::from_mut_slice(vec![0, 0, 0, 1, 0x67, 0x42]);
        buffer
            .get_mut()
            .unwrap()
            .set_flags(gst::BufferFlags::DISCONT | gst::BufferFlags::DELTA_UNIT);
        let sample = gst::Sample::builder().buffer(&buffer).caps(&caps).build();
        let (mut buffers, mut bytes, mut keyframes) = (0, 0, 0);
        collect_encoded(
            &sample,
            &mut session,
            token,
            &mut buffers,
            &mut bytes,
            &mut keyframes,
        )
        .unwrap();
        assert_eq!(buffers, 1);
        assert_eq!(bytes, 6);
        assert_eq!(keyframes, 0);
        assert_eq!(session.queued_access_units(), 1);
        assert_eq!(
            session.h264_continuity(token).unwrap(),
            (before + 1, H264Discontinuity::Pipeline)
        );
    }

    #[test]
    fn latency_summary_sorts_and_selects_percentiles() {
        assert_eq!(LatencySummary::from_samples(Vec::new()).samples, 0);
        let summary = LatencySummary::from_samples(vec![5.0, 1.0, 3.0, 2.0, 4.0]);
        assert_eq!(summary.samples, 5);
        assert_eq!(summary.min_ms, 1.0);
        assert_eq!(summary.p50_ms, 3.0);
        assert_eq!(summary.p95_ms, 5.0);
        assert_eq!(summary.max_ms, 5.0);
        let single = LatencySummary::from_samples(vec![7.5]);
        assert_eq!(single.p50_ms, 7.5);
        assert_eq!(single.p95_ms, 7.5);
    }

    #[test]
    fn capture_run_bounds_seconds_and_keyframe_index() {
        assert!(CaptureRun::new(0, None).is_err());
        assert!(CaptureRun::new(MAX_SECONDS + 1, None).is_err());
        assert!(CaptureRun::new(1, Some(0)).is_err());
        assert!(CaptureRun::new(1, Some(30)).is_err());
        let run = CaptureRun::new(2, Some(1)).unwrap();
        assert_eq!(run.max_frames(), 60);
        assert_eq!(run.seconds, 2);
        assert_eq!(CaptureRun::default().seconds, DEFAULT_SECONDS);
        assert_eq!(
            CaptureRun::new(MAX_SECONDS, None).unwrap().max_frames(),
            MAX_FRAMES
        );
    }

    #[test]
    fn keyframe_flagged_sample_is_counted() {
        gst::init().unwrap();
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        let caps = gst::Caps::builder("video/x-h264")
            .field("stream-format", "byte-stream")
            .field("alignment", "au")
            .build();
        let buffer = gst::Buffer::from_mut_slice(vec![0, 0, 0, 1, 0x67, 0x42]);
        let sample = gst::Sample::builder().buffer(&buffer).caps(&caps).build();
        let (mut buffers, mut bytes, mut keyframes) = (0, 0, 0);
        collect_encoded(
            &sample,
            &mut session,
            token,
            &mut buffers,
            &mut bytes,
            &mut keyframes,
        )
        .unwrap();
        assert_eq!(keyframes, 1);
    }

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
    fn accepts_native_rgba_geometry_and_rejects_bad_format() {
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
        let frame = validate_rgba(&sample).unwrap();
        assert_eq!((frame.width, frame.height), (64, 64));
        assert_eq!(frame.bytes.len(), 64 * 64 * 4);

        let bad_format = gst::Caps::builder("video/x-raw")
            .field("format", "RGB")
            .field("width", 64i32)
            .field("height", 64i32)
            .build();
        let sample = gst::Sample::builder()
            .buffer(&gst::Buffer::with_size(64 * 64 * 3).unwrap())
            .caps(&bad_format)
            .build();
        assert!(validate_rgba(&sample).is_err());

        let zero = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", 0i32)
            .field("height", 0i32)
            .build();
        let sample = gst::Sample::builder()
            .buffer(&gst::Buffer::with_size(0).unwrap())
            .caps(&zero)
            .build();
        assert!(validate_rgba(&sample).is_err());
    }
    #[test]
    fn asynchronous_gstreamer_fault_is_reported_before_timeout() {
        gst::init().unwrap();
        let source =
            pipeline("videotestsrc num-buffers=3 ! identity error-after=1 ! fakesink sync=false")
                .unwrap();
        source.set_state(gst::State::Playing).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let error = loop {
            if let Err(error) = check_pipeline_error(&source, "capture source") {
                break error;
            }
            assert!(Instant::now() < deadline, "expected a GStreamer bus error");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(error.contains("capture source pipeline error"), "{error}");
        source.set_state(gst::State::Null).unwrap();
        assert_eq!(source.current_state(), gst::State::Null);
    }

    #[test]
    #[ignore = "requires functioning H.264 encoder plugins"]
    fn failing_synthetic_capture_reports_bus_error_and_cleans_up() {
        gst::init().unwrap();
        let caps = probe_encoders().unwrap();
        let encoder = [
            (caps.nvidia, H264Encoder::Nvidia),
            (caps.x264, H264Encoder::X264),
            (caps.openh264, H264Encoder::OpenH264),
        ]
        .into_iter()
        .find_map(|(available, e)| available.then_some(e))
        .expect("a functional encoder is required");
        let source = pipeline(
            "videotestsrc num-buffers=8 is-live=true ! identity error-after=1 ! videoconvert ! video/x-raw,format=RGBA,width=320,height=180 ! appsink name=raw_sink sync=false max-buffers=2 drop=true",
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let error = bridge_pipeline(&source, encoder, 0, &cancelled, None, CaptureRun::default())
            .unwrap_err();
        assert!(error.contains("capture source pipeline error"), "{error}");
        assert_eq!(source.current_state(), gst::State::Null);
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
        let result =
            bridge_pipeline(&source, encoder, 0, &cancelled, None, CaptureRun::default()).unwrap();
        assert_eq!(result.input_frames, 8);
        assert!(result.encoded_buffers > 0);
    }
    #[test]
    #[ignore = "requires functioning H.264 encoder plugins"]
    fn cancellation_interrupts_a_stalled_source_and_cleans_up() {
        gst::init().unwrap();
        let caps = probe_encoders().unwrap();
        let encoder = [
            (caps.nvidia, H264Encoder::Nvidia),
            (caps.x264, H264Encoder::X264),
            (caps.openh264, H264Encoder::OpenH264),
        ]
        .into_iter()
        .find_map(|(available, e)| available.then_some(e))
        .expect("a functional encoder is required");
        let source = pipeline(
            "appsrc is-live=true format=time caps=video/x-raw,format=RGBA,width=320,height=180,framerate=30/1 ! appsink name=raw_sink sync=false max-buffers=2 drop=true",
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Release);
            });
            let start = Instant::now();
            let error =
                bridge_pipeline(&source, encoder, 0, &cancelled, None, CaptureRun::default())
                    .unwrap_err();
            assert!(error.contains("portal session closed"), "{error}");
            assert!(start.elapsed() < Duration::from_secs(2));
        });
        assert_eq!(source.current_state(), gst::State::Null);
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
        let error = bridge_pipeline(&source, encoder, 0, &cancelled, None, CaptureRun::default())
            .unwrap_err();
        assert!(error.contains("portal session closed"), "{error}");
        assert_eq!(source.current_state(), gst::State::Null);
    }
}
