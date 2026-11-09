# ADR 0002 — Platform-neutral host core and capability-selected backends

Status: **proposed for review (2026-11-09)**. No cross-desktop support is claimed until the compatibility matrix is verified.

## Context

The architecture in `docs/architecture-target.md` correctly favors XDG portals, PipeWire and WebRTC over invented protocols. However its `NiriBackend` and `DesktopPortalBackend` are described as host-level building blocks, and the independent-session flow assumes private compositors provide the same portal/PipeWire integration as the user's graphical login. Neither is safe as a universal abstraction. The implementation still includes Niri-specific catalog/lifecycle code and Linux FDs and PipeWire node identifiers at useful but low-level boundaries.

**Decision:** separate Vista's portable domain/transport/session orchestration from platform integration. Implement **Linux only** for now, initially Niri and then verified GNOME/KDE and other compositors. Keep a future Windows/macOS adapter viable without implementing or guaranteeing it.

## Architecture

```text
iOS / Android clients
    | protobuf control + WebRTC media (no desktop-specific wire types)
HostService / policy / identity / leases / generation / lifecycle
    | capability registry + provider-selection + grant-bound handles
    +-- CatalogProvider         optional enumeration and change events
    +-- WindowActionProvider    supported semantic operations
    +-- CaptureProvider         chooser/grants -> stream source
    +-- InputProvider           scoped devices/held-state cleanup
    +-- ClipboardProvider       independently authorized data ownership
    +-- AudioProvider           authorized source/sink
    +-- SessionRuntime          launch/attach/terminate supervised targets
    +-- PermissionBroker        consent/revocation/portal or OS permission
    |
    +-- LinuxDesktopRuntime (composition, not a single Niri backend)
    |      XDG portal/PipeWire/GStreamer + typed desktop-specific catalog/action adapters
    |      PortalCapture, PortalInput; optional private-session capture/input
    +-- FutureOtherOsRuntime [design seam ONLY]
    |
MediaEngine / Encoder / WebRTC / Diagnostics (provider-agnostic source contract)
```

Each provider is *independently* optional and advertises evidence-based support. A platform runtime composes providers based on the active desktop, available portal interfaces, version, and actual grants; it is not selected merely by `XDG_CURRENT_DESKTOP`.

## Contract requirements

- `HostPlatform` is a registry/factory for providers; it must not be one giant trait implemented by every desktop.
- Portable types: `HostId`, `TargetRef { opaque_id, kind, generation }`, `TargetScope`, `CapabilitySnapshot`, `GrantRef`, `CaptureSourceRef`, `MediaSource`, `InputLease`, `HostError`. All local IDs are scoped to a host/provider/session and generation. Handles are never implicitly serialized as local OS handles.
- `CatalogProvider`: optional `snapshot` and `subscribe`; does **not** grant access to pixels or input. A portal-picked window is not guaranteed to map to the compositor's catalog window ID. Show a permission-aware "choose a window" flow where mapping cannot be verified.
- `CaptureProvider`: `probe`, `request_source` (with consent), `start`, `stop`; return a grant-bound source and frame/format/geometry/closure stream. **Never** broaden window capture to monitor/output capture silently. Do not expose a PipeWire node id or fd in protocol messages.
- `InputProvider`: `probe_scope`, `acquire`, `inject`, `release_all`, `revoke`; specify whether scope is whole desktop, session or genuinely window-scoped. Shared-desktop input must not claim per-window isolation without proof. A separate target generation and permission epoch are checked before each effect.
- `WindowActionProvider`: supports typed semantic actions; no arbitrary shell or assumptions that workspace/focus controls exist. `ClipboardProvider` and `AudioProvider` have separate grants and revocation lifetimes.
- `SessionRuntime`: launching is independent from the *method* of capturing its surfaces. Private Wayland socket/process ownership is not filesystem/network or same-UID isolation; threat model must state the real boundary.
- `MediaSource` is a native *media-ingest contract*, not a mandatory CPU RGBA buffer: implement Linux PipeWire/GStreamer or GPU-native source variants behind the host boundary so zero-copy remains possible. Keep GStreamer and Linux-specific fd handling out of core protocol/domain packages.
- All async operations are cancellation-aware and bounded in memory and shutdown; provider crash/removal changes availability and bumps relevant generations/revisions.
- Stable errors: unsupported capability, unavailable backend, consent required/denied, stale target, stale grant, permission revoked, invalid scope, lifecycle end. Never map unsupported features to misleading success.

## Selection policy

1. Detect Linux session type, portal availability and source/device type masks, **then probe functional support**. A portal installed on disk is not proof it supports selected window capture, libei input, or clipboard.
2. Prefer XDG desktop portal for consensual capture and RemoteDesktop input where verified. The same backend is **not** required for catalog/actions or private sessions.
3. For desktop-specific functionality attach optional adapters (Niri IPC, later other DE APIs). No Niri socket is required for portable catalog-less portal-only streaming.
4. Private supervised session runtime may use a direct compositor-specific capture/input bridge **only for its own controlled compositor**, subject to a documented permission/isolation policy; it must not use unsafe shortcuts against the real desktop.
5. Feature and error visibility is sent through the existing capability/action contract. Capability discovery cannot grant permission.

## Operating modes

| Mode | Catalog | Capture | Input | Security |
|---|---|---|---|---|
| Existing shared desktop window | optional native catalog or picker only | portal approved single-window source if supported | portal RemoteDesktop/EIS if supported, often desktop-wide | user consent; no claimed per-window input isolation |
| Existing shared desktop display | optional | separate, explicit monitor grant | explicit desktop scope | **never** fallback from failed window capture |
| Managed independent app | app/session registry | adapter for managed compositor/session, portal only if available | session-specific adapter | private display/process lifetime; sandbox hardening separate |
| Future other OS | corresponding native providers | native capture APIs | native input APIs | same portable Vista wire/leases, new OS consent |

## Risks, compatibility and alternatives

- GNOME, KDE, Niri, wlroots compositors and X11 differ in portal interfaces and catalog/semantic-action APIs. Support must be **graded per capability**, not one blanket compositor support badge.
- A user-selected portal window may not carry an identifier correlatable with a Niri/GNOME/KDE catalog entry. Binding requires verified correlation or picker-only UX.
- ScreenCast/RemoteDesktop permission persistence varies by backend and must be explicitly negotiated. Treat restore tokens as sensitive and grant-scoped.
- Portal clipboard is attached to a compatible session and must be requested **before** its session starts; otherwise it may be unavailable.
- Avoid building a Vista-specific Wayland compositor module, a custom general-purpose desktop portal, or a generalized cross-OS framework now. Upstream platform facilities first.

## Delivery and tests

1. Land portable provider traits and mock providers in a small `host-core` seam before reworking Niri implementation. No Linux imports in this seam.
2. Adapt existing Niri catalog/actions to optional providers; portal capture/input remains independent. Remove direct Niri assumptions from `HostService` and the client.
3. Build a Linux capability/permission probe that reports actual portal/interface/source/device support without reading pixels or injecting input by default.
4. Test headless fake providers: catalog-present/capture-absent; catalog-absent/portal-picker-only; capture-present/input-denied; session capture without a desktop portal; compositor restart; permission revocation; stale ID/epoch; no monitor fallback.
5. Define and run an opt-in integration matrix on Niri, GNOME Wayland, KDE Plasma Wayland, and at least one wlroots compositor. Capture exact versions and evidence; failures are allowed and truthfully advertised.
6. Preserve WebRTC/protobuf/lease semantics unchanged; hardware benchmarks verify zero-copy and capability overhead have not regressed.

## Release rule

A Linux desktop is "supported" only for the capabilities demonstrated in the compatibility matrix. No claims of complete desktop independence or Windows/macOS host support until tests exist. This decision requires updates to tracker #66, goal #8 and work items #57/#60/#62.
