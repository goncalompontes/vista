# Live per-window capture verification

Status: **verified on real hardware, 2026-11-09.** This records an explicitly
consented ScreenCast capture of one window on Niri through to bounded in-memory
H.264 access units. No pixels or window contents are stored in this document or
in the repository.

## Environment

| Item | Value |
|---|---|
| OS | Arch Linux, kernel 7.2.14-arch1-1 |
| Compositor | niri 26.04 (8ed0da4), Wayland |
| ScreenCast portal | xdg-desktop-portal 1.22.1-2, xdg-desktop-portal-gnome 51.0-1, interface v5 |
| GPU / driver | NVIDIA GeForce RTX 5060 Laptop GPU, 615.78.08 |
| GStreamer / PipeWire | 1.28.8-1 / 1.6.10-1 |
| Output | eDP-2, 1920×1080, scale 1.0 |

`vista-host doctor --json` reported the live Niri socket and
`capabilities --json` reported ScreenCast source mask 7 (monitor, window,
virtual monitor) and cursor mask 7.

## Commands

```sh
# One in-memory frame through the portal-granted window stream:
vista-host capture-frame --confirm --json

# Bounded consented window capture -> StreamSession -> NVENC, with one
# keyframe request at input frame 10:
vista-host capture-h264 --confirm --seconds 5 --keyframe-at 10 --json
```

Both commands opened the desktop ScreenCast chooser. The operator selected one
window and pressed Share; no permission was persisted (`PersistMode::DoNot`).

## Evidence

`capture-frame` returned one decoded 1280×720 RGBA frame in memory:

```json
{
  "portal_session_started": true,
  "frames_captured": true,
  "frame": {
    "pipewire_node_id": 92,
    "width": 1280,
    "height": 720,
    "bytes": 3686400,
    "elapsed_ms": 2036
  },
  "arbitrary_window_capture_verified": false
}
```

`capture-h264` returned:

```json
{
  "portal_session_started": true,
  "pipewire_node_id": 92,
  "encoder": "nvidia",
  "input_frames": 29,
  "input_width": 1280,
  "input_height": 720,
  "encoded_buffers": 29,
  "encoded_bytes": 503613,
  "dropped_frames": 0,
  "dropped_access_units": 27,
  "elapsed_ms": 5022,
  "seconds": 5,
  "max_frames": 150,
  "keyframe_requests": 1,
  "keyframes_emitted": 2,
  "capture_to_encode_ms": {
    "samples": 28,
    "min_ms": 0.40273099999999995,
    "p50_ms": 0.833855,
    "p95_ms": 1.735725,
    "max_ms": 2.481705
  },
  "verified": true,
  "arbitrary_window_capture_verified": false
}
```

## What this proves

- The ScreenCast portal granted an explicitly consented, single-window source on
  Niri, and `pipewiresrc` read real frames from its restricted PipeWire remote.
- Native 1280×720 RGBA frames passed the bounded `StreamSession` queue and the
  NVENC H.264 encoder; 29 input frames produced 29 encoded buffers (503,613
  bytes) in 5.0 s.
- A downstream force-key-unit request at input frame 10 produced a second
  keyframe (`keyframe_requests: 1`, `keyframes_emitted: 2`).
- Capture-to-encoded-buffer timing was p50 0.83 ms / p95 1.74 ms over 28
  samples. This includes encoding availability but not network or decoder
  latency.
- The diagnostic persisted no pixels, wrote no files and opened no network
  connection.

## Limitations and honest non-claims

- `arbitrary_window_capture_verified` remains `false`: the operator selects a
  window in the portal chooser, so this does not prove capture of an arbitrary
  Niri window ID without consent.
- The run captured 29 frames in 5 s (~6 fps), not a 30 fps throughput claim;
  the diagnostic is synchronous and drops encoded access units because it has no
  consumer (`dropped_access_units: 27`).
- This is bounded capture, not indefinite streaming, a network media transport,
  client playback or a latency benchmark.
- One portal implementation and one GPU/driver were exercised. Results are
  machine- and portal-specific.

## Bugs found and fixed by this verification

Real capture initially failed in two ways, both fixed on
`feat/persistent-capture` before this record:

1. `pipewiresrc` rejected a forced 320×180 output with
   `error set output format: -22`; the source now captures native RGBA geometry
   and the encoder receives caps from the first frame.
2. The portal did not answer the `Closed` signal subscription or `Close` call in
   some runs, hanging the diagnostic; both waits are now bounded and reported.

A separate encoder-drain deadline was added so the end of the capture window
does not prevent a final EOS drain.
