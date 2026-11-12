# Changelog

All notable changes to Vista are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the 0.x rules
written in [RELEASES.md](RELEASES.md).

No release has been tagged yet. Everything below sits under **Unreleased** and
will become **0.1.0**, the first tag.

## [Unreleased]

### Added

- **Rust host for Linux.** Niri window and workspace discovery, a live window
  catalog with incremental updates, and a passive desktop-portal capability
  probe. `vista-host windows`, `window`, `watch`, `capabilities`,
  `desktop-probe` and `desktop-probe-watch` are the entry points.
- **Consented window capture.** Portal ScreenCast consent for a single window,
  bounded PipeWire frame diagnostics, and an in-memory RGBA → H.264 pipeline
  with NVENC preferred and x264/OpenH264 available. Verified on real Niri
  hardware: 29 frames at 1280×720 from an approved chooser, p50/p95
  capture-to-encode 0.83/1.74 ms
  ([evidence](docs/capture-verification.md)).
- **Versioned control protocol v1.** Pre-shared-key pairing with mutual HMAC
  confirmation, HKDF directional session keys, strict sequencing, deny-by-default
  permissions, and a bounded encrypted record layer.
- **Capability and action contract.** Versioned capability snapshots, action
  descriptors and results, target/generation freshness checks, revocable control
  leases, and a session-bound dispatcher that authorizes every action against the
  lease.
- **Authenticated media transport.** Media records carry presentation timestamp,
  keyframe and discontinuity metadata over an AEAD record layer on TCP, with
  keyframe-on-demand recovery through the control channel.
- **iOS/iPadOS client.** SwiftUI app and a portable `VistaCore` package with
  pairing, the encrypted control and media clients, a live window catalog, and
  H.264 playback through VideoToolbox.
- **Native Android client.** Kotlin/Jetpack Compose app with pairing, the
  encrypted session, live catalog, capability snapshot, semantic actions,
  authenticated H.264 playback through `MediaCodec`, touch-to-pointer input and
  hardware key events. Wire code is checked against the shared Rust/Swift
  fixture. Verified on an Android 36 emulator at 1920×1080
  ([evidence](docs/android-verification.md)).
- **Reference client.** `vista-host live-client` connects to a real host,
  receives video for a window, workspace or desktop target, and can switch
  targets in one session. Verified for 448 frames over 30 s at 1920×1080
  ([evidence](docs/e2e-verification.md)).
- **Licensing.** Vista's own source is now Apache-2.0, with a `NOTICE` and a
  third-party dependency inventory. Previously the repository had no license
  file.
- **Project documents.** `CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`,
  `GOVERNANCE.md`, `RELEASES.md`, a code of conduct, issue and pull-request
  templates, and a DCO check on pull requests.

### Changed

- Window discovery no longer shells out to `niri msg`; the host talks to Niri's
  documented JSON-over-Unix-socket IPC directly. See
  [ADR 0004](docs/adr/0004-license-and-niri-ipc-policy.md).
- The Android client is native Kotlin. The Skip transpile experiment was
  retired; the shared `VistaCore` sources remain iOS-only.

### Fixed

- A stream discontinuity now clears cached SPS/PPS and flushes queued sample
  buffers, removing the stale-frame state seen after selecting a new target on
  iOS.
- Live reference-client stream handling survives an early close and reports the
  close instead of hanging.

### Security

- Input injection is bound to a revocable control lease; revocation releases
  held input and rejects stale-epoch requests.

### Known limitations

These are stated so nobody discovers them as a surprise. They are not bugs
waiting for a report; see [SECURITY.md](SECURITY.md).

- `vista-host serve` is single-client, has no TLS, and is meant for loopback.
- Pairing uses a pre-shared secret with no forward secrecy.
- No discovery, no reconnection guarantee, no congestion control.
- No audio, no clipboard, no isolated application sessions.
- Window capture requires portal consent and, on Niri, a chooser; selecting a
  window currently focuses and fullscreens it, so capture is not isolated.
- No physical iOS device run; no physical Android device run.
