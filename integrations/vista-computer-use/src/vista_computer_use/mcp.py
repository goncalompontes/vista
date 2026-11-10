"""Minimal MCP JSON-RPC 2.0 stdio server.

MCP's stdio transport is newline-delimited JSON-RPC. The server advertises only
tools, which is enough for Codex, OpenCode, and other MCP clients; tool results
use standard text/image content plus optional structuredContent.
"""

from __future__ import annotations

import json
import sys
import traceback
from typing import Any, BinaryIO

from . import __version__
from .errors import ToolError
from .tools.common import Registry, error_result, jsonable

PROTOCOL_VERSIONS = ("2025-06-18", "2025-03-26", "2024-11-05")
SERVER_NAME = "vista-computer-use"
INSTRUCTIONS = (
    "Use vista_status first. Drive the vphone guest with accessibility-first tools "
    "(vista_ui_tree, vista_ui_tap_element, vista_tap/vista_text), take screenshots for "
    "visual checks, and use vista_host_start/vista_live_client for the Vista host path. "
    "Mutating tools require confirm:true. Screenshots and live media contain private content."
)


class McpServer:
    def __init__(self, registry: Registry) -> None:
        self.registry = registry
        self.initialized = False
        self.client_info: dict[str, Any] | None = None

    def handle_message(self, message: Any) -> dict[str, Any] | None:
        if not isinstance(message, dict):
            return self._error(None, -32600, "invalid request")
        if message.get("jsonrpc") != "2.0":
            return self._error(message.get("id"), -32600, "jsonrpc must be 2.0")
        method = message.get("method")
        if not isinstance(method, str):
            return self._error(message.get("id"), -32600, "method must be a string")
        params = message.get("params", {})
        if not isinstance(params, dict):
            return self._error(message.get("id"), -32602, "params must be an object")
        has_id = "id" in message
        request_id = message.get("id")

        if method == "initialize":
            return self._initialize(request_id, params)
        if method == "notifications/initialized":
            self.initialized = True
            return None
        if method == "notifications/cancelled":
            return None
        if method == "exit":
            return None
        if method == "ping":
            return self._result(request_id, {})
        if method == "tools/list":
            return self._result(request_id, {"tools": self.registry.list()})
        if method == "tools/call":
            return self._tools_call(request_id, params)
        if method == "resources/list":
            return self._result(request_id, {"resources": []})
        if method == "prompts/list":
            return self._result(request_id, {"prompts": []})
        if method == "logging/setLevel":
            return self._result(request_id, {})
        if not has_id:
            return None
        return self._error(request_id, -32601, f"method not found: {method}")

    def _initialize(self, request_id: Any, params: dict[str, Any]) -> dict[str, Any]:
        requested = params.get("protocolVersion")
        protocol = requested if requested in PROTOCOL_VERSIONS else PROTOCOL_VERSIONS[0]
        client_info = params.get("clientInfo")
        self.client_info = client_info if isinstance(client_info, dict) else None
        self.initialized = True
        return self._result(
            request_id,
            {
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {
                    "name": SERVER_NAME,
                    "title": "Vista Computer Use",
                    "version": __version__,
                },
                "instructions": INSTRUCTIONS,
            },
        )

    def _tools_call(self, request_id: Any, params: dict[str, Any]) -> dict[str, Any]:
        name = params.get("name")
        if not isinstance(name, str) or not name:
            return self._error(request_id, -32602, "tools/call requires a tool name")
        arguments = params.get("arguments", {})
        if arguments is None:
            arguments = {}
        if not isinstance(arguments, dict):
            return self._error(request_id, -32602, "tool arguments must be an object")
        try:
            result = self.registry.call(name, arguments)
        except ToolError as error:
            result = error_result(error)
        except Exception as error:  # noqa: BLE001 - convert unexpected failures to a tool result
            traceback.print_exc(file=sys.stderr)
            result = error_result(
                ToolError("internal_error", f"{type(error).__name__}: {error}")
            )
        payload: dict[str, Any] = {
            "content": jsonable(result.content),
            "isError": result.is_error,
        }
        if result.structured is not None:
            payload["structuredContent"] = jsonable(result.structured)
        return self._result(request_id, payload)

    @staticmethod
    def _result(request_id: Any, result: Any) -> dict[str, Any]:
        return {"jsonrpc": "2.0", "id": request_id, "result": result}

    @staticmethod
    def _error(request_id: Any, code: int, message: str) -> dict[str, Any]:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": code, "message": message},
        }

    def serve_stdio(
        self,
        input_stream: BinaryIO | None = None,
        output_stream: BinaryIO | None = None,
    ) -> None:
        input_stream = input_stream or sys.stdin.buffer
        output_stream = output_stream or sys.stdout.buffer
        while True:
            line = input_stream.readline()
            if not line:
                break
            if len(line) > 16 * 1024 * 1024:
                response = self._error(None, -32600, "request too large")
                self._write(output_stream, response)
                continue
            text = line.decode("utf-8", "replace").strip()
            if not text:
                continue
            try:
                message = json.loads(text)
            except json.JSONDecodeError as error:
                response = self._error(None, -32700, f"parse error: {error}")
                self._write(output_stream, response)
                continue
            if isinstance(message, list):
                response = self._error(None, -32600, "JSON-RPC batches are not supported")
                self._write(output_stream, response)
                continue
            response = self.handle_message(message)
            if response is not None:
                self._write(output_stream, response)

    @staticmethod
    def _write(output_stream: BinaryIO, response: dict[str, Any]) -> None:
        data = json.dumps(response, separators=(",", ":"), ensure_ascii=False)
        output_stream.write(data.encode("utf-8") + b"\n")
        output_stream.flush()
