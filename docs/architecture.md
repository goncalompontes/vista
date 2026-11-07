# Vista architecture

## Product targets

1. **Existing-window mode:** discover a window in the user's active Niri desktop, request authorized capture, encode and stream its content, return remote input only while explicitly permitted.
2. **Independent-app mode:** launch an application in its own managed Wayland environment and stream its surfaces without displaying them on the user's active workspace.
3. **Native iPhone/iPad client:** an app picker, per-app stream surface, touch-to-pointer semantics, keyboard/IME support, clipboard, audio, and reconnection.

Sill is a UX/reference application, not a Linux-compatible component that only needs a replacement backend. Linux and macOS differ substantially in compositor/window-capture capabilities and permissions.

## Boundaries

```text
 iOS/iPadOS client
   discovery / pairing / authentication
   app catalog and session lifecycle
   video decoder + audio output + input/clipboard
                  |
            secure transport
                  |
 Linux host daemon (Rust)
   API / authorization / session manager
   window catalog via compositor adapter
   existing-window capture adapter
   isolated-session launcher + nested compositor
   encoder / audio / clipboard / input adapters
                  |
 Niri IPC       PipeWire + portal       nested Wayland compositor
 (metadata)     (capture when allowed) (isolated remote apps)
```

### Niri discovery

Niri supports IPC for window metadata, stable-in-session window IDs, and focus/workspace state. `niri msg --json windows` is a useful, independently testable starting point. **Niri IPC listing is not a capture API.** The current host adapter shells out to the official CLI for a narrow, testable boundary; a future event-stream/subscriber and typed socket client can reduce overhead and track changes.

### Capture feasibility

Wayland deliberately prevents clients from arbitrarily reading other clients' buffers. Use compositor-cooperating capture such as xdg-desktop-portal + PipeWire when supported. Whether an existing *individual window* can be captured without user interaction depends on portal backend and compositor support; do not assume that Niri's screencast API provides an arbitrary stable window feed. Investigate Niri's cast/window support and portal integration empirically. Possible fallback: fullscreen/output capture and crop with limitations (occlusion, movements, privacy). **Never silently replace single-window capture with unrestricted desktop capture.**

### Independent sessions

Existing Niri can create headless/virtual outputs in some configurations, but a virtual monitor is not an isolated Wayland security/session namespace. A distinct Wayland compositor instance (e.g. a headless nested compositor) with a private socket and supervised process is the stronger basis for per-app isolation. A Niri patch/module is not a prerequisite and would couple host maintenance to the compositor; investigate compositor protocols before considering upstream contributions. Security, input, PipeWire and offscreen capture need explicit design.

### Media and transport (planned)

Prototype video path: portal/compositor capture → PipeWire frames → VA-API/NVENC/FFmpeg or GStreamer encoder → low-latency transport (prefer WebRTC for initial NAT traversal, encryption, congestion control) → VideoToolbox decoder. Do not choose a fixed codec/zero-copy path without benchmarking hardware. Audio needs its own routed source and synchronized playback. Messages and clipboard require consent and bounded payloads.

### Security

Explicit pairing, mutual authenticated session keys, least-privilege capture and control, visible session revocation, no unauthenticated host listening, no arbitrary command execution from client messages. Isolated process != security sandbox; define filesystem, network, credentials, seat and clipboard exposure before promising app isolation.

### State of implementation

Niri window discovery, incremental window catalog events and a read-only desktop portal ScreenCast capability probe are implemented. On the tested system, ScreenCast version 5 advertises source mask 7 (monitor/window/virtual-monitor) and cursor mask 7; this does **not** verify Niri-specific per-window capture or grant permission to record. Actual consented capture, encoding, transport, input, isolation and the iOS client remain architectural targets.
