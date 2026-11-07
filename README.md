# Vista

Vista is an open-source native application-streaming platform for Linux hosts and iPhone/iPad clients. It aims to stream **existing Wayland windows** and **independent remote application sessions**, with touch, keyboard, pointer, audio, and clipboard support.

Inspired by [Sill](https://github.com/Saffsanity/sill); not a fork or a drop-in Sill backend.

## Status

**Milestone 0 — live Linux host discovery.** The Rust host CLI discovers running Niri windows by ID, emits an incremental event stream for the window catalog, and probes the desktop ScreenCast portal without requesting capture. The iOS/iPadOS SwiftUI preview is now in `client/`, with no network connection or media yet. Vista does **not** yet capture pixels, stream video, authenticate clients, or launch isolated sessions. See [architecture](docs/architecture.md) and [roadmap](docs/roadmap.md).

## Develop

Requirements: Rust/Cargo, a running Niri compositor and the `niri` executable on PATH.

```sh
cd host
cargo test
cargo run -- doctor
cargo run -- windows
cargo run -- windows --json
cargo run -- window 5 --json
cargo run -- watch --json
cargo run -- capabilities --json
```

Window IDs come from the active Niri session and can change after a window closes or the compositor restarts. `--json` supports programmatic consumers. When running through a tool or service with a sanitized environment, export `NIRI_SOCKET` from the active Niri session before invoking the host CLI.

## Repository layout

- `host/`: Rust Linux host service foundation, including Niri window discovery, live window catalog events, and read-only portal capability diagnostics.
- `client/`: native iOS/iPadOS SwiftUI preview and portable Swift models; see [client setup](client/README.md).
- `docs/architecture.md`: host, transport, iOS, compositing and session architecture.
- `docs/roadmap.md`: phased implementation and acceptance milestones.

## Principles

Native-first, least privilege, compositor-aware integration without patching Niri, clear separation of window discovery/capture/encoding/transport/input, and no false claims that isolated Wayland sessions can be created solely through an existing Niri window.
