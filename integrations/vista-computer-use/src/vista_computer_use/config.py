"""Environment-driven configuration for the Vista computer-use MCP server.

The server is intentionally usable without installation: ``bin/vista-computer-use-mcp``
puts ``src`` on ``PYTHONPATH`` and the repository root is derived from the
package location. Every path can be overridden for a non-default checkout.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_VPHONE_CID = 3
DEFAULT_MAX_IMAGE_BYTES = 3_000_000
DEFAULT_REQUEST_TIMEOUT = 30.0


def _env(env: dict[str, str], name: str) -> str | None:
    value = env.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _env_path(env: dict[str, str], name: str) -> Path | None:
    value = _env(env, name)
    if value is None:
        return None
    return Path(value).expanduser()


def _env_int(env: dict[str, str], name: str, default: int) -> int:
    value = _env(env, name)
    if value is None:
        return default
    try:
        return int(value, 10)
    except ValueError as error:
        raise ValueError(f"{name} must be an integer, got {value!r}") from error


def _env_float(env: dict[str, str], name: str, default: float) -> float:
    value = _env(env, name)
    if value is None:
        return default
    try:
        return float(value)
    except ValueError as error:
        raise ValueError(f"{name} must be a number, got {value!r}") from error


def _workspace(env: dict[str, str], cwd: Path) -> Path:
    explicit = _env_path(env, "VISTA_HOME")
    if explicit is not None:
        return explicit.resolve()

    # src/vista_computer_use/config.py -> repo root is four parents up.
    derived = Path(__file__).resolve().parents[4]
    candidates = [derived, cwd, cwd.parent]
    for candidate in candidates:
        if (candidate / "host" / "Cargo.toml").is_file():
            return candidate.resolve()
    return cwd.resolve()


def _state_dir(env: dict[str, str]) -> Path:
    explicit = _env_path(env, "VISTA_CU_STATE_DIR")
    if explicit is not None:
        return explicit
    xdg = _env_path(env, "XDG_STATE_HOME")
    if xdg is not None:
        return xdg / "vista-computer-use"
    return Path.home() / ".local" / "state" / "vista-computer-use"


def _allowed_roots(
    env: dict[str, str], workspace: Path, state_dir: Path, screenshot_dir: Path
) -> tuple[Path, ...]:
    raw = _env(env, "VISTA_CU_ALLOWED_ROOTS")
    if raw is None:
        candidates = [workspace, state_dir, screenshot_dir, Path("/tmp/opencode")]
    else:
        candidates = [Path(item).expanduser() for item in raw.split(os.pathsep) if item]
    roots: list[Path] = []
    for candidate in candidates:
        resolved = candidate.resolve()
        if resolved not in roots:
            roots.append(resolved)
    return tuple(roots)


@dataclass(frozen=True)
class Config:
    """Resolved paths and defaults for one MCP server process."""

    workspace: Path
    vphone_home: Path
    vphone_devices: Path
    vphone_cid: int | None
    vphone_vm: str | None
    state_dir: Path
    screenshot_dir: Path
    allowed_write_roots: tuple[Path, ...] = field(default_factory=tuple)
    qmp_socket: Path | None = None
    vista_host_bin: Path | None = None
    max_image_bytes: int = DEFAULT_MAX_IMAGE_BYTES
    request_timeout: float = DEFAULT_REQUEST_TIMEOUT
    host_start_timeout: float = 20.0

    @classmethod
    def from_env(
        cls,
        env: dict[str, str] | None = None,
        cwd: Path | None = None,
    ) -> "Config":
        env = dict(os.environ if env is None else env)
        cwd = (cwd or Path.cwd()).resolve()
        workspace = _workspace(env, cwd)

        vphone_home = _env_path(env, "VPHONE_HOME") or (
            Path.home() / "projects" / "vphone-linux"
        )
        vphone_devices = _env_path(env, "VPHONE_DEVICES") or (
            Path.home() / "vphone-setup-work" / "devices"
        )
        state_dir = _state_dir(env)
        screenshot_dir = _env_path(env, "VISTA_CU_SCREENSHOT_DIR") or (
            workspace / "runs" / "computer-use" / "screenshots"
        )
        roots = _allowed_roots(env, workspace, state_dir, screenshot_dir)

        cid_raw = _env(env, "VPHONE_CID")
        vphone_cid: int | None = None
        if cid_raw is not None:
            try:
                vphone_cid = int(cid_raw, 10)
            except ValueError as error:
                raise ValueError(f"VPHONE_CID must be an integer, got {cid_raw!r}") from error
            if not 1 <= vphone_cid <= 0xFFFFFFFF:
                raise ValueError("VPHONE_CID must be between 1 and 4294967295")

        host_bin = _env_path(env, "VISTA_HOST_BIN")
        if host_bin is None:
            for candidate in (
                workspace / "target" / "debug" / "vista-host",
                workspace / "target" / "release" / "vista-host",
                workspace / "host" / "target" / "debug" / "vista-host",
                workspace / "host" / "target" / "release" / "vista-host",
            ):
                if candidate.is_file():
                    host_bin = candidate
                    break

        max_image_bytes = _env_int(
            env, "VISTA_CU_MAX_IMAGE_BYTES", DEFAULT_MAX_IMAGE_BYTES
        )
        if max_image_bytes < 1024:
            raise ValueError("VISTA_CU_MAX_IMAGE_BYTES must be at least 1024")
        request_timeout = _env_float(
            env, "VISTA_CU_TIMEOUT", DEFAULT_REQUEST_TIMEOUT
        )
        if request_timeout <= 0:
            raise ValueError("VISTA_CU_TIMEOUT must be positive")

        return cls(
            workspace=workspace,
            vphone_home=vphone_home,
            vphone_devices=vphone_devices,
            vphone_cid=vphone_cid,
            vphone_vm=_env(env, "VPHONE_VM"),
            state_dir=state_dir,
            screenshot_dir=screenshot_dir,
            allowed_write_roots=roots,
            qmp_socket=_env_path(env, "VISTA_CU_QMP") or _env_path(env, "VPHONE_QMP"),
            vista_host_bin=host_bin,
            max_image_bytes=max_image_bytes,
            request_timeout=request_timeout,
        )

    @property
    def vphoned_script(self) -> Path:
        return self.vphone_home / "tools" / "vphoned.sh"

    @property
    def qmp_script(self) -> Path:
        return self.vphone_home / "tools" / "qmp.py"

    @property
    def install_ipa_script(self) -> Path:
        return self.vphone_home / "tools" / "install-ipa.py"

    @property
    def app_logs_script(self) -> Path:
        return self.vphone_home / "tools" / "app-logs.py"

    @property
    def start_vm_script(self) -> Path:
        return self.vphone_home / "setup" / "start-vm.sh"

    def public(self) -> dict[str, object]:
        """Configuration safe to show in a status result."""
        return {
            "workspace": str(self.workspace),
            "vphone_home": str(self.vphone_home),
            "vphone_devices": str(self.vphone_devices),
            "vphone_cid": self.vphone_cid,
            "vphone_vm": self.vphone_vm,
            "state_dir": str(self.state_dir),
            "screenshot_dir": str(self.screenshot_dir),
            "qmp_socket": str(self.qmp_socket) if self.qmp_socket else None,
            "vista_host_bin": str(self.vista_host_bin) if self.vista_host_bin else None,
            "request_timeout": self.request_timeout,
            "max_image_bytes": self.max_image_bytes,
        }
