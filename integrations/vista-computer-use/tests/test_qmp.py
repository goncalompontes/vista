from __future__ import annotations

import json
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from support import RecordingRunner, make_config

from vista_computer_use.errors import ToolError
from vista_computer_use.qmp import (
    QmpClient,
    discover_qmp_socket,
    qcode_for_key,
    qmp_socket_from_cmdline,
)


class QmpParsingTests(unittest.TestCase):
    def test_socket_from_qmp_argument(self) -> None:
        self.assertEqual(
            qmp_socket_from_cmdline(
                ["qemu-system-aarch64", "-qmp", "unix:/tmp/a.sock,server=on,wait=off"]
            ),
            Path("/tmp/a.sock"),
        )
        self.assertEqual(
            qmp_socket_from_cmdline(["qemu", "-qmp=unix:/tmp/b.sock"]),
            Path("/tmp/b.sock"),
        )
        self.assertIsNone(qmp_socket_from_cmdline(["qemu", "-display", "none"]))

    def test_key_mapping(self) -> None:
        self.assertEqual(qcode_for_key("return"), "ret")
        self.assertEqual(qcode_for_key("A"), "a")
        self.assertEqual(qcode_for_key("7"), "7")
        with self.assertRaises(ToolError) as raised:
            qcode_for_key("f13")
        self.assertEqual(raised.exception.code, "invalid_arguments")


class QmpActionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.config = make_config(Path(self.tmp.name))
        self.sock = self.config.workspace / "qmp.sock"
        self.sock.touch()

    def test_tap_uses_scaled_grid_and_reuses_qmp_script(self) -> None:
        runner = RecordingRunner()
        client = QmpClient(self.config, runner, socket_path=self.sock)
        client.tap(224, 466, 448, 932)
        call = runner.calls[0]
        self.assertEqual(call.argv[3], "tap")
        self.assertAlmostEqual(float(call.argv[4]), 645.0)
        self.assertAlmostEqual(float(call.argv[5]), 1398.0)

    def test_swipe_key_and_screenshot(self) -> None:
        runner = RecordingRunner()
        client = QmpClient(self.config, runner, socket_path=self.sock)
        client.swipe(0, 0, 448, 932, 448, 932, 300, 20)
        client.key("return")
        output = self.config.screenshot_dir / "qmp.png"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(b"x")
        client.screenshot(output)
        self.assertEqual(runner.calls[0].argv[3], "swipe")
        self.assertEqual(runner.calls[1].argv[3:], ("key", "ret"))
        self.assertEqual(runner.calls[2].argv[3], "shot")
        self.assertEqual(runner.calls[2].argv[4], str(output))

    def test_missing_socket_is_a_clear_error(self) -> None:
        client = QmpClient(self.config, RecordingRunner(), socket_path=self.config.workspace / "absent.sock")
        with self.assertRaises(ToolError) as raised:
            client.require_socket()
        self.assertEqual(raised.exception.code, "qmp_unavailable")


class QmpQueryTests(unittest.TestCase):
    def test_query_handshake_and_hmp_forward(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        path = Path(tmp.name) / "qmp.sock"
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(path))
        server.listen(1)
        self.addCleanup(server.close)
        seen: list[dict[str, object]] = []

        def serve() -> None:
            for _ in range(2):
                connection, _ = server.accept()
                channel = connection.makefile("rwb")
                channel.write(b'{"QMP":{"version":{"qemu":{}},"capabilities":[]}}\n')
                channel.flush()
                while True:
                    line = channel.readline()
                    if not line:
                        break
                    message = json.loads(line)
                    seen.append(message)
                    if message.get("execute") == "qmp_capabilities":
                        channel.write(b'{"return":{}}\n')
                    elif message.get("execute") == "human-monitor-command":
                        channel.write(b'{"return":"ok"}\n')
                    else:
                        channel.write(b'{"return":{"running":true}}\n')
                    channel.flush()
                connection.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        config = make_config(Path(tmp.name))
        client = QmpClient(config, RecordingRunner(), socket_path=path)
        self.assertEqual(client.query("query-status"), {"running": True})
        result = client.forward_add("net0", 2222, 22)
        self.assertEqual(result["host_address"], "127.0.0.1:2222")
        self.assertEqual(
            seen[-1]["arguments"]["command-line"],
            "hostfwd_add net0 tcp:127.0.0.1:2222-:22",
        )
        thread.join(timeout=2)
        self.assertFalse(thread.is_alive())

    def test_discover_uses_configured_socket(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        config = make_config(Path(tmp.name))
        config = config.__class__(**{**config.__dict__, "qmp_socket": Path("/tmp/configured.sock")})
        self.assertEqual(discover_qmp_socket(config), Path("/tmp/configured.sock"))


if __name__ == "__main__":
    unittest.main()
