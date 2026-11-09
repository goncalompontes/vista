# Vista architecture

## Detailed design draft

The [client and host architecture package](design/README.md) records current product decisions, Sill interaction inventory, capability/input protocol, lifecycles, security, native screen flows, adapter boundaries and media trade-offs. It is proposed behavior, not implemented functionality. For those details it supersedes provisional choices in this overview, including the transport preference below. Track review in [goal #8](https://github.com/goncalompontes/vista/issues/8).

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

The Rust host has an independent H.264 encoder capability probe. It ranks GStreamer NVENC, x264 and OpenH264 element availability, requiring `h264parse`, and provides a bounded `videotestsrc` pipeline description with at most two queued encoded buffers. Element discovery alone does **not** prove codec initialization. The independent `vista-host codec-test --json` command builds a bounded 30-frame synthetic GStreamer pipeline with H.264 compression, a two-branch tee and bounded leaky queues, validating both compressed samples and 640×360 RGBA decoded samples. It attempts NVENC, x264 and OpenH264 in order when installed, and selects an installed H.264 decoder; a failed codec path falls back to the next available combination. On the tested NVIDIA host, `nvh264enc` and `nvh264dec` successfully produced real encoded and decoded buffers. This is not a PipeWire or network test, and the diagnostic's elapsed time is not streaming latency. Each attempt switches the pipeline to `Null` on success or error. The separate `vista-host bridge-test --json` runs a permission-free synthetic RGBA source through `StreamSession` into GStreamer AppSrc and a real H.264 encoder, with bounded queues, parser validation and automatic cleanup. On the tested NVIDIA host, all eight synthetic frames produced encoded H.264 buffers. The synthetic session authorization is a test fixture, **not a portal grant**. A new opt-in `vista-host capture-h264 --confirm` diagnostic now connects a portal-authorized single-window PipeWire source to a bounded `StreamSession` and an in-memory H.264 encoder. GStreamer VideoFrame layout mapping copies padded rows into tightly packed RGBA; a bounded two-buffer AppSink handles frames and a portal `Closed` listener cancels the worker. The diagnostic selects NVENC, x264, or OpenH264 by attempting runtime encoding, with fallback after an error. It captures at most eight frames for a five-second worker deadline, with the source normalized to 320×180 for this diagnostic (not full-resolution streaming). Manual user-approved capture and portal revocation are not yet verified. `capture-h264 --window ID` additionally watches the explicit Niri target through the event stream, so target removal or compositor-stream loss cancels the worker; `WindowLifecycle` (`host/src/lifecycle.rs`, `lifecycle-test --json`) makes removal, missing snapshot, portal closure, permission revocation, compositor restart and resize explicit and idempotent, with resize flushing both queues and advancing the H.264 geometry discontinuity. No media is sent over a network or to an iOS decoder. The worker deadline does not bound user portal negotiation or all pipeline teardown operations.

### Compressed-frame handoff (implemented, not networked)

After successful portal-only authorization and GStreamer H.264 encoding, the diagnostic can hand off validated `video/x-h264,stream-format=byte-stream,alignment=au` samples to `StreamSession` as in-memory access units. Each unit contains a PTS (when supplied by the encoder), an encoded-buffer keyframe flag and at most 16 MiB of bytes. The queue holds at most two units and drops the oldest on backpressure. Resize and portal revocation purge it; stale authorization tokens cannot enqueue or dequeue. The current probe drains the queue without transmitting video, and reports encoded-unit drops. The encoded queue may drop an IDR or one or more delta units. **Implemented:** `decoder_sync::H264SyncGate` exposes explicit `H264SyncEvent::Discontinuity` and `AccessUnit` events under the same permission token, with a continuity generation incremented by encoded queue overflows, geometry changes, or reported GStreamer `DISCONT`. A newly connected or reset decoder receives a discontinuity before any data and waits for an Annex-B H.264 IDR preceded by SPS/PPS within the same access unit; queued delta units are suppressed during recovery. The live encoder's `h264parse config-interval=-1` requests SPS/PPS at each IDR, and its encoded AppSink no longer silently evicts buffered access units (max two, backpressured). Encoder keyframe flags are advisory; the gate inspects Annex-B NAL headers instead. This is a conservative *NAL-level recovery heuristic*, not complete H.264 syntax verification, an in-band keyframe request, or a guarantee of codec compatibility. Upstream or future transport drops must be signaled explicitly by the producer or trigger a consumer `reset()` (and encoder IDR request when supported). The current diagnostic still drains encoded units locally; it does **not** implement pairing, encryption, network transport, or native iOS playback.

A separate `vista-host sync-test --json` diagnostic closes the local loop without a desktop source: synthetic RGBA frames pass through `StreamSession` into the real H.264 encoder, the `H264SyncGate`, and a real H.264 decoder. It injects one explicit pipeline discontinuity after a fixed frame count, verifies that queued delta access units are suppressed until a fresh SPS/PPS-prefixed IDR is decoded, and reports encoded/decoded counts plus per-frame encode→handoff→decode traversal latency. Recovery is not left to a periodic GOP: when the consumer sees the gate awaiting an IDR it sends an explicit keyframe request through a channel and the producer restarts the encoder, whose first access unit is a fresh SPS/PPS IDR (one run: 1 request, 1 restart, recovery confirmed). A new `keyframe-control-test` closes that loop locally: an authenticated control client sends `KeyframeRequest` over the AEAD record transport, the host hook sets the producer handle, and the synthetic encoder restarts (one run: 1 server request, 1 producer request, 1 restart, 10 decoded frames). It is localhost-only and sends no media to the client. A separate `media-stream --serve/--connect` diagnostic now sends real synthetic H.264 access units between two processes: the producer seals each AU with ChaCha20-Poly1305 and writes a length-prefixed record over localhost TCP; the consumer decrypts, decodes and reports frames/latency (one run: 12 records / 60,997 ciphertext bytes, 12 decoded frames / 2,764,800 RGBA bytes, 83–447 ms receive-to-decode). The session key is generated by the server and printed in the test offer; it is **not** derived from the pairing protocol, the client is not authenticated, and no media is sent to iOS. This is a media-transport boundary test, not a production stream. A paired diagnostic test runs the same 250-frame-GOP pipeline without the restart and confirms it cannot recover, then with the restart and confirms recovery. In one run on the tested NVIDIA host (`nvh264enc` → `nvh264dec`), 11 of 12 encoded access units decoded, one was suppressed during recovery, and measured traversal latency was approximately 43–200 ms (median about 167 ms). This includes encoder buffering and 30 fps synthetic pacing; it is **not** capture latency, network round-trip time, or a low-latency claim. Each access unit emitted by the gate is sealed with the ChaCha20-Poly1305 media-record cipher (a fixed test key), written over a **localhost TCP loopback**, read back, opened, and then decoded. One run transported 11 records / 60,473 ciphertext bytes (60,297 plaintext) and still decoded 11 frames. The record layer bounds plaintext to 64 KiB and ciphertext to 64 KiB + 16 bytes; larger access units fail rather than being truncated. This verifies that real encoded media survives an encrypted TCP hop in the same process; it is **not** a remote/WebRTC media transport, a real pairing, or a separate client. The test does not exercise portal consent, PipeWire, transport-level keyframe requests, full-resolution content, or an iOS decoder.

### Security

Explicit pairing, mutual authenticated session keys, least-privilege capture and control, visible session revocation, no unauthenticated host listening, no arbitrary command execution from client messages. Isolated process != security sandbox; define filesystem, network, credentials, seat and clipboard exposure before promising app isolation.

### Control protocol and pairing (implemented, transport-neutral)

`host/src/protocol.rs` defines a versioned JSON control plane for the host and
client. Control messages are length-prefixed (64 KiB maximum), carry strict
per-direction sequence numbers, negotiate a protocol version, and use an
explicit state machine (`awaiting hello` → `awaiting proof` → `paired` →
`active`). Pairing uses a 256-bit secret transferred out of band (for example a
QR code), HMAC-SHA256 confirmation in both directions and HKDF-SHA256 session
keys; offers expire after 120 seconds. Video, audio, input and clipboard are
denied by default, and the host re-checks every requested permission before a
session becomes active.

After pairing, a bounded ChaCha20-Poly1305 record layer encrypts control
frames with implicit per-direction nonce counters and AAD bound to the session
and direction. `protocol-loopback` exercises the full pairing and encrypted
`SessionOpen`/`KeyframeRequest`/`Ping` over a real `127.0.0.1` TCP socket; a
one-shot `protocol-serve` listener and `protocol-client` reference client run
the same exchange across two processes (non-loopback binding requires an
explicit `--allow-remote`). This
is a loopback diagnostic and record boundary, **not** a production transport:
there is no public listener, discovery, TLS/QUIC/WebRTC, metadata protection,
congestion control or iOS client, and the protocol has no forward secrecy, host
identity signature or external audit. The full contract, threat model and
WebRTC-versus-custom transport analysis are in [protocol.md](protocol.md).
`vista-host protocol-test --json` verifies the in-memory handshake and the
wrong-secret, replay, expiry, permission-escalation and version-mismatch paths.

**Control-to-media and two-process transport.** `keyframe-control-test` closes the control/media loop locally: an authenticated control client sends `KeyframeRequest` over the AEAD record transport, a server hook sets a producer request handle, and the synthetic encoder restarts to emit a fresh SPS/PPS IDR (one run: 1 server request, 1 producer request, 1 restart, 10 decoded frames). Separately, `media-stream --serve/--connect` sends real synthetic H.264 access units between two processes: each AU is sealed with ChaCha20-Poly1305 and length-prefixed over localhost TCP; the receiver decrypts and decodes (one run: 12 records / 63,342 ciphertext bytes, 12 decoded frames / 2,764,800 RGBA bytes, 137–484 ms receive-to-decode). The media-stream client now runs the versioned control pairing handshake first, opens a video-only session and derives the media key from the authenticated session keys; the offer carries only the out-of-band test pairing secret, and a wrong secret is rejected before any media is decrypted. The client also sends an authenticated `KeyframeRequest` over the reverse record direction; the host accepts it and restarts the encoder (one run: 1 request, 1 restart), and the receiver decodes all 12 frames. Neither path sends media to iOS; host sessions accept at most one keyframe request per second, and reconnect/backpressure remain future work.

### Capabilities, actions and portable input (implemented contract, no injection)

`host/src/capability.rs` implements the backend-agnostic contract described in
the [architecture package](design/protocol.md): versioned capability snapshots,
semantic action requests and results, bounded parameter schemas,
target/revision freshness checks, deny-by-default permission revalidation,
per-connection request de-duplication and rate limits, bounded menu trees and
portable input with explicit release-all cleanup. `ActionRegistry` owns an
`ActionBackend` adapter; `NiriActionBackend` advertises only focus, close and
workspace semantics it can actually execute, and marks maximize unsupported
rather than inventing an equivalent. `vista-host capability-test --json` proves
allowed, denied, unsupported, unavailable, stale, malformed and duplicate paths
plus capability updates without reconnect and held-key release.
`vista-host actions --window ID` builds the same snapshot for a live Niri window
and reflects the requested grants (missing grants appear as `required`); it is
read-only and executes no native action. `ControlLease` owns one session's
action/input grants: revoke advances the grant epoch, clears the grants and
releases all held input, while re-grant advances the epoch again. It is not yet
attached to the network transport or native injector. Shared fixtures
are in `docs/fixtures/capabilities-v1.json`; Swift `Codable` models mirror the
same shapes. No native input injection, menu retrieval or client UI wiring is
implemented yet, and the diagnostic does not exercise a live Niri action.

### State of implementation

Niri window discovery, incremental window catalog events and a read-only desktop portal ScreenCast capability probe are implemented. An opt-in `capture-output --confirm` diagnostic uses grim/wlr-screencopy to capture the **entire active output** in memory, encodes one 1920x1080 frame with NVENC and decodes it (one run: 118,845 encoded / 8,294,400 decoded bytes, 504 ms codec time), reporting only metadata and persisting no pixels. It is an explicit full-output fallback and **not** portal per-window capture; it cannot verify window isolation, occlusion, cursor semantics or portrait behavior expected of the portal path. On the tested system, ScreenCast version 5 advertises source mask 7 (monitor/window/virtual-monitor) and cursor mask 7; this does **not** verify Niri-specific per-window capture or grant permission to record. A SwiftUI iPhone/iPad app **preview** and cross-platform `VistaCore` model package now exist under `client/`. They use explicit sample data and do not connect to the host. An explicitly invoked `vista-host capture-consent --confirm` command requests a single-window portal ScreenCast selection without persistent grants, prints negotiated PipeWire node IDs, and closes the session without connecting to PipeWire. The separate `vista-host capture-frame --confirm` command requests the same window-only permission, opens the portal's restricted PipeWire remote and attempts to read one RGBA GStreamer sample (five-second sample wait), emitting only its dimensions, byte length and elapsed time. It shuts down the pipeline and closes the portal session on success or error. A separate opt-in `capture-burst --confirm` diagnostic attempts up to eight transient in-memory frames from a single portal-selected window, reporting timing and detected size changes without saving content. Its overall five-second frame sampling deadline does not bound portal authorization or pipeline startup/shutdown. Permission-free GStreamer tests verify single-frame, multi-frame and monitor capture, timeout/EOS behavior, invalid samples, and pipeline cleanup; all pass. These do not verify live portal/PipeWire behavior. An additional explicitly authorized `capture-monitor --confirm` diagnostic reads transient frames for at most three seconds or 120 frames with a two-buffer AppSink queue, retaining only aggregate statistics and reporting frame-limit, deadline or EOS termination. Its synthetic tests cover these termination modes, invalid caps and pipeline cleanup; the deadline bounds the sampling loop, not desktop consent or pipeline shutdown. Live chooser approval and frame retrieval are not yet manually verified. A `vista-host pipewire-loopback --json` diagnostic now creates a local synthetic `pipewiresink` video node and reads it back through `pipewiresrc` with the same RGBA caps and frame-bound validation used by the capture path; one run delivered 5 frames / 1,152,000 bytes at 320x180 in 188 ms. It performs **no portal consent and reads no user content**, so it verifies the PipeWire/GStreamer boundary, not per-window portal capture. Niri window-ID-targeted capture is not verified, even if the portal chooser returns a frame. The control-plane pairing/session state machine, localhost AEAD control transport and capability/action contract are implemented but not deployed. Media transport, native input routing, isolated sessions and a functioning iOS client remain architectural targets.
