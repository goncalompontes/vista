# Computer-use MCP for the vphone client

Vista's iOS client is developed on Linux with vphone-linux, which runs patched
iOS firmware under QEMU. The reusable computer-use package in
[`integrations/vista-computer-use`](../integrations/vista-computer-use) turns
that workflow plus the Vista host service into a stdio MCP server usable from
both Codex and OpenCode.

It exposes named tools for:

- VM status, start, wait and graceful stop;
- guest screenshots, screen geometry and unlock;
- accessibility tree inspection and element taps;
- tap, swipe, key, text and hardware-button input;
- app install, launch, terminate, logs and crash reports;
- guest network inspection, DNS resolution and QMP host-port forwarding;
- the Vista host service and its reference `live-client`.

The server wraps the existing vphone-linux scripts (`vphoned`, `qmp.py`,
`install-ipa.py`, `app-logs.py`, `start-vm.sh`) and `vista-host`; it does not
replace them. Read the package [README](../integrations/vista-computer-use/README.md)
for configuration and the tool reference, [RESEARCH](../integrations/vista-computer-use/docs/RESEARCH.md)
for the inspected APIs and compatibility assumptions, and [END_TO_END](../integrations/vista-computer-use/docs/END_TO_END.md)
for the host-service + emulator walkthrough.

## Why this shape

- **Accessibility first.** `vista_ui_tree` and `vista_ui_tap_element` are
  cheaper and more deterministic than vision-only loops. Screenshots remain the
  fallback for SpringBoard and surfaces without a verified foreground app.
- **Cross-client.** MCP stdio is supported by Codex and OpenCode. The package
  ships a Codex plugin manifest plus a direct Codex config and an OpenCode
  config example; there is no client-specific extension in the server.
- **Safety over convenience.** Inputs are schema-validated, paths are confined,
  destructive actions require `confirm:true`, VM shutdown is always graceful,
  and there is no raw shell or RPC escape hatch.
- **Honest gaps.** QMP is optional; `ui.tree` requires a verified foreground
  app; the host service has no TLS; live desktop capture is intrusive. These are
  documented rather than hidden.

## Quick verification

```sh
cd integrations/vista-computer-use
./bin/vista-computer-use-mcp --check
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

The `--check` path is read-only: it reports the active VM/CID, vphoned health,
guest screen, QMP availability and host-service state.
