# Host adapters, session ownership and media design

Status: proposed architecture. Backend support and performance require evidence; an interface is not a claim that a compositor supports it.

## Components and boundaries

| Component | Owns | Must not own |
| --- | --- | --- |
| Identity / pairing service | Keys, paired devices, bootstrap and revocation | Native input injection |
| Connection / policy service | Authenticated contexts, grants, epochs and request limits | Backend-specific shortcuts |
| Catalog provider | Window/app/source identities and lifetime changes | Authorization inferred from discovery |
| Capability / action registry | Semantic schemas, current action state, dispatch validation | Arbitrary client-provided shell |
| Capture provider | Explicit source grants, frames, geometry and closure | Network identity |
| Input adapter | Scoped lease, portable input translation and release-all | Trusting client permissions |
| Session supervisor | Processes, display namespace, app allowlist, teardown and grace policy | Claiming sandboxing from a socket |
| Media engine | Encode, decode boundary metadata, recovery and queues | Granting capture/control |
| Transport adapter | Negotiated authenticated delivery and bounded channels | Selecting source or silently broadening scope |

Proposed Rust interface concepts (not compiled API): CatalogProvider.snapshot/subscribe; CaptureProvider.request/start/stop; InputAdapter.acquire/inject/cancel; ActionProvider.describe/invoke; SessionSupervisor.launch/attach/stop; EncoderFactory.probe/create; Transport.negotiate/send/receive/close.

Use typed opaque handles bound to target generation and grant epoch. Async operations support cancellation and bounded shutdown. A capability provider publishes implemented and verified operations, not guessed support based on OS name. Adapter errors map to protocol errors with a sanitized diagnostic correlation ID.

Niri CLI IPC discovery can remain behind its existing adapter. Prefer official compositor operations for semantic actions. Generic input injection must declare actual seat/window scope; do not assume a focused-window mouse event is privately scoped to that window.

## Three distinct execution modes

| Mode | Process/display ownership | Local desktop effect | Security claim |
| --- | --- | --- | --- |
| Existing window | Existing user application | Shared focus/pointer with explicit grant | Host-user desktop access within actual adapter scope |
| Relocated existing window | Existing process moved to virtual output | Window leaves/changes local layout | No process/security isolation; optional capability |
| Independent application | Supervised app under separate managed compositor/socket | Intended separate display/input path | Display/process-lifetime separation only unless hardened separately |

A separate compositor is not a sandbox: same UID may still expose files, environment, D-Bus, audio, credentials and network. Independent-session design must decide environment allowlist, private runtime directory, Wayland socket, D-Bus access, audio routing, filesystem/network restrictions, GPU device access and resource limits before claiming hardened isolation.

Prototype compositor selection remains open pending #5. Supervise each session with a unique runtime identity, process group/cgroup where available and crash cleanup. On host restart, reconcile owned sessions using unforgeable ownership metadata; do not kill unrelated user processes. Launch only configured app IDs and validated predefined arguments. Grace/termination policy is in lifecycle-security.md.

## Transport decision record

D2 chooses direct LAN/Tailscale connectivity, not a media transport. Proposed control plane is TLS/WebSocket. Media remains behind an adapter until a small interoperability/performance spike selects it.

| Option | Strength | Cost / required evidence | Proposed use |
| --- | --- | --- | --- |
| Framed video over a separate pinned TLS connection | Small direct-path implementation, native library availability | TCP head-of-line stalls and pacing/congestion behavior; no built-in media recovery | Initial candidate for first-frame spike, not latency commitment |
| WebRTC media + authenticated signaling | RTP/media feedback, congestion machinery, encrypted media and future traversal integration | Rust/iOS integration, packaging, resource usage and signaling identity binding | Compare before committing to production transport |
| Custom QUIC media | Streams/datagrams, potentially flexible prioritization | Considerable application congestion/recovery/interoperability work | Deferred; do not invent a transport now |

WebSocket is TCP-based ([RFC 6455](https://www.rfc-editor.org/rfc/rfc6455.html)). WebRTC data channels use SCTP over DTLS ([RFC 8831](https://www.rfc-editor.org/rfc/rfc8831.html)); media transport is a distinct RTP/SRTP path described by [RFC 8835](https://www.rfc-editor.org/rfc/rfc8835.html). Do not stream encoded media through a reliable data channel and call it equivalent to WebRTC's media pipeline.

For either candidate, media authorization is bound to the already authenticated client/session, direction, target, grant epoch and fresh negotiation nonce. If a separate TLS channel is used, require the paired client identity and a one-use, short-lived session binding. If WebRTC is used, carry offers/answers and DTLS fingerprints through authenticated control signaling and bind the resulting transport to the session. No unauthenticated SDP or reusable media URL grants.

## Media contract

Client advertises decoder profiles/levels, size/fps limits and audio formats; host advertises actual encoder paths with verified/probed distinction. Negotiate the intersection before capture starts. H.264 is the initial candidate because PR #6 implements a local path, not because every device supports every profile. Specify profile, level, dimensions, color primaries/transfer/matrix/range and transport framing explicitly. HEVC is optional after negotiation and validation.

Control-plane fields: session_id, stream_id, target generation, grant_epoch, geometry_epoch, codec/config revision, content rectangle, rotation, negotiated limits and media continuity generation. Target resize creates a new geometry epoch; no input against old displayed geometry. Source content changes may require encoder restart and a new codec configuration.

Transport adapter owns packetization, depacketization and frame ordering. The existing Annex-B/AU internal handoff is not automatically a wire format: WebRTC may packetize NALs differently. For a framed-TLS spike specify bounded length-prefixed access units and metadata, not arbitrary unbounded concatenation. Compressed units remain capped at 16 MiB per existing diagnostic boundary, with negotiated lower frame limits where practical.

Decoder starts only with compatible codec configuration and a decodable recovery frame. Overflow, loss, resize, reconnect and configuration change emit discontinuity; discard dependent frames until resynchronization. Producers/transport signal loss explicitly. Add a rate-limited keyframe-request path; PR #6's header gate does not actively request IDRs and is not complete H.264 validation.

Separate clocks:
- Capture/encode timestamps use host monotonic time.
- Presentation timestamps are relative to a declared media clock/timebase.
- Receiver uses local monotonic arrival/presentation times.
- Cross-machine one-way latency requires clock-offset estimation with uncertainty; never subtract unrelated wall clocks.
- Report decode and queue times separately from capture/network estimates.

Bound raw, encoded, outgoing and decode queues independently by count/bytes and age. Prefer current frames; dropping encoded dependencies triggers recovery. Initial raw/encoded two-item queues mirror PR #6 diagnostics, not a tuned production setting. Control and revoke messages must not sit behind video bytes; separate control/media channels and bound queues. Sender telemetry reports pressure; receiver may request lower resolution/fps/bitrate within policy.

Audio is optional with declared per-app/session/system scope. Negotiate codec/sample rate/channel layout and clock relationship; bound playout delay, resynchronize after discontinuities, stop on revoke/background and flush stale audio on reconnect. Device mute is local. Do not promise per-app capture when only system audio is possible.

## Runtime gates

Before selecting transport, measure first frame, sustained text readability, capture-to-display latency with stated method, jitter/loss recovery, CPU/GPU/RAM, bandwidth and resize/revoke behavior on the actual NVIDIA Linux host and native iPhone/iPad. Browser preview and synthetic codec traversal are not substitutes. Test visible, occluded and closed windows separately. No silent full-desktop fallback.
