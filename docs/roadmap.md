# Vista implementation plan

## M0 — host foundation (in progress)

- [x] Initialize project structure and Rust host crate.
- [x] Niri window metadata discovery and JSON CLI, with mocked transport tests.
- [x] Live Niri IPC smoke test.
- [x] Niri event stream subscription and window catalog reconciliation.
- [x] Read-only Niri and desktop portal ScreenCast capability probe with structured partial failures (not a proof of functional window capture).

## M1 — existing-window video proof of concept

- [ ] Verify actual xdg-desktop-portal consented per-window capture on Niri (portal source flags alone do not prove it).
- [ ] Implement explicitly consented PipeWire capture and frame lifecycle.
- [ ] Encode frames and show a locally decoded test stream with latency measurements.
- [ ] Handle window removal, resize, occlusion and permissions safely.

## M2 — secure remote session

- [ ] Host-client authenticated pairing and session state machine. A versioned, transport-neutral control-frame codec and state machine now exist (`host/src/protocol.rs`, `protocol-test`) with HMAC-SHA256 confirmation, HKDF session keys, version negotiation, strict sequencing, offer expiry and deny-by-default permissions; negative tests cover wrong secrets, replay/order, expiry, permission escalation and version mismatch, and Swift-compatible JSON fixtures exist. No listener, discovery, TLS/QUIC/WebRTC transport, frame encryption, host identity signature or iOS integration yet; the PSK design has no forward secrecy. A versioned, transport-neutral control-frame codec and state machine now exist (`host/src/protocol.rs`, `protocol-test`) with HMAC-SHA256 confirmation, HKDF session keys, version negotiation, strict sequencing, offer expiry and deny-by-default permissions; negative tests cover wrong secrets, replay/order, expiry, permission escalation and version mismatch, and Swift-compatible JSON fixtures exist. No listener, discovery, TLS/QUIC/WebRTC transport, frame encryption, host identity signature or iOS integration yet; the PSK design has no forward secrecy.
- [ ] Low-latency encrypted transport and reconnect/backpressure. A bounded ChaCha20-Poly1305 control-record layer now encrypts frames after pairing, using implicit per-direction nonce counters and session/direction AAD; tamper, replay and wrong-direction records are rejected, and a localhost-only `protocol-loopback` diagnostic exercises the full pairing plus encrypted SessionOpen/KeyframeRequest/Ping over a real TCP socket. No production listener, TLS/QUIC/WebRTC, discovery, metadata protection, congestion control, reconnect/backpressure handling or media transport yet.
- [ ] Input translation: pointer, touch gestures, keyboard and modifiers.
- [ ] Per-stream permissions and revoke mechanism.

## M3 — iOS/iPadOS client

- [x] Initial SwiftUI iPhone/iPad interface and portable Swift model package (preview data only; macOS build verification pending).
- [ ] Actual host discovery, authenticated pairing and live catalog integration.
- [ ] Hardware video decoder, responsive native input and keyboard UX.
- [ ] App launcher/gallery informed by Sill's polished UX.
- [ ] Audio, clipboard, session reconnect and background handling.

## M4 — independent remote applications

- [ ] Benchmark headless nested compositor candidates with isolated Wayland sockets.
- [ ] Implement per-session app launcher, supervisor and teardown.
- [ ] Capture and stream independent surfaces; route input/audio/clipboard.
- [ ] Define real security and resource isolation separate from display isolation.

## M5 — reliability and distribution

- [ ] Cross-GPU / mixed DPI / HDR / scaling performance tests.
- [ ] Arch package + systemd user service, reproducible developer setup.
- [ ] Integration tests, privacy/security review and network stress tests.

Each milestone must be verified on actual hardware where relevant. The architecture document distinguishes implementation from feasibility hypotheses.
