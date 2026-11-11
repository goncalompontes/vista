# Host provider interface contracts and worker handoff

Status: **proposed design**, to be reviewed with [ADR 0002](../adr/0002-host-platform-neutral-core.md) in PR #67. Illustrative Rust-like shapes below are *not compiled source* and are intentionally independent of an async runtime, concrete OS types and crate names.

## Modules and dependency direction

```text
vista-domain  (model: IDs, grants, capabilities, errors, leases, state machines)
      ↑
vista-host-core (HostService, policy enforcement, provider registry, broker, session router)
      ↑
vista-platform-linux   (portal, PipeWire, desktop catalog/action providers, managed compositor)
vista-platform-fake    (deterministic fixtures)
      │
vista-media (source ingest, encoder, WebRTC and telemetry: platform-native sub-adapters)
vista-wire  (protobuf encode/decode, schema version/feature negotiation)
vista-cli   (composition root and opt-in diagnostics)
```

The implementation may retain current crates initially. The *dependency rule* matters more than crate count: domain and wire modules must not import `niri_ipc`, `ashpd`, `zbus`, `pipewire`, `std::os::fd`, `wayland_*` or `gstreamer`. Media ingest is intentionally allowed to be platform-dependent **behind** the portable source handle.

## Portable identities and handle ownership

- `HostId`: cryptographically pinned identity; a discovered network address is not an identity.
- `SessionId`: host-issued, non-reused within service generation. Distinguish authenticated client connection vs application session.
- `TargetRef`: host-issued opaque ID + kind (`approved_picker_source`, `catalog_window`, `managed_app`, `display`) + session scope + generation. Native backend IDs stay in a private mapping; do not serialize local PIDs/Niri window IDs.
- `GrantRef`: host-generated unforgeable reference bound to authenticated principal, provider, source, approved action kind, permission epoch, scope and expiry. Grants are *not* derived from client-reported flags.
- `CaptureSourceRef`: belongs to one capture provider and its grant; not interchangeable with a `TargetRef`. Optional verified catalog/source binding requires trusted provider evidence; titles/geometry/focus are insufficient evidence.
- `InputLease`: connection-/session-bound owner and active held-state ledger, separately gated by input grant and scope. Release-all must be idempotent and available even when rate-limited or disconnected.
- Any host provider restart, target deletion/recreation, grant revocation or session replacement invalidates affected handles and increments epochs. All stale requests fail before native side effects.

## Provider API sketches

```rust
// illustrative signatures only, not a buildable API
trait CatalogProvider {
    async fn snapshot(&self) -> Result<CatalogSnapshot, HostError>;
    async fn subscribe(&self) -> Result<BoundedStream<CatalogDelta>, HostError>;
}
trait CaptureProvider {
    async fn probe(&self) -> CaptureAvailability;
    async fn request_source(&self, requested: SourceKind, principal: PrincipalId)
        -> Result<ApprovedSource, HostError>; // may present native consent UI
    async fn start(&self, approved: &ApprovedSource, grant: &GrantRef)
        -> Result<MediaSourceHandle, HostError>;
    async fn stop(&self, source: MediaSourceHandle) -> Result<(), HostError>;
}
trait InputProvider {
    async fn describe(&self, scope: TargetScope) -> InputAvailability;
    async fn acquire(&self, grant: &GrantRef, scope: TargetScope)
        -> Result<InputDeviceLease, HostError>;
    async fn inject(&self, lease: &InputDeviceLease, event: PortableInputEvent)
        -> Result<(), HostError>;
    async fn release_all(&self, lease: &InputDeviceLease) -> Result<(), HostError>;
}
trait WindowActionProvider {
    async fn describe(&self, target: TargetRef) -> ActionSnapshot;
    async fn invoke(&self, checked: AuthorizedAction) -> Result<ActionResult, HostError>;
}
trait SessionRuntime {
    async fn launch(&self, allowed_app: AllowedAppSpec, principal: PrincipalId)
        -> Result<ManagedSession, HostError>;
    async fn attach(&self, session: SessionId) -> Result<ManagedSession, HostError>;
    async fn stop(&self, session: SessionId) -> Result<(), HostError>;
}
```

`PermissionBroker`, `ClipboardProvider` and `AudioProvider` follow the same probe/grant/use/revoke pattern. Concrete providers may be implemented in multiple cooperating objects rather than forced into a superclass. The host core may use boxed async trait objects, generics, or enum dispatch after profiling; keep behavior testable through fakes.

**Authoritative sequence:** parse client request -> authenticate principal and connection -> validate target generation, lease, grant scope/epoch and policy -> obtain provider handle -> **revalidate** freshness immediately before native side effect -> execute bounded operation -> publish capability/session revision -> acknowledge result. Provider errors must be sanitized, structured and never turned into a 'successful' no-op.

## Media source boundary (protect hardware zero-copy)

`MediaSourceHandle` lives in the platform/media process. A platform ingest implementation advertises supported memory types and negotiates frames/PTS/geometry/metadata with the encoder: PipeWire DMABUF/shared memory, GStreamer source element, or future native texture/CPU frames. Host core only sees opaque source lifetime and stream diagnostics; it never manipulates native file descriptors or frame planes. An encoder/transport PR may choose one measured path, but must not force avoidable CPU RGBA copies for all platforms.

## Provider selection without false guarantees

1. Inspect installed candidate providers, **then** probe relevant interface versions, operations and permission state.
2. Register independently successful catalog, capture, input, action, clipboard, audio and session providers; this is **not** "Niri vs GNOME vs KDE" as a one-of choice.
3. On startup, expose capabilities *by operation and target scope*, including `unsupported` and `requires_consent` states. A provider becoming unavailable invalidates affected handles and pushes new capability revision.
4. Prefer the portal-approved window source for the user's active desktop. If the native catalog is absent, present a picker-only session. Never remap it to catalog ID without verified evidence.
5. For a managed independent app, choose providers owned by that app's managed compositor/session. Shared-desktop portal providers are not an automatic fallback.
6. Report view-only when video works but remote input does not. Report no single-window capture when only monitor capture is available; separately offer monitor mode with its own user consent.

## Sequence A: picker-only shared-desktop window

```text
Client                 HostService/Policy       LinuxCaptureProvider      Portal
  | open picker ---------> |                           |                     |
  |                        | request WINDOW source ---> | ---- user chooser -> |
  |                        | <--- approved grant/source  | <--- WINDOW source --|
  | <--- stream target ---- |                           |                     |
  | start video ----------> | check grant + generation ->| start native source  |
  | <====== WebRTC video == | --- MediaSource -> Encoder |                     |
  | revoke --------------> | revoke epoch/close source ->| cleanup PipeWire     |
```

No catalog window ID is required. The client must not pretend it selected the window that was originally shown in a compositor window list unless binding evidence exists.

## Sequence B: catalog + portal selector

A native catalog can suggest a particular window, but the portal owns the final user selection. Resolve selected portal source to catalog object **only if** the provider proves identity. Otherwise create a separate `approved_picker_source` target. Do not use title/PID/geometry matching as a security guarantee.

## Sequence C: independent managed app

```text
Client -> HostService: launch allowed app
HostService -> SessionRuntime: create owned compositor + private socket + app
SessionRuntime -> SessionCaptureProvider: capture **owned** app/session surfaces
SessionRuntime -> SessionInputProvider: session-scoped devices (no host desktop)
HostService -> MediaEngine/WebRTC: common ingest + stream
Client -> HostService: input/action with session-target lease
HostService -> owned InputProvider: validated injection
Crash/revoke/stop -> terminate owned process tree, release input and media,
                     delete owned runtime artifacts, invalidate generations
```

This diagram says nothing about operating-system sandboxing. The supervisor documents if the same-UID app can still access user files/credentials/network.

## Required scenario fixtures for implementers

| Fixture | Expected result |
|---|---|
| fake platform has no catalog | picker stream exists; no fabricated window IDs |
| catalog has window but no CaptureProvider | list-only/view-unavailable, no monitor fallback |
| WINDOW absent, MONITOR present | window request denied; different explicit monitor consent path |
| video approved, input denied | live view-only stream; no input side effect |
| input scope is desktop, user requests private window | deny incompatible scope and report it |
| compositor/provider restart | handles stale, new generation, re-probe and cleanup |
| portal source not reliably correlated with catalog | target is picker-sourced, not catalog-sourced |
| selected target closes/revokes mid-frame | stop stream, discard stale buffers, release lease |
| managed session has no portal service | session-owned capture/input may work; shared desktop untouched |
| native encoder only supports DMA-BUF | negotiated platform ingest without forced portable RGBA copy |
| unknown protobuf optional feature | ignore safely; unsupported required capability returns structured error |
| reconnect after lost media | new connection ID, capability resync, keyframe requested and no stale action replay |

## PR slices for #68 and #71

- PR 1: type-only portable models, errors, capability scopes and fake-provider contract tests.
- PR 2: provider registry and policy router using fakes; regression tests for revoked/stale input.
- PR 3: adapt existing Niri catalog/actions as optional provider without rewriting its diagnostics.
- PR 4: integrate portal picker as an independent capture provider, with explicit unbound source semantics.
- PR 5: add Linux probe/report and partial-support UI fixtures; then live opt-in validation.
- PR 6: integrate media ingest and secure WebRTC, keeping domain/protocol seams intact.

Worker decisions allowed: exact Rust async trait/dependency mechanism, module location and internal synchronization once documented. Decisions **not** delegated: new wire schema, target/grant meaning, consent scope, Niri-as-mandatory dependency, transport cryptography or silent fallback to a wider capture scope.
