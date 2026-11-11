# Input path verification

Recorded evidence that a portable input event travels from the reference client
through the authenticated control channel, the target-scoped control lease and
the host injector, instead of being stopped at a unit-test fake.

## 2026-11-11 — native Android input effects

The newer [Android verification record](android-verification.md) observes actual
drag, secondary click, scroll, text and modifier release in a disposable GTK
window. It also records the host fix from accelerated `ydotool --absolute`
motion to a native uinput absolute pointer. The older record below proves only
injector acceptance; it does not establish accurate pointer placement.

## 2026-11-11 — earlier single absolute pointer event

**Host:** Arch Linux, Niri, commit `main` with the `--pointer` fix below.
**Injection backend:** the host's `ydotoold`/`/dev/uinput` injector, enabled
because `/dev/uinput` exists and is writable by the user.

```sh
./target/debug/vista-host serve --confirm --allow-input --once \
  --listen 127.0.0.1:0 --json
# {"protocol":"1.0","address":"127.0.0.1:42429","input":true,"fps":15, ...}

./target/debug/vista-host live-client --connect 127.0.0.1:42429 \
  --secret-hex <hex> --target desktop --frames 30 \
  --allow-input --pointer 0.5:0.5 --json
# {"frames":30,"bytes":405896,"width":1920,"height":1080,"encoder":"x264enc",
#  "pointer":{"sent":true,"x":0.5,"y":0.5},"verified":true}
```

Host report after the client closed:

```json
{"client_id":"client","catalog_revision":2,"stream_target":{"kind":"desktop"},
 "frames":31,"bytes":418742,"dropped":0,"actions_completed":0,
 "input_events":1,"reason":"client closed the session"}
```

`input_events: 1` is the host's own count after `apply_live_input` returned
successfully; no `input rejected:` diagnostic was emitted. The client reports
`sent: true`, so both ends of the path agree.

### What this establishes

- A `PointerAbsolute` event survives the encrypted control framing, the session
  permission check, the lease's target and geometry-epoch validation, and the
  injector call.
- `--pointer X:Y` previously accepted the flag but always sent `0.5:0.5`
  (hard-coded); it now parses and validates normalized coordinates, with tests
  for out-of-range, missing-separator and non-numeric input.
- The client reports whether it actually sent the event instead of ignoring the
  write result.

### What this does not establish

- No pixel, screenshot or compositor query proves the visible cursor moved; the
  evidence is that the injector accepted the event. Storing frames or pointer
  imagery would violate the project's "metadata, not pixels" rule.
- No click, drag, scroll, keyboard or text commit was exercised, and
  `actions_completed` is 0, so the semantic `action.invoke` path is still only
  covered by unit tests.
- The pointer was left at the centre of the display; the probe does not restore
  a previous position because the compositor does not expose one.
