"""Screen, accessibility, and input tools."""

from __future__ import annotations

import base64
import binascii
import struct
import time
from pathlib import Path
from typing import Any

from ..errors import ToolError
from ..validation import (
    bool_value,
    check_keys,
    confirm_value,
    enum_value,
    int_value,
    number_value,
    output_path,
    resolve_point,
    resolve_point_pair,
    selector_params,
    str_value,
)
from .common import (
    Services,
    Tool,
    confirm_property,
    image_result,
    schema,
    text_result,
)

_BUTTONS = ("home", "lock", "power", "volume-up", "volume-down")


def _screen(services: Services) -> dict[str, Any] | None:
    try:
        return services.screen.get()
    except ToolError:
        return None


def _png_dimensions(data: bytes) -> tuple[int, int] | None:
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    width, height = struct.unpack(">II", data[16:24])
    return int(width), int(height)


def _saved_name(prefix: str, suffix: str) -> str:
    stamp = time.strftime("%Y%m%d-%H%M%S")
    return f"{prefix}-{stamp}{suffix}"


def _write_screenshot(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    try:
        path.chmod(0o600)
    except OSError:
        pass


def _summarize_tree(tree: dict[str, Any]) -> str:
    status = tree.get("status")
    count = tree.get("count")
    elements = tree.get("elements")
    if not isinstance(elements, list):
        elements = tree.get("nodes")
    lines: list[str] = []
    if isinstance(count, int):
        lines.append(f"{count} UI element(s); status={status or 'unknown'}")
    if isinstance(elements, list):
        for index, element in enumerate(elements[:60]):
            if not isinstance(element, dict):
                lines.append(f"{index}: {element!r}")
                continue
            label = element.get("label") or element.get("text") or element.get("value") or ""
            role = element.get("role") or element.get("type") or ""
            identifier = element.get("identifier") or element.get("id") or ""
            frame = element.get("frame")
            clickable = element.get("clickable")
            parts = [f"{index}: {label!r}"]
            if role:
                parts.append(f"role={role}")
            if identifier:
                parts.append(f"id={identifier}")
            if frame is not None:
                parts.append(f"frame={frame}")
            if clickable is not None:
                parts.append(f"clickable={clickable}")
            lines.append(" ".join(parts))
        if len(elements) > 60:
            lines.append(f"… {len(elements) - 60} more element(s)")
    else:
        roots = tree.get("roots")
        if isinstance(roots, list):
            lines.append(f"nested accessibility tree with {len(roots)} root(s)")
        else:
            lines.append("accessibility tree returned without a recognized element list")
    for key in ("truncated", "truncation_reasons", "action_eligible", "format", "source"):
        if key in tree:
            lines.append(f"{key}: {tree[key]}")
    return "\n".join(lines)


def screenshot(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"source", "path", "include_image"})
    source = enum_value(args, "source", ("auto", "guest", "qmp"), default="auto")
    include_image = bool_value(args, "include_image", default=True)
    raw_override = args.get("path")
    if raw_override is not None and not isinstance(raw_override, str):
        raise ToolError("invalid_arguments", "path must be a string")

    chosen = source
    data: bytes | None = None
    metadata: dict[str, Any] = {}
    final_path: Path | None = None
    if chosen in {"auto", "guest"}:
        try:
            shot = services.vphone.screenshot()
            encoded = shot.get("data")
            if not isinstance(encoded, str):
                raise ToolError("invalid_response", "screen.screenshot returned no data")
            try:
                data = base64.b64decode(encoded, validate=True)
            except (binascii.Error, ValueError) as error:
                raise ToolError("invalid_response", "screenshot data was not valid base64") from error
            chosen = "guest"
            metadata.update(
                {
                    "source": "guest",
                    "mime_type": shot.get("mime_type") or "image/jpeg",
                    "width": shot.get("width"),
                    "height": shot.get("height"),
                    "coordinate_scale": shot.get("coordinate_scale"),
                }
            )
        except ToolError:
            if source == "guest":
                raise
            chosen = "qmp"
    if data is None:
        chosen = "qmp"
        socket_path = services.qmp.require_socket()
        path = output_path(
            {"path": raw_override} if raw_override else {},
            services.config,
            default_dir=services.config.screenshot_dir,
            default_name=_saved_name("vista-qmp", ".png"),
            allowed_suffixes=[".png"],
        )
        services.qmp.screenshot(path)
        final_path = path
        data = path.read_bytes()
        dimensions = _png_dimensions(data)
        metadata.update(
            {
                "source": "qmp",
                "mime_type": "image/png",
                "width": dimensions[0] if dimensions else None,
                "height": dimensions[1] if dimensions else None,
                "coordinate_scale": 1,
                "qmp_socket": str(socket_path),
            }
        )

    if data is None:
        raise ToolError("unavailable", "no screenshot source produced data")
    suffix = ".png" if str(metadata.get("mime_type")) == "image/png" else ".jpg"
    path = final_path or output_path(
        {"path": raw_override} if raw_override else {},
        services.config,
        default_dir=services.config.screenshot_dir,
        default_name=_saved_name("vista", suffix),
        allowed_suffixes=[suffix],
    )
    _write_screenshot(path, data)
    metadata.update(
        {
            "saved_to": str(path),
            "bytes": len(data),
            "image_included": include_image and len(data) <= services.config.max_image_bytes,
        }
    )
    if include_image and len(data) > services.config.max_image_bytes:
        metadata["image_omitted_reason"] = (
            f"screenshot is {len(data)} bytes, above the "
            f"{services.config.max_image_bytes} byte image-content limit"
        )
    payload = base64.b64encode(data).decode("ascii")
    return image_result(
        data_b64=payload,
        mime_type=str(metadata.get("mime_type") or "image/jpeg"),
        metadata=metadata,
        include_image=bool(metadata["image_included"]),
        text=(
            f"{chosen} screenshot {metadata.get('width')}x{metadata.get('height')} "
            f"saved to {path} ({len(data)} bytes)"
        ),
    )


def screen_info(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, set())
    data: dict[str, Any] = {}
    try:
        screen = services.vphone.screen()
        data["screen"] = screen
        data["coordinate_systems"] = {
            "points": {"width": screen.get("width"), "height": screen.get("height")},
            "pixels_per_point": screen.get("scale", 1),
            "input_default": "points",
        }
    except ToolError as error:
        data["screen"] = {"error": error.as_dict()}
    try:
        data["orientation"] = services.vphone.rpc("display.orientation", timeout=8)
    except ToolError as error:
        data["orientation"] = {"error": error.as_dict()}
    data["qmp"] = services.qmp.status()
    return text_result(data)


def unlock(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"passcode", "timeout_seconds", "confirm"})
    confirm_value(args)
    passcode: str | None = None
    if "passcode" in args:
        passcode = str_value(args, "passcode", min_len=1, max_len=64)
    timeout_seconds = number_value(
        args, "timeout_seconds", default=30.0, minimum=1, maximum=120
    )
    data = services.vphone.unlock(passcode, timeout_seconds)
    services.screen.invalidate()
    return text_result(data)


def ui_tree(services: Services, args: dict[str, Any]) -> Any:
    allowed = {
        "nested",
        "max_elements",
        "max_depth",
        "timeout_ms",
        "visible_only",
        "clickable_only",
        "limit",
    }
    check_keys(args, allowed)
    nested = bool_value(args, "nested", default=False)
    max_elements = int_value(
        args, "max_elements", default=200, minimum=1, maximum=2000
    )
    params: dict[str, Any] = {"max_elements": max_elements}
    if nested:
        forbidden = [key for key in ("visible_only", "clickable_only", "limit") if key in args]
        if forbidden:
            raise ToolError(
                "invalid_arguments",
                "nested trees do not accept visible_only, clickable_only, or limit",
                {"forbidden": forbidden},
            )
        params["nested"] = True
        params["max_depth"] = int_value(args, "max_depth", default=32, minimum=1, maximum=32)
        params["timeout_ms"] = int_value(
            args, "timeout_ms", default=5000, minimum=100, maximum=10000
        )
    else:
        if "visible_only" in args:
            params["visible_only"] = bool_value(args, "visible_only")
        if "clickable_only" in args:
            params["clickable_only"] = bool_value(args, "clickable_only")
        if "limit" in args:
            params["limit"] = int_value(args, "limit", minimum=1, maximum=2000)
    tree = services.vphone.ui_tree(params)
    summary = _summarize_tree(tree)
    return text_result(
        tree,
        summary,
        structured={"tree": tree, "summary": summary},
    )


def ui_element_at(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"x", "y", "space"})
    x, y = resolve_point(args, _screen(services))
    data = services.vphone.ui_element_at(x, y)
    return text_result({"x": x, "y": y, "element": data})


def ui_tap_element(services: Services, args: dict[str, Any]) -> Any:
    selector = selector_params(args)
    data = services.vphone.ui_tap_element(selector)
    return text_result({"selector": selector, "result": data})


def ui_wait(services: Services, args: dict[str, Any]) -> Any:
    allowed = {"text", "identifier", "role", "match", "index", "appear", "timeout"}
    check_keys(args, allowed)
    selector_args = {key: args[key] for key in ("text", "identifier", "role", "match", "index") if key in args}
    selector = selector_params(selector_args)
    appear = bool_value(args, "appear", default=True)
    timeout = number_value(args, "timeout", default=10.0, minimum=0.1, maximum=60)
    data = services.vphone.ui_wait(selector, appear, timeout)
    return text_result(
        {
            "selector": selector,
            "appear": appear,
            "timeout_seconds": timeout,
            "result": data,
        }
    )


def tap(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"x", "y", "space", "backend"})
    backend = enum_value(args, "backend", ("guest", "qmp"), default="guest")
    screen = _screen(services)
    x, y = resolve_point(args, screen)
    if backend == "qmp":
        if screen is None:
            raise ToolError("qmp_unavailable", "QMP input needs the guest screen geometry")
        width, height, _ = _geometry(screen)
        result = services.qmp.tap(x, y, width, height)
    else:
        result = services.vphone.tap(x, y)
    return text_result({"backend": backend, "x": x, "y": y, "result": result})


def _geometry(screen: dict[str, Any]) -> tuple[int, int, float]:
    width = screen.get("width")
    height = screen.get("height")
    scale = screen.get("scale", 1.0)
    if not isinstance(width, int) or width <= 0 or not isinstance(height, int) or height <= 0:
        raise ToolError("invalid_response", "guest screen geometry is unavailable")
    return width, height, float(scale) if isinstance(scale, (int, float)) else 1.0


def swipe(services: Services, args: dict[str, Any]) -> Any:
    allowed = {
        "x1",
        "y1",
        "x2",
        "y2",
        "space",
        "duration_ms",
        "steps",
        "backend",
    }
    check_keys(args, allowed)
    backend = enum_value(args, "backend", ("guest", "qmp"), default="guest")
    screen = _screen(services)
    (x1, y1), (x2, y2) = resolve_point_pair(args, screen, "x1", "y1", "x2", "y2")
    duration_ms = int_value(args, "duration_ms", default=300, minimum=50, maximum=5000)
    steps = int_value(args, "steps", default=20, minimum=2, maximum=200)
    if backend == "qmp":
        if screen is None:
            raise ToolError("qmp_unavailable", "QMP input needs the guest screen geometry")
        width, height, _ = _geometry(screen)
        result = services.qmp.swipe(
            x1, y1, x2, y2, width, height, duration_ms, steps
        )
    else:
        result = services.vphone.swipe(
            x1, y1, x2, y2, duration_ms / 1000.0, steps
        )
    return text_result(
        {
            "backend": backend,
            "from": [x1, y1],
            "to": [x2, y2],
            "duration_ms": duration_ms,
            "steps": steps,
            "result": result,
        }
    )


def key(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name", "backend"})
    name = str_value(args, "name", min_len=1, max_len=64)
    if any(char.isspace() or ord(char) < 0x20 for char in name):
        raise ToolError("invalid_arguments", "name must not contain whitespace or controls")
    backend = enum_value(args, "backend", ("guest", "qmp"), default="guest")
    result = services.qmp.key(name) if backend == "qmp" else services.vphone.key(name)
    return text_result({"backend": backend, "name": name, "result": result})


def text(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"text", "delay_ms"})
    value = str_value(args, "text", min_len=1, max_len=10_000, multiline=True)
    delay_ms = int_value(args, "delay_ms", default=30, minimum=0, maximum=1000)
    result = services.vphone.type_text(value, delay_ms)
    return text_result(
        {
            "characters": len(value),
            "delay_ms": delay_ms,
            "result": result,
        }
    )


def button(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name"})
    name = enum_value(args, "name", _BUTTONS)
    result = services.vphone.button(name)
    return text_result({"name": name, "result": result})


def register(registry: Any, services: Services) -> None:
    registry.add(
        Tool(
            name="vista_screenshot",
            title="Screenshot the guest",
            description=(
                "Capture the emulated iPhone screen. Guest screenshots use vphoned's native "
                "capture and return a JPEG; QMP is a fallback that returns PNG. The file is saved "
                "under runs/computer-use/screenshots by default and returned as image content."
            ),
            input_schema=schema(
                properties={
                    "source": {
                        "type": "string",
                        "enum": ["auto", "guest", "qmp"],
                        "description": "Capture source; auto tries vphoned first.",
                        "default": "auto",
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional output path inside an allowed write root.",
                        "maxLength": 4096,
                    },
                    "include_image": {
                        "type": "boolean",
                        "description": "Return MCP image content in addition to the saved path.",
                        "default": True,
                    },
                },
            ),
            handler=lambda args: screenshot(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_screen_info",
            title="Guest screen and orientation",
            description=(
                "Read device.screen and display.orientation. Coordinates used by input and UI "
                "tools are screen points; pixel observations must be divided by scale."
            ),
            input_schema=schema(),
            handler=lambda args: screen_info(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_unlock",
            title="Unlock the guest screen",
            description=(
                "Wake and unlock the guest through vphoned's screen.unlock. Requires confirm:true. "
                "Pass a passcode only when the VM is configured with one."
            ),
            input_schema=schema(
                properties={
                    "passcode": {
                        "type": "string",
                        "description": "Optional guest passcode; never returned in results.",
                        "maxLength": 64,
                    },
                    "timeout_seconds": {
                        "type": "number",
                        "description": "Maximum unlock wait (1-120 seconds).",
                        "minimum": 1,
                        "maximum": 120,
                        "default": 30,
                    },
                    "confirm": confirm_property(),
                },
                required=["confirm"],
            ),
            handler=lambda args: unlock(services, args),
            destructive=True,
        )
    )
    registry.add(
        Tool(
            name="vista_ui_tree",
            title="Inspect the accessibility tree",
            description=(
                "Return the foreground app's accessibility elements. Flat mode supports "
                "visibility/clickability filters and a limit; nested mode preserves structure "
                "and rejects those filters. The full tree is in structuredContent."
            ),
            input_schema=schema(
                properties={
                    "nested": {"type": "boolean", "default": False},
                    "max_elements": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 2000,
                        "default": 200,
                    },
                    "max_depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 32,
                        "description": "Nested mode only.",
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "minimum": 100,
                        "maximum": 10000,
                        "description": "Nested mode only.",
                    },
                    "visible_only": {
                        "type": "boolean",
                        "description": "Flat mode only.",
                    },
                    "clickable_only": {
                        "type": "boolean",
                        "description": "Flat mode only.",
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 2000,
                        "description": "Flat mode only.",
                    },
                },
            ),
            handler=lambda args: ui_tree(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_ui_element_at",
            title="Element at a point",
            description="Return the accessibility element at a guest point or pixel coordinate.",
            input_schema=schema(
                properties={
                    "x": {"type": "number", "minimum": 0},
                    "y": {"type": "number", "minimum": 0},
                    "space": {
                        "type": "string",
                        "enum": ["points", "pixels"],
                        "default": "points",
                    },
                },
                required=["x", "y"],
            ),
            handler=lambda args: ui_element_at(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_ui_tap_element",
            title="Tap an accessibility element",
            description=(
                "Tap an element by text, identifier, or role using vphoned's selector. "
                "At least one selector field is required."
            ),
            input_schema=schema(
                properties={
                    "text": {"type": "string", "maxLength": 512},
                    "identifier": {"type": "string", "maxLength": 512},
                    "role": {"type": "string", "maxLength": 64},
                    "match": {
                        "type": "string",
                        "enum": ["contains", "exact"],
                        "default": "contains",
                    },
                    "index": {"type": "integer", "minimum": 0, "maximum": 1000},
                },
            ),
            handler=lambda args: ui_tap_element(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_ui_wait",
            title="Wait for an element",
            description="Wait until a selector appears or disappears, using vphoned's ui.wait.",
            input_schema=schema(
                properties={
                    "text": {"type": "string", "maxLength": 512},
                    "identifier": {"type": "string", "maxLength": 512},
                    "role": {"type": "string", "maxLength": 64},
                    "match": {
                        "type": "string",
                        "enum": ["contains", "exact"],
                        "default": "contains",
                    },
                    "index": {"type": "integer", "minimum": 0, "maximum": 1000},
                    "appear": {"type": "boolean", "default": True},
                    "timeout": {
                        "type": "number",
                        "minimum": 0.1,
                        "maximum": 60,
                        "default": 10,
                    },
                },
            ),
            handler=lambda args: ui_wait(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_tap",
            title="Tap the guest screen",
            description=(
                "Tap at guest screen coordinates. The default backend is vphoned; qmp is a "
                "fallback that reuses tools/qmp.py with the guest geometry."
            ),
            input_schema=schema(
                properties={
                    "x": {"type": "number", "minimum": 0},
                    "y": {"type": "number", "minimum": 0},
                    "space": {
                        "type": "string",
                        "enum": ["points", "pixels"],
                        "default": "points",
                    },
                    "backend": {
                        "type": "string",
                        "enum": ["guest", "qmp"],
                        "default": "guest",
                    },
                },
                required=["x", "y"],
            ),
            handler=lambda args: tap(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_swipe",
            title="Swipe the guest screen",
            description="Swipe between two guest coordinates with a bounded duration and step count.",
            input_schema=schema(
                properties={
                    "x1": {"type": "number", "minimum": 0},
                    "y1": {"type": "number", "minimum": 0},
                    "x2": {"type": "number", "minimum": 0},
                    "y2": {"type": "number", "minimum": 0},
                    "space": {
                        "type": "string",
                        "enum": ["points", "pixels"],
                        "default": "points",
                    },
                    "duration_ms": {
                        "type": "integer",
                        "minimum": 50,
                        "maximum": 5000,
                        "default": 300,
                    },
                    "steps": {
                        "type": "integer",
                        "minimum": 2,
                        "maximum": 200,
                        "default": 20,
                    },
                    "backend": {
                        "type": "string",
                        "enum": ["guest", "qmp"],
                        "default": "guest",
                    },
                },
                required=["x1", "y1", "x2", "y2"],
            ),
            handler=lambda args: swipe(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_key",
            title="Press a key",
            description=(
                "Press one keyboard key by name. The guest backend passes the name to vphoned; "
                "the QMP backend accepts common names and single letters/digits."
            ),
            input_schema=schema(
                properties={
                    "name": {"type": "string", "minLength": 1, "maxLength": 64},
                    "backend": {
                        "type": "string",
                        "enum": ["guest", "qmp"],
                        "default": "guest",
                    },
                },
                required=["name"],
            ),
            handler=lambda args: key(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_text",
            title="Type text",
            description=(
                "Type text into the focused field through vphoned's input.type. This is "
                "character input, not a shortcut or shell command."
            ),
            input_schema=schema(
                properties={
                    "text": {"type": "string", "minLength": 1, "maxLength": 10000},
                    "delay_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 1000,
                        "default": 30,
                    },
                },
                required=["text"],
            ),
            handler=lambda args: text(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_button",
            title="Press a hardware button",
            description="Press an iPhone hardware button through vphoned's input.button.",
            input_schema=schema(
                properties={
                    "name": {"type": "string", "enum": list(_BUTTONS)},
                },
                required=["name"],
            ),
            handler=lambda args: button(services, args),
        )
    )
