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
    let result = (|| {
        let sink = pipeline
            .by_name("frame_sink")
            .ok_or_else(|| "frame_sink missing".to_string())?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "frame_sink has wrong type".to_string())?;
        let started = Instant::now();
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("cannot start PipeWire pipeline: {e}"))?;
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_nseconds(FRAME_WAIT.as_nanos() as u64))
            .ok_or_else(|| "no video frame received within five seconds".to_string())?;
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
    let _ = pipeline.set_state(gst::State::Null);
    // The portal-owned FD stays alive through the pipeline shutdown.
    result
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
