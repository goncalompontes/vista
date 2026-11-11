# Research notes and compatibility decisions

Recorded 2026-11-10 while building the Vista computer-use MCP. This is the
evidence behind the interface, not a claim that every underlying feature is
production-ready.

## 1. What already existed

### vphone-linux host tools

Inspected in `~/projects/vphone-linux` (moved here after the initial inspection):

| Tool | Reused for | Notes |
| --- | --- | --- |
| `tools/vphoned.sh` | Documented CLI and transport reference | Keeps stdin open for vphoned; every request blocks for `VPHONED_TIMEOUT` (measured 1.046 s at 1 s, 2.012 s at 2 s, 3.013 s at 3 s). Not used for per-input RPC. |
| `tools/qmp.py` | Screenshot, tap, swipe, Home, key actions | Fixed 1290x2796 coordinate grid; the adapter pre-scales guest points into that grid. |
| `tools/install-ipa.py` | IPA upload, in-guest signing, install and optional launch | AF_VSOCK, 4 MiB socket buffers, 600 s timeout. |
| `tools/app-logs.py` | Bounded process log collection | `logs.syslog` wrapper; `--json` emits one entry per line. |
| `setup/start-vm.sh` | VM start/stop/unlock | Finds the QEMU for a disk, avoids a duplicated CID, detaches QEMU, waits for vphoned, unlocks. |
| `tools/boot-test.sh` | QMP socket discovery / test-run convention | Adds `-qmp unix:runs/RUN/qmp.sock`; a normal `start-vm.sh` boot does not. |
| `run-vphone.sh` | Understanding QEMU/vsock/GPU options | vhost-vsock is the default; `VSOCK=uds` exposes `$VSOCK_DIR/$CID.sock`. |

### vphoned API (from `Research/vphoned_http_api.md` and daemon sources)

The guest daemon is HTTP/1.1 on vsock port 1339. The methods used here:

- Device: `device.snapshot`, `device.screen` (`width`, `height`, `scale`,
  `locked`, `screen_off`, `orientation`), `device.network`.
- Screen/display: `screen.screenshot` (base64 JPEG plus `mime_type`, `width`,
  `height`, `coordinate_scale`), `screen.unlock`, `display.orientation`.
- Input: `input.tap`, `input.swipe` (`x1,y1,x2,y2,seconds,steps`), `input.key`,
  `input.type` (`text`, `delay_ms`), `input.button` (`home`, `lock`, `power`,
  `volume-up`, `volume-down`), plus `input.touch`/`input.hid`/`input.drag`
  (not exposed).
- UI: `ui.tree`/`accessibility.tree` (`nested`, `max_elements`, `max_depth`,
  `timeout_ms`, `visible_only`, `clickable_only`, `limit`), `ui.element_at`,
  `ui.tap_element`, `ui.wait`, `ui.wait_gone`.
  - Flat and nested trees are different contracts. Nested mode rejects
    `visible_only`, `clickable_only` and `limit`; its IDs are diagnostic and
    cannot be used as tap selectors.
  - Both require a verified foreground app/PID. On the tested VM, a Home Screen
    request returns `400 {"error":{"code":"failed","message":"frontmost
    application could not be verified"}}`. The MCP surfaces this as a tool error
    instead of pretending the tree is empty.
- Apps: `apps.list`, `apps.search`, `apps.launch`, `apps.terminate`,
  `apps.foreground`; install is a file upload plus `apps.install`.
- Logs: `logs.syslog`, `logs.crashes`, `logs.crash`.
- Network: `network.ipv4.get`, `network.hostname.get`, `network.resolve`,
  `network.capture`.
- Access control: vphoned rejects a non-loopback `Host`, an `Origin` header, and
  non-JSON POSTs. The existing tools send `Host: localhost`. vphone-linux's host
  proxy with a bearer token is a separate optional product feature and is not
  required for AF_VSOCK access from the same machine.

### QMP

A live `boot-test` VM exposes QMP. Verified commands: `qmp_capabilities`,
`query-status`, `query-display-options`, and
`human-monitor-command` with `help hostfwd_add` / `hostfwd_remove`. QEMU 10.2.2
accepts:

```text
hostfwd_add net0 tcp:127.0.0.1:2222-:22
hostfwd_remove net0 tcp:127.0.0.1:2222
```

`tools/qmp.py` hardcodes 1290x2796 for absolute pointer normalization. The
adapter therefore scales guest points into that grid before invoking it; the
normalized QEMU axis is unchanged regardless of the guest's actual resolution.

### Vista host service

Inspected `host/src/host_service.rs`, `input_inject.rs`, `live_media.rs` and the
`serve`/`live-client` command handling in `host/src/main.rs`:

- `vista-host serve --confirm [--listen ADDR] [--fps N] [--allow-input]
  [--allow-window-actions|--allow-desktop-actions|--allow-clipboard]
  [--once] [--json]` prints a JSON `HostInvite` with `protocol`, `address`,
  `secret_hex`, `host_name`, `input`, `fps`, then serves one client at a time.
- `vista-host live-client --connect ADDR --secret-hex HEX --target
  window:ID|workspace:ID|desktop [--frames N] [--h264 FILE] [--pointer X:Y]
  [--text TEXT] [--allow-input] [--json]` is the reference end-to-end client:
  pairing, stream selection, authenticated H.264, optional input, JSON report.
- Input injection uses a private `ydotoold` socket when `/dev/uinput` is
  available; otherwise the invite reports `input:false`.
- The producer uses `grim` full-output capture; a window target is focused and
  fullscreened first. This is a real visible side effect.

### Vista iOS client workflow

`client/ios/vphone.sh` already builds the real SwiftUI app with `xtool`, installs
it through `install-ipa.py`, launches `dev.vista.client`, reads logs and lists
crashes. The MCP's `vista_client_deploy` wraps the install/launch half; it does
not run `xtool` or pretend to be an Xcode simulator.

## 2. Public MCP patterns considered

- **MCP transports** (`modelcontextprotocol.io/specification/2025-06-18`):
  stdio is newline-delimited JSON-RPC 2.0; tool results use `content` and may
  include `structuredContent`; `tools/list` returns JSON Schema descriptors.
  Both Codex and OpenCode support local stdio servers.
- **mobile-next/mobile-mcp**: accessibility-first, coordinate fallback,
  screenshot, app install/launch/terminate, logs/crashes, and one server across
  clients. This validated the tool grouping and the "accessibility first,
  screenshots for verification" workflow.
- **whitesmith/ios-simulator-mcp**: screenshot, UI hierarchy, tap/swipe/type,
  app management, logs; it caps screenshots for efficient transmission and
  documents UI-hierarchy limitations on the home screen. This validated the
  image-size cap and the explicit "home screen tree is unavailable" caveat.
- **Codex plugin manifests**: `.codex-plugin/plugin.json` can declare
  `"mcpServers": "./.mcp.json"` and a `skills` directory; bundled plugins use
  the same shape.
- **OpenCode**: local MCP servers are configured under `mcp` with
  `type: "local"`, a command array, and an `environment` object.

## 3. Decisions

| Decision | Why |
| --- | --- |
| Python stdlib server, no dependencies | Runs from a checkout with `python3`; easy to audit; no npm/venv bootstrap. |
| stdio first | Both Codex and OpenCode support it; no port, auth, or lifecycle to manage. HTTP can be added later without changing tools. |
| Reuse scripts for install/log/lifecycle/QMP | They encode vphone-specific behavior (signing, QEMU disk locks, CID checks, qmp.py geometry). |
| Direct bounded HTTP transport for vphoned RPC | `vphoned.sh` blocks for its socat timeout; 1 s per tap is unusable. The transport mirrors existing Python implementations and supports AF_VSOCK + UDS. |
| Accessibility first, screenshot second | Cheaper and more deterministic than vision-only loops; matches mobile-mcp and ios-simulator-mcp practice. |
| Strict schemas + `additionalProperties: false` | A typo must fail before an input event or file write, not silently do something else. |
| Confirm destructive/mutating tools | Prevents an agent from stopping a VM, installing code, exposing a port, or capturing the desktop without an explicit intent. |
| No raw RPC or shell tool | vphoned is root in the guest; exposing `script.eval` or arbitrary methods would defeat the safe interface. Add named tools when a need is proven. |
| QMP optional | `start-vm.sh` VMs do not expose QMP; vphoned covers the normal path. QMP is for boot-test VMs, screenshots when vphoned is down, and hostfwd. |
| Persistent host-service state file | The Rust service is long-lived and prints its invite once; the MCP must survive between tool calls. File is mode 0600. |

## 4. Compatibility assumptions

- MCP protocol versions `2025-06-18`, `2025-03-26`, and `2024-11-05` are accepted;
  the server echoes the client's version when supported, otherwise the newest.
- Clients that ignore `structuredContent` still receive readable JSON text and
  standard image content.
- Clients that impose a tool timeout should allow long IPA installs and VM
  shutdowns. Codex example uses `tool_timeout_sec = 900`.
- The server is single-user and local. It does not add TLS, multi-client
  arbitration, or remote authentication.
- The guest must be booted, unlocked, and have a verified foreground app for
  `ui.tree`. The MCP does not auto-unlock without `confirm:true`.
- AF_VSOCK requires the Linux guest CID; the UDS branch is selected only when
  `$VSOCK_DIR/<cid>.sock` exists.
- One MCP process owns one host-service state file. Two MCP processes starting
  the host service concurrently will race; the second sees the first's state
  only if it uses the same state dir and starts after the file is written.
- The host service binary must be built from the current working tree; a stale
  `host/target/debug/vista-host` from an older checkout may not support `serve`.
  The server prefers `target/debug` and reports a clear build hint.
