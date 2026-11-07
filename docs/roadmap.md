# Vista implementation plan

## M0 — host foundation (in progress)

- [x] Initialize project structure and Rust host crate.
- [x] Niri window metadata discovery and JSON CLI, with mocked transport tests.
- [x] Live Niri IPC smoke test.
- [x] Niri event stream subscription and window catalog reconciliation.
- [x] Read-only Niri and desktop portal ScreenCast capability probe with structured partial failures (not a proof of functional window capture).

## M1 — existing-window video proof of concept

- [ ] Verify actual xdg-desktop-portal consented per-window capture on Niri (portal source flags alone do not prove it). A `capture-consent --confirm` diagnostic now negotiates an opt-in *window-only* ScreenCast session and reports PipeWire node IDs, without accessing frames; live consent/stream verification is pending.
- [ ] Implement explicitly consented PipeWire capture and frame lifecycle. Bounded single-frame and eight-frame GStreamer diagnostics have synthetic frame, partial-sequence timeout, and cleanup tests; manual desktop/portal verification and an indefinite streaming lifecycle are pending.
- [ ] Encode frames and show a locally decoded test stream with latency measurements.
- [ ] Handle window removal, resize, occlusion and permissions safely.

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
