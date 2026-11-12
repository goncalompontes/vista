# Vista

Vista streams your **existing Linux desktop windows** to native iOS and Android
clients — instead of starting a separate virtual desktop and rebuilding your
session inside it.

[![Rust host](https://github.com/goncalompontes/vista/actions/workflows/rust.yml/badge.svg)](https://github.com/goncalompontes/vista/actions/workflows/rust.yml)
[![iOS client](https://github.com/goncalompontes/vista/actions/workflows/ios.yml/badge.svg)](https://github.com/goncalompontes/vista/actions/workflows/ios.yml)
[![Portable client checks](https://github.com/goncalompontes/vista/actions/workflows/client-portable.yml/badge.svg)](https://github.com/goncalompontes/vista/actions/workflows/client-portable.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

## Status

**Experimental end-to-end MVP.** Verified on 2026-11-11: a real Niri
host captured a 1920×1080 desktop, encoded it to H.264, and delivered it over an
authenticated connection to an Android 36 emulator (654 frames over ~75 s) and to
a Rust reference client (448 frames over 30 s, including two target switches).
Touch and hardware-key input reached the host. The iOS client runs and plays
video in a VM; it has not been exercised end to end through its UI.

## What works today

- **Window, workspace and desktop targets.** Pick one window, one workspace, or
  the active desktop. Switching targets in one session restarts the producer
  cleanly.
- **Consented capture.** xdg-desktop-portal ScreenCast for a selected window,
  with a `grim` full-output fallback. Consent is explicit and per run.
- **H.264 with keyframe recovery.** NVENC preferred, with x264 and OpenH264
  available. A decoder that loses sync requests a keyframe over the control
  channel; the producer restarts the encoder and emits a fresh SPS/PPS IDR.
- **An authenticated control channel.** Pre-shared-key pairing with mutual HMAC
  confirmation, HKDF directional session keys, strict sequencing, and a bounded
  AEAD record layer.
- **Capability-driven actions and input.** The client receives a versioned
  capability snapshot and can invoke described actions, send portable pointer
  and key events, and commit text. Every action is re-authorised against a
  revocable lease.
- **Native clients.** iOS/iPadOS SwiftUI with a portable Swift core and
  VideoToolbox playback; Android in Kotlin/Jetpack Compose with `MediaCodec`.

## Build and run the host

Requirements: Linux with [Niri](https://github.com/YaLTeR/niri), Rust 1.88+, and
the GStreamer development packages. Clients need iOS 17+ or Android 8+ (API 28).

On Debian/Ubuntu, install the build dependencies first:

```sh
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
```

Then build, test, and inspect the live environment:

```sh
git clone https://github.com/goncalompontes/vista
cd vista/host
cargo test --workspace
cargo run -- doctor
cargo run -- windows
```

`doctor` reports whether the Niri socket is reachable. `windows` lists the
targets you can stream. Then start the experimental host and pair a client:

```sh
cargo run -- serve --confirm --listen 127.0.0.1:0
```

It prints a one-shot pairing offer and accepts a single client.

## What does not work yet

- Audio and clipboard.
- Discovery, multi-client support, reconnection guarantees and congestion
  control.
- TLS or certificate-based identity: the current pairing secret has no forward
  secrecy, and `vista-host serve` has no TLS. Keep it on loopback.
- Isolated application sessions. Window capture requires portal consent, and on
  Niri selecting a window focuses and fullscreens it, so capture is not
  isolated.
- Physical-device runs on iOS or Android (emulator/VM only so far).

If you need audio, clipboard, and a mature multi-client experience today, use
[Sunshine](https://github.com/LizardByte/Sunshine) with
[Moonlight](https://moonlight-stream.org/), or your desktop's RDP/VNC server.
Vista's bet is on *existing windows* and *native clients*, and it is early.

## How it differs

- **Existing windows, not a rebuilt desktop.** Vista asks the compositor and the
  desktop portal for a window that is already running. It does not spawn a
  headless session and ask you to log in again.
- **Compositor-aware without patching it.** Niri is read through its documented
  JSON-over-Unix-socket IPC, with no GPL helper crate linked into the host. The
  same interface is written to work for other desktops through portal
  capabilities.
- **A protocol, not a codec tunnel.** Capability snapshots, action descriptors,
  lease-scoped input, and a versioned wire format with shared fixtures across
  Rust, Swift and Kotlin. The protocol is documented in
  [docs/protocol.md](docs/protocol.md).
- **Evidence over adjectives.** Claims about working features name the run that
  produced them and what that run did not prove. Where something is unverified,
  the docs say so.

## Client apps

- **iOS/iPadOS** — SwiftUI app in `client/ios`, built with Xcode and
  [XcodeGen](https://github.com/yonaskolb/XcodeGen). See
  [client/README.md](client/README.md).
- **Android** — native Kotlin/Compose app. Build the debug APK with
  `client/android/build-android.sh`; JDK 17+ and Android SDK 36 required. See
  [client/android/README.md](client/android/README.md).
- **Rust reference client** — `vista-host live-client`, used for the recorded
  end-to-end runs.
- **Browser preview** — a UI iteration mock, not a client; see
  [client/preview/README.md](client/preview/README.md).

## Documentation

- [Protocol and threat model](docs/protocol.md) — wire contract, pairing, states.
- [Architecture](docs/architecture.md) and
  [target architecture](docs/architecture-target.md).
- [Delivery plan](docs/plan.md) and [roadmap](docs/roadmap.md).
- [Verification records](docs/e2e-verification.md) — what was run, on what
  hardware, and what it does not prove.
- [Host diagnostics](docs/diagnostics.md) — the full one-shot command catalogue.
- [Computer-use MCP](docs/computer-use.md) — the vphone/QEMU iOS workflow and host tools.
- [ADRs](docs/adr/) — the decisions behind media transport, platform-neutral
  core, device enrollment, licensing, and schema ownership.
- [Sill reference map](docs/sill-reference.md) — the UI/source map for the client
  work.

## Repository layout

- `host/` — Rust Linux host: Niri discovery and IPC, portal capture, H.264
  encoding, control and media transport, capability/action dispatch, input.
- `client/core/` — portable Swift package shared by the iOS app and tested on
  Linux.
- `client/ios/` — native iOS/iPadOS SwiftUI app.
- `client/android/` — native Kotlin/Jetpack Compose app.
- `client/preview/` — browser UI mock for iteration.
- `integrations/vista-computer-use/` — cross-client MCP server for the
  vphone/QEMU iOS workflow and the host service.
- `docs/` — protocol, architecture, verification evidence, ADRs, plan.

## Project

- [Contributing](CONTRIBUTING.md) — setup, tests, commit conventions, DCO.
- [Support](SUPPORT.md) — what is supported and how to ask.
- [Security](SECURITY.md) — private reporting and known limitations.
- [Governance](GOVERNANCE.md) — who decides, and the path to more maintainers.
- [Release policy](RELEASES.md) and [changelog](CHANGELOG.md).
- [Code of conduct](CODE_OF_CONDUCT.md).
- **License:** [Apache-2.0](LICENSE), with [NOTICE](NOTICE) and
  [third-party notices](THIRD_PARTY_NOTICES.md).

Vista is inspired by [Sill](https://github.com/Saffsanity/sill); it is not a
fork and not a drop-in Sill backend. Sill does not endorse Vista.

## Principles

Native-first. Least privilege. Compositor-aware integration without patching the
compositor. A clear separation between discovery, capture, encoding, transport
and input. No claim that an isolated Wayland session can be created from an
existing compositor window. No false claims about reliability — if a path was
not exercised, the docs say so.
