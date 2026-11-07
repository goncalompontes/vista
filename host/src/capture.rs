//! Explicitly user-authorized ScreenCast diagnostics.
//!
//! The consent probe never reads pixels. The separate frame probe reads only
//! one in-memory PipeWire sample after the user approves the desktop chooser.
//! Neither diagnostic persists permissions or proves arbitrary Niri window-ID
//! capture; portal stream negotiation alone does not verify readable frames.

use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use serde::Serialize;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

const FRAME_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize)]
pub struct FrameMetadata {
    pub pipewire_node_id: u32,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub bytes: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct FrameBurstReport {
    pub portal_session_started: bool,
    pub frames: Vec<FrameMetadata>,
    pub resize_events: usize,
    pub elapsed_ms: u128,
    pub arbitrary_window_capture_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct FirstFrameReport {
    pub portal_session_started: bool,
    pub frames_captured: bool,
    pub frame: Option<FrameMetadata>,
    pub arbitrary_window_capture_verified: bool,
}

/// The caller must have explicitly opted in and the desktop portal must grant
/// access. Only inspects one in-memory sample; no pixels are written or sent.
pub async fn first_frame_probe() -> Result<FirstFrameReport, String> {
    let portal = Screencast::new().await.map_err(|e| e.to_string())?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|e| e.to_string())?;

    let capture_result = async {
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
        if streams.len() != 1 {
            return Err(format!(
                "expected one portal-selected window stream, got {}",
                streams.len()
            ));
        }
        let stream = &streams[0];
        if stream.source_type() != Some(SourceType::Window) {
            return Err(
                "portal did not identify the selection as a window; refusing capture".into(),
            );
        }
        let node_id = stream.pipe_wire_node_id();
        let fd = portal
            .open_pipe_wire_remote(&session, Default::default())
            .await
            .map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || sample_first_frame(fd, node_id))
            .await
            .map_err(|e| format!("capture worker failed: {e}"))?
    }
    .await;

    let close_result = session.close().await.map_err(|e| e.to_string());
    let frame = capture_result?;
    close_result?;
    Ok(FirstFrameReport {
        portal_session_started: true,
        frames_captured: true,
        frame: Some(frame),
        // A user-selected portal window is not equivalent to arbitrary Niri ID capture.
        arbitrary_window_capture_verified: false,
    })
}

/// Collects up to eight frames from a portal-selected window, keeping all pixels
/// transient and returning metadata only. A fresh chooser grant is required.
pub async fn frame_burst_probe() -> Result<FrameBurstReport, String> {
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
            return Err("portal did not grant exactly one window stream".into());
        }
        let node = streams[0].pipe_wire_node_id();
        let fd = portal
            .open_pipe_wire_remote(&session, Default::default())
            .await
            .map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || sample_frame_burst(fd, node))
            .await
            .map_err(|e| format!("capture worker failed: {e}"))?
    }
    .await;
    let close = session.close().await.map_err(|e| e.to_string());
    let report = result?;
    close?;
    Ok(report)
}

fn sample_frame_burst(fd: OwnedFd, node_id: u32) -> Result<FrameBurstReport, String> {
    gst::init().map_err(|e| format!("GStreamer initialization: {e}"))?;
    let description = format!(
        "pipewiresrc fd={} target-object={} do-timestamp=true ! videoconvert ! video/x-raw,format=RGBA ! appsink name=frame_sink sync=false max-buffers=2 drop=true",
        fd.as_raw_fd(),
        node_id
    );
    let element =
        gst::parse::launch(&description).map_err(|e| format!("GStreamer pipeline: {e}"))?;
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned a non-pipeline element".to_string())?;
    let started = Instant::now();
    let frames = read_frame_sequence(&pipeline, node_id, 8, Duration::from_secs(5))?;
    let resize_events = frames
        .windows(2)
        .filter(|pair| pair[0].width != pair[1].width || pair[0].height != pair[1].height)
        .count();
    Ok(FrameBurstReport {
        portal_session_started: true,
        frames,
        resize_events,
        elapsed_ms: started.elapsed().as_millis(),
        arbitrary_window_capture_verified: false,
    })
}

fn sample_first_frame(fd: OwnedFd, node_id: u32) -> Result<FrameMetadata, String> {
    gst::init().map_err(|e| format!("GStreamer initialization: {e}"))?;
    // Both interpolated values are integers from the kernel/portal, not user text.
    let description = format!(
        "pipewiresrc fd={} target-object={} num-buffers=1 do-timestamp=true ! videoconvert ! video/x-raw,format=RGBA ! appsink name=frame_sink sync=false max-buffers=1 drop=true",
        fd.as_raw_fd(),
        node_id
    );
    let element =
        gst::parse::launch(&description).map_err(|e| format!("GStreamer pipeline: {e}"))?;
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned a non-pipeline element".to_string())?;
    // The portal-owned FD stays alive through the pipeline shutdown.
    read_bounded_sample(&pipeline, node_id, FRAME_WAIT)
}

/// Testable GStreamer boundary shared by the restricted PipeWire source and
/// synthetic, permission-free sources. Always shuts the pipeline down.
fn read_bounded_sample(
    pipeline: &gst::Pipeline,
    node_id: u32,
    timeout: Duration,
) -> Result<FrameMetadata, String> {
    let result = (|| {
        let sink = pipeline
            .by_name("frame_sink")
            .ok_or_else(|| "frame_sink missing".to_string())?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "frame_sink has wrong type".to_string())?;
        let started = Instant::now();
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start capture pipeline: {e}"))?;
        let timeout_ns = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_nseconds(timeout_ns))
            .ok_or_else(|| format!("no video frame received within {} ms", timeout.as_millis()))?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| "received sample without buffer".to_string())?;
        let caps = sample
            .caps()
            .ok_or_else(|| "received sample without caps".to_string())?;
        let structure = caps
            .structure(0)
            .ok_or_else(|| "sample caps contain no structure".to_string())?;
        let width = structure.get::<i32>("width").ok();
        let height = structure.get::<i32>("height").ok();
        if width.is_none_or(|w| w <= 0) || height.is_none_or(|h| h <= 0) {
            return Err("received sample without valid video dimensions".to_string());
        }
        if buffer.size() == 0 {
            return Err("received empty video frame".to_string());
        }
        Ok(FrameMetadata {
            pipewire_node_id: node_id,
            width,
            height,
            bytes: buffer.size(),
            elapsed_ms: started.elapsed().as_millis(),
        })
    })();
    let shutdown = pipeline
        .set_state(gst::State::Null)
        .map_err(|e| format!("could not stop capture pipeline: {e}"));
    match (result, shutdown) {
        (Ok(metadata), Ok(_)) => Ok(metadata),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

/// Reads a bounded number of live samples without keeping pixel buffers.
/// A single overall deadline bounds the sampling loop (not portal negotiation).
fn read_frame_sequence(
    pipeline: &gst::Pipeline,
    node_id: u32,
    frame_limit: usize,
    timeout: Duration,
) -> Result<Vec<FrameMetadata>, String> {
    if frame_limit == 0 || frame_limit > 8 {
        return Err("frame limit must be between 1 and 8".into());
    }
    let result = (|| {
        let sink = pipeline
            .by_name("frame_sink")
            .ok_or_else(|| "frame_sink missing".to_string())?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "frame_sink has wrong type".to_string())?;
        let start = Instant::now();
        let deadline = start + timeout;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start capture pipeline: {e}"))?;
        let mut frames = Vec::with_capacity(frame_limit);
        while frames.len() < frame_limit {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "received {} of {frame_limit} frames within {} ms",
                    frames.len(),
                    timeout.as_millis()
                ));
            }
            let ns = u64::try_from(remaining.as_nanos()).unwrap_or(u64::MAX);
            let sample = sink
                .try_pull_sample(gst::ClockTime::from_nseconds(ns))
                .ok_or_else(|| {
                    format!(
                        "received {} of {frame_limit} frames within {} ms",
                        frames.len(),
                        timeout.as_millis()
                    )
                })?;
            let buffer = sample.buffer().ok_or("received sample without buffer")?;
            let caps = sample.caps().ok_or("received sample without caps")?;
            let structure = caps
                .structure(0)
                .ok_or("sample caps contain no structure")?;
            let width = structure.get::<i32>("width").ok();
            let height = structure.get::<i32>("height").ok();
            if width.is_none_or(|w| w <= 0) || height.is_none_or(|h| h <= 0) {
                return Err("received sample without valid dimensions".into());
            }
            if buffer.size() == 0 {
                return Err("received empty video frame".into());
            }
            frames.push(FrameMetadata {
                pipewire_node_id: node_id,
                width,
                height,
                bytes: buffer.size(),
                elapsed_ms: start.elapsed().as_millis(),
            });
        }
        Ok(frames)
    })();
    let shutdown = pipeline
        .set_state(gst::State::Null)
        .map_err(|e| format!("could not stop capture pipeline: {e}"));
    match (result, shutdown) {
        (Ok(frames), Ok(_)) => Ok(frames),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

#[derive(Debug, Serialize)]
pub struct NegotiatedStream {
    pub pipewire_node_id: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
}

#[derive(Debug, Serialize)]
pub struct ConsentProbeReport {
    pub portal_session_started: bool,
    pub streams: Vec<NegotiatedStream>,
    pub frames_captured: bool,
    pub arbitrary_window_capture_verified: bool,
}

/// May display a desktop portal dialog; only call for an explicit opt-in.
/// Restricts the portal's source selector to a single window, with no restore
/// token and no PipeWire connection. Explicitly closes the portal session.
pub async fn consent_probe() -> Result<ConsentProbeReport, ashpd::Error> {
    let portal = Screencast::new().await?;
    let session = portal.create_session(Default::default()).await?;
    // Always attempt to close the session, including cancellation and failures.
    let negotiation = async {
        portal
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(enumflags2::BitFlags::from(SourceType::Window))
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?;

        // The portal may reject this request, or the user can cancel the chooser.
        let response = portal
            .start(&session, None, Default::default())
            .await?
            .response()?;
        Ok::<_, ashpd::Error>(
            response
                .streams()
                .iter()
                .map(|stream| NegotiatedStream {
                    pipewire_node_id: stream.pipe_wire_node_id(),
                    size: stream.size(),
                    position: stream.position(),
                })
                .collect::<Vec<_>>(),
        )
    }
    .await;

    let close_result = session.close().await;
    let streams = negotiation?;
    close_result?;

    Ok(ConsentProbeReport {
        portal_session_started: true,
        streams,
        frames_captured: false,
        arbitrary_window_capture_verified: false,
    })
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn synthetic_source_produces_one_bounded_rgba_frame() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "videotestsrc num-buffers=1 ! video/x-raw,width=32,height=24,format=RGBA ! appsink name=frame_sink sync=false max-buffers=1",
        )
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let metadata = read_bounded_sample(&pipeline, 17, Duration::from_secs(2)).unwrap();
        assert_eq!(metadata.pipewire_node_id, 17);
        assert_eq!(metadata.width, Some(32));
        assert_eq!(metadata.height, Some(24));
        assert_eq!(metadata.bytes, 32 * 24 * 4);
        assert_eq!(pipeline.current_state(), gst::State::Null);
    }

    #[test]
    fn synthetic_sequence_samples_multiple_frames_and_cleans_up() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "videotestsrc num-buffers=3 ! video/x-raw,width=32,height=24,format=RGBA ! appsink name=frame_sink sync=false max-buffers=3",
        ).unwrap().downcast::<gst::Pipeline>().unwrap();
        let frames = read_frame_sequence(&pipeline, 17, 3, Duration::from_secs(2)).unwrap();
        assert_eq!(frames.len(), 3);
        assert!(
            frames
                .iter()
                .all(|f| f.width == Some(32) && f.height == Some(24) && f.bytes == 3072)
        );
        assert_eq!(pipeline.current_state(), gst::State::Null);
    }

    #[test]
    fn incomplete_sequence_times_out_and_cleans_up() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "videotestsrc num-buffers=1 ! video/x-raw,width=32,height=24,format=RGBA ! appsink name=frame_sink sync=false max-buffers=2",
        ).unwrap().downcast::<gst::Pipeline>().unwrap();
        let error = read_frame_sequence(&pipeline, 17, 2, Duration::from_millis(200)).unwrap_err();
        assert!(error.contains("received 1 of 2 frames"), "{error}");
        assert_eq!(pipeline.current_state(), gst::State::Null);
    }

    #[test]
    fn unproductive_source_times_out_and_shuts_down() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "appsrc is-live=true format=time ! video/x-raw,width=32,height=24,format=RGBA ! appsink name=frame_sink sync=false max-buffers=1",
        )
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let error = read_bounded_sample(&pipeline, 17, Duration::from_millis(50)).unwrap_err();
        assert!(error.contains("no video frame received"), "{error}");
        assert_eq!(pipeline.current_state(), gst::State::Null);
    }
}
