# Vista

Vista is an open-source native application-streaming platform for Linux hosts and iPhone/iPad clients. It aims to stream **existing Wayland windows** and **independent remote application sessions**, with touch, keyboard, pointer, audio, and clipboard support.

Inspired by [Sill](https://github.com/Saffsanity/sill); not a fork or a drop-in Sill backend. For iOS/iPadOS development, use the [upstream Sill source-to-Vista map](docs/sill-reference.md) before reimplementing UI controls.

## Status

**Milestone 0 — live Linux host discovery.** The Rust host CLI discovers running Niri windows by ID, emits an incremental event stream for the window catalog, and probes the desktop ScreenCast portal without requesting capture. The iOS/iPadOS SwiftUI shell is now in `client/`, and its `VistaCore` package implements and tests the v1 control protocol, pairing crypto and encrypted record layer; the app can pair with a host and open an authenticated control session, but has no catalog, media or input yet. Vista now has an interactively verified live pixel-capture path: on 2026-11-09 an explicitly consented Niri window was captured through the ScreenCast portal, encoded with NVENC and bounded in memory (29/29 frames at 1280×720, p50/p95 capture-to-encode 0.83/1.74 ms, one requested keyframe produced a second IDR); see [capture verification](docs/capture-verification.md). It does **not** yet have network video streaming, a remote client, or isolated application sessions. The opt-in `capture-h264 --confirm` diagnostic now wires a portal-approved window to PipeWire, bounded RGBA handling and an in-memory H.264 encoder, but requires manual desktop approval to verify the actual live path. `capture-h264 --confirm --window ID` additionally watches the explicit Niri target, and a deterministic `WindowLifecycle` guard (`lifecycle-test --json`) maps window removal, portal closure, permission revocation, compositor restart and resize to safe flush/termination decisions. The bounded run is configurable (`--seconds 1..120`, up to 3600 frames), reports capture-to-encode p50/p95 latency and keyframe counts, and can request a keyframe at an input frame (`--keyframe-at`) to exercise IDR recovery. Hardware verification is recorded in [capture verification](docs/capture-verification.md). An opt-in `capture-consent` diagnostic requests a single-window ScreenCast selection and reports negotiated PipeWire nodes, then closes the session without accessing pixels. An opt-in `capture-output` diagnostic now captures the entire active output through grim/wlr-screencopy, encodes one real 1920x1080 frame with NVENC and decodes it, reporting only dimensions and byte counts; it is an explicit full-output fallback, not portal per-window capture, and persists no pixels. A separate opt-in `capture-frame` diagnostic now attempts to read exactly one selected window frame in memory through the portal-authorized PipeWire remote, returning only timing/size metadata. It has compiled but has not yet been verified against a manually approved desktop chooser. A `pipewire-loopback` diagnostic now creates a local synthetic PipeWire video node and reads it back through the same `pipewiresrc` caps and RGBA bound checks used by capture, without requesting portal consent; one run delivered 5 frames / 1,152,000 bytes at 320x180. It does not prove per-window portal capture or real content. An in-memory `sync-test` now verifies the permission-scoped decode path end to end (synthetic RGBA → H.264 → decoder-resynchronization gate → ChaCha20-Poly1305 media record over a localhost TCP loopback → H.264 decoder) with one injected loss and reports traversal latency; after the loss the consumer requests a keyframe and the producer restarts the encoder to produce a fresh SPS/PPS IDR rather than waiting for a periodic GOP. It uses no desktop pixels or external network, and the record cipher is a fixed test key rather than a real pairing. See [architecture](docs/architecture.md) and [roadmap](docs/roadmap.md).

A versioned, transport-neutral control protocol now exists in `host/src/protocol.rs`: bounded JSON frames, explicit version negotiation, a 256-bit out-of-band pairing secret with HMAC-SHA256 confirmation, HKDF session keys, strict sequencing, expiring offers and deny-by-default permissions. It is verified in memory by `protocol-test`, and a localhost-only TCP loopback (`protocol-loopback`) now pairs and exchanges ChaCha20-Poly1305-encrypted control records. A one-shot `protocol-serve` listener and `protocol-client` reference client can perform the same exchange across two processes; the listener refuses non-loopback addresses unless `--allow-remote` is explicit. The fixtures and contract are in [docs/protocol.md](docs/protocol.md). An authenticated `keyframe-control-test` now carries `KeyframeRequest` from a control client over the AEAD record transport and restarts the synthetic encoder (1 request, 1 restart, 10 decoded frames in one run). A two-process `media-stream` now runs the control pairing handshake first, derives the media key from the authenticated video-only session and decodes 12/12 synthetic H.264 access units (2,764,800 bytes) in the receiving process; wrong pairing secrets are rejected. The client also sends an authenticated `KeyframeRequest` over the reverse record direction, causing one encoder restart and a fresh IDR. The offer carries only the out-of-band test pairing secret, not the media key. Neither is a paired/client-authenticated production stream. A backend-agnostic capability/action/portable-input contract now exists in `host/src/capability.rs`: hosts advertise versioned capabilities, the registry revalidates target/revision/permissions before execution, and `capability-test --json` exercises denial, staleness, availability, duplicate, reconnect and held-input cleanup paths. `actions --window ID` builds a live read-only snapshot for an existing Niri window and reflects the requested grant state instead of inventing permission. A revocable per-session control lease (`host/src/control_lease.rs`) owns action/input grants, advances the grant epoch on revoke, rejects stale action/input requests and releases all held buttons/keys. A session-bound dispatcher (`host/src/control_dispatch.rs`) now attaches that lease to an authenticated `HostSession`, derives authorization from the lease, serves the grant-scoped capability snapshot and routes `action.invoke` to truthful `action.result` messages; wrong-session, revoked, stale-grant, wrong-target and malformed requests are rejected before the backend runs, and `control-dispatch-test --json` proves it through a real in-memory pairing. The one-shot `protocol-serve` listener now accepts an opt-in action policy (`--window ID` plus `--allow-*` grants) and dispatches one client action over the encrypted socket; `--fake-actions` is diagnostic-only and the default backend is Niri. A reusable media-record codec now carries one H.264 access unit per AEAD record with PTS, keyframe and discontinuity metadata (`host/src/media_transport.rs`); the two-process synthetic stream uses it end to end, a bounded queue drops the oldest unit and marks the next one discontinuous, and [ADR 0001](adr/0001-media-transport.md) records the custom-record decision plus rejected WebRTC/QUIC alternatives. There is still **no production listener, discovery, TLS/QUIC/WebRTC media transport, native input injection or iOS integration**.

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

## Repository layout

- `host/`: Rust Linux host service foundation, including Niri window discovery, live window catalog events, and read-only portal capability diagnostics.
- `client/`: native iOS/iPadOS SwiftUI shell, portable Swift models, control protocol/pairing, and an [interactive Arch browser preview](client/preview/README.md) for UI iteration; see [client setup](client/README.md).
- `docs/protocol.md`: versioned host/client control protocol, pairing, threat model and transport recommendation.
- `docs/architecture.md`: host, transport, iOS, compositing and session architecture.
- `docs/roadmap.md`: phased implementation and acceptance milestones.
- `docs/adr/`: architecture decisions; [0001](docs/adr/0001-media-transport.md) covers the media record transport.
- `docs/plan.md`: ordered delivery plan with phase gates and the next PR sequence.
- `docs/capture-verification.md`: recorded real-hardware per-window capture evidence and limitations.

## Principles

Native-first, least privilege, compositor-aware integration without patching Niri, clear separation of window discovery/capture/encoding/transport/input, and no false claims that isolated Wayland sessions can be created solely through an existing Niri window.