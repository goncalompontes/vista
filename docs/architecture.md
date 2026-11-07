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

### Media capability discovery

The Rust host has an independent H.264 encoder capability probe. It ranks GStreamer NVENC, x264 and OpenH264 element availability, requiring `h264parse`, and provides a bounded `videotestsrc` pipeline description with at most two queued encoded buffers. Element discovery alone does **not** prove codec initialization. The independent `vista-host codec-test --json` command builds a bounded 30-frame synthetic GStreamer pipeline with H.264 compression, a two-branch tee and bounded leaky queues, validating both compressed samples and 640×360 RGBA decoded samples. It attempts NVENC, x264 and OpenH264 in order when installed, and selects an installed H.264 decoder; a failed codec path falls back to the next available combination. On the tested NVIDIA host, `nvh264enc` and `nvh264dec` successfully produced real encoded and decoded buffers. This is not a PipeWire or network test, and the diagnostic's elapsed time is not streaming latency. Each attempt switches the pipeline to `Null` on success or error. The separate `vista-host bridge-test --json` runs a permission-free synthetic RGBA source through `StreamSession` into GStreamer AppSrc and a real H.264 encoder, with bounded queues, parser validation and automatic cleanup. On the tested NVIDIA host, all eight synthetic frames produced encoded H.264 buffers. The synthetic session authorization is a test fixture, **not a portal grant**. No encoder is currently connected to a live capture session, transport or iOS decoder.

### Security

Explicit pairing, mutual authenticated session keys, least-privilege capture and control, visible session revocation, no unauthenticated host listening, no arbitrary command execution from client messages. Isolated process != security sandbox; define filesystem, network, credentials, seat and clipboard exposure before promising app isolation.

### State of implementation

Niri window discovery, incremental window catalog events and a read-only desktop portal ScreenCast capability probe are implemented. On the tested system, ScreenCast version 5 advertises source mask 7 (monitor/window/virtual-monitor) and cursor mask 7; this does **not** verify Niri-specific per-window capture or grant permission to record. A SwiftUI iPhone/iPad app **preview** and cross-platform `VistaCore` model package now exist under `client/`. They use explicit sample data and do not connect to the host. An explicitly invoked `vista-host capture-consent --confirm` command requests a single-window portal ScreenCast selection without persistent grants, prints negotiated PipeWire node IDs, and closes the session without connecting to PipeWire. The separate `vista-host capture-frame --confirm` command requests the same window-only permission, opens the portal's restricted PipeWire remote and attempts to read one RGBA GStreamer sample (five-second sample wait), emitting only its dimensions, byte length and elapsed time. It shuts down the pipeline and closes the portal session on success or error. A separate opt-in `capture-burst --confirm` diagnostic attempts up to eight transient in-memory frames from a single portal-selected window, reporting timing and detected size changes without saving content. Its overall five-second frame sampling deadline does not bound portal authorization or pipeline startup/shutdown. Permission-free GStreamer tests verify single-frame, multi-frame and monitor capture, timeout/EOS behavior, invalid samples, and pipeline cleanup; all pass. These do not verify live portal/PipeWire behavior. An additional explicitly authorized `capture-monitor --confirm` diagnostic reads transient frames for at most three seconds or 120 frames with a two-buffer AppSink queue, retaining only aggregate statistics and reporting frame-limit, deadline or EOS termination. Its synthetic tests cover these termination modes, invalid caps and pipeline cleanup; the deadline bounds the sampling loop, not desktop consent or pipeline shutdown. Live chooser approval and frame retrieval are not yet manually verified. Niri window-ID-targeted capture is not verified, even if the portal chooser returns a frame. Actual frame capture, encoding, transport, input, pairing, isolated sessions and a functioning iOS client remain architectural targets.
