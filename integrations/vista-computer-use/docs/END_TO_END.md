# End-to-end: Vista host service + vphone client

This walkthrough runs the Vista Rust host service, drives the real SwiftUI
client inside the vphone/QEMU iOS emulator, and verifies both paths with the MCP
server.

There are two complementary checks:

1. **Reference client check** (`vista_live_client`): the Rust `live-client`
   pairs with `vista-host`, selects a stream, receives encrypted H.264, and
   optionally sends input. This proves the host side without touching the iOS
   UI.
2. **Guest client check** (`vista_client_deploy` + `vista_ui_*` +
   `vista_screenshot` + `vista_app_logs`): install and launch `Vista.ipa` in the
   emulator, pair it through the UI, and observe its logs/screen.

The live service is experimental. Window selection focuses and fullscreens the
target and captures the whole output via `grim`; it does not isolate window
pixels. The service is single-client and has no TLS.

## 0. Prerequisites

- vphone-linux is installed and a VM is booted with vphoned answering.
- The Vista host binary is current:

```sh
cd /home/pontes/projects/vista
cargo build --bin vista-host
```

- The Quest/desktop session has Niri and a working `grim` for host capture.
- For input injection, `/dev/uinput` must be accessible to the user; otherwise
  `vista-host` reports `input:false` and the client can still receive video.
- The iOS client IPA exists. Build it once (outside the MCP, because this is a
  slow Xcode/xtool step):

```sh
cd /home/pontes/projects/vista/client/ios
./vphone.sh build        # writes client/ios/xtool/Vista.ipa
./vphone.sh check        # xtool + guest vphoned health
```

## 1. Configure the MCP client once

Use the direct Codex or OpenCode config in the package README. Verify the
server's read-only view first:

```sh
cd /home/pontes/projects/vista/integrations/vista-computer-use
./bin/vista-computer-use-mcp --check
```

Expected: `VM: <name>`, `vphoned: ok`, `QMP: available|unavailable`, and
`vista-host: stopped|running`.

## 2. Start the VM and make the guest usable

If the VM is already running:

```json
{"name": "vista_vm_status", "arguments": {}}
{"name": "vista_unlock", "arguments": {"confirm": true, "timeout_seconds": 30}}
{"name": "vista_screen_info", "arguments": {}}
```

`vista_screen_info` returns point and pixel geometry. In the tested 1x VM this
is 448x932 points and 448x932 pixels; a 3x VM reports 1290x2796 pixels with a
scale of 3. All `vista_tap`/`vista_swipe`/`vista_ui_*` coordinates default to
points. Add `"space": "pixels"` when reading pixel coordinates from a
screenshot.

If the VM is stopped:

```json
{"name": "vista_vm_start", "arguments": {"name": "vphone-iPhone17-3-2662", "wait_seconds": 0}}
{"name": "vista_vm_wait", "arguments": {"name": "vphone-iPhone17-3-2662", "timeout_seconds": 180}}
```

`vista_vm_start` returns immediately; `vista_vm_wait` polls vphoned health and
returns `ready:false, timed_out:true` rather than hanging forever.

If QMP reports a paused VM, resume it before waiting for vphoned; a paused VM
may still hold substantial host memory and GPU resources. Software emulation,
repeated screen captures, and encoding can also put noticeable load on the
host. Start at a low frame rate when debugging on a shared desktop.

## 3. Start the Vista host service

```json
{
  "name": "vista_host_start",
  "arguments": {
    "listen": "127.0.0.1:7443",
    "fps": 15,
    "allow_input": true,
    "allow_window_actions": true,
    "allow_desktop_actions": true
  }
}
```

The result contains an invite:

```json
{
  "address": "127.0.0.1:7443",
  "secret_hex": "…64 hex characters…",
  "input": true,
  "fps": 15
}
```

Keep this invite. `vista_host_status` returns it again; `vista_status` redacts
the secret. The service stays alive across MCP calls. If `input` is false, check
`/dev/uinput` and `ydotoold`; video still works.

For a non-loopback listener you must add `"allow_remote": true` and
`"confirm": true`. This is not recommended: the current host service has no TLS.

## 4. Reference-client smoke test

This is the fastest proof that the host service is alive and streaming:

```json
{
  "name": "vista_live_client",
  "arguments": {
    "target": "desktop",
    "frames": 10,
    "allow_input": false,
    "confirm": true
  }
}
```

Omit `address` and `secret_hex` to use the managed service's invite. Expected
report fields: `frames`, `bytes`, `width`, `height`, `encoder`, `verified`.
With `frames: 10` at 15 fps the call should finish in a few seconds.

`target: "window:ID"` focuses and fullscreens that Niri window before capture.
Use `vista_windows` to find IDs. `target: "workspace:ID"` focuses that virtual
desktop. `desktop` captures the focused output. All of these are visible side
effects; the tool requires `confirm:true`.

If you also want to exercise input, add `"allow_input": true`,
`"pointer": "0.5:0.5"`, and optionally `"text": "hello"`. This sends a pointer
event and a text commit through the host's ydotool injector.

## 5. Install and launch the guest client

```json
{"name": "vista_client_deploy", "arguments": {"confirm": true}}
```

This installs `client/ios/xtool/Vista.ipa` and launches `dev.vista.client`. It
does not build the IPA. To install a different build:

```json
{
  "name": "vista_client_deploy",
  "arguments": {
    "ipa_path": "client/ios/xtool/Vista.ipa",
    "bundle_id": "dev.vista.client",
    "launch": true,
    "confirm": true
  }
}
```

Check that the app is running:

```json
{"name": "vista_app_list", "arguments": {"filter": "running"}}
{"name": "vista_app_launch", "arguments": {"bundle_id": "dev.vista.client"}}
```

## 6. Pair the iOS client through the UI

Take a screenshot first:

```json
{"name": "vista_screenshot", "arguments": {"include_image": true}}
```

Then inspect the accessibility tree:

```json
{"name": "vista_ui_tree", "arguments": {"max_elements": 120, "visible_only": true}}
```

The tree may fail with `frontmost application could not be verified` on the
Home Screen or a system surface. Once Vista is frontmost, retry. The flat tree
rows include labels, roles, identifiers, frames, and clickability; use
`vista_ui_tap_element` by label when possible:

```json
{"name": "vista_ui_tap_element", "arguments": {"text": "Host address", "match": "contains"}}
{"name": "vista_text", "arguments": {"text": "127.0.0.1:7443"}}
{"name": "vista_ui_tap_element", "arguments": {"text": "Pairing secret", "match": "contains"}}
{"name": "vista_text", "arguments": {"text": "<secret_hex from vista_host_status>"}}
{"name": "vista_ui_tap_element", "arguments": {"text": "Connect", "match": "contains"}}
```

If the app has no accessible labels, fall back to screenshot coordinates:

```json
{"name": "vista_tap", "arguments": {"x": 224, "y": 466}}
{"name": "vista_text", "arguments": {"text": "127.0.0.1:7443"}}
```

Remember: a screenshot from a 3x VM is 1290x2796 pixels but input defaults to
448x932 points. Pass `"space": "pixels"` or divide by the reported scale.

After connecting, verify the screen and logs:

```json
{"name": "vista_screenshot", "arguments": {"include_image": true}}
{"name": "vista_app_logs", "arguments": {"process": "Vista", "subsystem": "dev.vista.client", "seconds": 15}}
```

The host service prints a session report to its stdout log after the client
disconnects; `vista_host_status` includes the log paths.

The host address entered in the guest must be reachable from the guest's
network namespace. `127.0.0.1` inside the iPhone is the guest itself, not the
Linux host. With QEMU user networking, the gateway was `10.0.2.2` in the tested
setup; reachability of a loopback-only host listener requires an appropriate
forwarding arrangement. Use the actual networking configuration for the VM.

## 7. Observe and troubleshoot

| Symptom | Check |
| --- | --- |
| `vista_status` says vphoned unavailable | Is QEMU running? Is the CID right? `vista_vm_wait`; inspect `VPHONE_HOME/runs` or the VM's `serial.log`. |
| `ui.tree` fails with "frontmost application could not be verified" | The guest is on SpringBoard or a surface without a verified foreground app. Launch the app, or use screenshots/coordinates. |
| `vista_host_start` fails with "did not print a valid invite" | Run `cargo build --bin vista-host` and check the returned stdout/stderr log; a stale binary may not support `serve`. |
| `input:false` in the invite | `/dev/uinput` is unavailable or `ydotoold` could not start. Video still works; input is disabled honestly. |
| `vista_live_client` returns `verified:false` | Check the target window ID, `grim`, and the host log. A window target is fullscreened, so the desktop will visibly change. |
| `vista_network_forward` says QMP unavailable | The VM was booted without `-qmp`; use a `boot-test.sh` run or start QEMU with a QMP socket. |
| Screenshot saved but no image content | The image is above `VISTA_CU_MAX_IMAGE_BYTES`; open the saved path or raise the limit. |

## 8. Teardown

```json
{"name": "vista_host_stop", "arguments": {"confirm": true}}
{"name": "vista_app_terminate", "arguments": {"bundle_id": "dev.vista.client"}}
{"name": "vista_vm_stop", "arguments": {"confirm": true}}
```

`vista_vm_stop` shuts down through vphoned and waits; it does not SIGKILL QEMU.
If it times out, close the VM window instead of killing the process, to avoid
APFS corruption.

## What was actually exercised

On 2026-11-10 the iOS app in vphone paired with the managed host, displayed the
live catalog, and showed real H.264 video for the active output and a virtual
desktop. Both desktop targets were selected in the same paired session. A
window was selected afterward, but its view continued showing an older frame;
the cause and sustained playback remain unverified. A disposable window input
test did not record the test phrase, so keyboard, pointer, and host actions
still need end-to-end verification. The earlier test of a real window did
display video before a control-sequencing disconnect; subsequent protocol and
lifecycle fixes passed automated tests, but the remaining window failure was
not resolved in that run.

The MCP was exercised over its actual stdio protocol against the live VM and
host. QMP screenshots and taps worked; guest accessibility queries sometimes
timed out, and accessibility frames did not always match guest hit coordinates.
The cua-driver host window capture could not establish the QEMU surface's
identity under Niri, so the guest MCP/QMP route was used for this session. A
Codex configuration entry was added locally, but a new session must verify
whether the server is loaded as native tools.

Rust workspace tests, 49 Swift core tests, the host build, and the iOS IPA build
passed after the protocol fixes. Existing Swift warnings remain. Unit tests for
the MCP cover transport, command validation, and tool dispatch. At handoff the
managed host was stopped and the VM paused to reduce desktop load. The known
next steps are to diagnose stale window video and slow UI response, verify
continued frame delivery and capture stop/switch behavior, and exercise input
and a safe host action against a disposable window.
