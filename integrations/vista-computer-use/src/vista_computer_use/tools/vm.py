"""VM status and lifecycle tools."""

from __future__ import annotations

from typing import Any

from ..errors import ToolError
from ..validation import (
    bool_value,
    check_keys,
    confirm_value,
    int_value,
    number_value,
    str_value,
)
from .common import (
    Services,
    Tool,
    confirm_property,
    schema,
    text_result,
)

_VM_NAME_SCHEMA = {
    "type": "string",
    "description": "VM directory name under the vphone devices directory.",
    "minLength": 1,
    "maxLength": 64,
}


def _redact_host(status: dict[str, Any]) -> dict[str, Any]:
    copy = dict(status)
    invite = copy.get("invite")
    if isinstance(invite, dict):
        copy["invite"] = {**invite, "secret_hex": "<redacted>"}
    state = copy.get("state")
    if isinstance(state, dict):
        state_copy = dict(state)
        state_invite = state_copy.get("invite")
        if isinstance(state_invite, dict):
            state_copy["invite"] = {**state_invite, "secret_hex": "<redacted>"}
        copy["state"] = state_copy
    return copy


def status(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, set())
    vms = services.vm.list()
    active: dict[str, Any] | None = None
    try:
        vm = services.vm.resolve(None)
        active = vm.as_dict()
        cid = vm.cid
    except ToolError as error:
        active = {"error": error.as_dict()}
        cid = services.config.vphone_cid or 3

    health: dict[str, Any]
    screen: dict[str, Any] | None = None
    health_ok = False
    client = services.client_for(cid)
    try:
        health = client.health(timeout=5)
        health_ok = health.get("status") == "ok"
        try:
            screen = client.screen()
        except ToolError as error:
            screen = {"error": error.as_dict()}
    except ToolError as error:
        health = {"error": error.as_dict()}

    qmp = services.qmp.status()
    host = _redact_host(services.host.status())
    try:
        host_binary = services.host.host_binary().is_file()
    except ToolError:
        host_binary = False
    paths = {
        "vphoned": services.config.vphoned_script.is_file(),
        "qmp": services.config.qmp_script.is_file(),
        "install_ipa": services.config.install_ipa_script.is_file(),
        "app_logs": services.config.app_logs_script.is_file(),
        "start_vm": services.config.start_vm_script.is_file(),
        "vista_host": host_binary,
    }
    data = {
        "ready": health_ok,
        "config": services.config.public(),
        "cid": cid,
        "binaries": paths,
        "vms": [vm.as_dict() for vm in vms],
        "active_vm": active,
        "guest": {
            "health": health,
            "screen": screen,
        },
        "qmp": qmp,
        "host_service": host,
    }
    summary = [
        f"Vista computer-use: {'ready' if health_ok else 'not ready'}",
        f"CID: {cid}",
        f"VM: {active.get('name') if active and 'name' in active else 'unknown'}",
        f"vphoned: {'ok' if health_ok else 'unavailable'}",
        f"QMP: {'available' if qmp.get('available') else 'unavailable'}",
        f"vista-host: {'running' if host.get('running') else 'stopped'}",
    ]
    return text_result(data, "\n".join(summary))


def vm_status(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name"})
    name = str_value(args, "name", default="", allow_empty=True) or None
    vm = services.vm.resolve(name)
    data: dict[str, Any] = vm.as_dict()
    client = services.client_for(vm.cid)
    try:
        data["health"] = client.health(timeout=5)
    except ToolError as error:
        data["health"] = {"error": error.as_dict()}
    try:
        data["screen"] = client.screen()
    except ToolError as error:
        data["screen"] = {"error": error.as_dict()}
    return text_result(data)


def vm_start(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name", "cid", "headless", "unlock", "wait_seconds"})
    name = str_value(args, "name", default="", allow_empty=True) or None
    cid = int_value(args, "cid", default=None, minimum=1, maximum=0xFFFFFFFF) if "cid" in args else None
    headless = bool_value(args, "headless", default=False)
    unlock = bool_value(args, "unlock", default=True)
    wait_seconds = number_value(args, "wait_seconds", default=0.0, minimum=0, maximum=900)
    data = services.vm.start(
        name=name,
        cid=cid,
        headless=headless,
        unlock=unlock,
        wait_seconds=wait_seconds,
    )
    return text_result(data)


def vm_wait(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name", "cid", "timeout_seconds"})
    name = str_value(args, "name", default="", allow_empty=True) or None
    cid = int_value(args, "cid", default=None, minimum=1, maximum=0xFFFFFFFF) if "cid" in args else None
    timeout_seconds = number_value(
        args, "timeout_seconds", default=120.0, minimum=1, maximum=900
    )
    data = services.vm.wait(name=name, cid=cid, timeout_seconds=timeout_seconds)
    return text_result(data)


def vm_stop(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"name", "timeout_seconds", "confirm"})
    confirm_value(args)
    name = str_value(args, "name", default="", allow_empty=True) or None
    timeout_seconds = number_value(
        args, "timeout_seconds", default=150.0, minimum=10, maximum=300
    )
    data = services.vm.stop(name=name, timeout_seconds=timeout_seconds)
    return text_result(data)


def register(registry: Any, services: Services) -> None:
    registry.add(
        Tool(
            name="vista_status",
            title="Vista computer-use status",
            description=(
                "Read-only inventory: VM processes and CID, vphoned health and screen, "
                "QMP availability, and the Vista host-service state."
            ),
            input_schema=schema(),
            handler=lambda args: status(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_vm_status",
            title="vPhone VM status",
            description="Read-only status for one VM, including vphoned health and screen geometry.",
            input_schema=schema(
                properties={"name": _VM_NAME_SCHEMA},
            ),
            handler=lambda args: vm_status(services, args),
            read_only=True,
            idempotent=True,
        )
    )
    registry.add(
        Tool(
            name="vista_vm_start",
            title="Start the vPhone VM",
            description=(
                "Start a VM through vphone-linux's setup/start-vm.sh. The script runs detached; "
                "return immediately by default, or pass wait_seconds to wait for vphoned health. "
                "The VM window is shown unless headless is true."
            ),
            input_schema=schema(
                properties={
                    "name": _VM_NAME_SCHEMA,
                    "cid": {
                        "type": "integer",
                        "description": "Override the vsock CID (default: the VM's recorded CID).",
                        "minimum": 1,
                        "maximum": 4294967295,
                    },
                    "headless": {
                        "type": "boolean",
                        "description": "Start without the reims-vgpu window.",
                        "default": False,
                    },
                    "unlock": {
                        "type": "boolean",
                        "description": "Ask vphoned to unlock the guest after boot.",
                        "default": True,
                    },
                    "wait_seconds": {
                        "type": "number",
                        "description": "Wait this long for vphoned health before returning (0-900).",
                        "minimum": 0,
                        "maximum": 900,
                        "default": 0,
                    },
                },
            ),
            handler=lambda args: vm_start(services, args),
            idempotent=False,
        )
    )
    registry.add(
        Tool(
            name="vista_vm_wait",
            title="Wait for vPhone vphoned",
            description=(
                "Poll vphoned health for an already-booting VM. Returns ready=false with "
                "timed_out=true rather than raising when the deadline expires. Pass the same "
                "cid used at start if it was overridden."
            ),
            input_schema=schema(
                properties={
                    "name": _VM_NAME_SCHEMA,
                    "cid": {
                        "type": "integer",
                        "description": "Override the vsock CID.",
                        "minimum": 1,
                        "maximum": 4294967295,
                    },
                    "timeout_seconds": {
                        "type": "number",
                        "description": "Maximum wait (1-900 seconds).",
                        "minimum": 1,
                        "maximum": 900,
                        "default": 120,
                    },
                },
            ),
            handler=lambda args: vm_wait(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_vm_stop",
            title="Stop the vPhone VM",
            description=(
                "Gracefully shut down the guest through vphoned (system.shutdown) using "
                "setup/start-vm.sh --stop. Requires confirm:true. Does not SIGKILL QEMU, "
                "because an unclean stop can corrupt the APFS disk."
            ),
            input_schema=schema(
                properties={
                    "name": _VM_NAME_SCHEMA,
                    "timeout_seconds": {
                        "type": "number",
                        "description": "Maximum graceful shutdown wait (10-300 seconds).",
                        "minimum": 10,
                        "maximum": 300,
                        "default": 150,
                    },
                    "confirm": confirm_property(),
                },
                required=["confirm"],
            ),
            handler=lambda args: vm_stop(services, args),
            destructive=True,
        )
    )
