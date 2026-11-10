"""Vista host-service lifecycle and reference-client runner.

The long-lived Rust service prints a JSON invite on stdout and then serves one
client at a time. This module keeps the child alive across MCP calls, stores its
invite/pid in a mode-0600 state file, and exposes a bounded ``live-client``
smoke test.
"""

from __future__ import annotations

import json
import os
import re
import signal
import socket
import subprocess
import time
from pathlib import Path
from typing import Any

from .commands import (
    host_live_client_argv,
    host_serve_argv,
    host_windows_argv,
)
from .config import Config
from .errors import ToolError
from .runner import Runner
from .validation import address_is_loopback, parse_host_port

_SECRET = re.compile(r"^[0-9a-fA-F]{64}$")


def _tail(path: Path, limit: int = 4000) -> str:
    try:
        value = path.read_text(errors="replace")
    except OSError:
        return ""
    return value[-limit:]


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    # A terminated child remains visible as a zombie until reaped. Treat it as
    # gone and reap it here so repeated status/stop calls are idempotent.
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return False
    fields = stat.split()
    if len(fields) > 2 and fields[2] == "Z":
        _reap(pid)
        return False
    return True


def _reap(pid: int) -> None:
    try:
        os.waitpid(pid, os.WNOHANG)
    except (ChildProcessError, OSError):
        pass


def _cmdline(pid: int) -> list[str]:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return []
    return [part.decode("utf-8", "replace") for part in raw.split(b"\0") if part]


def _looks_like_host_service(pid: int) -> bool:
    parts = _cmdline(pid)
    if not parts:
        return False
    joined = " ".join(parts)
    return "vista-host" in joined and " serve" in joined


def _connectable(address: str, timeout: float = 0.4) -> bool:
    try:
        host, port = parse_host_port(address)
    except ToolError:
        return False
    if host in {"0.0.0.0", ""}:
        host = "127.0.0.1"
    elif host == "::":
        host = "::1"
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _redact_state(state: dict[str, Any]) -> dict[str, Any]:
    """Hide a fixed --secret-hex value in the recorded argv."""
    safe = dict(state)
    argv = safe.get("argv")
    if isinstance(argv, list):
        redacted: list[Any] = []
        hide_next = False
        for item in argv:
            if hide_next:
                redacted.append("<redacted>")
                hide_next = False
                continue
            redacted.append(item)
            if item == "--secret-hex":
                hide_next = True
        safe["argv"] = redacted
    return safe


class HostServiceManager:
    def __init__(
        self,
        config: Config,
        runner: Runner,
        binary: Path | None = None,
    ) -> None:
        self.config = config
        self.runner = runner
        self.binary = binary
        self._process: subprocess.Popen[bytes] | None = None

    @property
    def state_path(self) -> Path:
        return self.config.state_dir / "host-service.json"

    def host_binary(self) -> Path:
        if self.binary is not None:
            if not self.binary.is_file():
                raise ToolError("not_configured", f"vista-host binary not found: {self.binary}")
            return self.binary
        if self.config.vista_host_bin is not None:
            if self.config.vista_host_bin.is_file():
                return self.config.vista_host_bin
            raise ToolError(
                "not_configured",
                f"VISTA_HOST_BIN does not exist: {self.config.vista_host_bin}",
            )
        raise ToolError(
            "not_configured",
            "vista-host binary not found; run `cargo build --bin vista-host` in the Vista workspace",
            {
                "checked": [
                    str(self.config.workspace / "target" / "debug" / "vista-host"),
                    str(self.config.workspace / "target" / "release" / "vista-host"),
                    str(self.config.workspace / "host" / "target" / "debug" / "vista-host"),
                    str(self.config.workspace / "host" / "target" / "release" / "vista-host"),
                ]
            },
        )

    def read_state(self) -> dict[str, Any] | None:
        try:
            data = json.loads(self.state_path.read_text())
        except (OSError, json.JSONDecodeError):
            return None
        return data if isinstance(data, dict) else None

    def _write_state(self, state: dict[str, Any]) -> None:
        self.config.state_dir.mkdir(parents=True, exist_ok=True)
        try:
            os.chmod(self.config.state_dir, 0o700)
        except OSError:
            pass
        temporary = self.state_path.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(state, indent=2, sort_keys=True))
        os.chmod(temporary, 0o600)
        os.replace(temporary, self.state_path)

    def status(self) -> dict[str, Any]:
        state = self.read_state()
        if state is None:
            return {
                "running": False,
                "managed": False,
                "state": None,
            }
        pid = state.get("pid")
        if not isinstance(pid, int):
            return {
                "running": False,
                "managed": True,
                "stale": True,
                "state": _redact_state(state),
            }
        alive = _pid_alive(pid)
        managed = alive and _looks_like_host_service(pid)
        invite = state.get("invite") if isinstance(state.get("invite"), dict) else None
        listening = bool(invite and _connectable(str(invite.get("address", ""))))
        return {
            "running": managed,
            "managed": managed,
            "stale": not managed,
            "pid": pid,
            "listening": listening,
            "state": _redact_state(state),
            "invite": invite,
        }

    def start(
        self,
        *,
        listen: str,
        fps: int,
        allow_remote: bool,
        allow_window_actions: bool,
        allow_desktop_actions: bool,
        allow_clipboard: bool,
        allow_input: bool,
        once: bool,
        secret_hex: str | None,
        start_timeout: float = 20.0,
    ) -> dict[str, Any]:
        if not allow_remote and not address_is_loopback(listen):
            raise ToolError(
                "invalid_arguments",
                "refusing a non-loopback listener without allow_remote: true",
                {"listen": listen},
            )
        if secret_hex is not None and not _SECRET.fullmatch(secret_hex):
            raise ToolError("invalid_arguments", "secret_hex must be 64 hexadecimal characters")
        existing = self.status()
        if existing.get("running"):
            return {**existing, "started": False, "already_running": True}
        if existing.get("stale") and self.state_path.exists():
            self.state_path.unlink(missing_ok=True)

        binary = self.host_binary()
        logs = self.config.state_dir / "logs"
        logs.mkdir(parents=True, exist_ok=True)
        stamp = time.strftime("%Y%m%d-%H%M%S")
        stdout_path = logs / f"host-service-{stamp}.out.log"
        stderr_path = logs / f"host-service-{stamp}.err.log"
        argv = host_serve_argv(
            binary,
            listen,
            fps,
            allow_remote,
            allow_window_actions,
            allow_desktop_actions,
            allow_clipboard,
            allow_input,
            once,
            secret_hex,
        )
        with open(stdout_path, "wb") as stdout, open(stderr_path, "wb") as stderr:
            process = subprocess.Popen(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
                cwd=str(self.config.workspace),
            )
        self._process = process
        invite: dict[str, Any] | None = None
        deadline = time.monotonic() + start_timeout
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            invite = self._read_invite(stdout_path)
            if invite is not None:
                break
            time.sleep(0.1)
        if invite is None:
            if process.poll() is None:
                try:
                    os.kill(process.pid, signal.SIGTERM)
                except OSError:
                    pass
            raise ToolError(
                "command_failed",
                "vista-host serve did not print a valid invite",
                {
                    "returncode": process.poll(),
                    "stdout": _tail(stdout_path),
                    "stderr": _tail(stderr_path),
                },
            )
        state = {
            "pid": process.pid,
            "invite": invite,
            "argv": argv,
            "started_at": time.time(),
            "stdout": str(stdout_path),
            "stderr": str(stderr_path),
        }
        self._write_state(state)
        return {
            "started": True,
            "already_running": False,
            "running": True,
            "managed": True,
            "pid": process.pid,
            "listening": _connectable(str(invite.get("address", ""))),
            "invite": invite,
            "state": _redact_state(state),
        }

    @staticmethod
    def _read_invite(path: Path) -> dict[str, Any] | None:
        try:
            text = path.read_text(errors="replace")
        except OSError:
            return None
        for line in text.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if (
                isinstance(value, dict)
                and isinstance(value.get("address"), str)
                and isinstance(value.get("secret_hex"), str)
            ):
                return value
        return None

    def stop(self, *, force: bool = False) -> dict[str, Any]:
        status = self.status()
        if not status.get("running"):
            if self.state_path.exists():
                self.state_path.unlink(missing_ok=True)
            return {**status, "stopped": False, "already_stopped": True}
        pid = status["pid"]
        if not _looks_like_host_service(pid):
            raise ToolError(
                "unsafe_stop",
                "refusing to signal a process that no longer looks like vista-host serve",
                {"pid": pid},
            )
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            self.state_path.unlink(missing_ok=True)
            self._reap_owned(pid)
            return {**status, "stopped": True, "already_stopped": True}
        deadline = time.monotonic() + (3.0 if force else 5.0)
        while time.monotonic() < deadline:
            if not _pid_alive(pid):
                self.state_path.unlink(missing_ok=True)
                self._reap_owned(pid)
                return {**status, "stopped": True, "already_stopped": False}
            time.sleep(0.1)
        if force:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            time.sleep(0.2)
            if not _pid_alive(pid):
                self.state_path.unlink(missing_ok=True)
                self._reap_owned(pid)
                return {**status, "stopped": True, "already_stopped": False}
        raise ToolError(
            "timeout",
            "vista-host did not exit after SIGTERM",
            {"pid": pid, "force_available": True},
        )

    def _reap_owned(self, pid: int) -> None:
        if self._process is None or self._process.pid != pid:
            return
        try:
            self._process.wait(timeout=1)
        except (subprocess.TimeoutExpired, OSError):
            pass
        self._process = None

    def invite(self) -> dict[str, Any]:
        status = self.status()
        if not status.get("running") or not isinstance(status.get("invite"), dict):
            raise ToolError("not_running", "vista-host serve is not running")
        return status["invite"]

    def windows(self, timeout: float = 15.0) -> Any:
        binary = self.host_binary()
        result = self.runner.run(host_windows_argv(binary), timeout=timeout)
        if result.timed_out:
            raise ToolError("timeout", "vista-host windows timed out")
        if result.returncode != 0:
            raise ToolError(
                "command_failed",
                "vista-host windows failed",
                {
                    "returncode": result.returncode,
                    "stderr": result.stderr_text[-2000:],
                },
            )
        text = result.stdout_text.strip()
        try:
            return json.loads(text)
        except json.JSONDecodeError as error:
            raise ToolError(
                "invalid_response",
                f"vista-host windows returned invalid JSON: {error}",
                {"stdout": text[-2000:]},
            ) from error

    def run_live_client(
        self,
        *,
        address: str,
        secret_hex: str,
        target: str,
        frames: int,
        h264_path: Path | None,
        pointer: str | None,
        text: str | None,
        allow_input: bool,
        timeout: float,
    ) -> dict[str, Any]:
        binary = self.host_binary()
        argv = host_live_client_argv(
            binary,
            address,
            secret_hex,
            target,
            frames,
            h264_path,
            pointer,
            text,
            allow_input,
        )
        result = self.runner.run(argv, timeout=timeout)
        if result.timed_out:
            raise ToolError(
                "timeout",
                f"live client timed out after {timeout:g}s",
                {"frames": frames},
            )
        if result.returncode != 0:
            raise ToolError(
                "command_failed",
                "vista-host live-client failed",
                {
                    "returncode": result.returncode,
                    "stdout": result.stdout_text[-3000:],
                    "stderr": result.stderr_text[-3000:],
                },
            )
        report: Any = None
        for line in reversed(result.stdout_text.splitlines()):
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                report = json.loads(line)
                break
            except json.JSONDecodeError:
                continue
        if not isinstance(report, dict):
            raise ToolError(
                "invalid_response",
                "live-client did not print a JSON report",
                {"stdout": result.stdout_text[-3000:]},
            )
        report = dict(report)
        report["stderr"] = result.stderr_text.strip()
        return report
