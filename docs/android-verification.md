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
- The Android Gradle build and 11 JVM unit tests pass, including the shared
  Rust/Swift pairing vector and a scripted control + media handshake.

## What this does not establish

- No physical Android device run; `MediaCodec` behaviour on a phone's hardware
  decoder is unverified.
- No pixel comparison or latency measurement. The frame count is the host's
  send count and the client's decode path, not a frame-rate or quality claim.
- Pointer movement was not visually verified (the host count proves the
  injector accepted the events). No drag, scroll, keyboard or text commit was
  exercised.
- The host path still captures the whole output and fullscreens a selected
  window; it is single-client and has no TLS. Treat the pairing secret and the
  LAN as trusted.
- No reconnect, backpressure, congestion control, discovery or audio/clipboard.
