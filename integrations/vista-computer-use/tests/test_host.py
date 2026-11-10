from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from support import RecordingRunner, make_config

from vista_computer_use.errors import ToolError
from vista_computer_use.host import HostServiceManager
from vista_computer_use.runner import CommandResult


class HostServiceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.config = make_config(Path(self.tmp.name))
        self.host = self.config.workspace / "host" / "target" / "debug" / "vista-host"
        self.host.write_text(
            "#!/usr/bin/env python3\n"
            "import json, time\n"
            "print(json.dumps({'protocol':'1.0','address':'127.0.0.1:59999',"
            "'secret_hex':'ab'*32,'host_name':'test','input':False,'fps':15}), flush=True)\n"
            "time.sleep(30)\n"
        )
        self.host.chmod(0o755)

    def test_start_reads_invite_and_stop_terminates(self) -> None:
        manager = HostServiceManager(self.config, RecordingRunner())
        started = manager.start(
            listen="127.0.0.1:0",
            fps=15,
            allow_remote=False,
            allow_window_actions=False,
            allow_desktop_actions=False,
            allow_clipboard=False,
            allow_input=False,
            once=False,
            secret_hex=None,
            start_timeout=5,
        )
        self.assertTrue(started["started"])
        self.assertEqual(started["invite"]["secret_hex"], "ab" * 32)
        self.assertTrue(manager.state_path.is_file())
        self.assertEqual(manager.state_path.stat().st_mode & 0o777, 0o600)
        status = manager.status()
        self.assertTrue(status["running"])
        stopped = manager.stop()
        self.assertTrue(stopped["stopped"])
        self.assertFalse(manager.state_path.exists())

    def test_start_refuses_remote_without_confirmation(self) -> None:
        manager = HostServiceManager(self.config, RecordingRunner())
        with self.assertRaises(ToolError) as raised:
            manager.start(
                listen="0.0.0.0:7443",
                fps=15,
                allow_remote=False,
                allow_window_actions=False,
                allow_desktop_actions=False,
                allow_clipboard=False,
                allow_input=False,
                once=False,
                secret_hex=None,
            )
        self.assertEqual(raised.exception.code, "invalid_arguments")

    def test_live_client_parses_report(self) -> None:
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=b'{"frames":3,"bytes":100,"verified":true}\n',
                stderr=b"",
                duration_ms=1,
            )
        )
        manager = HostServiceManager(self.config, runner)
        report = manager.run_live_client(
            address="127.0.0.1:7443",
            secret_hex="cd" * 32,
            target="desktop",
            frames=3,
            h264_path=None,
            pointer=None,
            text=None,
            allow_input=False,
            timeout=10,
        )
        self.assertTrue(report["verified"])
        self.assertEqual(runner.calls[0].argv[1], "live-client")
        self.assertIn("--json", runner.calls[0].argv)

    def test_windows_parses_json(self) -> None:
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=json.dumps([{"id": 1, "title": "x"}]).encode(),
                stderr=b"",
                duration_ms=1,
            )
        )
        self.assertEqual(HostServiceManager(self.config, runner).windows()[0]["id"], 1)


if __name__ == "__main__":
    unittest.main()
