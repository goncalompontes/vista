from __future__ import annotations

import unittest
from pathlib import Path

from vista_computer_use.commands import (
    app_logs_argv,
    format_coord,
    hmp_hostfwd_add,
    hmp_hostfwd_remove,
    host_live_client_argv,
    host_serve_argv,
    host_windows_argv,
    install_ipa_argv,
    point_to_qmp_grid,
    qmp_key_argv,
    qmp_screenshot_argv,
    qmp_swipe_argv,
    qmp_tap_argv,
    start_vm_argv,
    stop_vm_argv,
)

ROOT = Path("/opt/vphone")
TOOLS = ROOT / "tools"
PY = "/usr/bin/python3"
HOST = Path("/opt/vista/host/target/debug/vista-host")
SOCK = Path("/tmp/qmp.sock")


class QmpCommandTests(unittest.TestCase):
    def test_point_scaling_uses_the_guest_geometry(self) -> None:
        gx, gy = point_to_qmp_grid(224, 466, 448, 932)
        self.assertAlmostEqual(gx, 645.0)
        self.assertAlmostEqual(gy, 1398.0)

    def test_tap_forwards_scaled_coordinates(self) -> None:
        argv = qmp_tap_argv(PY, TOOLS / "qmp.py", SOCK, 224, 466, 448, 932)
        self.assertEqual(argv[:4], [PY, str(TOOLS / "qmp.py"), str(SOCK), "tap"])
        self.assertAlmostEqual(float(argv[4]), 645.0)
        self.assertAlmostEqual(float(argv[5]), 1398.0)

    def test_swipe_includes_duration_and_steps(self) -> None:
        argv = qmp_swipe_argv(PY, TOOLS / "qmp.py", SOCK, 0, 0, 448, 932, 448, 932, 300, 20)
        self.assertEqual(argv[3], "swipe")
        self.assertEqual(argv[8:], ["300", "20"])

    def test_key_and_screenshot(self) -> None:
        self.assertEqual(
            qmp_key_argv(PY, TOOLS / "qmp.py", SOCK, ["ret"])[3:],
            ["key", "ret"],
        )
        self.assertEqual(
            qmp_screenshot_argv(PY, TOOLS / "qmp.py", SOCK, Path("/tmp/out.png"))[3:],
            ["shot", "/tmp/out.png"],
        )

    def test_coord_formatting_is_stable(self) -> None:
        self.assertEqual(format_coord(1.5), "1.5")
        self.assertEqual(format_coord(2.0), "2")
        self.assertEqual(format_coord(0.0), "0")


class HostCommandTests(unittest.TestCase):
    def test_serve_command_includes_grants_and_secret(self) -> None:
        argv = host_serve_argv(
            HOST,
            "127.0.0.1:7443",
            24,
            False,
            True,
            False,
            True,
            True,
            True,
            "ab" * 32,
        )
        self.assertEqual(argv[:6], [str(HOST), "serve", "--confirm", "--json", "--listen", "127.0.0.1:7443"])
        self.assertIn("--fps", argv)
        self.assertIn("24", argv)
        self.assertIn("--allow-window-actions", argv)
        self.assertIn("--allow-clipboard", argv)
        self.assertIn("--allow-input", argv)
        self.assertIn("--once", argv)
        self.assertEqual(argv[-2:], ["--secret-hex", "ab" * 32])
        self.assertNotIn("--allow-remote", argv)

    def test_live_client_command(self) -> None:
        argv = host_live_client_argv(
            HOST,
            "127.0.0.1:7443",
            "cd" * 32,
            "window:8",
            10,
            Path("/tmp/out.h264"),
            "0.5:0.5",
            "hello",
            True,
        )
        self.assertEqual(argv[:5], [str(HOST), "live-client", "--connect", "127.0.0.1:7443", "--secret-hex"])
        self.assertIn("--target", argv)
        self.assertIn("window:8", argv)
        self.assertIn("--h264", argv)
        self.assertIn("--pointer", argv)
        self.assertIn("--text", argv)
        self.assertIn("--allow-input", argv)
        self.assertEqual(host_windows_argv(HOST), [str(HOST), "windows", "--json"])


class VphoneScriptCommandTests(unittest.TestCase):
    def test_install_ipa_command(self) -> None:
        argv = install_ipa_argv(
            PY,
            TOOLS / "install-ipa.py",
            Path("/tmp/App.ipa"),
            5,
            "dev.vista.client",
        )
        self.assertEqual(
            argv,
            [
                PY,
                str(TOOLS / "install-ipa.py"),
                "/tmp/App.ipa",
                "--cid",
                "5",
                "--launch",
                "dev.vista.client",
            ],
        )

    def test_app_logs_command(self) -> None:
        argv = app_logs_argv(
            PY,
            TOOLS / "app-logs.py",
            "Vista",
            3,
            12,
            "dev.vista.client",
        )
        self.assertEqual(argv[0], PY)
        self.assertIn("Vista", argv)
        self.assertIn("--subsystem", argv)
        self.assertIn("dev.vista.client", argv)
        self.assertIn("--json", argv)

    def test_start_and_stop_commands(self) -> None:
        start = start_vm_argv("bash", ROOT / "setup/start-vm.sh", "phone", 3, True, False)
        self.assertEqual(
            start,
            [
                "bash",
                str(ROOT / "setup/start-vm.sh"),
                "--cid",
                "3",
                "--no-window",
                "--no-unlock",
                "phone",
            ],
        )
        self.assertEqual(
            stop_vm_argv("bash", ROOT / "setup/start-vm.sh", "phone"),
            ["bash", str(ROOT / "setup/start-vm.sh"), "--stop", "phone"],
        )


class ForwardCommandTests(unittest.TestCase):
    def test_hostfwd_commands_bind_loopback_only(self) -> None:
        self.assertEqual(
            hmp_hostfwd_add("net0", 2222, 22),
            "hostfwd_add net0 tcp:127.0.0.1:2222-:22",
        )
        self.assertEqual(
            hmp_hostfwd_remove("net0", 2222),
            "hostfwd_remove net0 tcp:127.0.0.1:2222",
        )


if __name__ == "__main__":
    unittest.main()
