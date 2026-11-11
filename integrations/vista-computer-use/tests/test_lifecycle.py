from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from support import RecordingRunner, make_config

from vista_computer_use.errors import ToolError
from vista_computer_use.lifecycle import (
    VmManager,
    find_qemu_for_disk,
    list_vms,
    qemu_pids,
)


def _write_cmdline(proc_root: Path, pid: int, parts: list[str]) -> None:
    directory = proc_root / str(pid)
    directory.mkdir(parents=True)
    (directory / "cmdline").write_bytes(b"\0".join(part.encode() for part in parts) + b"\0")


class LifecycleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.config = make_config(self.root)
        self.config = self.config.__class__(
            **{**self.config.__dict__, "vphone_devices": self.root / "devices"}
        )
        self.devices = self.root / "devices"
        self.device = self.devices / "phone-a"
        self.device.mkdir(parents=True)
        (self.device / "Disk.img").write_bytes(b"")
        (self.device / ".create-state").write_text("sanity\nbooted\n")
        (self.device / "cid").write_text("5\n")
        self.proc = self.root / "proc"
        self.proc.mkdir()
        _write_cmdline(
            self.proc,
            1234,
            [
                "/opt/qemu-system-aarch64",
                "-drive",
                f"file={self.device / 'Disk.img'},if=none",
            ],
        )

    def test_find_qemu_and_list_vms(self) -> None:
        self.assertEqual(find_qemu_for_disk(self.device / "Disk.img", self.proc), 1234)
        vms = list_vms(self.config, self.proc)
        self.assertEqual(len(vms), 1)
        self.assertTrue(vms[0].running)
        self.assertEqual(vms[0].cid, 5)
        self.assertEqual(vms[0].name, "phone-a")

    def test_ambiguous_vm_requires_a_name(self) -> None:
        second = self.devices / "phone-b"
        second.mkdir(parents=True)
        (second / "Disk.img").write_bytes(b"")
        (second / ".create-state").write_text("booted\n")
        manager = VmManager(self.config, RecordingRunner(), self.proc)
        with self.assertRaises(ToolError) as raised:
            manager.resolve(None)
        self.assertEqual(raised.exception.code, "ambiguous_vm")
        self.assertEqual(manager.resolve("phone-b").name, "phone-b")

    def test_stop_uses_the_vphone_script(self) -> None:
        manager = VmManager(self.config, RecordingRunner(), self.proc)
        result = manager.stop("phone-a")
        self.assertTrue(result["stopped"])
        call = manager.runner.calls[0]
        self.assertEqual(call.argv[-2:], ("--stop", "phone-a"))

    def test_start_returns_immediately_with_log(self) -> None:
        # Make the fixture VM look stopped so start() takes the spawn path.
        (self.proc / "1234" / "cmdline").unlink()
        script = self.config.start_vm_script
        script.write_text("#!/bin/sh\nexit 0\n")
        script.chmod(0o755)
        manager = VmManager(self.config, RecordingRunner(), self.proc)
        result = manager.start("phone-a", cid=5, headless=True, unlock=False)
        self.assertTrue(result["started"])
        self.assertTrue(Path(result["log"]).is_file())

    def test_wait_returns_ready_from_health(self) -> None:
        class FakeClient:
            def __init__(self, *args: object, **kwargs: object) -> None:
                pass

            def health(self, timeout: float = 8.0) -> dict[str, object]:
                return {"status": "ok"}

            def screen(self) -> dict[str, object]:
                return {"width": 448, "height": 932, "scale": 1}

        manager = VmManager(self.config, RecordingRunner(), self.proc)
        with mock.patch("vista_computer_use.lifecycle.VphoneClient", FakeClient):
            result = manager.wait("phone-a", cid=5, timeout_seconds=5)
        self.assertTrue(result["ready"])
        self.assertEqual(result["cid"], 5)

    def test_qemu_inventory_reports_only_qemu(self) -> None:
        rows = qemu_pids(self.proc)
        self.assertEqual(rows[0]["pid"], 1234)


if __name__ == "__main__":
    unittest.main()
