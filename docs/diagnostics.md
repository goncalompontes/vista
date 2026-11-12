# Host diagnostics

`vista-host` carries a set of one-shot diagnostics. They exist to produce
evidence for a specific claim, not as a user-facing workflow. Each one prints
either human-readable text or `--json`, and most are safe to run without
granting capture or injecting input.

Run them from `host/`:

```sh
cd host
cargo run -- <command> [flags]
```

## Inspect the environment

```sh
cargo run -- doctor                      # Niri socket and compositor reachability
cargo run -- capabilities --json         # Niri capability report
cargo run -- windows                     # window list
cargo run -- windows --json              # machine-readable window list
cargo run -- window 5 --json             # one window
cargo run -- watch --json                # live catalog events
cargo run -- desktop-probe --json        # passive portal/compositor metadata
cargo run -- desktop-probe-watch --json  # re-probe on change; Ctrl-C to stop
```

`desktop-probe` never opens a chooser, reads pixels, or injects input. See
[linux-desktop-probe.md](linux-desktop-probe.md) for limitations and the tested
desktop matrix.

## Capability and action contracts

None of these capture pixels or touch the desktop.

```sh
cargo run -- capability-test --json        # backend-agnostic capability/action contract
cargo run -- control-dispatch-test --json  # session-bound dispatch through an in-memory session
cargo run -- actions --window 5 --json     # read-only live snapshot; add --allow-window-actions
                                           # or --allow-desktop-actions to show granted state
cargo run -- protocol-test --json          # in-memory pairing/session state machine
cargo run -- lifecycle-test --json         # window lifecycle guard, no compositor access
```

## Capture (opens a desktop consent chooser)

These commands ask the portal for permission. Pixels stay in memory and are
never written to a file or sent over a network.

```sh
cargo run -- capture-consent --confirm --json   # chooser only; no frames
cargo run -- capture-frame --confirm --json     # one frame
cargo run -- capture-burst --confirm --json     # up to eight frames
cargo run -- capture-monitor --confirm --json   # up to 3 s or 120 frames
cargo run -- capture-h264 --confirm [--window ID] [--seconds N] [--keyframe-at N] --json
cargo run -- capture-output --confirm --json    # full-output wlr-screencopy fallback
```

`capture-h264` wires an explicit window grant through PipeWire, a bounded
session, and an in-memory H.264 encoder. `capture-output` uses `grim` instead of
the portal and is a fallback, not the supported path.

## Encoding and media loopback

No desktop capture and no network access.

```sh
cargo run -- encoders --json         # H.264 encoder factory discovery
cargo run -- codec-test --json       # bounded synthetic encode/decode
cargo run -- bridge-test --json      # synthetic RGBA -> session -> H.264
cargo run -- sync-test --json        # encode -> sync gate -> AEAD/TCP record -> decode, one loss
cargo run -- pipewire-loopback --json  # synthetic PipeWire video-node loopback
```

`codec-test`, `bridge-test` and `sync-test` require working GStreamer codec
plugins. `sync-test` proves a recovery path with one injected loss; it does not
prove network behaviour.

## Control and media transport

Localhost by default. A non-loopback listener requires an explicit opt-in flag.

```sh
cargo run -- protocol-loopback --json             # single-process encrypted loopback
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --json
cargo run -- protocol-client --connect 127.0.0.1:PORT --secret-hex HEX --json
cargo run -- protocol-client --connect 127.0.0.1:PORT --secret-hex HEX \
  --allow-input --action window.focus --json
cargo run -- keyframe-control-test --json         # authenticated keyframe request -> encoder restart
cargo run -- media-stream --serve --listen 127.0.0.1:0 --json
cargo run -- media-stream --connect ADDR --secret-hex HEX --json
```

Opt-in action policy over the one-shot listener, with a fake backend and no
desktop side effects:

```sh
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --window 5 \
  --allow-window-actions --allow-input --fake-actions --json
```

Include a bounded Niri window catalog in the authenticated session:

```sh
cargo run -- protocol-serve --confirm --listen 127.0.0.1:0 --catalog --json
```

## Live host and reference client

`serve` is the experimental end-to-end host: it captures, encodes and streams to
one client. `live-client` is the Rust reference client.

```sh
cargo run -- serve --confirm --listen 127.0.0.1:0 --json
cargo run -- live-client --connect ADDR --secret-hex HEX \
  --target window:ID|workspace:ID|desktop [--frames N] [--h264 FILE] \
  [--pointer X:Y] [--text TEXT] --json
```

The recorded run is in [e2e-verification.md](e2e-verification.md). The host is
single-client, has no TLS, and is intended for loopback only.

## Reading results

- Exit code `0` means the diagnostic ran and its checks passed. A `verified:
  false` field means the run completed but the claim did not hold; read the
  fields around it.
- `--json` output is a report, not a stable API. See
  [RELEASES.md](../RELEASES.md) for what the compatibility promise covers.
- Per-subsystem evidence and limitations:
  [capture-verification.md](capture-verification.md),
  [input-verification.md](input-verification.md),
  [live-client-verification.md](live-client-verification.md),
  [android-verification.md](android-verification.md),
  [e2e-verification.md](e2e-verification.md).
