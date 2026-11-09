# Vista target architecture and migration plan

## Purpose

Vista is a host-platform-neutral streaming architecture with a **Linux-only implementation** initially, serving native iPhone/iPad clients. Neither its domain model nor wire protocol depends on Niri, Wayland, a particular Linux desktop, or Linux itself. Cross-OS hosting is a future extension, **not** a current support claim. It supports two host modes:

1. Existing-window mode: capture approved windows from the user's current Wayland desktop.
2. Independent-session mode: launch applications inside a supervised private Wayland session.

The protocol is host-backend agnostic. The host advertises capabilities and semantic actions; the client renders only actions the current target supports.

## Architectural boundaries

- **Portable host service/core (Rust):** discovery, pairing, policy, capability routing, permission/grant/lease management, target identity and generation, session orchestration, transport and lifecycle. No compositor, PipeWire, D-Bus or OS handle types cross its public domain/wire boundary.
- **Host platform providers (Linux initially):** independently selected catalog, window actions, capture, input, clipboard, audio, permission and independent-session runtime adapters. A desktop need not implement every provider.
- **Protocol schema:** one protobuf schema in `proto/vista.proto`, generated for Rust, Swift and Android/Skip.
- **Media transport:** WebRTC PeerConnection. Video and audio use native media tracks; ordered data channels carry Vista control messages and input.
- **Client (Swift/iOS first, Android/Skip mirrored):** pairing, discovery, generated protocol models, WebRTC, SwiftUI catalog/action UI, touch/keyboard/pointer mapping and reconnect state.
- **Vista domain layer:** catalog, capabilities, semantic actions, permissions, control leases, target generations and session lifecycle. These remain Vista-owned because they are not provided by generic streaming protocols.

## Host components

- `DiscoveryService`: Bonjour/mDNS advertisement and manual/Tailscale endpoint fallback.
- `PairingService`: QR/one-time bootstrap, pinned host identity and revocation.
- `LinuxDesktopRuntime`: composes available providers based on actual interfaces, runtime permissions and verified operations; uses a portal-only picker path if compositor-specific catalog IPC is unavailable.
- `NiriCatalogAndActions`: optional exact-version `niri-ipc` provider for Niri only; no production `niri msg` subprocesses. It must never be required by WebRTC or generic capture.
- `PortalCaptureProvider` / `PortalInputProvider` / `PortalClipboardProvider`: xdg-desktop-portal and PipeWire/`ashpd` where verified. Their availability and grants are independent; a private compositor may need a separate session-local adapter.
- `CapturePipeline`: portal/PipeWire frames to GStreamer.
- `Encoder`: hardware H.264 first, with negotiated profile/bitrate/keyframe control.
- `WebRtcSession`: GStreamer `webrtcbin`, signaling, tracks, data channels and statistics.
- `InputBackend`: RemoteDesktop portal plus libei/EIS.
- `AudioBackend`: PipeWire/GStreamer audio track.
- `SessionRuntime` and `SessionSupervisor`: generic launch/attach/stop contract; Linux implementation owns private compositor process tree, app lifecycle, policy and cleanup. No assumption that headless compositors have working desktop portals.
- `PolicyStore`: permissions, leases, allowed targets and audit-safe diagnostics.

## Client components

- Discovery and pairing coordinator.
- Generated protocol model layer.
- WebRTC peer connection and native H.264/audio rendering.
- Catalog/action model and capability-driven UI.
- Input router with explicit pointer, touch, keyboard and text semantics.
- Clipboard coordinator with permission state.
- Session/target lifecycle state machine with generation checks.
- Diagnostics view for connection, codec, bitrate, loss, latency and capability failures.

## Canonical protocol

Messages cover:

- protocol and schema version negotiation;
- host identity and pairing bootstrap;
- target catalog, target kind and lifecycle generation;
- capability declarations and semantic actions;
- action invocation, result and error;
- control lease acquire/renew/release;
- pointer, touch, keyboard and text input;
- clipboard permissions and updates;
- WebRTC signaling metadata;
- audio/input feature negotiation.

The schema must use stable numeric identifiers, explicit optionality, forward-compatible unknown-field handling and fixture vectors shared by Rust, Swift and Android. Generated code is the only production wire representation.

## Transport and security

WebRTC is the production media and control transport. Use its DTLS-SRTP and congestion control for media, and ordered/reliable data channels for control. A small signaling/bootstrap exchange negotiates identity, schema version and SDP/ICE; it is not a second media protocol.

Pairing uses a QR/one-time token and pinned host identity. Revocation invalidates the pairing. Vista must not keep a parallel application-level AEAD protocol in production. The current custom TCP/ChaCha transport remains only as labelled diagnostics and migration fixtures until the WebRTC path is proven.

## Data flow

Existing-window mode on Linux:
Compositor -> user-approved ScreenCast portal (when WINDOW source is supported) -> PipeWire -> GStreamer encoder -> WebRTC video track -> iOS renderer. Catalog enumeration is optional and does not authorize or uniquely identify the portal selection.

Independent-session mode on Linux:
SessionRuntime -> supervised private Wayland compositor -> verified **session-local capture provider** (portal/PipeWire only where available, otherwise owned-compositor capture) -> same media/WebRTC path. Input uses a scope-appropriate session-local adapter, not the live desktop's portal by assumption.

Input flows in reverse through data channels -> lease/policy router -> selected InputProvider: RemoteDesktop/EIS on a verified shared Linux desktop or session-specific input on an owned compositor. Clipboard and audio are independently grantable. A failed window capture must never silently broaden to output capture.

## Lifecycle and failure rules

Every target has a monotonically increasing generation. Actions include the generation and are rejected when stale. Control is lease based: one owner, renewal timeout, explicit release and cleanup on disconnect. Reconnect recreates the PeerConnection, renegotiates capabilities and requests a fresh keyframe. Revoked permissions immediately stop the affected track/backend. Shutdown must release input devices, portals, PipeWire nodes, subprocesses and temporary credentials.

## Migration sequence

1. Freeze the prototype wire protocol and record current interoperability fixtures.
2. Land canonical protobuf schema and generated clients and the portable host/provider seams with fake adapters.
3. Implement WebRTC signaling and a synthetic-source vertical slice **without Niri as a dependency**; build optional typed `niri-ipc` catalog/actions in parallel.
4. Add a Linux portal capability/permission probe, verify Niri and then GNOME/KDE (plus a wlroots environment) with a published per-capability matrix.
5. Connect real consented ScreenCast/PipeWire/H.264 capture to WebRTC and iOS using the picker-only path first; correlate catalog to portal sources only when proven.
6. Bind host identity and pairing to secure signaling/WebRTC before any real network playback; discovery can proceed alongside pairing.
7. Add RemoteDesktop/libei input and capability-driven actions, with explicit view-only fallback.
8. Add separately authorized clipboard and audio.
9. Benchmark and select the independent-session compositor and session-local media/input adapters; implement supervision with explicit isolation limitations.
10. Retire production custom media/control transport and hand-maintained wire mirrors **only after** authenticated WebRTC and generated-schema acceptance.
11. Run LAN/Tailscale performance, security and recovery gates before release. Evaluate X11/Xorg through #72 as a later Linux adapter, not an initial release gate.

## Engineering rules for worker agents

Every implementation PR links one child issue and records the acceptance evidence. Workers may choose local implementation details, but must not change the wire schema, transport, security model, capability semantics or session boundaries without an ADR and parent issue update. No new custom cryptography, media framing, compositor protocol or duplicated generated model layer may be introduced. A PR is complete only when its tests, fixtures, documentation and rollback behavior are included.

## Release gates

- Schema: cross-language fixtures and version evolution.
- Transport: real existing-window stream to iOS with reconnect and fresh keyframe.
- Desktop integration: separately advertised input, audio, clipboard and single-window capture permissions/revocation, with explicit partial/view-only modes.
- Linux portability: one test-backed compatibility matrix for Niri, GNOME Wayland, KDE Plasma Wayland and a wlroots environment; failures downgrade individual advertised capabilities, not the entire host.
- Cross-platform boundary: contract and mock tests prove no Niri/Linux-only identifier, PipeWire node, FD or native modifier leaks into protocol/domain types.
- Sessions: isolated app cannot affect shared desktop.
- Security: pairing, identity pinning, revocation and no secret leakage.
- Performance: latency, frame pacing, bitrate, loss recovery and resource budgets measured against Sunshine/Moonlight.

## Platform neutrality and verified support

[ADR 0002](adr/0002-host-platform-neutral-core.md) is the proposed normative host-boundary decision. The [Linux compatibility and probing design](design/linux-desktop-compatibility.md) defines how partial support is discovered, advertised and tested. The [provider interface and worker handoff specification](design/provider-interface-contracts.md) gives typed contracts, state-machine sequences, fixtures and PR-sized implementation slices. These documents should be reviewed and accepted before implementation of the cross-desktop adapters.

**Worker rule:** never assume that catalog listing implies capture; that ScreenCast implies input; that a user-selected portal stream maps to a compositor window ID; or that a private headless compositor provides a session portal. Use provider-specific, grant-bound opaque handles and scoped input leases. Niri remains the first tested provider, not a core requirement.
