//! Explicitly user-authorized ScreenCast portal negotiation diagnostic.
//!
//! This diagnostic deliberately never connects to the PipeWire remote, reads
//! pixels, or persists portal permissions. A returned node ID is proof that a
//! portal stream was negotiated, NOT proof that frames can be read or that an
//! arbitrary Niri window ID can be captured.

use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use serde::Serialize;

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
/// token and no PipeWire connection. The session closes when dropped.
pub async fn consent_probe() -> Result<ConsentProbeReport, ashpd::Error> {
    let portal = Screencast::new().await?;
    let session = portal.create_session(Default::default()).await?;
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
    let streams = response
        .streams()
        .iter()
        .map(|stream| NegotiatedStream {
            pipewire_node_id: stream.pipe_wire_node_id(),
            size: stream.size(),
            position: stream.position(),
        })
        .collect();

    Ok(ConsentProbeReport {
        portal_session_started: true,
        streams,
        frames_captured: false,
        arbitrary_window_capture_verified: false,
    })
}
