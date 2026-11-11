# Vista Computer Use MCP

A small, dependency-free MCP server that makes Vista's existing vphone/QEMU
workflow reusable from Codex, OpenCode, and any other MCP client.

It wraps the tools that already exist in this workspace instead of reimplementing
them:

- vphone-linux's vphoned HTTP API (a bounded transport adapted from the
  existing `vphoned.sh` / `vphone-window.py` implementations),
  `install-ipa.py`, `app-logs.py`, `qmp.py`
- `vphone-linux/setup/start-vm.sh`
- `vista-host serve` / `vista-host live-client`

The package is also a Codex plugin (`.codex-plugin/plugin.json` and `.mcp.json`)
and ships an OpenCode config example. The same server binary works with both.
See [docs/RESEARCH.md](docs/RESEARCH.md) for the inspected APIs and
compatibility decisions, and [docs/END_TO_END.md](docs/END_TO_END.md) for the
host-service + emulator workflow.

## What it exposes

| Group | Tools |
| --- | --- |
| Status / lifecycle | `vista_status`, `vista_vm_status`, `vista_vm_start`, `vista_vm_wait`, `vista_vm_stop` |
| Screen | `vista_screenshot`, `vista_screen_info`, `vista_unlock` |
| Accessibility | `vista_ui_tree`, `vista_ui_element_at`, `vista_ui_tap_element`, `vista_ui_wait` |
| Input | `vista_tap`, `vista_swipe`, `vista_key`, `vista_text`, `vista_button` |
| Apps / logs | `vista_app_list`, `vista_app_install`, `vista_app_launch`, `vista_app_terminate`, `vista_app_logs`, `vista_app_crashes`, `vista_client_deploy` |
| Networking | `vista_network_info`, `vista_network_resolve`, `vista_network_forward` |
| Vista host | `vista_host_start`, `vista_host_status`, `vista_host_stop`, `vista_windows`, `vista_live_client` |

The intended loop is accessibility-first: inspect with `vista_ui_tree`, tap with
`vista_ui_tap_element` (or coordinates), type with `vista_text`, and use
`vista_screenshot` for visual verification. Screenshots are saved under
`runs/computer-use/screenshots/` by default and may also be returned as MCP
image content.

## Requirements

- Linux with `python3` 3.10+ (stdlib only; no `pip install` required).
- A vphone-linux checkout with a running VM and `vphoned` on vsock, default
  `~/projects/vphone-linux` and CID 3.
- `socat` and the vphone tools' normal dependencies.
- For host tools: a built `vista-host` binary. From the Vista workspace:
  `cargo build --bin vista-host` (the binary is normally at
  `target/debug/vista-host`).

## Quick start

```sh
cd integrations/vista-computer-use

# Read-only check of paths, VM, vphoned, QMP and host state.
./bin/vista-computer-use-mcp --check

# Show the resolved configuration.
./bin/vista-computer-use-mcp --config-json

# List tools as JSON (useful when debugging a client).
./bin/vista-computer-use-mcp --list-tools
```

The server itself is normally launched by the MCP client over stdio:
`./bin/vista-computer-use-mcp`.

## Codex

For a direct local install, add this to `~/.codex/config.toml` (use absolute
paths; the example is in [codex.config.toml.example](codex.config.toml.example)):

```toml
[mcp_servers.vista_computer_use]
command = "/home/pontes/projects/vista/integrations/vista-computer-use/bin/vista-computer-use-mcp"
args = []
startup_timeout_sec = 20
tool_timeout_sec = 900

[mcp_servers.vista_computer_use.env]
VISTA_HOME = "/home/pontes/projects/vista"
VPHONE_HOME = "/home/pontes/projects/vphone-linux"
VPHONE_CID = "3"
```

The directory is also a Codex plugin: `.codex-plugin/plugin.json` points at
`.mcp.json`, and `skills/vista-computer-use/SKILL.md` is loaded through the
plugin's `skills` entry. Install it through a local marketplace if you use
Codex's plugin manager, or use the direct MCP config above.

## OpenCode

Add this to `opencode.json` / `opencode.jsonc` (see
[opencode.jsonc.example](opencode.jsonc.example)):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "vista_computer_use": {
      "type": "local",
      "command": [
        "/home/pontes/projects/vista/integrations/vista-computer-use/bin/vista-computer-use-mcp"
      ],
      "enabled": true,
      "environment": {
        "VISTA_HOME": "/home/pontes/projects/vista",
        "VPHONE_HOME": "/home/pontes/projects/vphone-linux",
        "VPHONE_CID": "3"
      }
    }
  }
}
```

To use the optional skill in OpenCode, copy
`skills/vista-computer-use/` into `~/.config/opencode/skills/` (or the
project's skill directory if your OpenCode version supports one). The MCP tools
work without the skill.

## Configuration

All settings come from environment variables; there is no config file to keep
in sync.

| Variable | Default | Meaning |
| --- | --- | --- |
| `VISTA_HOME` | derived from the package path | Vista workspace root; used for the host binary, client IPA, screenshots and allowed write roots. |
| `VPHONE_HOME` | `~/projects/vphone-linux` | vphone-linux checkout. |
| `VPHONE_DEVICES` | `~/vphone-setup-work/devices` | Directory containing VM bundles (the same default as `setup/start-vm.sh`). |
| `VPHONE_CID` | auto / 3 | vsock CID. Auto uses the selected VM's `cid` file, then 3. The server resolves the active CID at startup; after starting a VM with a different CID, set `VPHONE_CID` and restart the MCP server. |
| `VPHONE_VM` | auto | VM name to use when more than one VM exists. |
| `VISTA_CU_QMP` / `VPHONE_QMP` | auto-discovered | QMP socket path. Discovery reads running QEMU command lines and `runs/*/qmp.sock`. |
| `VISTA_HOST_BIN` | `target/{debug,release}/vista-host`, then `host/target/...` | Host-service binary. |
| `VISTA_CU_STATE_DIR` | `$XDG_STATE_HOME/vista-computer-use` or `~/.local/state/vista-computer-use` | Host-service pid/invite and logs. |
| `VISTA_CU_SCREENSHOT_DIR` | `<VISTA_HOME>/runs/computer-use/screenshots` | Default screenshot destination. |
| `VISTA_CU_ALLOWED_ROOTS` | workspace, state dir, screenshot dir, `/tmp/opencode` | Colon-separated roots for explicit screenshot/output paths. |
| `VISTA_CU_MAX_IMAGE_BYTES` | `3000000` | Above this size a screenshot is saved but not embedded as MCP image content. |
| `VISTA_CU_TIMEOUT` | `30` | Default vphoned request timeout in seconds. |
| `VSOCK_DIR` | `/tmp/vphone-vsock` | When `<cid>.sock` exists, the transport uses the UDS vsock path instead of AF_VSOCK. |

## Coordinate systems

- `device.screen` reports **screen points** (`width`, `height`, `scale`).
  Input and accessibility tools default to points.
- `screen.screenshot` reports pixel dimensions and `coordinate_scale`.
  Use `"space": "pixels"` to pass pixel observations; the server divides by the
  guest scale before sending input.
- The QMP fallback reuses `tools/qmp.py`. Because that script normalizes against
  a fixed 1290x2796 grid, this server pre-scales guest points into that grid;
  QEMU then receives the correct normalized axis. Guest/vphoned input remains
  the preferred path.

## Safety model

- No `shell=True`; every subprocess is an argv list.
- Strict argument validation: unknown keys, wrong types, out-of-range values,
  control characters, bad bundle IDs/URLs/secrets, and out-of-screen
  coordinates are rejected before a command runs.
- Mutating tools require `confirm: true`: `vista_unlock`, `vista_vm_stop`,
  `vista_app_install`, `vista_client_deploy`, `vista_network_forward`,
  `vista_host_stop`, `vista_live_client`, and non-loopback
  `vista_host_start`.
- `vista_vm_stop` is graceful through vphoned. It never SIGKILLs QEMU, because
  an unclean stop can corrupt the APFS disk.
- `vista_network_forward` binds only `127.0.0.1` and uses QMP user-networking.
- Explicit output paths must resolve inside an allowed write root. Screenshots
  are written with mode `0600`.
- The host-service state file is mode `0600` and contains the local pairing
  secret. `vista_status` redacts it; `vista_host_status` returns it so the agent
  can pair the iOS client.
- Passcodes are never echoed or logged. Tool arguments are not logged.
- Screenshots, guest logs, and live desktop media can contain private data.
  Treat results as sensitive and ask before capturing a user's desktop.

## Tests

```sh
cd integrations/vista-computer-use
PYTHONPATH=src python3 -m unittest discover -s tests -v
PYTHONPATH=src python3 -m compileall -q src tests
```

The tests cover request validation, path confinement, vphoned HTTP framing,
command construction for vphoned/QMP/install/log/lifecycle/host tools, QMP
handshake and forwarding, host-service child lifecycle, tool dispatch, and the
MCP initialize/list/call/stdio paths. They use fakes and never touch a live VM
or the desktop.

## Limitations and honest gaps

- **Linux/vphone only.** This is not an Android emulator or an Xcode Simulator
  bridge. It targets `vphone-linux` running patched iOS under QEMU.
- **No arbitrary shell or RPC escape hatch.** The server exposes named,
  validated tools. It intentionally does not expose `script.eval`, file writes,
  system reboot, uninstall, or raw vphoned methods.
- **Accessibility depends on the guest.** `ui.tree` requires a verified
  foreground application; SpringBoard/home-screen snapshots can fail with
  "frontmost application could not be verified". Use screenshots and
  coordinate taps there.
- **No multi-touch/pinch, screen recording, OCR, audio, or clipboard tools yet.**
  The underlying vphoned API has more methods; they are deliberately not all
  exposed.
- **Host service is single-client.** `vista-host serve` accepts one session at a
  time, and `vista_live_client` is a smoke/reference client, not a replacement
  for the iOS UI.
- **Window targets are intrusive.** `vista_live_client` with a `window:ID`
  target focuses/fullscreens that window; `desktop` captures the focused output.
  The tool returns metadata rather than pixels, but the side effect is real.
- **QMP is optional.** A VM started by `setup/start-vm.sh` has no QMP socket, so
  `vista_screenshot(source="qmp")` and `vista_network_forward` are unavailable
  unless the VM was booted with `-qmp`.
- **No TLS/remote security.** Keep `vista-host` on loopback. The pairing secret
  is a local test secret, not a production identity.

## Layout

```
integrations/vista-computer-use/
├── bin/vista-computer-use-mcp          # source-checkout launcher
├── .codex-plugin/plugin.json           # Codex plugin manifest
├── .mcp.json                           # Codex plugin MCP declaration
├── codex.config.toml.example
├── opencode.jsonc.example
├── docs/END_TO_END.md                  # Vista host + guest client workflow
├── docs/RESEARCH.md                    # inspected APIs and decisions
├── skills/vista-computer-use/SKILL.md  # agent workflow guidance
├── src/vista_computer_use/
│   ├── commands.py                     # pure argv builders
│   ├── config.py                       # environment configuration
│   ├── host.py                         # vista-host serve/live-client
│   ├── lifecycle.py                    # VM discovery and start/stop/wait
│   ├── mcp.py                          # MCP stdio JSON-RPC
│   ├── qmp.py                          # QMP status, qmp.py actions, hostfwd
│   ├── validation.py                   # strict argument/path checks
│   ├── vphone.py                       # vphone tool adapter
│   ├── vphoned.py                      # bounded HTTP-over-vsock transport
│   └── tools/                          # tool registry and handlers
└── tests/                              # fakes + unit tests
```

The `vphoned.py` transport is the one place that speaks the wire protocol
directly. It exists because `vphoned.sh` keeps socat's stdin open and therefore
blocks for the full `VPHONED_TIMEOUT` on every request; a 1-second floor per tap
makes an accessibility loop impractical. Its framing mirrors the existing
`tools/window/vphone-window.py` and `tools/app-logs.py` implementations, supports
both AF_VSOCK and the UDS transport, and is covered by tests.
