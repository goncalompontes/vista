# Android client verification

Recorded evidence for the native Android client against a live Vista host. This
is emulator evidence, not a physical-device claim. No screenshots are committed:
the captured surface contains the host desktop.

## 2026-11-11 — control, live H.264 and touch input

**Client:** `client/android` debug APK (`dev.vista.client`, `vista.module.MainActivity`),
x86_64 Android 36 `VistaPixel` AVD, headless (`-no-window -gpu swiftshader_indirect`).

**Host:** Arch Linux, Niri, NVIDIA RTX 5060 Laptop, software `x264enc` (no GStreamer
NVENC plugin installed), `vista-host` built from the `feat/android-native-client`
branch on top of `d262e1d`.

```sh
NIRI_SOCKET=/run/user/1000/niri.wayland-1.xxxx.sock WAYLAND_DISPLAY=wayland-1 \
  ./target/debug/vista-host serve --confirm --listen 0.0.0.0:7443 \
  --secret-hex <32-byte hex> --allow-remote --allow-input --once --json

adb install -r client/android/Android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.vista.client/vista.module.MainActivity
# Host address 10.0.2.2, port 7443, the same 64-character pairing secret.
```

The emulator reaches the host through `10.0.2.2`; the host sees the connection
from `127.0.0.1`.

**Observed protocol sequence (client logcat, `VistaWire` tag):**

```text
control: send plain client_hello
control: recv plain server_hello
control: send plain pairing_request
control: recv plain pairing_accepted
control: send secure session_open
control: recv secure session_opened
control: recv secure media.offer
control: send secure stream.select
media:   send plain client_hello -> pairing -> session_open (plaintext handshake)
media:   recv plain session_opened
media:   send secure keyframe_request
control: recv secure stream.started
control: recv secure catalog.snapshot / capabilities.snapshot ...
```

**Host report after the client was force-stopped:**

```json
{"client_id":"client","catalog_revision":8,"stream_target":{"kind":"desktop"},
 "frames":182,"bytes":2047869,"dropped":0,"actions_completed":0,
 "input_events":12,"reason":"peer disconnected"}
```

`input_events: 12` is the host's own count after `apply_live_input` returned
successfully for each event. Three taps on the emulator produce four events per
tap (button press, absolute position, position, button release). The host log
contained no `input rejected` line and the client reported no transport error.

The emulator rendered a letterboxed live stream labelled
`Streaming 1920×1080 (x264enc)` and `Touch: enabled`; the frame counter advanced
to 182 before the client was stopped.

### Second run — touch and hardware key events

The same setup with the key logging enabled. One tap on the video surface and
two `adb shell input keyevent 59` (left Shift) invocations:

```text
VistaInput: send pointer.button
VistaInput: send pointer.absolute
VistaInput: send pointer.absolute
VistaInput: send pointer.button
VistaInput: send key.physical
VistaInput: send key.physical
VistaInput: send key.physical
VistaInput: send key.physical
```

```json
{"client_id":"client","catalog_revision":8,"stream_target":{"kind":"desktop"},
 "frames":197,"bytes":1888847,"dropped":0,"actions_completed":0,
 "input_events":8,"reason":"peer disconnected"}
```

Each `adb keyevent` produces a press and a release, so the eight host-accepted
events correspond exactly to the four pointer and four key events the client
logged. Shift was used deliberately: it exercises the HID-usage-to-evdev path
without typing any character into the live host desktop.

### Third run — 75-second soak

The client was left connected for about 75 seconds with no interaction:

```json
{"client_id":"client","catalog_revision":40,"stream_target":{"kind":"desktop"},
 "frames":654,"bytes":6724459,"dropped":0,"actions_completed":0,
 "input_events":0,"reason":"peer disconnected"}
```

The emulator showed `Live · 600 frames`, `Streaming desktop · 1920×1080
(x264enc)`, `Target: desktop` and `Touch: enabled`. The host reported no
dropped frames and no media error. The configured target was 15 fps; the
effective rate was about 8.7 fps (654 frames over ~75 s), consistent with the
`grim` capture loop plus software `x264enc` and emulator decode, not with a
stall. This is not a latency or throughput benchmark.

### Fourth run — host restart and automatic reconnect

While the client was streaming, the host process was killed and restarted with
the same pairing secret (a new `serve` process, not a resumed session). No
input was sent to the client.

```text
13:21:32 E VistaWire: control connection failed: ECONNREFUSED
13:21:34 E VistaWire: control connection failed: ECONNREFUSED
13:21:38 E VistaWire: control connection failed: ECONNREFUSED
13:21:43 D VistaWire: control: send plain client_hello
13:21:44 D VistaWire: control: send secure stream.select
13:21:44 D VistaWire: media: send plain client_hello
13:21:44 D VistaWire: control: recv secure stream.started
```

The restarted host logged `client connected`, reopened the media endpoint, and
the emulator showed `Live · 120 frames` with the 1920×1080 stream again. The
client retried with a short exponential backoff while the host was down.

## What this establishes

- The Kotlin control client pairs with the real host, opens an encrypted
  session, receives `media.offer`, `catalog.snapshot`, `capabilities.snapshot`
  and `stream.started`.
- The media connection runs the plaintext handshake, derives its own session
  keys and decrypts AEAD-framed access units; `MediaCodec` decodes actual host
  H.264 into a `SurfaceView`.
- `stream.select` starts the host producer, and a media-side `keyframe_request`
  is accepted.
- Touch events travel as `input.event` over the encrypted control channel,
  through the target-scoped lease and the uinput injector; the host accepted
  all 12 events with no rejection.
- Hardware key events (`key.physical`, HID usage page 0x07) follow the same
  path; four Shift press/release events were logged by the client and counted
  by the host as part of the same eight accepted events.
- Sustained playback: the client stayed connected for ~75 s and the host
  delivered 654 authenticated records (6.7 MB) with zero drops and no errors.
- Automatic reconnect: after the host was killed and restarted, the client
  re-paired, reopened the media connection and resumed video without user
  action. This replaces the previous `--once` host after every disconnect.
- The Android Gradle build and 14 JVM unit tests pass, including the shared
  Rust/Swift pairing vector, media parsing, Annex-B, key mapping and a scripted
  control + media handshake.

## What this does not establish

- No physical Android device run; `MediaCodec` behaviour on a phone's hardware
  decoder is unverified.
- No pixel comparison or latency measurement. The frame counts are the host's
  send count, not a frame-rate, smoothness or quality claim; the soak ran at an
  effective ~8.7 fps against a configured 15 fps.
- Pointer movement was not visually verified (the host count proves the
  injector accepted the events). No drag or scroll was exercised.
- Only Shift was sent as a physical key; the full key mapping has JVM tests but
  no typed-character or shortcut run. Text commit is implemented and was not
  sent in this run.
- The host path still captures the whole output and fullscreens a selected
  window; it is single-client and has no TLS. Treat the pairing secret and the
  LAN as trusted.
- The reconnect test restarted the host process; it does not prove seamless
  session resumption, media buffering across a network partition, backpressure,
  congestion control, discovery or audio/clipboard.
