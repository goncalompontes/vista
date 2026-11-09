# Vista target architecture and migration plan

## Purpose

Vista is a native Linux-to-iPhone/iPad streaming system. It supports two host modes:

1. Existing-window mode: capture approved windows from the user's current Wayland desktop.
2. Independent-session mode: launch applications inside a supervised private Wayland session.

The protocol is host-backend agnostic. The host advertises capabilities and semantic actions; the client renders only actions the current target supports.

## Architectural boundaries

- **Host daemon (Rust):** discovery, pairing, policy, compositor adapters, portals, capture, encoding, WebRTC, sessions and lifecycle.
- **Protocol schema:** one protobuf schema in `proto/vista.proto`, generated for Rust, Swift and Android/Skip.
- **Media transport:** WebRTC PeerConnection. Video and audio use native media tracks; ordered data channels carry Vista control messages and input.
- **Client (Swift/iOS first, Android/Skip mirrored):** pairing, discovery, generated protocol models, WebRTC, SwiftUI catalog/action UI, touch/keyboard/pointer mapping and reconnect state.
- **Vista domain layer:** catalog, capabilities, semantic actions, permissions, control leases, target generations and session lifecycle. These remain Vista-owned because they are not provided by generic streaming protocols.

## Host components

- `DiscoveryService`: Bonjour/mDNS advertisement and manual/Tailscale endpoint fallback.
- `PairingService`: QR/one-time bootstrap, pinned host identity and revocation.
- `NiriBackend`: exact-version `niri-ipc` adapter; no production `niri msg` subprocesses.
- `DesktopPortalBackend`: xdg-desktop-portal ScreenCast, RemoteDesktop and Clipboard through `ashpd`; PipeWire streams.
- `CapturePipeline`: portal/PipeWire frames to GStreamer.
- `Encoder`: hardware H.264 first, with negotiated profile/bitrate/keyframe control.
- `WebRtcSession`: GStreamer `webrtcbin`, signaling, tracks, data channels and statistics.
- `InputBackend`: RemoteDesktop portal plus libei/EIS.
- `AudioBackend`: PipeWire/GStreamer audio track.
- `SessionSupervisor`: private compositor process tree, app lifecycle, policy and cleanup.
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

Existing-window mode:
Wayland compositor -> ScreenCast portal -> PipeWire -> GStreamer encoder -> WebRTC video track -> iOS renderer.

Independent-session mode:
SessionSupervisor -> private Wayland compositor -> portal/PipeWire -> same capture and transport path.

Input flows in reverse through ordered data channels -> capability/action router -> RemoteDesktop portal/libei. Clipboard and audio use their standard desktop interfaces and explicit permissions.

## Lifecycle and failure rules

Every target has a monotonically increasing generation. Actions include the generation and are rejected when stale. Control is lease based: one owner, renewal timeout, explicit release and cleanup on disconnect. Reconnect recreates the PeerConnection, renegotiates capabilities and requests a fresh keyframe. Revoked permissions immediately stop the affected track/backend. Shutdown must release input devices, portals, PipeWire nodes, subprocesses and temporary credentials.

## Migration sequence

1. Freeze the prototype wire protocol and record current interoperability fixtures.
2. Land canonical protobuf schema and generated clients.
3. Replace Niri subprocess calls with the typed `niri-ipc` adapter.
4. Implement WebRTC signaling and a synthetic-source vertical slice.
5. Connect real ScreenCast/PipeWire/H.264 capture to WebRTC and iOS.
6. Add discovery, pairing and standard security.
7. Add RemoteDesktop/libei input and capability-driven actions.
8. Add clipboard and audio.
9. Benchmark and select the independent-session compositor; implement supervision.
10. Retire production custom media/control transport and hand-maintained wire mirrors.
11. Run LAN/Tailscale performance, security and recovery gates before release.

## Engineering rules for worker agents

Every implementation PR links one child issue and records the acceptance evidence. Workers may choose local implementation details, but must not change the wire schema, transport, security model, capability semantics or session boundaries without an ADR and parent issue update. No new custom cryptography, media framing, compositor protocol or duplicated generated model layer may be introduced. A PR is complete only when its tests, fixtures, documentation and rollback behavior are included.

## Release gates

- Schema: cross-language fixtures and version evolution.
- Transport: real existing-window stream to iOS with reconnect and fresh keyframe.
- Desktop integration: input, audio and clipboard permission/revocation.
- Sessions: isolated app cannot affect shared desktop.
- Security: pairing, identity pinning, revocation and no secret leakage.
- Performance: latency, frame pacing, bitrate, loss recovery and resource budgets measured against Sunshine/Moonlight.
