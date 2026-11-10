# Vista

Vista is an open-source native application-streaming platform for Linux hosts and iPhone/iPad clients. It aims to stream **existing Wayland windows** and **independent remote application sessions**, with touch, keyboard, pointer, audio, and clipboard support.

Inspired by [Sill](https://github.com/Saffsanity/sill); not a fork or a drop-in Sill backend. For iOS/iPadOS development, use the [upstream Sill source-to-Vista map](docs/sill-reference.md) before reimplementing UI controls.

## Status

**Experimental end-to-end MVP, in progress.** The Rust host can pair with the
iOS client, advertise Niri windows and virtual desktops, and send encrypted
H.264 to the client's VideoToolbox view. On 2026-11-10, a vphone iOS VM paired
with the host and displayed video for the active desktop and a virtual desktop
in the same session. Selecting an individual window subsequently showed a
stale frame; sustained playback and target switching still need investigation.
Keyboard/pointer input and host actions are implemented but have not yet been
verified end to end through the iOS UI. See the [test record and next steps](integrations/vista-computer-use/docs/END_TO_END.md#what-was-actually-exercised).

The experimental `vista-host serve` is single-client and has no TLS. Keep it
on loopback and treat its pairing invite as a secret. Its live capture path
uses `grim` to repeatedly capture the active output; selecting a window focuses
and fullscreens it, so window capture is not isolated. Encoding prefers NVIDIA
NVENC when available, but screen capture uses the compositor's GPU. Independent
application sessions, audio, clipboard, production discovery, and robust
reconnection are not yet implemented.

Earlier capture and transport diagnostics are described in
[capture verification](docs/capture-verification.md), [architecture](docs/architecture.md),
and [the roadmap](docs/roadmap.md). They exercise different paths and do not
by themselves establish the end-to-end MVP's reliability.

## Develop

Requirements: Rust 1.88+ (edition 2024), a running Niri compositor and the `niri` executable on PATH. CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on Ubuntu with the GStreamer development packages. The optional `capture-frame` diagnostic also needs GStreamer development libraries and plugins (`gstreamer-1.0`, `gstreamer-app-1.0`, `pipewiresrc`, `videoconvert` and `appsink`), PipeWire and a working desktop ScreenCast portal.

```sh
cd host
cargo test
cargo run -- doctor
cargo run -- windows
cargo run -- windows --json
cargo run -- window 5 --json
cargo run -- watch --json
cargo run -- capabilities --json
# Backend-agnostic capability/action contract diagnostic; no capture or input injection:
cargo run -- capability-test --json
# Session-bound capability/action dispatch through an in-memory paired session:
cargo run -- control-dispatch-test --json
# Read-only capability discovery for a live Niri window (no action is executed);
# add --allow-window-actions/--allow-desktop-actions to show granted state:
cargo run -- actions --window 5 --json
# In-memory control-protocol pairing/session diagnostic; no network, capture or input:
cargo run -- protocol-test --json
# Localhost-only TCP pairing + encrypted control-record loopback; no external listener:
cargo run -- protocol-loopback --json
# One-shot control listener (prints a pairing offer, accepts one client):
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --json
# Opt-in action policy over that listener (fake backend = no desktop side effects):
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --window 5 \
  --allow-window-actions --allow-input --fake-actions --json
# Reference client: video-only, or read the snapshot and send one action:
cargo run -- protocol-client --connect 127.0.0.1:PORT --secret-hex HEX --json
cargo run -- protocol-client --connect 127.0.0.1:PORT --secret-hex HEX \
  --allow-input --action window.focus --json
# Include a bounded Niri window catalog in the authenticated session:
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --catalog --json
# Authenticated localhost control KeyframeRequest -> encoder restart:
cargo run -- keyframe-control-test --json
# Two-process encrypted synthetic H.264 media stream (localhost; test key):
cargo run -- media-stream --serve --listen 127.0.0.1:0 --json
cargo run -- media-stream --connect ADDR --secret-hex HEX --json
# Read-only H.264 encoder factory discovery; no desktop capture or GPU encoding:
cargo run -- encoders --json
# Synthetic, permission-free bounded H.264 encode/decode test; requires working codec plugins:
cargo run -- codec-test --json
# Synthetic RGBA -> bounded StreamSession -> H.264 encoder bridge, no desktop access:
cargo run -- bridge-test --json
# Synthetic encode -> session -> sync gate -> AEAD/TCP media record -> decode, with one injected loss:
cargo run -- sync-test --json
# Opens a desktop consent chooser; does NOT capture frames:
cargo run -- capture-consent --confirm --json
# Opens a desktop consent chooser; attempts one in-memory frame (no file or network output):
cargo run -- capture-frame --confirm --json
# Opens a desktop consent chooser; inspects up to eight transient frames (no file or network output):
cargo run -- capture-burst --confirm --json
# Opens a desktop consent chooser; observes transient frames for at most 3 seconds or 120 frames:
cargo run -- capture-monitor --confirm --json
# Explicit window-only consent; transient PipeWire RGBA -> StreamSession -> H.264 (up to 8 frames / 5 seconds):
cargo run -- capture-h264 --confirm [--window ID] [--seconds N] [--keyframe-at N] --json
# Deterministic window lifecycle guard; no compositor or capture access:
cargo run -- lifecycle-test --json
# Explicit full-output wlr-screencopy fallback; captures the active output in memory and encodes/decodes one frame:
cargo run -- capture-output --confirm --json
```

Window IDs come from the active Niri session and can change after a window closes or the compositor restarts. `--json` supports programmatic consumers. When running through a tool or service with a sanitized environment, export `NIRI_SOCKET` from the active Niri session before invoking the host CLI.

## Computer-use MCP for the vphone client

The vphone/QEMU iOS-client workflow and the host service are also exposed
through a reusable stdio MCP server at
[`integrations/vista-computer-use`](integrations/vista-computer-use). It wraps
the existing vphone-linux tools and `vista-host serve`/`live-client`, and ships
Codex plugin/manifest examples plus an OpenCode config. Tools cover VM
lifecycle/status, screenshots, accessibility trees, tap/swipe/key/text input,
app install/launch/logs, guest networking, and the host reference-client path.
See [computer-use.md](docs/computer-use.md) for the overview,
[the package README](integrations/vista-computer-use/README.md) for setup, and
[the end-to-end walkthrough](integrations/vista-computer-use/docs/END_TO_END.md)
for pairing the real iOS client in the emulator.

## Repository layout

- `host/`: Rust Linux host service foundation, including Niri window discovery, live window catalog events, and read-only portal capability diagnostics.
- `client/`: native iOS/iPadOS SwiftUI shell, portable Swift models, control protocol/pairing, and an [interactive Arch browser preview](client/preview/README.md) for UI iteration; see [client setup](client/README.md).
- `integrations/vista-computer-use/`: cross-client MCP server and Codex plugin for the vphone/QEMU iOS workflow and the Vista host service.
- `docs/protocol.md`: versioned host/client control protocol, pairing, threat model and transport recommendation.
- `docs/architecture.md`: host, transport, iOS, compositing and session architecture.
- `docs/roadmap.md`: phased implementation and acceptance milestones.
- `docs/adr/`: architecture decisions; [0001](docs/adr/0001-media-transport.md) covers the media record transport.
- `docs/plan.md`: ordered delivery plan with phase gates and the next PR sequence.
- `docs/capture-verification.md`: recorded real-hardware per-window capture evidence and limitations.

## Principles

Native-first, least privilege, compositor-aware integration without patching Niri, clear separation of window discovery/capture/encoding/transport/input, and no false claims that isolated Wayland sessions can be created solely through an existing Niri window.