"""Vista host-service and reference-client tools."""

from __future__ import annotations

from typing import Any

from ..errors import ToolError
from ..validation import (
    address_is_loopback,
    bool_value,
    check_keys,
    confirm_value,
    int_value,
    listen_value,
    number_value,
    parse_host_port,
    path_value,
    pointer_value,
    secret_hex_value,
    str_value,
    target_value,
)
from .common import (
    Services,
    Tool,
    confirm_property,
    schema,
    text_result,
)


def host_start(services: Services, args: dict[str, Any]) -> Any:
    allowed = {
        "listen",
        "fps",
        "allow_remote",
        "allow_window_actions",
        "allow_desktop_actions",
        "allow_clipboard",
        "allow_input",
        "once",
        "secret_hex",
        "confirm",
    }
    check_keys(args, allowed)
    listen = listen_value(args)
    fps = int_value(args, "fps", default=15, minimum=1, maximum=30)
    allow_remote = bool_value(args, "allow_remote", default=False)
    if allow_remote and not address_is_loopback(listen):
        confirm_value(args)
    secret = secret_hex_value(args, "secret_hex") if "secret_hex" in args else None
    data = services.host.start(
        listen=listen,
        fps=fps,
        allow_remote=allow_remote,
        allow_window_actions=bool_value(args, "allow_window_actions", default=False),
        allow_desktop_actions=bool_value(args, "allow_desktop_actions", default=False),
        allow_clipboard=bool_value(args, "allow_clipboard", default=False),
        allow_input=bool_value(args, "allow_input", default=False),
        once=bool_value(args, "once", default=False),
        secret_hex=secret,
        start_timeout=services.config.host_start_timeout,
    )
    return text_result(data)


def host_status(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, set())
    data = services.host.status()
    return text_result(data)


def host_stop(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"force", "confirm"})
    confirm_value(args)
    force = bool_value(args, "force", default=False)
    data = services.host.stop(force=force)
    return text_result(data)


def host_windows(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, set())
    windows = services.host.windows()
    return text_result({"windows": windows, "count": len(windows) if isinstance(windows, list) else None})


def _address(value: str) -> str:
    normalized = listen_value({"address": value}, "address")
    _, port = parse_host_port(normalized)
    if port == 0:
        raise ToolError("invalid_arguments", "address must include the actual listening port")
    return normalized


def live_client(services: Services, args: dict[str, Any]) -> Any:
    allowed = {
        "address",
        "secret_hex",
        "target",
        "frames",
        "h264_path",
        "pointer",
        "text",
        "allow_input",
        "timeout_seconds",
        "confirm",
    }
    check_keys(args, allowed)
    confirm_value(args)
    invite = services.host.invite() if "address" not in args or "secret_hex" not in args else None
    address = _address(
        str_value(args, "address", default=invite["address"] if invite else "", min_len=1)
    )
    secret = secret_hex_value(
        args,
        "secret_hex",
        default=invite["secret_hex"] if invite else "",
    )
    target = target_value(args)
    frames = int_value(args, "frames", default=30, minimum=1, maximum=600)
    pointer = pointer_value(args) if "pointer" in args else None
    text_value = str_value(args, "text", default="", allow_empty=True, max_len=4096, multiline=True) or None
    allow_input = bool_value(args, "allow_input", default=False)
    if (text_value or pointer) and not allow_input:
        raise ToolError("invalid_arguments", "pointer/text require allow_input: true")
    h264_path = (
        path_value(
            args,
            "h264_path",
            services.config,
            for_write=True,
            allowed_suffixes=(".h264", ".bin"),
        )
        if "h264_path" in args
        else None
    )
    if h264_path is not None:
        h264_path.parent.mkdir(parents=True, exist_ok=True)
    timeout = number_value(
        args, "timeout_seconds", default=60.0, minimum=5, maximum=300
    )
    report = services.host.run_live_client(
        address=address,
        secret_hex=secret,
        target=target,
        frames=frames,
        h264_path=h264_path,
        pointer=pointer,
        text=text_value,
        allow_input=allow_input,
        timeout=timeout,
    )
    safe = {key: value for key, value in report.items() if key != "secret_hex"}
    safe["address"] = address
    safe["target"] = target
    return text_result(safe)


def register(registry: Any, services: Services) -> None:
    registry.add(
        Tool(
            name="vista_host_start",
            title="Start the Vista host service",
            description=(
                "Start `vista-host serve` in the background and return its pairing invite. "
                "The process stays alive across MCP calls; use vista_host_status and "
                "vista_host_stop. --allow-remote on a non-loopback listener requires confirm:true."
            ),
            input_schema=schema(
                properties={
                    "listen": {
                        "type": "string",
                        "description": "Control listener address (default 127.0.0.1:7443).",
                        "default": "127.0.0.1:7443",
                        "maxLength": 128,
                    },
                    "fps": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 30,
                        "default": 15,
                    },
                    "allow_remote": {"type": "boolean", "default": False},
                    "allow_window_actions": {"type": "boolean", "default": False},
                    "allow_desktop_actions": {"type": "boolean", "default": False},
                    "allow_clipboard": {"type": "boolean", "default": False},
                    "allow_input": {
                        "type": "boolean",
                        "description": "Enable ydotool input injection when /dev/uinput is available.",
                        "default": False,
                    },
                    "once": {
                        "type": "boolean",
                        "description": "Exit after one client session.",
                        "default": False,
                    },
                    "secret_hex": {
                        "type": "string",
                        "description": "Optional fixed 32-byte pairing secret as hex; generated if omitted.",
                        "minLength": 64,
                        "maxLength": 64,
                    },
                    "confirm": confirm_property("Required only with allow_remote on a non-loopback address."),
                },
            ),
            handler=lambda args: host_start(services, args),
            destructive=True,
        )
    )
    registry.add(
        Tool(
            name="vista_host_status",
            title="Vista host-service status",
            description=(
                "Return the managed host-service pid, listener, invite, and whether it is "
                "running/listening. The invite contains the local pairing secret."
            ),
            input_schema=schema(),
            handler=lambda args: host_status(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_host_stop",
            title="Stop the Vista host service",
            description=(
                "SIGTERM the host service started by vista_host_start. Requires confirm:true. "
                "force:true escalates to SIGKILL after the graceful wait."
            ),
            input_schema=schema(
                properties={
                    "force": {"type": "boolean", "default": False},
                    "confirm": confirm_property(),
                },
                required=["confirm"],
            ),
            handler=lambda args: host_stop(services, args),
            destructive=True,
        )
    )
    registry.add(
        Tool(
            name="vista_windows",
            title="List Niri windows",
            description="Read the host's Niri window catalog to choose a window:ID stream target.",
            input_schema=schema(),
            handler=lambda args: host_windows(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_live_client",
            title="Run the Vista reference client",
            description=(
                "Exercise the host service end to end with `vista-host live-client`: pairing, "
                "stream selection, encrypted H.264 media, and optional input. Requires confirm:true "
                "because a window target is fullscreened and desktop pixels are captured. "
                "Omit address/secret_hex to use the running managed service's invite."
            ),
            input_schema=schema(
                properties={
                    "address": {"type": "string", "maxLength": 128},
                    "secret_hex": {"type": "string", "minLength": 64, "maxLength": 64},
                    "target": {
                        "type": "string",
                        "description": "desktop, window:ID, or workspace:ID",
                        "default": "desktop",
                    },
                    "frames": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 600,
                        "default": 30,
                    },
                    "h264_path": {
                        "type": "string",
                        "description": "Optional .h264 output path inside an allowed write root.",
                        "maxLength": 4096,
                    },
                    "pointer": {
                        "type": "string",
                        "description": "Optional normalized X:Y pointer event (requires allow_input).",
                    },
                    "text": {
                        "type": "string",
                        "description": "Optional text commit (requires allow_input).",
                        "maxLength": 4096,
                    },
                    "allow_input": {"type": "boolean", "default": False},
                    "timeout_seconds": {
                        "type": "number",
                        "minimum": 5,
                        "maximum": 300,
                        "default": 60,
                    },
                    "confirm": confirm_property(),
                },
                required=["confirm"],
            ),
            handler=lambda args: live_client(services, args),
            destructive=True,
        )
    )
