"""VM discovery and lifecycle through vphone-linux's own scripts.

``setup/start-vm.sh`` already knows how to avoid a second QEMU on one disk,
choose a free vsock CID, detach QEMU, wait for vphoned, and unlock the guest.
This module adds discovery, non-interactive selection, and bounded waiting; it
does not reimplement the launcher.
"""

from __future__ import annotations

import os
import re
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from .commands import start_vm_argv, stop_vm_argv
from .config import Config
from .errors import ToolError
from .runner import Runner
from .vphone import VphoneClient

_VM_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")


def _cmdline(pid: str, proc_root: Path) -> list[str] | None:
    try:
        raw = (proc_root / pid / "cmdline").read_bytes()
    except OSError:
        return None
    if not raw:
        return None
    return [part.decode("utf-8", "replace") for part in raw.split(b"\0") if part]


def _pids(proc_root: Path) -> Iterable[str]:
    try:
        return (entry.name for entry in proc_root.iterdir() if entry.name.isdigit())
    except OSError:
        return ()


def find_qemu_for_disk(disk: Path, proc_root: Path = Path("/proc")) -> int | None:
    needle = f"file={disk}"
    for pid in _pids(proc_root):
        parts = _cmdline(pid, proc_root)
        if not parts or not any("qemu-system" in part for part in parts[:2]):
            continue
        if any(needle in part for part in parts):
            return int(pid)
    return None


@dataclass(frozen=True)
class VmInfo:
    name: str
    path: Path
    cid: int
    running: bool
    qemu_pid: int | None

    def as_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "path": str(self.path),
            "cid": self.cid,
            "running": self.running,
            "qemu_pid": self.qemu_pid,
        }


def _read_cid(path: Path) -> int:
    try:
        value = int((path / "cid").read_text().strip(), 10)
    except (OSError, ValueError):
        return 3
    return value if 1 <= value <= 0xFFFFFFFF else 3


def list_vms(config: Config, proc_root: Path = Path("/proc")) -> list[VmInfo]:
    root = config.vphone_devices
    try:
        directories = sorted(entry for entry in root.iterdir() if entry.is_dir())
    except OSError:
        return []
    vms: list[VmInfo] = []
    for directory in directories:
        disk = directory / "Disk.img"
        if not disk.is_file():
            continue
        try:
            state_lines = (directory / ".create-state").read_text().splitlines()
        except OSError:
            continue
        if "booted" not in {line.strip() for line in state_lines}:
            continue
        pid = find_qemu_for_disk(disk, proc_root)
        vms.append(
            VmInfo(
                name=directory.name,
                path=directory,
                cid=_read_cid(directory),
                running=pid is not None,
                qemu_pid=pid,
            )
        )
    return vms


class VmManager:
    def __init__(self, config: Config, runner: Runner, proc_root: Path = Path("/proc")) -> None:
        self.config = config
        self.runner = runner
        self.proc_root = proc_root

    def list(self) -> list[VmInfo]:
        return list_vms(self.config, self.proc_root)

    def resolve(self, name: str | None) -> VmInfo:
        vms = self.list()
        if name:
            if not _VM_NAME.fullmatch(name):
                raise ToolError("invalid_arguments", "VM name has an invalid format")
            for vm in vms:
                if vm.name == name:
                    return vm
            raise ToolError(
                "not_found",
                f"no VM named {name}",
                {"available": [vm.name for vm in vms]},
            )
        if self.config.vphone_vm:
            return self.resolve(self.config.vphone_vm)
        if len(vms) == 1:
            return vms[0]
        if not vms:
            raise ToolError(
                "not_found",
                "no complete VM was found",
                {"devices": str(self.config.vphone_devices)},
            )
        raise ToolError(
            "ambiguous_vm",
            "multiple VMs exist; pass name explicitly",
            {"available": [vm.name for vm in vms]},
        )

    def status(self, name: str | None = None) -> dict[str, Any]:
        vm = self.resolve(name)
        return vm.as_dict()

    def start(
        self,
        name: str | None,
        cid: int | None,
        headless: bool,
        unlock: bool,
        wait_seconds: float = 0.0,
    ) -> dict[str, Any]:
        vm = self.resolve(name)
        if vm.running:
            return {
                **vm.as_dict(),
                "started": False,
                "already_running": True,
            }
        if cid is not None and not 1 <= cid <= 0xFFFFFFFF:
            raise ToolError("invalid_arguments", "cid must be between 1 and 4294967295")
        script = self.config.start_vm_script
        if not script.is_file():
            raise ToolError(
                "not_configured",
                f"vphone start script not found: {script}",
                {"vphone_home": str(self.config.vphone_home)},
            )
        logs = self.config.state_dir / "logs"
        logs.mkdir(parents=True, exist_ok=True)
        stamp = time.strftime("%Y%m%d-%H%M%S")
        log_path = logs / f"vm-start-{vm.name}-{stamp}.log"
        argv = start_vm_argv(
            "bash",
            script,
            vm.name,
            cid if cid is not None else vm.cid,
            headless,
            unlock,
        )
        with open(log_path, "wb") as log:
            process = subprocess.Popen(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                cwd=str(self.config.workspace),
                env={**os.environ, "VSETUP_DEVICES": str(self.config.vphone_devices)},
            )
        # Give the script a moment to fail on a taken CID or a missing disk.
        time.sleep(0.35)
        if process.poll() is not None and process.returncode != 0:
            raise ToolError(
                "command_failed",
                "VM start script exited immediately",
                {
                    "returncode": process.returncode,
                    "log": str(log_path),
                    "tail": self._tail(log_path),
                },
            )
        result: dict[str, Any] = {
            "name": vm.name,
            "cid": cid if cid is not None else vm.cid,
            "started": True,
            "already_running": False,
            "pid": process.pid,
            "log": str(log_path),
            "headless": headless,
            "unlock": unlock,
            "wait": None,
        }
        if wait_seconds > 0:
            result["wait"] = self.wait(
                vm.name,
                cid if cid is not None else vm.cid,
                timeout_seconds=wait_seconds,
            )
        return result

    def wait(
        self,
        name: str | None,
        cid: int | None,
        timeout_seconds: float,
    ) -> dict[str, Any]:
        vm = self.resolve(name)
        cid = cid if cid is not None else vm.cid
        client = VphoneClient(self.config, self.runner, cid)
        started = time.monotonic()
        deadline = started + timeout_seconds
        last_error: str | None = None
        while time.monotonic() < deadline:
            try:
                health = client.health(timeout=5)
                if health.get("status") == "ok":
                    screen: dict[str, Any] | None = None
                    try:
                        screen = client.screen()
                    except ToolError as error:
                        last_error = error.message
                    return {
                        "name": vm.name,
                        "cid": cid,
                        "ready": True,
                        "health": health,
                        "screen": screen,
                        "elapsed_ms": int((time.monotonic() - started) * 1000),
                    }
                last_error = f"vphoned status: {health.get('status')!r}"
            except ToolError as error:
                last_error = error.message
            time.sleep(1.5)
        return {
            "name": vm.name,
            "cid": cid,
            "ready": False,
            "timed_out": True,
            "elapsed_ms": int((time.monotonic() - started) * 1000),
            "last_error": last_error,
        }

    def stop(self, name: str | None, timeout_seconds: float = 150.0) -> dict[str, Any]:
        vm = self.resolve(name)
        if not vm.running:
            return {**vm.as_dict(), "stopped": False, "already_stopped": True}
        script = self.config.start_vm_script
        if not script.is_file():
            raise ToolError("not_configured", f"vphone start script not found: {script}")
        result = self.runner.run(
            stop_vm_argv("bash", script, vm.name),
            timeout=timeout_seconds,
            env={"VSETUP_DEVICES": str(self.config.vphone_devices)},
        )
        if result.timed_out:
            raise ToolError(
                "timeout",
                "graceful VM shutdown did not finish in time; close the VM window to power it off",
                {"name": vm.name, "qemu_pid": vm.qemu_pid, "timeout_seconds": timeout_seconds},
            )
        if result.returncode != 0:
            raise ToolError(
                "command_failed",
                "VM shutdown failed",
                {
                    "returncode": result.returncode,
                    "stdout": result.stdout_text[-2000:],
                    "stderr": result.stderr_text[-2000:],
                },
            )
        return {
            "name": vm.name,
            "stopped": True,
            "already_stopped": False,
            "stdout": result.stdout_text.strip(),
        }

    @staticmethod
    def _tail(path: Path, limit: int = 3000) -> str:
        try:
            data = path.read_text(errors="replace")
        except OSError:
            return ""
        return data[-limit:]


def qemu_pids(proc_root: Path = Path("/proc")) -> list[dict[str, Any]]:
    """Read-only inventory used by the status tool."""
    found: list[dict[str, Any]] = []
    for pid in _pids(proc_root):
        parts = _cmdline(pid, proc_root)
        if not parts or not any("qemu-system" in part for part in parts[:2]):
            continue
        found.append({"pid": int(pid), "cmdline": parts})
    return found
