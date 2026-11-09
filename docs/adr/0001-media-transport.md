# ADR 0001 — media transport

Status: **accepted for the current milestone (2026-11-09)**. Not a production
security or internet-transport claim.

## Context

Vista already has:

- a versioned authenticated control channel with a 256-bit out-of-band PSK,
  HKDF session keys and a ChaCha20-Poly1305 record layer (`host/src/transport.rs`);
- a bounded, permission-scoped H.264 access-unit boundary (`StreamSession`);
- a synthetic two-process media diagnostic that derives its media key from the
  authenticated session and drives keyframe recovery (`host/src/media_stream.rs`);
- an iOS client that must eventually receive the same access units.

Missing: a reusable media-record transport with explicit timestamps, keyframe
and discontinuity metadata, bounded producer behavior, and a documented decision
for how iOS will receive media.

## Decision

Use a **dedicated length-prefixed ChaCha20-Poly1305 record stream over a reliable,
ordered byte connection**, separate from control framing:

- One record = one H.264 access unit (`host/src/media_transport.rs`).
- The media key comes from the authenticated control session's directional
  `SessionKeys`; the fixed test keys in diagnostics never apply to a real pair.
- Record plaintext: 8-byte big-endian PTS, one flags byte (keyframe,
  discontinuity), then Annex-B access-unit bytes.
- Bound: 16 MiB per access unit; a producer queue of depth 2.
- On queue pressure the oldest unit is dropped and the next delivered unit
  carries `discontinuity = true`.
- A reconnect or decoder reset requests a keyframe over the authenticated
  control channel; the producer must restart/reconfigure to emit SPS/PPS + IDR.
- The record cipher is parameterised (`RecordCipher::with_limit`) so control
  frames keep their 64 KiB cap while media can carry a full access unit.

The initial deployment target is LAN/localhost. Relay/NAT traversal, congestion
control and loss recovery are explicit future work, not implied by this choice.

## Rejected alternatives

### WebRTC (data channel or RTP media)

Pros: ICE/NAT traversal, congestion control, mature iOS support.

Rejected now because it adds a large native stack on the Rust host, a signaling
protocol we do not have, ICE/TURN infrastructure, and a substantially larger
security-review surface before the first real client playback. The access-unit
and discontinuity semantics already exist and are testable without it. WebRTC
can be reconsidered when remote (non-LAN) streaming is in scope; the media
record boundary is transport-neutral enough to be adapted.

### QUIC

Pros: multiplexing, connection migration, TLS 1.3, loss-aware streams.

Rejected now because it introduces a QUIC implementation/dependency and a second
cryptographic stack while the control channel is still PSK-based with no forward
secrecy. It does not remove the need for the access-unit queue, keyframe and
discontinuity rules defined here. It remains a candidate for the remote phase.

### Plain TCP without AEAD

Rejected: media would be readable and malleable on the network, and a paired
client would not be authenticated at the media layer.

## Consequences

- Media requires a reliable ordered connection; a lost connection is handled as a
  discontinuity plus keyframe request, not per-packet recovery.
- No forward secrecy (inherited from the PSK control design) and no metadata
  protection beyond what the record encryption provides.
- No congestion control: the producer must stay bounded and the consumer must
  tolerate dropped units through the explicit discontinuity.
- Header/timestamp protection is included in the AEAD plaintext, so tampering
  with PTS or flags fails authentication.
- Tests cover round-trip metadata, tamper/wrong-direction rejection, truncation,
  the 16 MiB bound, queue-drop discontinuity, and the existing two-process
  synthetic stream now uses this codec end to end.

## Follow-up work

- Transport implementation over the real host/client connection (not just the
  synthetic diagnostic), with reconnect handling.
- Remote deployment: evaluate WebRTC/QUIC, certificate identity and forward
  secrecy.
- Congestion/bitrate feedback from the client and adaptive encoder settings.
- iOS media path (VideoToolbox) consuming these records.
