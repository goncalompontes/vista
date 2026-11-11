from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from support import FakeTransport, RecordingRunner, make_services

from vista_computer_use.mcp import McpServer
from vista_computer_use.runner import CommandResult
from vista_computer_use.tools import build_registry


def call(server: McpServer, name: str, arguments: dict[str, object]) -> dict[str, object]:
    response = server.handle_message(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
    )
    assert response is not None
    return response["result"]  # type: ignore[return-value]


class DeviceToolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.transport = FakeTransport()
        self.services = make_services(Path(self.tmp.name), transport=self.transport)
        self.server = McpServer(build_registry(self.services))

    def test_tap_uses_points(self) -> None:
        result = call(self.server, "vista_tap", {"x": 10, "y": 20})
        self.assertFalse(result["isError"])
        self.assertEqual(self.transport.calls[-1][:2], ("input.tap", {"x": 10, "y": 20}))

    def test_tap_converts_pixels(self) -> None:
        self.transport.rpc_handler = lambda method, params: (
            {"width": 448, "height": 932, "scale": 2}
            if method == "device.screen"
            else {"ok": True}
        )
        # The cache is fresh; replace it with a new one.
        from vista_computer_use.vphone import ScreenCache

        self.services.screen = ScreenCache(self.services.vphone)
        result = call(self.server, "vista_tap", {"x": 40, "y": 60, "space": "pixels"})
        self.assertFalse(result["isError"])
        self.assertEqual(self.transport.calls[-1][1], {"x": 20, "y": 30})

    def test_out_of_bounds_tap_is_rejected(self) -> None:
        result = call(self.server, "vista_tap", {"x": 448, "y": 20})
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "invalid_arguments")

    def test_nested_tree_rejects_flat_filters(self) -> None:
        result = call(
            self.server,
            "vista_ui_tree",
            {"nested": True, "visible_only": True},
        )
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "invalid_arguments")

    def test_screenshot_saves_file_and_returns_image(self) -> None:
        result = call(self.server, "vista_screenshot", {})
        self.assertFalse(result["isError"])
        self.assertEqual(result["content"][0]["type"], "image")
        metadata = result["structuredContent"]
        self.assertTrue(Path(metadata["saved_to"]).is_file())
        self.assertEqual(metadata["source"], "guest")

    def test_screenshot_rejects_path_outside_roots(self) -> None:
        result = call(
            self.server,
            "vista_screenshot",
            {"path": "/etc/vista-shot.jpg"},
        )
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "unsafe_path")

    def test_unlock_requires_confirm(self) -> None:
        result = call(self.server, "vista_unlock", {})
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "confirmation_required")


class AppToolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.services = make_services(Path(self.tmp.name))
        self.server = McpServer(build_registry(self.services))

    def test_install_requires_confirmation_and_file(self) -> None:
        result = call(self.server, "vista_app_install", {"ipa_path": "missing.ipa"})
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "confirmation_required")

    def test_install_with_confirmation(self) -> None:
        ipa = self.services.config.workspace / "App.ipa"
        ipa.write_bytes(b"package")
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=b"Installed dev.vista.client\n",
                stderr=b"",
                duration_ms=1,
            )
        )
        self.services.vphone.runner = runner
        result = call(
            self.server,
            "vista_app_install",
            {"ipa_path": str(ipa), "confirm": True},
        )
        self.assertFalse(result["isError"])
        self.assertEqual(result["structuredContent"]["install"]["installed_bundle_id"], "dev.vista.client")

    def test_logs_are_bounded_to_recent_entries(self) -> None:
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=b'{"message":"one"}\n{"message":"two"}\n{"message":"three"}\n',
                stderr=b"",
                duration_ms=1,
            )
        )
        self.services.vphone.runner = runner
        result = call(
            self.server,
            "vista_app_logs",
            {"process": "Vista", "max_entries": 2},
        )
        self.assertFalse(result["isError"])
        entries = result["structuredContent"]["entries"]
        self.assertEqual([entry["message"] for entry in entries], ["two", "three"])
        self.assertEqual(result["structuredContent"]["omitted"], 1)

    def test_client_deploy_requires_confirm(self) -> None:
        result = call(self.server, "vista_client_deploy", {})
        self.assertTrue(result["isError"])
        self.assertEqual(result["structuredContent"]["error"]["code"], "confirmation_required")


class NetworkToolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.services = make_services(Path(self.tmp.name))
        self.server = McpServer(build_registry(self.services))

    def test_forward_requires_confirm_and_builds_hmp(self) -> None:
        result = call(
            self.server,
            "vista_network_forward",
            {"action": "add", "host_port": 2222, "guest_port": 22},
        )
        self.assertTrue(result["isError"])
        result = call(
            self.server,
            "vista_network_forward",
            {"action": "add", "host_port": 2222, "guest_port": 22, "confirm": True},
        )
        self.assertFalse(result["isError"])
        self.assertEqual(
            self.services.qmp.calls[-1],
            ("forward_add", ("net0", 2222, 22)),
        )

    def test_resolve_rejects_bad_domain(self) -> None:
        result = call(self.server, "vista_network_resolve", {"host": "bad host"})
        self.assertTrue(result["isError"])


class VistaToolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.services = make_services(Path(self.tmp.name))
        self.server = McpServer(build_registry(self.services))

    def test_host_start_exposes_invite(self) -> None:
        result = call(self.server, "vista_host_start", {})
        self.assertFalse(result["isError"])
        self.assertEqual(result["structuredContent"]["invite"]["secret_hex"], "11" * 32)

    def test_live_client_uses_managed_invite(self) -> None:
        call(self.server, "vista_host_start", {})
        result = call(
            self.server,
            "vista_live_client",
            {"frames": 3, "confirm": True},
        )
        self.assertFalse(result["isError"])
        self.assertEqual(self.services.host.live_calls[-1]["frames"], 3)
        self.assertNotIn("secret_hex", result["structuredContent"])

    def test_status_redacts_secret(self) -> None:
        call(self.server, "vista_host_start", {})
        result = call(self.server, "vista_status", {})
        self.assertFalse(result["isError"])
        self.assertEqual(
            result["structuredContent"]["host_service"]["invite"]["secret_hex"],
            "<redacted>",
        )


if __name__ == "__main__":
    unittest.main()
