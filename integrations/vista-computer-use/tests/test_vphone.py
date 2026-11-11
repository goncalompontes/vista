from __future__ import annotations

import json
import socket
import sys
import tempfile
import unittest
from pathlib import Path

from support import FakeTransport, RecordingRunner, make_config

from vista_computer_use.errors import ToolError
from vista_computer_use.runner import CommandResult
from vista_computer_use.vphone import VphoneClient
from vista_computer_use.vphoned import HttpResponse, VphonedTransport, _http_error, _read_response


def _response_bytes(status: str, body: bytes, *, length: int | None = None) -> bytes:
    length = len(body) if length is None else length
    return (
        f"HTTP/1.1 {status}\r\nContent-Length: {length}\r\nContent-Type: application/json\r\n\r\n".encode()
        + body
    )


class HttpReaderTests(unittest.TestCase):
    def test_reads_exact_content_length(self) -> None:
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        left.sendall(_response_bytes("200 OK", b'{"result":1}'))
        left.close()
        response = _read_response(right)
        self.assertEqual(response.status, 200)
        self.assertEqual(json.loads(response.body), {"result": 1})

    def test_skips_uds_ok_prefix_lines(self) -> None:
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        left.sendall(b"OK 1024\r\n" + _response_bytes("200 OK", b'{"ok":true}'))
        left.close()
        response = _read_response(right, skip_prefix_lines=True)
        self.assertTrue(json.loads(response.body)["ok"])

    def test_short_body_is_rejected(self) -> None:
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        left.sendall(_response_bytes("200 OK", b"{}", length=10))
        left.close()
        with self.assertRaises(ToolError) as raised:
            _read_response(right)
        self.assertEqual(raised.exception.code, "invalid_response")

    def test_http_error_extracts_guest_message(self) -> None:
        response = HttpResponse(
            status=400,
            reason="Bad Request",
            headers={},
            body=b'{"error":{"code":"failed","message":"frontmost application could not be verified"}}',
        )
        error = _http_error(response)
        self.assertEqual(error.code, "guest_error")
        self.assertIn("frontmost", error.message)


class VphoneClientTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.config = make_config(Path(self.tmp.name))

    def test_health_and_rpc_use_the_transport(self) -> None:
        transport = FakeTransport(health={"status": "ok", "ip": "10.0.2.15"})
        client = VphoneClient(self.config, RecordingRunner(), 3, transport=transport)
        self.assertEqual(client.health()["ip"], "10.0.2.15")
        client.tap(10, 20)
        self.assertEqual(
            transport.calls[0][:2], ("input.tap", {"x": 10, "y": 20})
        )

    def test_guest_error_surfaces(self) -> None:
        def fail(method: str, params: dict[str, object]) -> object:
            raise ToolError("guest_error", "nope", {"guest_code": "failed"})

        client = VphoneClient(
            self.config, RecordingRunner(), 3, transport=FakeTransport(rpc=fail)
        )
        with self.assertRaises(ToolError) as raised:
            client.screen()
        self.assertEqual(raised.exception.code, "guest_error")

    def test_install_ipa_parses_installed_bundle(self) -> None:
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=b"uploaded 10 bytes\ninstalled in 1 s: {}\nlaunched dev.vista.client\n",
                stderr=b"",
                duration_ms=1,
            )
        )
        client = VphoneClient(self.config, runner, 3, transport=FakeTransport())
        ipa = self.config.workspace / "App.ipa"
        ipa.write_bytes(b"x")
        result = client.install_ipa(ipa, "dev.vista.client")
        self.assertEqual(result["installed_bundle_id"], "dev.vista.client")
        self.assertEqual(runner.calls[0].argv[0], sys.executable)

    def test_app_logs_parses_json_lines_and_marks_truncation(self) -> None:
        runner = RecordingRunner(
            lambda argv, timeout, env: CommandResult(
                argv=argv,
                returncode=0,
                stdout=b'{"message":"one"}\n{"message":"two"}\n',
                stderr=b"collecting ...\n(truncated: narrow it)\n",
                duration_ms=1,
            )
        )
        client = VphoneClient(self.config, runner, 3, transport=FakeTransport())
        result = client.app_logs("Vista", "dev.vista.client", 5)
        self.assertEqual([entry["message"] for entry in result["entries"]], ["one", "two"])
        self.assertTrue(result["truncated"])

    def test_transport_uses_uds_socket_when_present(self) -> None:
        # The UDS branch is selected by existence; _read_response is covered
        # above. This test keeps the path decision deterministic.
        transport = VphonedTransport(self.config, 3)
        self.assertIsNone(transport._uds_path())
        socket_path = Path(tempfile.mkdtemp()) / "3.sock"
        sock = socket.socket(socket.AF_UNIX)
        sock.bind(str(socket_path))
        self.addCleanup(sock.close)
        # A bound but non-listening socket is enough for is_socket().
        import os

        old = os.environ.get("VSOCK_DIR")
        os.environ["VSOCK_DIR"] = str(socket_path.parent)
        try:
            self.assertEqual(transport._uds_path(), socket_path)
        finally:
            if old is None:
                os.environ.pop("VSOCK_DIR", None)
            else:
                os.environ["VSOCK_DIR"] = old


if __name__ == "__main__":
    unittest.main()
