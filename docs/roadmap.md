# Vista implementation plan

## M0 — host foundation (in progress)

- [x] Initialize project structure and Rust host crate.
- [x] Niri window metadata discovery and JSON CLI, with mocked transport tests.
- [x] Live Niri IPC smoke test.
- [x] Niri event stream subscription and window catalog reconciliation.
- [x] Read-only Niri and desktop portal ScreenCast capability probe with structured partial failures (not a proof of functional window capture).

## M1 — existing-window video proof of concept

- [ ] Verify actual xdg-desktop-portal consented per-window capture on Niri (portal source flags alone do not prove it). A `capture-consent --confirm` diagnostic now negotiates an opt-in *window-only* ScreenCast session and reports PipeWire node IDs, without accessing frames; live consent/stream verification is pending.
- [ ] Implement explicitly consented PipeWire capture and frame lifecycle. Bounded single-frame and eight-frame GStreamer diagnostics have synthetic frame, partial-sequence timeout, and cleanup tests. Both readers now validate RGBA caps, dimensions, and frame-buffer bounds, including negative tests. Burst reads distinguish a terminated stream from a silent timeout and test cleanup in both cases. A three-second/120-frame aggregate-only capture monitor now exercises a longer-lived capture lifecycle with bounded AppSink buffering; synthetic EOS, timeout, invalid-cap and frame-limit tests verify shutdown. Manual desktop/portal verification and an indefinite streaming lifecycle are pending.
- [ ] Encode frames and show a locally decoded test stream with latency measurements. H.264 factory discovery and a bounded synthetic encode/decode CLI now exist; codec-test verifies actual compressed and decompressed samples without desktop capture. A separate `bridge-test --json` diagnostic now transfers eight synthetic RGBA frames through the permission-scoped `StreamSession` queue into a real GStreamer H.264 AppSrc encoder; NVIDIA hardware integration passed. The opt-in `capture-h264 --confirm` diagnostic now wires an explicitly portal-granted single window through the restricted PipeWire stream and bounded session into an in-memory H.264 encoder, with closure detection and bounded termination. Synthetic tests exercise the same worker boundary, fault injection, and prompt cancellation of a stalled source, with pipeline cleanup assertions. The diagnostic inspects both GStreamer pipeline buses for asynchronous source/encoder errors instead of reporting all failures as timeouts, and prioritizes portal closure over a simultaneously ready worker result. Actual Niri window-to-H.264 output and interactive revocation still require manually approved verification. The separate `bridge-test` uses synthetic authorization; `capture-h264` requires an actual portal window grant but has not yet been tested against an approved chooser. A new `sync-test --json` diagnostic closes the local loop from synthetic RGBA through the permission-scoped queue, the real H.264 encoder, the decoder-resynchronization gate and a real H.264 decoder, injecting one loss and verifying suppressed deltas plus recovery on a fresh IDR; it reports in-process encode→decode traversal latency, not capture or network latency. None of these prove live capture latency or iOS playback.
- [ ] Handle window removal, resize, occlusion and permissions safely. A pure host-side stream-session guard now enforces explicit authorization, token invalidation on revocation, terminal shutdown, bounded two-frame latest-first backpressure, resolution-change queue flushing and strict RGBA geometry validation, with unit tests. The bounded diagnostic now connects this guard to the portal/PipeWire worker, and a portal `Closed` notification cancels capture. Continuous streaming, complete compositor/window disappearance handling, and interactive revocation verification remain pending.

The host now also has a **transport-neutral, permission-scoped H.264 access-unit handoff**: a two-unit bounded queue, 16 MiB maximum per unit, presentation timestamp/keyframe metadata, oldest-unit drop counters, resize flushing, and revocation invalidation. The live capture-to-encoder diagnostic fills and drains this in-memory handoff; it does **not** send bytes to a client. The encoded sink explicitly negotiates Annex-B byte-stream H.264 with AU alignment. The new transport-neutral `H264SyncGate` now signals explicit decoder discontinuities for startup, encoded-queue overflow, resize, GStreamer `DISCONT`, consumer resets/reconnects, invalid bitstream headers and conservative codec-configuration changes, suppressing delta units until an SPS/PPS-prefixed IDR. The live H.264 parser is configured to repeat parameter sets at IDR and its encoded AppSink uses backpressure rather than silent drop. This is NAL-header recovery gating, **not** full H.264 validation, automatic IDR requests, detection of every upstream loss, or an authenticated streaming client; future producers/transport must signal any additional losses and request recovery keyframes as needed. Authenticated network transport remains separate. Unit tests and the ignored synthetic live-worker test cover this boundary. No real Niri capture or remote playback is claimed.

## M2 — secure remote session

- [ ] Host-client authenticated pairing and session state machine.
- [ ] Low-latency encrypted transport and reconnect/backpressure.
- [ ] Input translation: pointer, touch gestures, keyboard and modifiers.
- [ ] Per-stream permissions and revoke mechanism.

## M3 — iOS/iPadOS client

- [x] Initial SwiftUI iPhone/iPad interface and portable Swift model package (preview data only; macOS build verification pending).
- [ ] Actual host discovery, authenticated pairing and live catalog integration.
- [ ] Hardware video decoder, responsive native input and keyboard UX.
- [ ] App launcher/gallery informed by Sill's polished UX. See [Sill UI source reference and reuse map](sill-reference.md) before implementing iOS screens.
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
