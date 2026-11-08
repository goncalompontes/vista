//! Decoder-facing H.264 resynchronization for permission-scoped frame handoff.
//!
//! This is transport-neutral and contains no decoder, network access or portal
//! permission grant. It emits discontinuity events and forwards delta frames only
//! after observing an Annex-B IDR preceded by SPS and PPS in the same access unit.
//! NAL header inspection is a conservative *recovery heuristic*, NOT full H.264
//! SPS/PPS/slice parsing or proof that an arbitrary bitstream is decodable.

use crate::session::{
    H264AccessUnit, H264Discontinuity, SessionError, SessionToken, StreamSession,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    InvalidAnnexB,
    Session(SessionError),
}

/// Consumers must flush/reset their decoder on Discontinuity, and must not
/// decode old access units following a reset. No bytes are retained by the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H264SyncEvent {
    Discontinuity(H264Discontinuity),
    AccessUnit(H264AccessUnit),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct NalSummary {
    idr: bool,
    /// True only if an IDR follows BOTH parameter-set NAL types in this AU.
    recovery_point: bool,
    parameter_sets: bool,
}

fn prefix_len(data: &[u8]) -> Option<usize> {
    if data.starts_with(&[0, 0, 0, 1]) {
        Some(4)
    } else if data.starts_with(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

/// Scan start codes without allocation; no deep RBSP interpretation is done.
/// Encoded AUs should be produced by GStreamer's byte-stream `h264parse`.
fn scan_access_unit(data: &[u8]) -> Result<NalSummary, SyncError> {
    let mut pos = 0;
    let mut sps = false;
    let mut pps = false;
    let mut summary = NalSummary::default();
    while pos < data.len() {
        let prefix = prefix_len(&data[pos..]).ok_or(SyncError::InvalidAnnexB)?;
        let header_pos = pos + prefix;
        if header_pos >= data.len() {
            return Err(SyncError::InvalidAnnexB);
        }
        let mut end = header_pos + 1;
        while end + 2 < data.len() && prefix_len(&data[end..]).is_none() {
            end += 1;
        }
        if end + 2 >= data.len() {
            end = data.len();
        }
        // Require a nonempty NAL payload beyond the header, and reject illegal
        // forbidden_zero_bit / reserved type zero. The other NAL types may be
        // extension types, so they are not needlessly excluded.
        let header = data[header_pos];
        let kind = header & 0x1f;
        if header & 0x80 != 0 || kind == 0 || end <= header_pos + 1 {
            return Err(SyncError::InvalidAnnexB);
        }
        match kind {
            7 => {
                sps = true;
                summary.parameter_sets = true;
            }
            8 => {
                pps = true;
                summary.parameter_sets = true;
            }
            5 => {
                summary.idr = true;
                summary.recovery_point |= sps && pps;
            }
            _ => {}
        }
        pos = end;
    }
    Ok(summary)
}

/// One gate per downstream decoder/transport consumer. Each call checks a
/// permission-scoped continuity generation; it never depends on polling an
/// unprotected cumulative loss counter.
pub struct H264SyncGate {
    awaiting_idr: bool,
    observed_generation: Option<u64>,
    pending_reset: Option<H264Discontinuity>,
}

impl Default for H264SyncGate {
    fn default() -> Self {
        Self::new()
    }
}

impl H264SyncGate {
    pub fn new() -> Self {
        Self {
            awaiting_idr: true,
            observed_generation: None,
            pending_reset: None,
        }
    }

    /// Call on consumer reconnect, decoder recreation or any transport gap.
    pub fn reset(&mut self) {
        self.awaiting_idr = true;
        self.pending_reset = Some(H264Discontinuity::ReceiverReset);
    }

    pub fn awaiting_idr(&self) -> bool {
        self.awaiting_idr
    }

    /// Poll one event. Startup/overflow/resize/pipeline/decoder reset produces
    /// a distinct discontinuity event before any subsequent recovery unit.
    /// Units that cannot resume decoding are discarded, not stored or emitted.
    pub fn next(
        &mut self,
        session: &mut StreamSession,
        token: SessionToken,
    ) -> Result<Option<H264SyncEvent>, SyncError> {
        let (generation, reason) = session.h264_continuity(token).map_err(SyncError::Session)?;
        if self.observed_generation != Some(generation) {
            let reset_reason = if self.observed_generation.is_none() && generation == 0 {
                H264Discontinuity::Startup
            } else {
                reason
            };
            self.observed_generation = Some(generation);
            self.awaiting_idr = true;
            self.pending_reset = None;
            return Ok(Some(H264SyncEvent::Discontinuity(reset_reason)));
        }
        if let Some(reason) = self.pending_reset.take() {
            return Ok(Some(H264SyncEvent::Discontinuity(reason)));
        }
        while let Some(unit) = session.dequeue_h264(token).map_err(SyncError::Session)? {
            let summary = match scan_access_unit(&unit.bytes) {
                Ok(value) => value,
                Err(error) => {
                    self.awaiting_idr = true;
                    self.pending_reset = Some(H264Discontinuity::InvalidBitstream);
                    return Err(error);
                }
            };
            if self.awaiting_idr {
                if !summary.recovery_point {
                    continue;
                }
                self.awaiting_idr = false;
            } else if summary.parameter_sets && !summary.idr {
                // Cannot verify parameter-set compatibility without parsing
                // their IDs/revisions; conservatively wait for a fresh IDR.
                self.awaiting_idr = true;
                return Ok(Some(H264SyncEvent::Discontinuity(
                    H264Discontinuity::CodecConfiguration,
                )));
            }
            return Ok(Some(H264SyncEvent::AccessUnit(unit)));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::FrameGeometry;

    fn unit(nals: &[u8]) -> H264AccessUnit {
        let mut bytes = Vec::new();
        for &kind in nals {
            bytes.extend_from_slice(&[0, 0, 0, 1, kind, 0x80]);
        }
        H264AccessUnit {
            pts_ns: Some(100),
            is_keyframe: nals.contains(&5),
            bytes,
        }
    }

    fn session() -> (StreamSession, SessionToken) {
        let mut session = StreamSession::new();
        let token = session.authorize().unwrap();
        session.start(token).unwrap();
        (session, token)
    }

    fn poll(
        gate: &mut H264SyncGate,
        s: &mut StreamSession,
        t: SessionToken,
    ) -> Option<H264SyncEvent> {
        gate.next(s, t).unwrap()
    }

    #[test]
    fn startup_and_recovery_require_ordered_sps_pps_idr() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(H264Discontinuity::Startup))
        );
        for nals in [&[1][..], &[7, 5], &[5, 7, 8], &[7, 8, 1]] {
            s.enqueue_h264(t, unit(nals)).unwrap();
            assert_eq!(poll(&mut gate, &mut s, t), None);
        }
        let recovery = unit(&[9, 7, 8, 5]);
        s.enqueue_h264(t, recovery.clone()).unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(recovery))
        );
        let delta = unit(&[1]);
        s.enqueue_h264(t, delta.clone()).unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(delta))
        );
    }

    #[test]
    fn overflow_signals_loss_before_any_surviving_unit() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        poll(&mut gate, &mut s, t);
        assert!(!gate.awaiting_idr());
        for _ in 0..3 {
            s.enqueue_h264(t, unit(&[1])).unwrap();
        }
        assert_eq!(s.dropped_access_units(), 1);
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(
                H264Discontinuity::QueueOverflow
            ))
        );
        assert!(gate.awaiting_idr());
        assert_eq!(poll(&mut gate, &mut s, t), None);
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(unit(&[7, 8, 5])))
        );
    }

    #[test]
    fn reconnect_requires_new_idr_and_emits_reset() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        poll(&mut gate, &mut s, t);
        gate.reset();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(
                H264Discontinuity::ReceiverReset
            ))
        );
        s.enqueue_h264(t, unit(&[1])).unwrap();
        assert_eq!(poll(&mut gate, &mut s, t), None);
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        assert!(matches!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(_))
        ));
    }

    #[test]
    fn invalid_annex_b_forces_reset_and_preserves_future_recovery() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        for bytes in [
            vec![1, 2, 3],
            vec![0, 0, 1],
            vec![0, 0, 1, 0x85],
            vec![0, 0, 1, 0x67, 0x80, 0, 0, 1, 0x88],
        ] {
            s.enqueue_h264(
                t,
                H264AccessUnit {
                    pts_ns: None,
                    is_keyframe: false,
                    bytes,
                },
            )
            .unwrap();
            assert_eq!(gate.next(&mut s, t), Err(SyncError::InvalidAnnexB));
            assert_eq!(
                poll(&mut gate, &mut s, t),
                Some(H264SyncEvent::Discontinuity(
                    H264Discontinuity::InvalidBitstream
                ))
            );
        }
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        assert!(matches!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(_))
        ));
    }

    #[test]
    fn resize_flushes_data_and_signals_discontinuity() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        s.enqueue_rgba(
            t,
            FrameGeometry {
                width: 2,
                height: 2,
            },
            vec![0; 16],
        )
        .unwrap();
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        s.enqueue_rgba(
            t,
            FrameGeometry {
                width: 4,
                height: 2,
            },
            vec![0; 32],
        )
        .unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(
                H264Discontinuity::GeometryChange
            ))
        );
        assert_eq!(poll(&mut gate, &mut s, t), None);
        s.enqueue_h264(t, unit(&[1])).unwrap();
        assert_eq!(poll(&mut gate, &mut s, t), None);
    }

    #[test]
    fn pipeline_discontinuity_and_revocation() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        s.mark_h264_discontinuity(t, H264Discontinuity::Pipeline)
            .unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(H264Discontinuity::Pipeline))
        );
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        s.revoke();
        assert_eq!(
            gate.next(&mut s, t),
            Err(SyncError::Session(SessionError::StaleToken))
        );
    }

    #[test]
    fn parameter_sets_without_idr_trigger_codec_reset() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        s.enqueue_h264(t, unit(&[7, 8, 5])).unwrap();
        poll(&mut gate, &mut s, t);
        s.enqueue_h264(t, unit(&[7, 8, 1])).unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::Discontinuity(
                H264Discontinuity::CodecConfiguration
            ))
        );
        assert!(gate.awaiting_idr());
    }

    #[test]
    fn ignores_keyframe_flag_without_idr_and_accepts_three_byte_codes() {
        let (mut s, t) = session();
        let mut gate = H264SyncGate::new();
        poll(&mut gate, &mut s, t);
        let mut wrongly_flagged = unit(&[7, 8, 1]);
        wrongly_flagged.is_keyframe = true;
        s.enqueue_h264(t, wrongly_flagged).unwrap();
        assert_eq!(poll(&mut gate, &mut s, t), None);
        let bytes = vec![0, 0, 1, 7, 0x42, 0, 0, 1, 8, 0x32, 0, 0, 1, 5, 0x80];
        s.enqueue_h264(
            t,
            H264AccessUnit {
                pts_ns: Some(101),
                is_keyframe: false,
                bytes: bytes.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            poll(&mut gate, &mut s, t),
            Some(H264SyncEvent::AccessUnit(H264AccessUnit {
                pts_ns: Some(101),
                is_keyframe: false,
                bytes
            }))
        );
    }
}
