"""Guest network inspection and QMP host-port forwarding."""

from __future__ import annotations

from typing import Any

from ..errors import ToolError
from ..validation import (
    bool_value,
    check_keys,
    confirm_value,
    domain_value,
    enum_value,
    int_value,
    netdev_value,
)
from .common import Services, Tool, confirm_property, schema, text_result


def network_info(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, set())
    data: dict[str, Any] = {}
    errors: dict[str, Any] = {}
    for key, method in (
        ("network", services.vphone.network),
        ("ipv4", services.vphone.network_ipv4),
        ("hostname", services.vphone.network_hostname),
    ):
        try:
            data[key] = method()
        except ToolError as error:
            errors[key] = error.as_dict()
    if errors:
        data["errors"] = errors
    try:
        data["guest_ip"] = services.vphone.health(timeout=5).get("ip")
    except ToolError:
        pass
    return text_result(data)


def network_resolve(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"host", "family", "port", "first_only", "timeout_ms"})
    host = domain_value(args)
    family = enum_value(args, "family", ("any", "ipv4", "ipv6"), default="any")
    port = int_value(args, "port", default=None, minimum=1, maximum=65535) if "port" in args else None
    first_only = bool_value(args, "first_only", default=False) if "first_only" in args else False
    timeout_ms = int_value(args, "timeout_ms", default=3000, minimum=100, maximum=10000)
    data = services.vphone.network_resolve(host, family, port, first_only, timeout_ms)
    return text_result(data)


def network_forward(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"action", "host_port", "guest_port", "netdev", "confirm"})
    confirm_value(args)
    action = enum_value(args, "action", ("add", "remove"))
    host_port = int_value(args, "host_port", minimum=1024, maximum=65535)
    netdev = netdev_value(args) if "netdev" in args else "net0"
    if action == "remove":
        data = services.qmp.forward_remove(netdev, host_port)
    else:
        guest_port = int_value(args, "guest_port", minimum=1, maximum=65535)
        data = services.qmp.forward_add(netdev, host_port, guest_port)
    data["note"] = (
        "The forward binds 127.0.0.1 only. QEMU user-networking must be the "
        "VM's network backend; this is unavailable on VMs booted without QMP."
    )
    return text_result(data)


def register(registry: Any, services: Services) -> None:
    registry.add(
        Tool(
            name="vista_network_info",
            title="Guest network information",
            description=(
                "Read device.network, the guest's en0 IPv4 configuration, and local hostname. "
                "Useful for connecting the emulator to a host service."
            ),
            input_schema=schema(),
            handler=lambda args: network_info(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_network_resolve",
            title="Resolve a hostname in the guest",
            description="Resolve a hostname from inside the emulated guest through vphoned.",
            input_schema=schema(
                properties={
                    "host": {"type": "string", "maxLength": 253},
                    "family": {
                        "type": "string",
                        "enum": ["any", "ipv4", "ipv6"],
                        "default": "any",
                    },
                    "port": {"type": "integer", "minimum": 1, "maximum": 65535},
                    "first_only": {"type": "boolean", "default": False},
                    "timeout_ms": {
                        "type": "integer",
                        "minimum": 100,
                        "maximum": 10000,
                        "default": 3000,
                    },
                },
                required=["host"],
            ),
            handler=lambda args: network_resolve(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_network_forward",
            title="Forward a guest TCP port",
            description=(
                "Add or remove a QEMU user-networking host-to-guest TCP forward on "
                "127.0.0.1, through QMP's hostfwd_add/hostfwd_remove. Requires a QMP "
                "socket and confirm:true. Prefer ports 1024-65535."
            ),
            input_schema=schema(
                properties={
                    "action": {"type": "string", "enum": ["add", "remove"]},
                    "host_port": {
                        "type": "integer",
                        "minimum": 1024,
                        "maximum": 65535,
                    },
                    "guest_port": {
                        "type": "integer",
                        "description": "Required for add.",
                        "minimum": 1,
                        "maximum": 65535,
                    },
                    "netdev": {
                        "type": "string",
                        "description": "QEMU netdev id, normally net0.",
                        "default": "net0",
                    },
                    "confirm": confirm_property(),
                },
                required=["action", "host_port", "confirm"],
            ),
            handler=lambda args: network_forward(services, args),
            destructive=True,
        )
    )
