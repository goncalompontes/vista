"""Thin adapter over vphone-linux's existing host-side tools.

App installation and log collection invoke the existing
``tools/install-ipa.py`` and ``tools/app-logs.py`` scripts with validated argv
lists. vphoned RPC uses the bounded transport in :mod:`vphoned`, which follows
the same wire contract as the existing Python helpers; see that module for why
the shell wrapper is not used for per-input calls.
"""

from __future__ import annotations

import json
import re
import time
from pathlib import Path
from typing import Any, Mapping

from .commands import (
    app_logs_argv,
    default_python,
    install_ipa_argv,
)
from .config import Config
from .errors import ToolError
from .runner import Runner
from .vphoned import VphonedTransport

_MAX_OUTPUT = 4000


def _tail(value: str, limit: int = _MAX_OUTPUT) -> str:
    value = value.strip()
    return value if len(value) <= limit else "…" + value[-limit:]


class VphoneClient:
    def __init__(
        self,
        config: Config,
        runner: Runner,
        cid: int,
        python: str | None = None,
        transport: VphonedTransport | None = None,
    ) -> None:
        self.config = config
        self.runner = runner
        self.cid = cid
        self.python = python or default_python()
        self.transport = transport or VphonedTransport(config, cid)

    # ------------------------------------------------------------------ transport
    def health(self, timeout: float = 8.0) -> dict[str, Any]:
        data = self.transport.get_json("/v1/health", timeout=timeout)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "vphoned health was not a JSON object")
        return data

    def rpc(
        self,
        method: str,
        params: Mapping[str, Any] | None = None,
        *,
        timeout: float | None = None,
    ) -> Any:
        timeout = float(timeout or self.config.request_timeout)
        return self.transport.rpc(
            method,
            dict(params) if params is not None else {},
            timeout=timeout,
        )

    # ------------------------------------------------------------------ device
    def screen(self) -> dict[str, Any]:
        data = self.rpc("device.screen", timeout=8)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "device.screen was not an object")
        return data

    def screenshot(self) -> dict[str, Any]:
        data = self.rpc("screen.screenshot", timeout=60)
        if not isinstance(data, dict) or not isinstance(data.get("data"), str):
            raise ToolError("invalid_response", "screen.screenshot returned no base64 data")
        return data

    def unlock(self, passcode: str | None, timeout_seconds: float) -> dict[str, Any]:
        params: dict[str, Any] = {}
        if passcode:
            params["passcode"] = passcode
        data = self.rpc("screen.unlock", params, timeout=timeout_seconds)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "screen.unlock was not an object")
        return data

    # ------------------------------------------------------------------ UI
    def ui_tree(self, params: Mapping[str, Any]) -> dict[str, Any]:
        data = self.rpc("ui.tree", params, timeout=30)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "ui.tree was not an object")
        return data

    def ui_element_at(self, x: float, y: float) -> Any:
        return self.rpc("ui.element_at", {"x": x, "y": y}, timeout=20)

    def ui_tap_element(self, selector: Mapping[str, Any]) -> Any:
        return self.rpc("ui.tap_element", dict(selector), timeout=30)

    def ui_wait(self, selector: Mapping[str, Any], appear: bool, timeout: float) -> Any:
        method = "ui.wait" if appear else "ui.wait_gone"
        params = dict(selector)
        params["timeout"] = timeout
        return self.rpc(method, params, timeout=timeout + 15.0)

    def tap(self, x: float, y: float) -> Any:
        return self.rpc("input.tap", {"x": x, "y": y}, timeout=20)

    def swipe(
        self,
        x1: float,
        y1: float,
        x2: float,
        y2: float,
        seconds: float,
        steps: int,
    ) -> Any:
        return self.rpc(
            "input.swipe",
            {
                "x1": x1,
                "y1": y1,
                "x2": x2,
                "y2": y2,
                "seconds": seconds,
                "steps": steps,
            },
            timeout=20 + seconds,
        )

    def key(self, name: str) -> Any:
        return self.rpc("input.key", {"name": name}, timeout=20)

    def type_text(self, text: str, delay_ms: int) -> Any:
        return self.rpc("input.type", {"text": text, "delay_ms": delay_ms}, timeout=30)

    def button(self, name: str) -> Any:
        return self.rpc("input.button", {"name": name}, timeout=20)

    # ------------------------------------------------------------------ apps
    def apps_list(self, filter_name: str) -> dict[str, Any]:
        data = self.rpc("apps.list", {"filter": filter_name}, timeout=30)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "apps.list was not an object")
        return data

    def apps_search(self, query: str) -> dict[str, Any]:
        data = self.rpc("apps.search", {"query": query}, timeout=30)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "apps.search was not an object")
        return data

    def apps_launch(self, bundle_id: str, url: str | None = None) -> Any:
        params: dict[str, Any] = {"bundle_id": bundle_id}
        if url:
            params["url"] = url
        return self.rpc("apps.launch", params, timeout=60)

    def apps_terminate(self, bundle_id: str) -> Any:
        return self.rpc("apps.terminate", {"bundle_id": bundle_id}, timeout=30)

    def apps_foreground(self) -> dict[str, Any]:
        data = self.rpc("apps.foreground", timeout=15)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "apps.foreground was not an object")
        return data

    def crash_reports(self, bundle_id: str | None) -> dict[str, Any]:
        params: dict[str, Any] = {}
        if bundle_id:
            params["bundle_id"] = bundle_id
        data = self.rpc("logs.crashes", params, timeout=30)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "logs.crashes was not an object")
        return data

    def network(self) -> dict[str, Any]:
        data = self.rpc("device.network", timeout=15)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "device.network was not an object")
        return data

    def network_ipv4(self) -> dict[str, Any]:
        data = self.rpc("network.ipv4.get", timeout=20)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "network.ipv4.get was not an object")
        return data

    def network_hostname(self) -> dict[str, Any]:
        data = self.rpc("network.hostname.get", timeout=20)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "network.hostname.get was not an object")
        return data

    def network_resolve(
        self,
        host: str,
        family: str,
        port: int | None,
        first_only: bool,
        timeout_ms: int,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {
            "host": host,
            "family": family,
            "first_only": first_only,
            "timeout_ms": timeout_ms,
        }
        if port is not None:
            params["port"] = port
        data = self.rpc("network.resolve", params, timeout=timeout_ms / 1000 + 10)
        if not isinstance(data, dict):
            raise ToolError("invalid_response", "network.resolve was not an object")
        return data

    # ------------------------------------------------------------------ files / tools
    def install_ipa(
        self,
        ipa_path: Path,
        launch_bundle_id: str | None,
        *,
        timeout: float = 900.0,
    ) -> dict[str, Any]:
        argv = install_ipa_argv(
            self.python,
            self.config.install_ipa_script,
            ipa_path,
            self.cid,
            launch_bundle_id,
        )
        result = self.runner.run(argv, timeout=timeout)
        if result.timed_out:
            raise ToolError(
                "timeout",
                f"IPA install timed out after {timeout:g}s",
                {"path": str(ipa_path)},
            )
        output = result.stdout_text
        if result.returncode != 0:
            raise ToolError(
                "command_failed",
                "IPA install failed",
                {
                    "returncode": result.returncode,
                    "stdout": _tail(output),
                    "stderr": _tail(result.stderr_text),
                },
            )
        installed = re.search(r"Installed\s+([A-Za-z0-9._-]+)", output)
        return {
            "exit_code": result.returncode,
            "stdout": output.strip(),
            "stderr": result.stderr_text.strip(),
            "installed_bundle_id": installed.group(1) if installed else launch_bundle_id,        }

    def app_logs(
        self,
        process: str,
        subsystem: str | None,
        seconds: int,
        *,
        timeout: float | None = None,
    ) -> dict[str, Any]:
        argv = app_logs_argv(
            self.python,
            self.config.app_logs_script,
            process,
            self.cid,
            seconds,
            subsystem,
            json_lines=True,
        )
        timeout = timeout or (seconds + 60.0)
        result = self.runner.run(argv, timeout=timeout)
        if result.timed_out:
            raise ToolError("timeout", f"log collection timed out after {timeout:g}s")
        if result.returncode != 0:
            raise ToolError(
                "command_failed",
                "log collection failed",
                {
                    "returncode": result.returncode,
                    "stdout": _tail(result.stdout_text),
                    "stderr": _tail(result.stderr_text),
                },
            )
        entries: list[Any] = []
        for line in result.stdout_text.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                entries.append(json.loads(line))
            except json.JSONDecodeError:
                entries.append({"raw": line})
        note = result.stderr_text.strip()
        return {
            "entries": entries,
            "count": len(entries),
            "truncated": "truncated" in note.lower(),
            "note": note or None,
        }


class ScreenCache:
    """Short-lived device.screen cache for coordinate validation.

    Each input call would otherwise add one vphoned round trip. The cache is
    invalidated by operations that can change the display state.
    """

    def __init__(self, client: VphoneClient, ttl: float = 5.0) -> None:
        self.client = client
        self.ttl = ttl
        self._value: dict[str, Any] | None = None
        self._at = 0.0

    def get(self) -> dict[str, Any]:
        now = time.monotonic()
        if self._value is not None and now - self._at < self.ttl:
            return self._value
        value = self.client.screen()
        self._value = value
        self._at = now
        return value

    def invalidate(self) -> None:
        self._value = None
        self._at = 0.0
