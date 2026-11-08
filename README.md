# Vista

Vista is an open-source native application-streaming platform for Linux hosts and iPhone/iPad clients. It aims to stream **existing Wayland windows** and **independent remote application sessions**, with touch, keyboard, pointer, audio, and clipboard support.

Inspired by [Sill](https://github.com/Saffsanity/sill); not a fork or a drop-in Sill backend. For iOS/iPadOS development, use the [upstream Sill source-to-Vista map](docs/sill-reference.md) before reimplementing UI controls.

## Status

**Milestone 0 — live Linux host discovery.** The Rust host CLI discovers running Niri windows by ID, emits an incremental event stream for the window catalog, and probes the desktop ScreenCast portal without requesting capture. The iOS/iPadOS SwiftUI preview is now in `client/`, with no network connection or media yet. Vista does **not** yet have an interactively verified live pixel-capture path, network video streaming, authenticated clients, or isolated application sessions. The opt-in `capture-h264 --confirm` diagnostic now wires a portal-approved window to PipeWire, bounded RGBA handling and an in-memory H.264 encoder, but requires manual desktop approval to verify the actual live path. An opt-in `capture-consent` diagnostic requests a single-window ScreenCast selection and reports negotiated PipeWire nodes, then closes the session without accessing pixels. A separate opt-in `capture-frame` diagnostic now attempts to read exactly one selected window frame in memory through the portal-authorized PipeWire remote, returning only timing/size metadata. It has compiled but has not yet been verified against a manually approved desktop chooser. An in-memory `sync-test` now verifies the permission-scoped decode path end to end (synthetic RGBA → H.264 → decoder-resynchronization gate → ChaCha20-Poly1305 media record over a localhost TCP loopback → H.264 decoder) with one injected loss and reports traversal latency; it uses no desktop pixels or external network, and the record cipher is a fixed test key rather than a real pairing. See [architecture](docs/architecture.md) and [roadmap](docs/roadmap.md).

## Develop

Requirements: Rust/Cargo, a running Niri compositor and the `niri` executable on PATH. The optional `capture-frame` diagnostic also needs GStreamer development libraries and plugins (`gstreamer-1.0`, `gstreamer-app-1.0`, `pipewiresrc`, `videoconvert` and `appsink`), PipeWire and a working desktop ScreenCast portal.

```sh
cd host
cargo test
cargo run -- doctor
cargo run -- windows
cargo run -- windows --json
cargo run -- window 5 --json
cargo run -- watch --json
cargo run -- capabilities --json
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
cargo run -- capture-h264 --confirm --json
```

Window IDs come from the active Niri session and can change after a window closes or the compositor restarts. `--json` supports programmatic consumers. When running through a tool or service with a sanitized environment, export `NIRI_SOCKET` from the active Niri session before invoking the host CLI.

## Repository layout

- `host/`: Rust Linux host service foundation, including Niri window discovery, live window catalog events, and read-only portal capability diagnostics.
- `client/`: native iOS/iPadOS SwiftUI shell, portable Swift models, and an [interactive Arch browser preview](client/preview/README.md) for UI iteration; see [client setup](client/README.md).
- `docs/architecture.md`: host, transport, iOS, compositing and session architecture.
- `docs/roadmap.md`: phased implementation and acceptance milestones.

## Principles

Native-first, least privilege, compositor-aware integration without patching Niri, clear separation of window discovery/capture/encoding/transport/input, and no false claims that isolated Wayland sessions can be created solely through an existing Niri window.
