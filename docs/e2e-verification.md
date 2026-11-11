# End-to-end streaming verification

Recorded evidence for the experimental host media path, driven by the Rust
reference client (`vista-host live-client`) instead of an iOS simulator or
Android emulator. The reference client speaks the same authenticated control and
media protocol as the native clients, so it exercises pairing, catalog, target
selection, media authentication, AEAD record framing and H.264 encoding without
a device in the loop.

This document records what was actually executed. It is not a claim about the
iOS or Android clients, which are verified separately by their own CI jobs.

## 2026-11-11 — real desktop capture to reference client

**Host:** Arch Linux, Niri 26.x, NVIDIA RTX 5060 Laptop (no GStreamer NVENC
plugin installed, so the software `x264enc` path was selected), GStreamer
1.28.9, commit `5d8ddbf` plus the encoder diagnostics from this change.

**Command:**

```sh
./target/debug/vista-host serve --confirm --listen 127.0.0.1:0 --json
# invite contains {"address":"127.0.0.1:PORT","secret_hex":"...","fps":15,...}
./target/debug/vista-host live-client --connect 127.0.0.1:PORT \
  --secret-hex HEX --target desktop --frames 60 --json
```

Each `live-client` run pairs over the encrypted control channel, authenticates a
separate media connection with keys derived from the same session, sends
`StreamSelect`, and counts authenticated media records until `--frames` is
reached.

| # | Target | Frames | Bytes | Elapsed | Geometry | Encoder | Verified |
|---|---|---|---|---|---|---|---|
| 1 | `desktop` | 60 | 709 822 | 4 234 ms | 1920×1080 | `x264enc` | yes |
| 2 | `desktop` | 60 | 728 083 | 4 341 ms | 1920×1080 | `x264enc` | yes |
| 3 | `workspace:1` (switch) | 60 | 750 576 | 4 202 ms | 1920×1080 | `x264enc` | yes |
| 4 | `desktop` (switch back) | 60 | 744 715 | 4 198 ms | 1920×1080 | `x264enc` | yes |
| 5 | `desktop` (sustained) | 448 | 5 043 406 | 30 052 ms | 1920×1080 | `x264enc` | yes |

`verified: true` means the client authenticated and decoded the record framing
and reached the requested frame count within the run budget; it does not compare
pixels.

### What this establishes

- A real Niri output capture reaches the client as authenticated H.264 records
  over the media connection, at the configured 15 fps (448 frames in 30.05 s is
  the frame budget, not a stall).
- Switching between the `desktop` and `workspace` target kinds in separate
  sessions restarts the producer and delivers a fresh stream each time.
- The host keeps serving after a client closes; the next client pairs from the
  same invite.

### What this does not establish

- No native client decoded anything: iOS and Android playback remain their own
  claims, and Android runtime playback is still unverified (#87).
- No pixel comparison was made across a target switch; the client-side stale
  frame fix is covered by unit tests, not by a captured-frame diff.
- Input injection and host actions were not exercised here (`serve` without
  `--allow-input`, and no action was invoked) — no pointer, keyboard or focus
  changes were made on the host desktop.
- No packet loss, congestion, reconnect, revocation or multi-client behaviour.
- 30 seconds is not a soak test.

### Environment prerequisite

The host cannot encode H.264 unless the GStreamer encoder plugins are installed.
On Arch, `gst-plugins-ugly` provides `x264enc` and `gst-plugins-bad` provides
`nvh264enc`/`openh264enc`; `gst-libav` provides `avdec_h264` for the decode-side
diagnostics. Before this change the host failed with a bare
`no usable H.264 encoder is available`; it now reports the package remedy and
points at `vista-host encoders --json`.

### Observed rough edges

- When a reference client finishes its frame budget and closes, the host logs
  `media session ended: media write failed: ... Broken pipe`. The session is
  cleaned up correctly, but a normal close is reported as an I/O failure.
- `serve` advertises a single-client invite yet accepts sequential clients; this
  is convenient for verification and should be documented or made explicit.
