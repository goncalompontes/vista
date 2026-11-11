from __future__ import annotations

import io
import json
import tempfile
import unittest
from pathlib import Path

from support import make_services

from vista_computer_use.mcp import McpServer, PROTOCOL_VERSIONS
from vista_computer_use.tools import build_registry


class McpProtocolTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.server = McpServer(build_registry(make_services(Path(self.tmp.name))))

    def test_initialize_negotiates_supported_version(self) -> None:
        response = self.server.handle_message(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "1"},
                },
            }
        )
        self.assertEqual(response["result"]["protocolVersion"], "2024-11-05")
        self.assertEqual(response["result"]["serverInfo"]["name"], "vista-computer-use")
        self.assertIn("tools", response["result"]["capabilities"])

    def test_unknown_initialize_version_falls_back(self) -> None:
        response = self.server.handle_message(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "1999-01-01"},
            }
        )
        self.assertEqual(response["result"]["protocolVersion"], PROTOCOL_VERSIONS[0])

    def test_tools_list_is_deterministic(self) -> None:
        response = self.server.handle_message(
            {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}
        )
        tools = response["result"]["tools"]
        names = [tool["name"] for tool in tools]
        self.assertEqual(len(names), 32)
        self.assertEqual(names[0], "vista_status")
        self.assertIn("vista_screenshot", names)
        for tool in tools:
            self.assertEqual(tool["inputSchema"]["type"], "object")
            self.assertFalse(tool["inputSchema"]["additionalProperties"])

    def test_unknown_tool_is_a_tool_error(self) -> None:
        response = self.server.handle_message(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "nope", "arguments": {}},
            }
        )
        self.assertTrue(response["result"]["isError"])
        self.assertEqual(
            response["result"]["structuredContent"]["error"]["code"], "unknown_tool"
        )

    def test_invalid_arguments_are_a_tool_error(self) -> None:
        response = self.server.handle_message(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "vista_tap", "arguments": {"x": 1}},
            }
        )
        self.assertTrue(response["result"]["isError"])
        self.assertEqual(
            response["result"]["structuredContent"]["error"]["code"], "invalid_arguments"
        )

    def test_notification_has_no_response(self) -> None:
        self.assertIsNone(
            self.server.handle_message(
                {"jsonrpc": "2.0", "method": "notifications/initialized"}
            )
        )

    def test_ping_and_empty_resource_lists(self) -> None:
        self.assertEqual(
            self.server.handle_message({"jsonrpc": "2.0", "id": 2, "method": "ping"})[
                "result"
            ],
            {},
        )
        self.assertEqual(
            self.server.handle_message(
                {"jsonrpc": "2.0", "id": 3, "method": "resources/list"}
            )["result"],
            {"resources": []},
        )

    def test_stdio_round_trip(self) -> None:
        stdin = io.BytesIO(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {"protocolVersion": PROTOCOL_VERSIONS[0]},
                }
            ).encode()
            + b"\n"
            + json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).encode()
            + b"\n"
        )
        stdout = io.BytesIO()
        self.server.serve_stdio(stdin, stdout)
        lines = [json.loads(line) for line in stdout.getvalue().splitlines()]
        self.assertEqual(lines[0]["result"]["protocolVersion"], PROTOCOL_VERSIONS[0])
        self.assertEqual(len(lines[1]["result"]["tools"]), 32)

    def test_malformed_json_gets_parse_error(self) -> None:
        stdin = io.BytesIO(b"{not json}\n")
        stdout = io.BytesIO()
        self.server.serve_stdio(stdin, stdout)
        response = json.loads(stdout.getvalue())
        self.assertEqual(response["error"]["code"], -32700)


if __name__ == "__main__":
    unittest.main()
