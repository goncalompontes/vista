"""QMP fallback for screenshots and host-side input.

Screen and input normally go through vphoned, which is resolution-independent
and works on every vphone-linux boot. QMP remains useful for a ``boot-test.sh``
VM and for screenshots when vphoned is unavailable. Actions reuse
``tools/qmp.py``; only status/forwarding use a small direct QMP request because
that script has no such operations.
"""

from __future__ import annotations

import json
import socket
from pathlib import Path
from typing import Any, Iterable

from .commands import (
    default_python,
    hmp_hostfwd_add,
    hmp_hostfwd_remove,
    qmp_home_argv,
    qmp_key_argv,
    qmp_screenshot_argv,
    qmp_swipe_argv,
    qmp_tap_argv,
)
from .config import Config
from .errors import ToolError
from .runner import Runner

# qmp.py's key argument is a QEMU qcode. Keep the mapping deliberately small and
# explicit; arbitrary qcodes remain available through the guest vphoned path.
_KEY_CODES = {
    "return": "ret",
    "enter": "ret",
    "tab": "tab",
    "space": "spc",
    "escape": "esc",
    "backspace": "backspace",
    "delete": "delete",
    "forward_delete": "delete",
    "home": "home",
    "end": "end",
    "pageup": "pgup",
    "pagedown": "pgdn",
    "up": "up",
    "down": "down",
    "left": "left",
    "right": "right",
    "insert": "insert",
}


def qcode_for_key(name: str) -> str:
    lowered = name.lower()
    if lowered in _KEY_CODES:
        return _KEY_CODES[lowered]
    if len(lowered) == 1 and (lowered.isalpha() or lowered.isdigit()):
        return lowered
    raise ToolError(
        "invalid_arguments",
        f"QMP backend does not know key {name!r}; use the guest backend or a single letter/digit",
        {"key": name},
    )


def qmp_socket_from_cmdline(parts: Iterable[str]) -> Path | None:
    parts = list(parts)
    for index, part in enumerate(parts):
        value: str | None = None
        if part == "-qmp" and index + 1 < len(parts):
            value = parts[index + 1]
        elif part.startswith("-qmp="):
            value = part.split("=", 1)[1]
        if value is None:
            continue
        for item in value.split(","):
            if item.startswith("unix:"):
                path = item[len("unix:") :]
                if path:
                    return Path(path)
    return None


def _proc_cmdline(pid: str, proc_root: Path) -> list[str] | None:
    try:
        raw = (proc_root / pid / "cmdline").read_bytes()
    except OSError:
        return None
    if not raw:
        return None
    return [part.decode("utf-8", "replace") for part in raw.split(b"\0") if part]


def discover_qmp_socket(
    config: Config,
    proc_root: Path = Path("/proc"),
) -> Path | None:
    if config.qmp_socket is not None:
        return config.qmp_socket
    try:
        pids = sorted(
            (entry.name for entry in proc_root.iterdir() if entry.name.isdigit()),
            key=lambda value: int(value),
            reverse=True,
        )
    except OSError:
        pids = []
    for pid in pids:
        parts = _proc_cmdline(pid, proc_root)
        if not parts or not any("qemu-system" in part for part in parts[:2]):
            continue
        path = qmp_socket_from_cmdline(parts)
        if path is not None and path.exists():
            return path
    runs = config.vphone_home / "runs"
    try:
        candidates = sorted(
            runs.glob("*/qmp.sock"),
            key=lambda path: path.stat().st_mtime,
            reverse=True,
        )
    except OSError:
        candidates = []
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


class QmpClient:
    def __init__(
        self,
        config: Config,
        runner: Runner,
        socket_path: Path | None = None,
        python: str | None = None,
    ) -> None:
        self.config = config
        self.runner = runner
        self._socket_path = socket_path
        self.python = python or default_python()

    @property
    def socket_path(self) -> Path | None:
        if self._socket_path is None:
            self._socket_path = discover_qmp_socket(self.config)
        return self._socket_path

    def require_socket(self) -> Path:
        socket_path = self.socket_path
        if socket_path is None:
            raise ToolError(
                "qmp_unavailable",
                "no running QEMU QMP socket was found; start the VM with a -qmp socket",
            )
        if not socket_path.exists():
            raise ToolError(
                "qmp_unavailable",
                f"QMP socket does not exist: {socket_path}",
                {"path": str(socket_path)},
            )
        return socket_path

    # ------------------------------------------------------------------ direct QMP
    def query(
        self,
        execute: str,
        arguments: dict[str, Any] | None = None,
        *,
        timeout: float = 3.0,
    ) -> Any:
        socket_path = self.require_socket()
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        try:
            sock.connect(str(socket_path))
            channel = sock.makefile("rwb")
            greeting = channel.readline()
            if not greeting:
                raise ToolError("qmp_error", "QMP closed before sending a greeting")
            try:
                greeting_data = json.loads(greeting)
            except json.JSONDecodeError as error:
                raise ToolError("qmp_error", f"QMP greeting was not JSON: {error}") from error
            if "QMP" not in greeting_data:
                raise ToolError("qmp_error", "QMP greeting did not contain QMP")

            self._send(channel, {"execute": "qmp_capabilities"})
            self._read_response(channel)
            message: dict[str, Any] = {"execute": execute}
            if arguments:
                message["arguments"] = arguments
            self._send(channel, message)
            return self._read_response(channel)
        except (OSError, TimeoutError) as error:
            raise ToolError(
                "qmp_error",
                f"QMP request failed: {error}",
                {"socket": str(socket_path), "execute": execute},
            ) from error
        finally:
            sock.close()

    @staticmethod
    def _send(channel: Any, message: dict[str, Any]) -> None:
        channel.write((json.dumps(message, separators=(",", ":")) + "\n").encode())
        channel.flush()

    @staticmethod
    def _read_response(channel: Any) -> Any:
        while True:
            line = channel.readline()
            if not line:
                raise ToolError("qmp_error", "QMP closed before answering")
            try:
                data = json.loads(line)
            except json.JSONDecodeError as error:
                raise ToolError("qmp_error", f"QMP returned invalid JSON: {error}") from error
            if "event" in data:
                continue
            if "error" in data:
                error = data["error"]
                raise ToolError("qmp_error", str(error.get("desc") or error), {"qmp": error})
            if "return" in data:
                return data["return"]

    def status(self) -> dict[str, Any]:
        socket_path = self.socket_path
        if socket_path is None or not socket_path.exists():
            return {
                "available": False,
                "socket": str(socket_path) if socket_path else None,
            }
        try:
            status = self.query("query-status")
        except ToolError as error:
            return {
                "available": False,
                "socket": str(socket_path),
                "error": error.as_dict(),
            }
        return {
            "available": True,
            "socket": str(socket_path),
            "status": status,
        }

    # ------------------------------------------------------------------ actions via qmp.py
    def _run_qmp(self, argv: list[str], *, timeout: float = 30.0) -> dict[str, Any]:
        result = self.runner.run(argv, timeout=timeout)
        if result.timed_out:
            raise ToolError("timeout", f"QMP action timed out after {timeout:g}s")
        if result.returncode != 0:
            raise ToolError(
                "qmp_error",
                "QMP action failed",
                {
                    "returncode": result.returncode,
                    "stdout": result.stdout_text[-2000:],
                    "stderr": result.stderr_text[-2000:],
                },
            )
        return {"returncode": result.returncode, "socket": str(self.require_socket())}

    def tap(self, x: float, y: float, width: int, height: int) -> dict[str, Any]:
        argv = qmp_tap_argv(
            self.python,
            self.config.qmp_script,
            self.require_socket(),
            x,
            y,
            width,
            height,
        )
        return self._run_qmp(argv)

    def swipe(
        self,
        x1: float,
        y1: float,
        x2: float,
        y2: float,
        width: int,
        height: int,
        duration_ms: int,
        steps: int,
    ) -> dict[str, Any]:
        argv = qmp_swipe_argv(
            self.python,
            self.config.qmp_script,
            self.require_socket(),
            x1,
            y1,
            x2,
            y2,
            width,
            height,
            duration_ms,
            steps,
        )
        return self._run_qmp(argv, timeout=duration_ms / 1000 + 30)

    def key(self, name: str) -> dict[str, Any]:
        argv = qmp_key_argv(
            self.python,
            self.config.qmp_script,
            self.require_socket(),
            [qcode_for_key(name)],
        )
        return self._run_qmp(argv)

    def home(self) -> dict[str, Any]:
        argv = qmp_home_argv(
            self.python,
            self.config.qmp_script,
            self.require_socket(),
        )
        return self._run_qmp(argv)

    def screenshot(self, output_path: Path) -> dict[str, Any]:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        argv = qmp_screenshot_argv(
            self.python,
            self.config.qmp_script,
            self.require_socket(),
            output_path,
        )
        return self._run_qmp(argv, timeout=60)

    # ------------------------------------------------------------------ host forwarding
    def forward_add(self, netdev: str, host_port: int, guest_port: int) -> dict[str, Any]:
        command = hmp_hostfwd_add(netdev, host_port, guest_port)
        result = self.query("human-monitor-command", {"command-line": command})
        return {
            "action": "add",
            "netdev": netdev,
            "host_address": f"127.0.0.1:{host_port}",
            "guest_port": guest_port,
            "hmp": command,
            "result": result,
        }

    def forward_remove(self, netdev: str, host_port: int) -> dict[str, Any]:
        command = hmp_hostfwd_remove(netdev, host_port)
        result = self.query("human-monitor-command", {"command-line": command})
        return {
            "action": "remove",
            "netdev": netdev,
            "host_address": f"127.0.0.1:{host_port}",
            "hmp": command,
            "result": result,
        }
