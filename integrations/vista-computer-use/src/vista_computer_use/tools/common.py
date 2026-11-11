"""Shared tool types and result helpers."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, Callable, Mapping

from ..config import Config
from ..errors import ToolError
from ..host import HostServiceManager
from ..qmp import QmpClient
from ..vphone import ScreenCache, VphoneClient


@dataclass(frozen=True)
class ToolResult:
    content: list[dict[str, Any]]
    structured: dict[str, Any] | None = None
    is_error: bool = False


@dataclass
class Services:
    config: Config
    vphone: VphoneClient
    qmp: QmpClient
    host: HostServiceManager
    screen: ScreenCache
    vm: Any = None
    vphone_factory: Callable[[int], VphoneClient] | None = None

    def client_for(self, cid: int) -> VphoneClient:
        if cid == self.vphone.cid:
            return self.vphone
        if self.vphone_factory is not None:
            return self.vphone_factory(cid)
        return self.vphone


Handler = Callable[[dict[str, Any]], ToolResult]


@dataclass(frozen=True)
class Tool:
    name: str
    title: str
    description: str
    input_schema: dict[str, Any]
    handler: Handler
    read_only: bool = False
    destructive: bool = False
    idempotent: bool = False
    open_world: bool = False

    def descriptor(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.input_schema,
            "annotations": {
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.idempotent,
                "openWorldHint": self.open_world,
            },
        }


class Registry:
    def __init__(self) -> None:
        self._tools: dict[str, Tool] = {}

    def add(self, tool: Tool) -> None:
        if tool.name in self._tools:
            raise ValueError(f"duplicate tool {tool.name}")
        self._tools[tool.name] = tool

    def list(self) -> list[dict[str, Any]]:
        return [tool.descriptor() for tool in self._tools.values()]

    def call(self, name: str, arguments: Mapping[str, Any] | None) -> ToolResult:
        tool = self._tools.get(name)
        if tool is None:
            raise ToolError("unknown_tool", f"unknown tool: {name}", {"tool": name})
        return tool.handler(dict(arguments or {}))


def jsonable(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): jsonable(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [jsonable(item) for item in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def json_text(value: Any, limit: int = 14_000) -> str:
    text = json.dumps(jsonable(value), indent=2, sort_keys=True, ensure_ascii=False)
    if len(text) > limit:
        return text[:limit] + f"\n… truncated ({len(text) - limit} characters)"
    return text


def text_result(
    data: Any,
    text: str | None = None,
    *,
    structured: dict[str, Any] | None = None,
) -> ToolResult:
    payload = structured if structured is not None else (data if isinstance(data, dict) else {"result": data})
    return ToolResult(
        content=[{"type": "text", "text": text or json_text(data)}],
        structured=payload,
    )


def image_result(
    *,
    data_b64: str,
    mime_type: str,
    metadata: dict[str, Any],
    include_image: bool,
    text: str | None = None,
) -> ToolResult:
    content: list[dict[str, Any]] = []
    if include_image:
        content.append({"type": "image", "data": data_b64, "mimeType": mime_type})
    content.append({"type": "text", "text": text or json_text(metadata)})
    return ToolResult(content=content, structured=metadata)


def error_result(error: ToolError) -> ToolResult:
    return ToolResult(
        content=[
            {
                "type": "text",
                "text": f"{error.code}: {error.message}",
            }
        ],
        structured={"error": error.as_dict()},
        is_error=True,
    )


def schema(
    *,
    properties: dict[str, Any] | None = None,
    required: list[str] | None = None,
    description: str | None = None,
) -> dict[str, Any]:
    value: dict[str, Any] = {
        "type": "object",
        "properties": properties or {},
        "additionalProperties": False,
    }
    if required:
        value["required"] = required
    if description:
        value["description"] = description
    return value


def confirm_property(description: str = "Must be true to confirm this mutating action.") -> dict[str, Any]:
    return {"type": "boolean", "description": description, "const": True}
