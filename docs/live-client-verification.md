# Live Niri reference-client verification

This note records the host-side reference-client checks performed for the Vista live-streaming workstream. It complements the broader MVP evidence in PR #97.

## Environment

- Compositor: Niri 26.04 (`8ed0da4`)
- Wayland display: `wayland-1`
- Active output: `eDP-2`, 1920×1080 at 144.003 Hz
- Capture/encoder: `grim` plus GStreamer `x264enc`
- Host listener: loopback TCP with authenticated control and encrypted media session

## Target checks

The Rust `vista-host live-client` connected to the real running host and requested eight access units per target. All three completed successfully:

| Target | Frames | Bytes | Elapsed | Geometry | Encoder |
|---|---:|---:|---:|---|---|
| `desktop` | 8 | 185,718 | 721 ms | 1920×1080 | `x264enc` |
| `workspace:12` | 8 | 158,915 | 736 ms | 1920×1080 | `x264enc` |
| `window:177` | 8 | 190,597 | 743 ms | 1920×1080 | `x264enc` |

Each invocation returned `verified: true`. These results demonstrate successful capture, encoding, authenticated media transport and access-unit reception for all three target selectors. They do not demonstrate isolated per-window capture: the current window path changes Niri focus/fullscreen state and captures the focused output.

## Reference-client failure handling

The reference client previously discarded every media-read error and treated any positive frame count as success, even when fewer frames than requested arrived. The receive path now:

- ignores only socket read timeouts (`WouldBlock` / `TimedOut`) while waiting for the next record;
- surfaces all other media transport, authentication and record errors with the received/requested frame counts;
- rejects EOF, stream-stop, stream-error and deadline completion unless the requested frame count was reached;
- preserves the underlying `MediaError` type from `read_media_record`, rather than flattening it into a string.

Deterministic tests cover exact completion, early termination with a useful count, and zero-frame failure.

## Limitations and failure modes

- A previous test run against a one-shot host got `Connection refused` for workspace/window targets because the host exited after the first client; the persistent host fixes the test setup, not the protocol.
- The media sender previously surfaced a broken pipe when a reference client closed immediately after receiving its requested records. Expected peer disconnects (`BrokenPipe`, `ConnectionReset`, `ConnectionAborted`) are now treated as normal media-session shutdown; other write failures remain errors. A deterministic test covers the classification.
- The initial workspace formatting check exposed pre-existing rustfmt drift in `capability.rs`, `catalog_wire.rs`, `input_inject.rs`, `live_media.rs` and `protocol.rs`. Those files were formatted with `cargo fmt --all` so the workspace-wide formatting gate can pass.
- No native iOS/Android playback, input injection, pixel-level target comparison, packet-loss recovery or reconnect behavior is claimed by these checks.
