//! Authenticated control-plane keyframe request → producer restart integration.
//!
//! A local control server uses the AEAD record transport; its client sends an
//! authenticated `KeyframeRequest` after opening a session. The server hook
//! sets a producer request flag, and the media pipeline restarts its encoder to
//! emit a fresh IDR. This is localhost-only and uses a synthetic source.

use crate::{
    protocol::{PairingSecret, unix_now},
    sync::{self, KeyframeRequestHandle},
    transport::{self, HostControlServer},
};
use serde::Serialize;
use std::thread;

#[derive(Debug, Serialize)]
pub struct KeyframeControlReport {
    /// Requests observed by the authenticated host session.
    pub server_keyframe_requests: u64,
    /// Requests consumed by the media producer through the handle.
    pub external_keyframe_requests: usize,
    pub encoder_restarts: usize,
    pub decoded_frames: usize,
    pub verified: bool,
}

/// Runs the control handshake and a synthetic media pipeline together. The
/// client's encrypted `KeyframeRequest` must reach the producer and cause a
/// restart; no media is sent to the client.
pub fn keyframe_control_probe() -> Result<KeyframeControlReport, String> {
    let secret = PairingSecret::generate().map_err(|error| format!("pairing secret: {error}"))?;
    let (server, offer) = HostControlServer::bind(
        "127.0.0.1:0".parse().expect("loopback address"),
        secret.clone(),
        unix_now(),
    )
    .map_err(|error| format!("control listener: {error}"))?;

    let request_handle = KeyframeRequestHandle::default();
    let pipeline_request = request_handle.clone();
    let pipeline = thread::spawn(move || {
        sync::synthetic_sync_recovery_with_external_request(&pipeline_request)
    });
    let hook_request = request_handle.clone();
    let server_thread =
        thread::spawn(move || server.accept_and_run_with_hook(move || hook_request.request()));

    let client = transport::connect(offer.address, secret)
        .map_err(|error| format!("control client failed: {error}"));
    let pipeline_report = pipeline
        .join()
        .map_err(|_| "media pipeline thread panicked".to_string())?
        .map_err(|error| format!("media pipeline failed: {error}"))?;
    let server_stats = server_thread
        .join()
        .map_err(|_| "control server thread panicked".to_string())?
        .map_err(|error| format!("control server failed: {error}"))?;
    client?;

    let verified = server_stats.keyframe_requests >= 1
        && pipeline_report.external_keyframe_requests >= 1
        && pipeline_report.encoder_restarts >= 1;
    Ok(KeyframeControlReport {
        server_keyframe_requests: server_stats.keyframe_requests,
        external_keyframe_requests: pipeline_report.external_keyframe_requests,
        encoder_restarts: pipeline_report.encoder_restarts,
        decoded_frames: pipeline_report.decoded_frames,
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PipeWire-free GStreamer H.264 plugins and localhost networking"]
    fn authenticated_keyframe_request_restarts_the_producer() {
        let report = keyframe_control_probe().unwrap();
        assert!(report.verified, "{report:?}");
        assert!(report.server_keyframe_requests >= 1);
        assert!(report.external_keyframe_requests >= 1);
        assert!(report.encoder_restarts >= 1);
    }
}
