# Vista delivery plan

Status as of **2026-11-11**. The ten baseline increments and the experimental
end-to-end MVP (PR #92) are merged; Rust host, iOS simulator and Linux
`VistaCore` CI are green on `main`. This document converts the milestone
checklist in [roadmap.md](roadmap.md) into an ordered, dependency-aware
sequence.

Related: [architecture](architecture.md), [protocol](protocol.md),
[Sill source map](sill-reference.md), and open issues
[#2](https://github.com/goncalompontes/vista/issues/2),
[#3](https://github.com/goncalompontes/vista/issues/3),
[#4](https://github.com/goncalompontes/vista/issues/4),
[#5](https://github.com/goncalompontes/vista/issues/5),
[#7](https://github.com/goncalompontes/vista/issues/7),
[#8](https://github.com/goncalompontes/vista/issues/8),
[#34](https://github.com/goncalompontes/vista/issues/34).

## Baseline

Implemented and tested in the repository:

- Niri window discovery, live catalog events and a read-only portal probe.
- Window-only ScreenCast consent, bounded PipeWire/frame diagnostics, output
  fallback, synthetic PipeWire loopback.
- Bounded H.264 encode/decode, synthetic end-to-end encode→sync gate→AEAD→decode,
  `StreamSession` backpressure/revocation and `H264SyncGate`.
- Versioned control protocol: PSK pairing, HMAC confirmation, HKDF keys, strict
  sequencing, permissions, encrypted localhost TCP control loopback, one-shot
  listener/client.
- Capability/action/portable-input contract, control lease, and a session-bound
  dispatcher that derives authorization from the lease and routes
  `action.invoke` to truthful results.
- Opt-in action policy on the one-shot `protocol-serve` listener with a
  two-process action probe.
- Window lifecycle guard for removal, portal close, revocation, compositor
  restart and resize, with an opt-in Niri watcher.
- Bounded persistent capture at native geometry with latency percentiles and
  keyframe-on-demand, verified on real Niri/NVENC hardware.
- Reusable H.264 media-record codec with PTS/keyframe/discontinuity and a
  bounded drop-discontinuity queue (ADR 0001).
- iOS/iPadOS Swift control transport, pairing crypto, Keychain-backed pairing
  sheet, live window catalog and VideoToolbox stream view with a synthetic
  self-test; Android Skip APK preview.

Demonstrated on 2026-11-11 with the Rust reference client (see
[e2e-verification.md](e2e-verification.md)):

- A real desktop capture encoded to H.264 and delivered over the authenticated
  media connection: 448 frames / 30 s at 1920x1080, plus two target-kind
  switches in one host session.

Not yet demonstrated anywhere:

- An action or input event reaching a real desktop through the transport. The
  dispatcher, lease and injector are unit-tested; no end-to-end run has moved a
  real pointer or typed real text.
- Native-client playback beyond the 2026-11-10 iOS run recorded in the README:
  no sustained iOS session, no Android runtime playback, no decoder recovery
  evidence from a real device.
- Indefinite streaming, interactive revocation, congestion control, packet-loss
  recovery, discovery, or isolated application sessions.
- Independent application sessions.

## Sequencing rules

1. **Prove the live host path before wrapping transport around it.** A streaming
   protocol is only worth building on top of pixels we can actually capture.
2. **Every claim must be executable evidence.** Diagnostics print metadata, not
   pixels; manual steps are labelled as manual. No fake pairing/stream success.
3. **One PR-sized increment with local tests.** Hardware/desktop steps are opt-in
   and report honest partial results.
4. **Client work consumes the contract; it does not invent one.** Swift models
   already mirror the fixtures; new wire fields land in Rust + fixtures first.
5. **Fail closed.** Revocation, stale epochs and missing grants reject before a
   side effect, on both sides of the wire.

## Critical path

```text
M1 live capture ──► M2 action/input over transport ──► M3 pairing + video on iOS
        │                        │                                  │
        └── window lifecycle ────┘                    Android parity ─┘
                                                       M4 independent sessions
                                                       M5 packaging/reliability
```

M4 and M5 depend on M1/M2 boundaries but can start on paper (benchmark, CI,
packaging) in parallel. Android follows M3 UI parity, not ahead of the protocol.

## Phase A — close the live host path (M1, issue #2)

**Goal:** a real, portal-approved Niri window streams H.264 access units with
measurable latency and safe lifecycle for several minutes.

| # | Increment | Deliverable | Verification |
|---|---|---|---|
| A1 | Attach capture to a real approved window | `capture-h264` verified against a manually approved chooser; bounded run report | Documented run: window → N frames → encoded/decoded bytes, latency percentiles |
| A2 | Persistent capture lifecycle | Indefinite (time/queue bounded) source → `StreamSession` → encoder; explicit stop/EOS | Ignored live test; synthetic long-run test; no unbounded queue growth |
| A3 | Window removal / resize / occlusion | Guard reacts to Niri catalog + portal `Closed`; flush on resize; revoke on loss | Unit tests for each transition; opt-in live close/resize run |
| A4 | Keyframe-on-demand plumbing | Consumer `KeyframeRequest` → producer restart/IDR in the live path | Existing sync tests extended to the live worker |

**Gate A:** a documented session streams one real window for ≥2 minutes with
honest metadata (no pixels stored), survives a resize, and stops cleanly when the
window closes. Until then, no transport claim is upgraded.

## Phase B — production control plane (M2, issues #3, #7)

**Goal:** an authenticated client can pair, read capabilities, invoke an action,
send portable input and recover from a dropped media consumer.

| # | Increment | Deliverable | Verification |
|---|---|---|---|
| B1 | Action policy on the listener | `protocol-serve` optionally attaches a `NiriActionBackend` + `ControlLease`; grant flags; snapshot pushed after `SessionOpened` | Two-process test: pair → snapshot → action.result; no grant → `required`/`PermissionRequired` |
| B2 | Portable input injection | Route `InputEvent` from the lease to a native injector; evaluate libei/portal RemoteDesktop before uinput; release-all on revoke/disconnect | Injection tests behind an opt-in flag; synthetic release-all coverage |
| B3 | Reconnect and backpressure | Consumer reset/reconnect requests a keyframe; bounded producer queue; session teardown | Loopback tests with induced disconnect and slow consumer |
| B4 | Media transport decision | Written comparison (WebRTC datachannel/media vs QUIC vs custom AEAD TCP) with threat model update | ADR in `docs/`; no code claim until chosen |

**Gate B:** two processes pair over the encrypted control channel, receive a
grant-scoped snapshot, complete one semantic action and one input event, survive a
consumer reconnect with a fresh IDR, and refuse a revoked lease without side
effects.

## Phase C — iOS/iPadOS client (M3, issue #4)

**Goal:** the SwiftUI shell becomes a real client against a Vista host.

| # | Increment | Deliverable | Verification |
|---|---|---|---|
| C1 | Transport + pairing | `Network.framework` control client in the iOS target; pairing secret from QR/paste; secure storage | `VistaCore` tests for framing/state; simulator run against a host |
| C2 | Live catalog + capabilities | Replace `DemoCatalog`; render snapshot; unavailable actions honestly disabled | Fixture-backed UI tests; simulator against `protocol-serve` |
| C3 | Hardware video | VideoToolbox decode of Annex-B access units; first frame, keyframe recovery, resize | Device/simulator playback against host capture |
| C4 | Input and lifecycle | Touch→absolute pointer, scroll, keyboard/IME, reconnect/error states, orientation | Accessibility/rotation/keyboard checks; disconnect/reconnect run |
| C5 | Audio, clipboard, background | Deferred after C1–C4 | Separate acceptance |

**Gate C:** issue #4's acceptance criteria: pair, catalog refresh and first remote
video playback against a Vista host, with disconnect/reconnect coverage.

## Phase D — independent applications (M4, issue #5)

| # | Increment | Deliverable |
|---|---|---|
| D1 | Benchmark nested compositors | Compare headless labwc/sway/weston: isolated Wayland socket, startup, input, capture |
| D2 | Session supervisor | Launch/stop one app in a private compositor; teardown leaves no processes |
| D3 | Stream the private surface | Capture + input/audio/clipboard routed to that session |
| D4 | Isolation model | Explicit resource/security boundaries beyond display isolation |

**Gate D:** launch an app in a private session and stream it while proving the
client cannot see or affect the host desktop.

## Phase E — reliability and distribution (M5, issue #8)

- **E1 CI:** GitHub Actions for `cargo test`/`clippy`/`fmt`, `swift test
  --package-path client/core`, and the Android Gradle build (issue #34).
- **E2 Hardware matrix:** mixed-GPU, fractional scaling, mixed DPI, HDR.
- **E3 Packaging:** Arch `PKGBUILD`, systemd user service, reproducible setup.
- **E4 Assurance:** network stress, privacy review, dependency and threat review.

## Executed increments (2026-11-09)

| Increment | PR | State |
|---|---|---|
| Action policy on the control listener | #47 | merged |
| Live window lifecycle guard | #48 | merged |
| Persistent capture + latency/keyframes | #50 | merged; real-hardware run recorded |
| Capture verification evidence | #55 | merged |
| Production media record transport + ADR | #51 | merged (codec/queue; real connection pending) |
| Swift control transport + pairing | #52 | merged; iOS CI green |
| Live catalog + capability snapshot UI data | #53 | merged; iOS CI green |
| VideoToolbox decode + stream view | #54 | merged; iOS CI green |
| Rust host CI | #49 | merged and green on `main` |

## Executed increments (2026-11-11)

| Increment | PR | State |
|---|---|---|
| Experimental end-to-end MVP (streaming, catalog, target selection, computer-use) | #92 | merged; Rust/iOS CI green |
| Live input bound to target-scoped control leases | #90 | merged |
| Niri JSON IPC provider instead of CLI subprocesses | #88 | merged |
| Portal source identity boundary (picker-only) | #82 | merged |
| CSR/mTLS enrollment conformance profile (spec only) | #83 | merged |
| Client interaction primitives | #85 | merged |
| Client README reconciled with MVP status | #91 | merged |
| License and Niri IPC audit | #84 | merged |
| Host platform-neutral architecture docs | #67 | merged |
| Canonical protocol, security and client specs | #76 | merged (design only) |
| Generated-source drift + Linux `VistaCore` CI gates | #94 | merged; green |
| Bounded media receive session + decoder seam | #95 | open |
| Encoder diagnostics + reference-client end-to-end evidence | this change | open |

### Next steps

1. Prove one action and one input event reaching a real desktop through the
   transport, with an opt-in run that states exactly what moved.
2. Reconcile the Android client with the shared core: the `VistaCore` sources are
   `#if !SKIP`-guarded, so the Skip build transpiles the UI but not the protocol
   types, and a clean transpile currently needs a documented path fix.
3. Add incremental catalog refresh and reconnect/backpressure handling.
4. Make the Android build a CI gate once the transpile is reproducible.
5. Continue M3 input/keyboard/audio/clipboard, then M4 independent sessions and
   M5 packaging/reliability.

## Definition of done (every increment)

- [ ] Behavior is honest: unavailable/denied paths are visible, not simulated.
- [ ] Unit tests cover success, denial, stale/revoked and malformed input.
- [ ] Opt-in hardware/desktop diagnostics report metadata only and clean up.
- [ ] Docs state what is implemented, what is verified, and what is not.
- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass
      (and Swift/Android checks where touched).

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Portal/window capture varies by compositor and portal version | Keep per-window consent path authoritative; full-output fallback stays explicit |
| Encoder/decoder differences (NVENC, VideoToolbox, MediaCodec) | Gate on decoded frames, not encoder success; keep synthetic fixtures |
| Scope creep into a general remote desktop | Enforce milestone gates and the "not claimed" lists |
| PSK design lacks forward secrecy | Documented; revisit with the media-transport ADR (B4) |
| Client and host drift | Wire changes land in Rust, fixtures and Swift models together |
| Hardware-only steps block CI | Opt-in diagnostics + deterministic fakes for every rule |

## Not planned yet

Production TLS/QUIC/WebRTC listener, discovery/mDNS, host identity signatures,
audio, clipboard, remote printing, multi-client sessions, and any claim of
production security. These stay out until the gates above are met.
