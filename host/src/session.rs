//! Permission-scoped stream lifecycle and bounded frame handoff.
//!
//! This state machine deliberately has no capture/transport side effects.
//! A future portal adapter must call `authorize` only after a successful,
//! window-only portal grant and must call `revoke` before stopping the pipeline.
//! Epoch tokens prevent a stale producer from delivering after replacement.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    AwaitingConsent,
    Authorized,
    Streaming,
    Revoked,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionToken(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionError {
    InvalidTransition,
    StaleToken,
    InvalidFrame,
    InvalidAccessUnit,
}

/// Why a decoder must discard reference state and await a fresh IDR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum H264Discontinuity {
    Startup,
    QueueOverflow,
    GeometryChange,
    Pipeline,
    ReceiverReset,
    InvalidBitstream,
    CodecConfiguration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameGeometry {
    pub width: u32,
    pub height: u32,
}

impl FrameGeometry {
    pub fn rgba_size(self) -> Option<usize> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        usize::try_from(self.width)
            .ok()?
            .checked_mul(usize::try_from(self.height).ok()?)?
            .checked_mul(4)
            .filter(|bytes| *bytes <= MAX_RAW_FRAME_BYTES)
    }
}

const MAX_RAW_FRAME_BYTES: usize = 256 * 1024 * 1024;
const MAX_QUEUED_FRAMES: usize = 2;
const MAX_H264_ACCESS_UNIT_BYTES: usize = 16 * 1024 * 1024;
const MAX_QUEUED_ACCESS_UNITS: usize = 2;

/// Transport-neutral compressed frame, produced only after authorized encoding.
/// The encoder must provide byte-stream H.264 with access-unit alignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct H264AccessUnit {
    pub pts_ns: Option<u64>,
    pub is_keyframe: bool,
    pub bytes: Vec<u8>,
}

/// Owns at most two transient frames; on pressure the oldest frame is dropped.
/// A source may only enqueue a frame with the current authorization token.
pub struct StreamSession {
    state: SessionState,
    epoch: u64,
    geometry: Option<FrameGeometry>,
    queue: VecDeque<Vec<u8>>,
    encoded_queue: VecDeque<H264AccessUnit>,
    dropped_frames: u64,
    dropped_access_units: u64,
    /// Generation changes whenever delivery continuity becomes uncertain.
    encoded_continuity: u64,
    encoded_discontinuity_reason: H264Discontinuity,
    resize_events: u64,
}

impl Default for StreamSession {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSession {
    pub fn new() -> Self {
        Self {
            state: SessionState::AwaitingConsent,
            epoch: 0,
            geometry: None,
            queue: VecDeque::with_capacity(MAX_QUEUED_FRAMES),
            encoded_queue: VecDeque::with_capacity(MAX_QUEUED_ACCESS_UNITS),
            dropped_frames: 0,
            dropped_access_units: 0,
            encoded_continuity: 0,
            encoded_discontinuity_reason: H264Discontinuity::Startup,
            resize_events: 0,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn queued_frames(&self) -> usize {
        self.queue.len()
    }

    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    pub fn queued_access_units(&self) -> usize {
        self.encoded_queue.len()
    }

    pub fn dropped_access_units(&self) -> u64 {
        self.dropped_access_units
    }

    /// Loss notification is authorization-scoped. An observer cannot use an
    /// expired grant to inspect the stream's continuity or queue contents.
    pub fn h264_continuity(
        &self,
        token: SessionToken,
    ) -> Result<(u64, H264Discontinuity), SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        Ok((self.encoded_continuity, self.encoded_discontinuity_reason))
    }

    /// Notify a downstream consumer of loss outside the bounded queue, e.g.
    /// GStreamer DISCONT. Caller must ensure a valid and active portal grant.
    pub fn mark_h264_discontinuity(
        &mut self,
        token: SessionToken,
        reason: H264Discontinuity,
    ) -> Result<(), SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        self.bump_h264_continuity(reason);
        Ok(())
    }

    fn bump_h264_continuity(&mut self, reason: H264Discontinuity) {
        self.encoded_continuity = self.encoded_continuity.wrapping_add(1);
        self.encoded_discontinuity_reason = reason;
    }

    pub fn resize_events(&self) -> u64 {
        self.resize_events
    }

    /// Caller must have already received an explicit window-only portal grant.
    pub fn authorize(&mut self) -> Result<SessionToken, SessionError> {
        if self.state != SessionState::AwaitingConsent {
            return Err(SessionError::InvalidTransition);
        }
        self.epoch = self.epoch.wrapping_add(1);
        // Epoch zero is never a valid issued token.
        if self.epoch == 0 {
            self.epoch = 1;
        }
        self.state = SessionState::Authorized;
        Ok(SessionToken(self.epoch))
    }

    pub fn start(&mut self, token: SessionToken) -> Result<(), SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Authorized {
            return Err(SessionError::InvalidTransition);
        }
        self.state = SessionState::Streaming;
        Ok(())
    }

    pub fn enqueue_rgba(
        &mut self,
        token: SessionToken,
        geometry: FrameGeometry,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        let expected = geometry.rgba_size().ok_or(SessionError::InvalidFrame)?;
        if bytes.len() != expected {
            return Err(SessionError::InvalidFrame);
        }
        if self.geometry.is_some_and(|old| old != geometry) {
            self.resize_events += 1;
            // Never mix old-geometry frames into a new resolution.
            self.queue.clear();
            self.encoded_queue.clear();
            self.bump_h264_continuity(H264Discontinuity::GeometryChange);
        }
        self.geometry = Some(geometry);
        if self.queue.len() == MAX_QUEUED_FRAMES {
            self.queue.pop_front();
            self.dropped_frames += 1;
        }
        self.queue.push_back(bytes);
        Ok(())
    }

    /// Adopt a new surface geometry. A changed geometry discards buffered raw
    /// and encoded data and advances the H.264 continuity generation, so the
    /// consumer must wait for a fresh SPS/PPS IDR. The first geometry establishes
    /// the baseline without a flush.
    pub fn resize(
        &mut self,
        token: SessionToken,
        geometry: FrameGeometry,
    ) -> Result<bool, SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        let changed = self.geometry.is_some_and(|old| old != geometry);
        self.geometry = Some(geometry);
        if changed {
            self.resize_events += 1;
            self.queue.clear();
            self.encoded_queue.clear();
            self.bump_h264_continuity(H264Discontinuity::GeometryChange);
        }
        Ok(changed)
    }

    /// A consumer must present the same live grant token as the producer.
    pub fn dequeue(&mut self, token: SessionToken) -> Result<Option<Vec<u8>>, SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        Ok(self.queue.pop_front())
    }

    /// Only a live grant can place encoded video into the bounded handoff queue.
    /// Callers validate H.264 caps, byte-stream format and AU alignment before this.
    /// The oldest access unit is dropped on pressure. This increments the
    /// continuity generation so each decoder can signal a reset explicitly.
    pub fn enqueue_h264(
        &mut self,
        token: SessionToken,
        unit: H264AccessUnit,
    ) -> Result<(), SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        if unit.bytes.is_empty() || unit.bytes.len() > MAX_H264_ACCESS_UNIT_BYTES {
            return Err(SessionError::InvalidAccessUnit);
        }
        if self.encoded_queue.len() == MAX_QUEUED_ACCESS_UNITS {
            self.encoded_queue.pop_front();
            self.dropped_access_units += 1;
            self.bump_h264_continuity(H264Discontinuity::QueueOverflow);
        }
        self.encoded_queue.push_back(unit);
        Ok(())
    }

    pub fn dequeue_h264(
        &mut self,
        token: SessionToken,
    ) -> Result<Option<H264AccessUnit>, SessionError> {
        self.verify(token)?;
        if self.state != SessionState::Streaming {
            return Err(SessionError::InvalidTransition);
        }
        Ok(self.encoded_queue.pop_front())
    }

    /// Revocation immediately discards all buffered pixels and invalidates tokens.
    /// Idempotent for an already revoked or closed session.
    pub fn revoke(&mut self) {
        self.queue.clear();
        self.encoded_queue.clear();
        self.geometry = None;
        self.epoch = self.epoch.wrapping_add(1);
        if self.state != SessionState::Closed {
            self.state = SessionState::Revoked;
        }
    }

    /// Terminal close is safe to call after any error, including failed consent.
    pub fn close(&mut self) {
        self.revoke();
        self.state = SessionState::Closed;
    }

    fn verify(&self, token: SessionToken) -> Result<(), SessionError> {
        if token.0 != self.epoch || token.0 == 0 {
            Err(SessionError::StaleToken)
        } else {
            Ok(())
        }
    }
}

impl Drop for StreamSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: FrameGeometry = FrameGeometry {
        width: 2,
        height: 2,
    };
    const RESIZED: FrameGeometry = FrameGeometry {
        width: 4,
        height: 2,
    };

    fn live_session() -> (StreamSession, SessionToken) {
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        (session, token)
    }

    #[test]
    fn cannot_stream_without_explicit_grant() {
        let mut session = StreamSession::new();
        assert_eq!(session.state(), SessionState::AwaitingConsent);
        assert_eq!(
            session.enqueue_rgba(SessionToken(0), SMALL, vec![0; 16]),
            Err(SessionError::StaleToken)
        );
        let token = session.authorize().unwrap();
        assert_eq!(session.authorize(), Err(SessionError::InvalidTransition));
        assert_eq!(
            session.enqueue_rgba(token, SMALL, vec![0; 16]),
            Err(SessionError::InvalidTransition)
        );
        session.start(token).unwrap();
        assert_eq!(session.start(token), Err(SessionError::InvalidTransition));
    }

    #[test]
    fn queue_drops_oldest_frame_under_pressure() {
        let (mut session, token) = live_session();
        for value in 0..4 {
            session.enqueue_rgba(token, SMALL, vec![value; 16]).unwrap();
        }
        assert_eq!(session.queued_frames(), 2);
        assert_eq!(session.dropped_frames(), 2);
        assert_eq!(session.dequeue(token).unwrap(), Some(vec![2; 16]));
        assert_eq!(session.dequeue(token).unwrap(), Some(vec![3; 16]));
        assert_eq!(session.dequeue(token).unwrap(), None);
    }

    #[test]
    fn resize_flushes_frames_of_previous_dimensions() {
        let (mut session, token) = live_session();
        session.enqueue_rgba(token, SMALL, vec![1; 16]).unwrap();
        session.enqueue_rgba(token, RESIZED, vec![2; 32]).unwrap();
        assert_eq!(session.resize_events(), 1);
        assert_eq!(session.queued_frames(), 1);
        assert_eq!(session.dequeue(token).unwrap(), Some(vec![2; 32]));
    }

    #[test]
    fn invalid_frames_never_enter_queue() {
        let (mut session, token) = live_session();
        for geometry in [
            FrameGeometry {
                width: 0,
                height: 4,
            },
            FrameGeometry {
                width: u32::MAX,
                height: u32::MAX,
            },
        ] {
            assert_eq!(
                session.enqueue_rgba(token, geometry, Vec::new()),
                Err(SessionError::InvalidFrame)
            );
        }
        assert_eq!(
            session.enqueue_rgba(token, SMALL, vec![0; 15]),
            Err(SessionError::InvalidFrame)
        );
        assert_eq!(session.queued_frames(), 0);
    }

    #[test]
    fn revocation_discards_pixels_and_rejects_stale_producers() {
        let (mut session, token) = live_session();
        session.enqueue_rgba(token, SMALL, vec![1; 16]).unwrap();
        session.revoke();
        assert_eq!(session.queued_frames(), 0);
        assert_eq!(session.state(), SessionState::Revoked);
        assert_eq!(
            session.enqueue_rgba(token, SMALL, vec![1; 16]),
            Err(SessionError::StaleToken)
        );
        assert_eq!(session.dequeue(token), Err(SessionError::StaleToken));
        assert_eq!(session.authorize(), Err(SessionError::InvalidTransition));
        session.close();
        assert_eq!(session.state(), SessionState::Closed);
        assert_eq!(session.queued_frames(), 0);
    }

    #[test]
    fn encoded_queue_is_bounded_and_preserves_metadata() {
        let (mut session, token) = live_session();
        for i in 0..4u8 {
            session
                .enqueue_h264(
                    token,
                    H264AccessUnit {
                        pts_ns: Some(u64::from(i) * 33_333_333),
                        is_keyframe: i == 0,
                        bytes: vec![0, 0, 0, 1, i],
                    },
                )
                .unwrap();
        }
        assert_eq!(session.queued_access_units(), 2);
        assert_eq!(session.dropped_access_units(), 2);
        let first = session.dequeue_h264(token).unwrap().unwrap();
        assert_eq!(first.pts_ns, Some(66_666_666));
        assert_eq!(first.bytes, vec![0, 0, 0, 1, 2]);
        assert!(!first.is_keyframe);
        assert_eq!(
            session.dequeue_h264(token).unwrap().unwrap().pts_ns,
            Some(99_999_999)
        );
        assert!(session.dequeue_h264(token).unwrap().is_none());
    }

    #[test]
    fn encoded_queue_rejects_invalid_sizes_and_invalid_state() {
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        let small = H264AccessUnit {
            pts_ns: None,
            is_keyframe: true,
            bytes: vec![1],
        };
        assert_eq!(
            session.enqueue_h264(token, small.clone()),
            Err(SessionError::InvalidTransition)
        );
        session.start(token).unwrap();
        assert_eq!(
            session.enqueue_h264(
                token,
                H264AccessUnit {
                    bytes: vec![],
                    ..small.clone()
                }
            ),
            Err(SessionError::InvalidAccessUnit)
        );
        assert_eq!(
            session.enqueue_h264(
                token,
                H264AccessUnit {
                    bytes: vec![1; MAX_H264_ACCESS_UNIT_BYTES + 1],
                    ..small.clone()
                }
            ),
            Err(SessionError::InvalidAccessUnit)
        );
        assert_eq!(session.queued_access_units(), 0);
        session.enqueue_h264(token, small).unwrap();
        session.revoke();
        assert_eq!(session.queued_access_units(), 0);
        assert_eq!(session.dequeue_h264(token), Err(SessionError::StaleToken));
        assert_eq!(
            session.enqueue_h264(
                token,
                H264AccessUnit {
                    pts_ns: None,
                    is_keyframe: true,
                    bytes: vec![1]
                }
            ),
            Err(SessionError::StaleToken)
        );
    }

    #[test]
    fn resize_clears_encoded_access_units() {
        let (mut session, token) = live_session();
        session.enqueue_rgba(token, SMALL, vec![0; 16]).unwrap();
        session
            .enqueue_h264(
                token,
                H264AccessUnit {
                    pts_ns: None,
                    is_keyframe: true,
                    bytes: vec![1],
                },
            )
            .unwrap();
        session.enqueue_rgba(token, RESIZED, vec![0; 32]).unwrap();
        assert_eq!(session.queued_access_units(), 0);
    }

    #[test]
    fn shutdown_before_consent_is_terminal() {
        let mut session = StreamSession::new();
        session.close();
        assert_eq!(session.authorize(), Err(SessionError::InvalidTransition));
        session.revoke();
        assert_eq!(session.state(), SessionState::Closed);
    }
}
