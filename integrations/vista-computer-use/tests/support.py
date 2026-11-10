"""Test doubles and fixture helpers."""

from __future__ import annotations

import base64
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence

from vista_computer_use.config import Config
from vista_computer_use.runner import CommandResult, Runner
from vista_computer_use.vphone import ScreenCache, VphoneClient


@dataclass
class RecordedCall:
    argv: tuple[str, ...]
    timeout: float
    env: dict[str, str]
    cwd: Path | None
    stdin: bytes | None


class RecordingRunner(Runner):
    def __init__(
        self,
        handler: Callable[[Sequence[str], float, Mapping[str, str] | None], CommandResult] | None = None,
    ) -> None:
        self.calls: list[RecordedCall] = []
        self.handler = handler

    def run(
        self,
        argv: Sequence[str],
        *,
        timeout: float,
        env: Mapping[str, str] | None = None,
        cwd: Path | None = None,
        stdin: bytes | None = None,
    ) -> CommandResult:
        argv_tuple = tuple(map(str, argv))
        self.calls.append(
            RecordedCall(
                argv=argv_tuple,
                timeout=timeout,
                env=dict(env or {}),
                cwd=cwd,
                stdin=stdin,
            )
        )
        if self.handler is not None:
            return self.handler(argv_tuple, timeout, env)
        return CommandResult(
            argv=argv_tuple,
            returncode=0,
            stdout=b"",
            stderr=b"",
            duration_ms=1,
        )


class FakeTransport:
    def __init__(
        self,
        *,
        health: dict[str, Any] | None = None,
        rpc: Callable[[str, dict[str, Any]], Any] | None = None,
    ) -> None:
        self.health = health or {"status": "ok", "ip": "10.0.2.15"}
        self.rpc_handler = rpc
        self.calls: list[tuple[str, dict[str, Any], float]] = []

    def get_json(self, path: str, *, timeout: float = 10.0) -> Any:
        if path != "/v1/health":
            raise AssertionError(f"unexpected GET {path}")
        return self.health

    def rpc(
        self,
        method: str,
        params: dict[str, Any] | None = None,
        *,
        timeout: float = 30.0,
    ) -> Any:
        payload = dict(params or {})
        self.calls.append((method, payload, timeout))
        if self.rpc_handler is not None:
            return self.rpc_handler(method, payload)
        if method == "device.screen":
            return {"width": 448, "height": 932, "scale": 1, "locked": True}
        if method == "screen.screenshot":
            return {
                "data": base64.b64encode(b"\xff\xd8\xff\xd9").decode("ascii"),
                "mime_type": "image/jpeg",
                "width": 448,
                "height": 932,
                "coordinate_scale": 1,
            }
        if method == "apps.list":
            return {"apps": [{"bundle_id": "dev.vista.client"}]}
        if method == "apps.search":
            return {"apps": [{"bundle_id": payload.get("query")}]}
        if method == "apps.foreground":
            return {"bundle_id": "dev.vista.client", "verified": True}
        if method == "logs.crashes":
            return {"crashes": [], "count": 0}
        if method == "device.network":
            return {"addresses": ["en0 10.0.2.15"]}
        if method == "network.ipv4.get":
            return {"address": "10.0.2.15"}
        if method == "network.hostname.get":
            return {"local_host_name": "iPhone"}
        if method == "network.resolve":
            return {"status": "resolved", "addresses": []}
        if method == "display.orientation":
            return {"degrees": 0}
        return {"ok": True, "method": method}


class FakeQmp:
    def __init__(self, socket_path: Path | None = None) -> None:
        self._socket = socket_path or Path("/tmp/fake-qmp.sock")
        self.calls: list[tuple[str, tuple[Any, ...]]] = []

    @property
    def socket_path(self) -> Path:
        return self._socket

    def require_socket(self) -> Path:
        return self._socket

    def status(self) -> dict[str, Any]:
        return {"available": True, "socket": str(self._socket), "status": {"running": True}}

    def tap(self, *args: Any) -> dict[str, Any]:
        self.calls.append(("tap", args))
        return {"returncode": 0}

    def swipe(self, *args: Any) -> dict[str, Any]:
        self.calls.append(("swipe", args))
        return {"returncode": 0}

    def key(self, *args: Any) -> dict[str, Any]:
        self.calls.append(("key", args))
        return {"returncode": 0}

    def home(self) -> dict[str, Any]:
        self.calls.append(("home", ()))
        return {"returncode": 0}

    def screenshot(self, path: Path) -> dict[str, Any]:
        self.calls.append(("screenshot", (path,)))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 40)
        return {"returncode": 0}

    def forward_add(self, netdev: str, host_port: int, guest_port: int) -> dict[str, Any]:
        self.calls.append(("forward_add", (netdev, host_port, guest_port)))
        return {"action": "add", "host_address": f"127.0.0.1:{host_port}"}

    def forward_remove(self, netdev: str, host_port: int) -> dict[str, Any]:
        self.calls.append(("forward_remove", (netdev, host_port)))
        return {"action": "remove", "host_address": f"127.0.0.1:{host_port}"}


class FakeHost:
    def __init__(self, config: Config, host_bin: Path | None = None) -> None:
        self.config = config
        self._host_bin = host_bin or (config.workspace / "host" / "target" / "debug" / "vista-host")
        self.started = False
        self.invite_value = {
            "protocol": "1.0",
            "address": "127.0.0.1:7443",
            "secret_hex": "11" * 32,
            "host_name": "Vista host",
            "input": False,
            "fps": 15,
        }
        self.live_calls: list[dict[str, Any]] = []
        self.window_rows: list[dict[str, Any]] = []

    def host_binary(self) -> Path:
        return self._host_bin

    def start(self, **kwargs: Any) -> dict[str, Any]:
        self.started = True
        self.start_kwargs = kwargs
        return {
            "started": True,
            "running": True,
            "managed": True,
            "pid": 1234,
            "listening": True,
            "invite": self.invite_value,
        }

    def status(self) -> dict[str, Any]:
        return {
            "running": self.started,
            "managed": self.started,
            "pid": 1234 if self.started else None,
            "listening": self.started,
            "invite": self.invite_value if self.started else None,
            "state": None,
        }

    def stop(self, *, force: bool = False) -> dict[str, Any]:
        self.started = False
        self.stop_force = force
        return {"stopped": True, "already_stopped": False}

    def invite(self) -> dict[str, Any]:
        if not self.started:
            from vista_computer_use.errors import ToolError

            raise ToolError("not_running", "not running")
        return self.invite_value

    def windows(self, timeout: float = 15.0) -> list[dict[str, Any]]:
        return self.window_rows

    def run_live_client(self, **kwargs: Any) -> dict[str, Any]:
        self.live_calls.append(kwargs)
        return {
            "address": kwargs["address"],
            "target": kwargs["target"],
            "frames": kwargs["frames"],
            "bytes": 1234,
            "width": 1280,
            "height": 720,
            "verified": True,
        }


class FakeVm:
    def __init__(self, config: Config) -> None:
        self.config = config
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def list(self) -> list[Any]:
        return []

    def resolve(self, name: str | None) -> Any:
        from vista_computer_use.errors import ToolError

        raise ToolError("not_found", "no VM")

    def start(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append(("start", kwargs))
        return {"started": True, "name": "test"}

    def wait(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append(("wait", kwargs))
        return {"ready": True}

    def stop(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append(("stop", kwargs))
        return {"stopped": True}


def make_config(tmp_path: Path) -> Config:
    workspace = tmp_path / "vista"
    vphone = tmp_path / "vphone"
    state = tmp_path / "state"
    screenshots = workspace / "runs" / "computer-use" / "screenshots"
    for path in (
        workspace / "host",
        vphone / "tools",
        vphone / "setup",
        state,
        screenshots,
    ):
        path.mkdir(parents=True, exist_ok=True)
    for path in (
        vphone / "tools" / "vphoned.sh",
        vphone / "tools" / "qmp.py",
        vphone / "tools" / "install-ipa.py",
        vphone / "tools" / "app-logs.py",
        vphone / "setup" / "start-vm.sh",
    ):
        path.write_text("#!/bin/sh\nexit 0\n")
        path.chmod(0o755)
    host_bin = workspace / "host" / "target" / "debug" / "vista-host"
    host_bin.parent.mkdir(parents=True, exist_ok=True)
    host_bin.write_text("#!/bin/sh\nexit 0\n")
    host_bin.chmod(0o755)
    return Config(
        workspace=workspace,
        vphone_home=vphone,
        vphone_devices=tmp_path / "devices",
        vphone_cid=3,
        vphone_vm=None,
        state_dir=state,
        screenshot_dir=screenshots,
        allowed_write_roots=(workspace, state, screenshots, Path("/tmp/opencode")),
        qmp_socket=tmp_path / "qmp.sock",
        vista_host_bin=host_bin,
    )


def make_services(
    tmp_path: Path,
    *,
    transport: FakeTransport | None = None,
    runner: RecordingRunner | None = None,
    rpc_handler: Callable[[str, dict[str, Any]], Any] | None = None,
) -> Any:
    from vista_computer_use.tools.common import Services

    config = make_config(tmp_path)
    runner = runner or RecordingRunner()
    transport = transport or FakeTransport(rpc=rpc_handler)
    vphone = VphoneClient(config, runner, 3, transport=transport)
    services = Services(
        config=config,
        vphone=vphone,
        qmp=FakeQmp(config.qmp_socket),
        host=FakeHost(config, config.vista_host_bin),
        screen=ScreenCache(vphone),
        vm=FakeVm(config),
    )
    return services
