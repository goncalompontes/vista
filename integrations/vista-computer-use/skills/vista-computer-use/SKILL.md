---
name: vista-computer-use
description: Drive the Vista vphone/QEMU iOS emulator and the Vista host service with the vista_computer_use MCP server. Use for screenshots, UI inspection, taps/swipes/text, app install/launch/logs, VM status/lifecycle, and end-to-end host/client checks.
---

# Vista computer use

Use the `vista_computer_use` MCP server for the vphone/QEMU iOS workflow. Prefer
accessibility over vision, and prefer named tools over raw coordinates.

## Start here

1. `vista_status` — check paths, VM, vphoned, QMP, and host-service state.
2. `vista_screen_info` — get point and pixel geometry plus orientation.
3. `vista_screenshot` — see the current screen; images may be private.
4. `vista_ui_tree` — inspect the foreground app when it has a verified
   foreground process.

Do not start a VM or install an app until you know which VM/CID is active. If
multiple VMs exist, pass `name` explicitly.

## Driving the guest

- Tap by element first: `vista_ui_tap_element` with `text`/`identifier`/`role`.
- Coordinates are **screen points** by default. Pass `"space": "pixels"` for
  values read from a screenshot on a scaled VM, or divide by `scale`.
- `vista_tap` and `vista_swipe` validate against `device.screen`.
- Type with `vista_text` after focusing a field; press keys with `vista_key`.
- Use `vista_button` for `home`, `lock`, `power`, `volume-up`, `volume-down`.
- If `ui.tree` says "frontmost application could not be verified", the guest is
  on SpringBoard or a surface without a verified app. Use a screenshot and
  coordinate taps; do not report the tree as empty.
- Nested `ui.tree` IDs are diagnostic only. Do not pass them to tap tools.

## Apps and logs

- `vista_app_list` with `filter: "running"` before launch/terminate.
- `vista_app_install` requires `confirm: true`; it installs an IPA through
  vphone-linux's signer.
- `vista_client_deploy` installs/launches the Vista iOS client from
  `client/ios/xtool/Vista.ipa`; build it first with
  `client/ios/vphone.sh build`.
- `vista_app_logs` reads the guest unified log for a process. Use
  `subsystem: "dev.vista.client"` for the Vista client's own `os.Logger` lines.
- `vista_app_crashes` lists crash reports.

## Vista host path

- `vista_host_start` starts `vista-host serve` and returns the invite. The
  service persists across tool calls.
- `vista_live_client` is the reference end-to-end check: pairing + stream +
  optional input. It requires `confirm: true`; a window target is fullscreened.
- `vista_windows` lists Niri windows for a `window:ID` target.
- `vista_host_stop` requires `confirm: true`.
- The host service's input is only enabled when `/dev/uinput` and `ydotoold`
  work; `input:false` in the invite is honest, not a failure to hide.

## Safety rules

- Never claim a screenshot or UI tree succeeded if the tool returned `isError`.
- Do not capture the user's desktop with `vista_live_client` unless the user
  asked for that check; it is visible and may contain private content.
- `vista_vm_stop` is graceful and never SIGKILLs QEMU. Do not kill QEMU manually
  to work around a timeout; close the VM window or report the timeout.
- Never put a guest passcode or the host pairing secret in a summary. Pass the
  secret only to the pairing field; do not echo it.
- Explicit output paths must be inside the configured write roots. Screenshots
  default to `runs/computer-use/screenshots/`.

## Common flow

```
vista_status
vista_unlock {confirm:true}
vista_client_deploy {confirm:true}
vista_screenshot
vista_ui_tree
vista_ui_tap_element {text:"Host address"}
vista_text {text:"127.0.0.1:7443"}
...
vista_app_logs {process:"Vista", subsystem:"dev.vista.client", seconds:15}
```
