//! Coordinated window/capture lifecycle guard.
//!
//! The guard turns catalog and portal events into explicit stream decisions:
//! keep streaming, flush encoded state for a geometry change, or terminate and
//! release the session. It owns no capture or network resources; the live worker
//! applies the outcome, and callers revoke any control lease when
//! `release_input` is set.
//!
//! This is the deterministic policy layer for M1 lifecycle work. It is unit
//! tested without a compositor; the opt-in live path reports the same
//! transitions from a real Niri event stream.

use crate::catalog::CatalogUpdate;
use crate::session::{FrameGeometry, SessionError, SessionToken, StreamSession};
use serde::Serialize;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleOutcome {
    /// The event does not change the stream.
    Continue,
    /// A geometry change flushed queued frames/access units; the next encoded
    /// unit must be a fresh SPS/PPS IDR.
    Flushed,
    /// The session was closed and must not be reused.
    Terminated {
        reason: &'static str,
        release_input: bool,
    },
    /// The guard already terminated; the event is ignored.
    AlreadyTerminated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleError {
    pub detail: String,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for LifecycleError {}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LifecycleStats {
    pub resize_flushes: u64,
    pub terminations: u64,
    pub ignored_after_termination: u64,
}

pub struct WindowLifecycle {
    window_id: u64,
    generation: String,
    geometry: Option<FrameGeometry>,
    terminated: Option<&'static str>,
    stats: LifecycleStats,
}

impl WindowLifecycle {
    pub fn new(window_id: u64, generation: impl Into<String>) -> Result<Self, LifecycleError> {
        let generation = generation.into();
        if generation.is_empty() {
            return Err(LifecycleError {
                detail: "target generation must not be empty".into(),
            });
        }
        Ok(Self {
            window_id,
            generation,
            geometry: None,
            terminated: None,
            stats: LifecycleStats::default(),
        })
    }

    pub fn window_id(&self) -> u64 {
        self.window_id
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated.is_some()
    }

    pub fn termination_reason(&self) -> Option<&'static str> {
        self.terminated
    }

    pub fn stats(&self) -> &LifecycleStats {
        &self.stats
    }

    /// Apply one incremental catalog update. Only removal of the captured window
    /// or a snapshot that no longer contains it terminates the stream.
    pub fn catalog_update(&mut self, update: &CatalogUpdate) -> LifecycleOutcome {
        if self.is_terminated() {
            self.stats.ignored_after_termination += 1;
            return LifecycleOutcome::AlreadyTerminated;
        }
        match update {
            CatalogUpdate::Snapshot { windows } => {
                if windows.iter().any(|window| window.id == self.window_id) {
                    LifecycleOutcome::Continue
                } else {
                    self.terminate("target_window_removed")
                }
            }
            CatalogUpdate::Removed { id } => {
                if *id == self.window_id {
                    self.terminate("target_window_removed")
                } else {
                    LifecycleOutcome::Continue
                }
            }
            CatalogUpdate::Upsert { .. } => LifecycleOutcome::Continue,
        }
    }

    /// Detect a compositor/session change. Niri does not expose a boot id, so the
    /// generation is derived from the socket/session name plus the window id.
    pub fn observe_generation(&mut self, generation: &str) -> LifecycleOutcome {
        if self.is_terminated() {
            self.stats.ignored_after_termination += 1;
            return LifecycleOutcome::AlreadyTerminated;
        }
        if generation != self.generation {
            self.terminate("compositor_restarted")
        } else {
            LifecycleOutcome::Continue
        }
    }

    /// Apply a surface geometry change. The first geometry is adopted without a
    /// flush; a later change flushes the bounded queues and advances the H.264
    /// continuity generation so the consumer requests a fresh IDR.
    pub fn resize(
        &mut self,
        geometry: FrameGeometry,
        session: &mut StreamSession,
        token: SessionToken,
    ) -> Result<LifecycleOutcome, SessionError> {
        if self.is_terminated() {
            self.stats.ignored_after_termination += 1;
            return Ok(LifecycleOutcome::AlreadyTerminated);
        }
        let changed = session.resize(token, geometry)?;
        self.geometry = Some(geometry);
        if changed {
            self.stats.resize_flushes += 1;
            Ok(LifecycleOutcome::Flushed)
        } else {
            Ok(LifecycleOutcome::Continue)
        }
    }

    pub fn portal_closed(&mut self) -> LifecycleOutcome {
        self.terminate("portal_closed")
    }

    pub fn permission_revoked(&mut self) -> LifecycleOutcome {
        self.terminate("permission_revoked")
    }

    pub fn session_closed(&mut self) -> LifecycleOutcome {
        self.terminate("session_closed")
    }

    /// Apply a termination outcome to the stream session. Flushes and
    /// continuations leave the session untouched.
    pub fn apply(outcome: LifecycleOutcome, session: &mut StreamSession) {
        if matches!(outcome, LifecycleOutcome::Terminated { .. }) {
            session.close();
        }
    }

    fn terminate(&mut self, reason: &'static str) -> LifecycleOutcome {
        if self.is_terminated() {
            self.stats.ignored_after_termination += 1;
            return LifecycleOutcome::AlreadyTerminated;
        }
        self.terminated = Some(reason);
        self.stats.terminations += 1;
        LifecycleOutcome::Terminated {
            reason,
            release_input: true,
        }
    }
}

/// Watches the Niri event stream for the captured window while capture runs.
///
/// The watcher is opt-in (`capture-h264 --window ID`). It reports removal of the
/// target window or the end of the compositor event stream. It never reads
/// pixels and does not persist permissions.
pub struct WindowWatcher {
    cancelled: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<&'static str>>>,
    child: Child,
    handle: Option<JoinHandle<()>>,
}

impl WindowWatcher {
    pub fn start(window_id: u64) -> Result<Self, String> {
        let generation = crate::capability::niri_generation(window_id);
        let mut child = Command::new("niri")
            .args(["msg", "--json", "event-stream"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start Niri window watcher: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Niri window watcher stdout was not available".to_string())?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        let flag = cancelled.clone();
        let reason_out = reason.clone();
        let handle = thread::spawn(move || {
            let mut catalog = crate::catalog::WindowCatalog::default();
            let Ok(mut guard) = WindowLifecycle::new(window_id, generation) else {
                *reason_out.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some("invalid_window_generation");
                flag.store(true, Ordering::Release);
                return;
            };
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(updates) = catalog.apply_line(&line) else {
                    continue;
                };
                for update in updates {
                    if let LifecycleOutcome::Terminated { reason, .. } =
                        guard.catalog_update(&update)
                    {
                        *reason_out.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
                        flag.store(true, Ordering::Release);
                        return;
                    }
                }
            }
            // The event stream ending after a snapshot means the compositor
            // session went away. Before the first snapshot it is just unavailable.
            *reason_out.lock().unwrap_or_else(|p| p.into_inner()) = Some(if catalog.ready() {
                "compositor_stream_ended"
            } else {
                "window_watch_unavailable"
            });
            flag.store(true, Ordering::Release);
        });
        Ok(Self {
            cancelled,
            reason,
            child,
            handle: Some(handle),
        })
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn reason(&self) -> Option<&'static str> {
        *self.reason.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Drop for WindowWatcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic diagnostic
// ---------------------------------------------------------------------------
#[derive(Debug, Serialize)]
pub struct LifecycleReport {
    pub window_id: u64,
    pub removal_terminates: bool,
    pub snapshot_missing_terminates: bool,
    pub portal_close_terminates: bool,
    pub permission_revoke_terminates: bool,
    pub compositor_restart_terminates: bool,
    pub resize_flushes: bool,
    pub resize_signals_geometry_discontinuity: bool,
    pub terminated_session_is_closed: bool,
    pub idempotent_termination: bool,
    pub verified: bool,
}

pub fn diagnostic() -> Result<LifecycleReport, LifecycleError> {
    use crate::Window;
    use crate::catalog::CatalogUpdate;
    use crate::session::{FrameGeometry, H264Discontinuity, StreamSession};

    const SMALL: FrameGeometry = FrameGeometry {
        width: 2,
        height: 2,
    };
    const LARGE: FrameGeometry = FrameGeometry {
        width: 4,
        height: 4,
    };

    fn streaming_session() -> (StreamSession, SessionToken) {
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        (session, token)
    }

    fn window(id: u64) -> Window {
        Window {
            id,
            title: Some("target".into()),
            app_id: Some("test".into()),
            pid: None,
            workspace_id: None,
            is_focused: false,
            is_floating: false,
            is_urgent: false,
        }
    }

    // Removal terminates an active stream and closes the session.
    let (mut session, _token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let removal = guard.catalog_update(&CatalogUpdate::Removed { id: 5 });
    let removal_terminates = matches!(
        removal,
        LifecycleOutcome::Terminated {
            reason: "target_window_removed",
            release_input: true
        }
    );
    WindowLifecycle::apply(removal, &mut session);
    let terminated_session_is_closed = session.state() == crate::session::SessionState::Closed;
    let idempotent_termination = matches!(
        guard.catalog_update(&CatalogUpdate::Removed { id: 5 }),
        LifecycleOutcome::AlreadyTerminated
    );

    // A snapshot that no longer contains the window terminates.
    let (mut session, _token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let snapshot_missing = guard.catalog_update(&CatalogUpdate::Snapshot {
        windows: vec![window(6)],
    });
    let snapshot_missing_terminates =
        matches!(snapshot_missing, LifecycleOutcome::Terminated { .. });
    WindowLifecycle::apply(snapshot_missing, &mut session);
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let snapshot_present_continues = matches!(
        guard.catalog_update(&CatalogUpdate::Snapshot {
            windows: vec![window(5)]
        }),
        LifecycleOutcome::Continue
    );

    // Portal close and permission revocation terminate.
    let (mut session, _token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let portal_close = guard.portal_closed();
    let portal_close_terminates = matches!(
        portal_close,
        LifecycleOutcome::Terminated {
            reason: "portal_closed",
            ..
        }
    );
    WindowLifecycle::apply(portal_close, &mut session);
    let (mut session, _token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let permission_revoke = guard.permission_revoked();
    let permission_revoke_terminates = matches!(
        permission_revoke,
        LifecycleOutcome::Terminated {
            reason: "permission_revoked",
            ..
        }
    );
    WindowLifecycle::apply(permission_revoke, &mut session);

    // A compositor/session generation change terminates.
    let (mut session, _token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let compositor_restart = guard.observe_generation("niri-other#w5");
    let compositor_restart_terminates = matches!(
        compositor_restart,
        LifecycleOutcome::Terminated {
            reason: "compositor_restarted",
            ..
        }
    );
    WindowLifecycle::apply(compositor_restart, &mut session);

    // Resize flushes queued state and signals a geometry discontinuity.
    let (mut session, token) = streaming_session();
    let mut guard = WindowLifecycle::new(5, "niri-test#w5")?;
    let first = guard.resize(SMALL, &mut session, token).unwrap();
    session
        .enqueue_rgba(token, SMALL, vec![0u8; SMALL.rgba_size().unwrap()])
        .unwrap();
    let second = guard.resize(LARGE, &mut session, token).unwrap();
    let resize_flushes = first == LifecycleOutcome::Continue
        && second == LifecycleOutcome::Flushed
        && session.queued_frames() == 0
        && session.resize_events() == 1;
    let resize_signals_geometry_discontinuity = matches!(
        session.h264_continuity(token),
        Ok((_, H264Discontinuity::GeometryChange))
    );

    let verified = removal_terminates
        && snapshot_missing_terminates
        && snapshot_present_continues
        && portal_close_terminates
        && permission_revoke_terminates
        && compositor_restart_terminates
        && resize_flushes
        && resize_signals_geometry_discontinuity
        && terminated_session_is_closed
        && idempotent_termination;

    Ok(LifecycleReport {
        window_id: 5,
        removal_terminates,
        snapshot_missing_terminates,
        portal_close_terminates,
        permission_revoke_terminates,
        compositor_restart_terminates,
        resize_flushes,
        resize_signals_geometry_discontinuity,
        terminated_session_is_closed,
        idempotent_termination,
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Window;

    fn window(id: u64) -> Window {
        Window {
            id,
            title: None,
            app_id: None,
            pid: None,
            workspace_id: None,
            is_focused: false,
            is_floating: false,
            is_urgent: false,
        }
    }

    fn streaming() -> (StreamSession, SessionToken) {
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        (session, token)
    }

    #[test]
    fn unrelated_removal_does_not_terminate() {
        let (_session, _token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        assert_eq!(
            guard.catalog_update(&CatalogUpdate::Removed { id: 6 }),
            LifecycleOutcome::Continue
        );
        assert!(!guard.is_terminated());
    }

    #[test]
    fn upsert_of_target_keeps_streaming_and_other_ids_are_ignored() {
        let (_session, _token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        assert_eq!(
            guard.catalog_update(&CatalogUpdate::Upsert { window: window(5) }),
            LifecycleOutcome::Continue
        );
        assert_eq!(
            guard.catalog_update(&CatalogUpdate::Upsert { window: window(9) }),
            LifecycleOutcome::Continue
        );
    }

    #[test]
    fn matching_generation_does_not_terminate() {
        let (_session, _token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        assert_eq!(
            guard.observe_generation("gen#w5"),
            LifecycleOutcome::Continue
        );
        assert!(!guard.is_terminated());
    }

    #[test]
    fn termination_events_are_idempotent() {
        let (mut session, _token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        assert!(matches!(
            guard.portal_closed(),
            LifecycleOutcome::Terminated {
                reason: "portal_closed",
                release_input: true
            }
        ));
        assert_eq!(
            guard.permission_revoked(),
            LifecycleOutcome::AlreadyTerminated
        );
        WindowLifecycle::apply(guard.portal_closed(), &mut session);
        assert_eq!(guard.stats().terminations, 1);
        assert_eq!(guard.stats().ignored_after_termination, 2);
    }

    #[test]
    fn resize_after_termination_is_ignored() {
        let (mut session, token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        WindowLifecycle::apply(guard.session_closed(), &mut session);
        let geometry = FrameGeometry {
            width: 4,
            height: 4,
        };
        assert_eq!(
            guard.resize(geometry, &mut session, token).unwrap(),
            LifecycleOutcome::AlreadyTerminated
        );
    }

    #[test]
    fn resize_flushes_encoded_queue_and_advances_continuity() {
        let (mut session, token) = streaming();
        let mut guard = WindowLifecycle::new(5, "gen#w5").unwrap();
        let small = FrameGeometry {
            width: 2,
            height: 2,
        };
        let large = FrameGeometry {
            width: 4,
            height: 4,
        };
        guard.resize(small, &mut session, token).unwrap();
        session
            .enqueue_h264(
                token,
                crate::session::H264AccessUnit {
                    pts_ns: Some(1),
                    is_keyframe: true,
                    bytes: vec![0, 0, 0, 1],
                },
            )
            .unwrap();
        let (before, _) = session.h264_continuity(token).unwrap();
        assert_eq!(
            guard.resize(large, &mut session, token).unwrap(),
            LifecycleOutcome::Flushed
        );
        let (after, reason) = session.h264_continuity(token).unwrap();
        assert!(after > before);
        assert_eq!(reason, crate::session::H264Discontinuity::GeometryChange);
        assert_eq!(session.queued_access_units(), 0);
        assert_eq!(guard.stats().resize_flushes, 1);
    }
}
