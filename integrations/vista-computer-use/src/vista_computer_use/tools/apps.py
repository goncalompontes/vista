"""App install/launch/log tools and the Vista client convenience deploy."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ..errors import ToolError
from ..validation import (
    bool_value,
    bundle_id_value,
    check_keys,
    confirm_value,
    enum_value,
    int_value,
    path_value,
    process_value,
    str_value,
    subsystem_value,
    url_value,
)
from .common import (
    Services,
    Tool,
    confirm_property,
    schema,
    text_result,
)

_DEFAULT_CLIENT_IPA = "client/ios/xtool/Vista.ipa"
_DEFAULT_BUNDLE = "dev.vista.client"
_MAX_IPA_BYTES = 8 * 1024 * 1024 * 1024


def _ipa_path(services: Services, args: dict[str, Any], key: str) -> Path:
    return path_value(
        args,
        key,
        services.config,
        must_exist=True,
        allowed_suffixes=(".ipa", ".tipa"),
        max_bytes=_MAX_IPA_BYTES,
    )


def app_list(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"filter", "query"})
    filter_name = enum_value(
        args, "filter", ("all", "running", "user", "system"), default="all"
    )
    query = str_value(args, "query", default="", allow_empty=True, max_len=128) or None
    if query:
        data = services.vphone.apps_search(query)
        data = {"apps": data.get("apps", data), "query": query}
    else:
        data = services.vphone.apps_list(filter_name)
        data = {"apps": data.get("apps", []), "filter": filter_name}
    apps = data.get("apps")
    data["count"] = len(apps) if isinstance(apps, list) else None
    return text_result(data)


def app_install(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"ipa_path", "launch_bundle_id", "confirm"})
    confirm_value(args)
    ipa_path = _ipa_path(services, args, "ipa_path")
    launch_bundle: str | None = None
    if "launch_bundle_id" in args:
        launch_bundle = bundle_id_value(args, "launch_bundle_id")
    result = services.vphone.install_ipa(ipa_path, launch_bundle)
    data: dict[str, Any] = {
        "ipa_path": str(ipa_path),
        "bytes": ipa_path.stat().st_size,
        "launch_bundle_id": launch_bundle,
        "install": result,
    }
    installed = result.get("installed_bundle_id") or launch_bundle
    if installed:
        try:
            data["app"] = services.vphone.apps_search(installed)
        except ToolError as error:
            data["app_error"] = error.as_dict()
    return text_result(data)


def app_launch(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"bundle_id", "url"})
    bundle_id = bundle_id_value(args, "bundle_id")
    url = url_value(args, "url") if "url" in args else None
    result = services.vphone.apps_launch(bundle_id, url)
    data: dict[str, Any] = {
        "bundle_id": bundle_id,
        "url": url,
        "result": result,
    }
    try:
        data["foreground"] = services.vphone.apps_foreground()
    except ToolError as error:
        data["foreground_error"] = error.as_dict()
    return text_result(data)


def app_terminate(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"bundle_id"})
    bundle_id = bundle_id_value(args, "bundle_id")
    result = services.vphone.apps_terminate(bundle_id)
    return text_result({"bundle_id": bundle_id, "result": result})


def app_logs(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"process", "subsystem", "seconds", "max_entries"})
    process = process_value(args)
    subsystem = subsystem_value(args) if "subsystem" in args else None
    seconds = int_value(args, "seconds", default=10, minimum=1, maximum=60)
    max_entries = int_value(args, "max_entries", default=500, minimum=1, maximum=5000)
    result = services.vphone.app_logs(process, subsystem, seconds)
    entries = result.get("entries")
    omitted = 0
    if isinstance(entries, list) and len(entries) > max_entries:
        omitted = len(entries) - max_entries
        entries = entries[-max_entries:]
    data = {
        "process": process,
        "subsystem": subsystem,
        "seconds": seconds,
        "entries": entries,
        "count": len(entries) if isinstance(entries, list) else 0,
        "omitted": omitted,
        "truncated": result.get("truncated"),
        "note": result.get("note"),
    }
    return text_result(data)


def app_crashes(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"bundle_id"})
    bundle_id = bundle_id_value(args, "bundle_id") if "bundle_id" in args else None
    result = services.vphone.crash_reports(bundle_id)
    return text_result({"bundle_id": bundle_id, **result})


def client_deploy(services: Services, args: dict[str, Any]) -> Any:
    check_keys(args, {"ipa_path", "bundle_id", "launch", "confirm"})
    confirm_value(args)
    bundle_id = bundle_id_value(args, "bundle_id", default=_DEFAULT_BUNDLE)
    launch = bool_value(args, "launch", default=True)
    ipa_args = dict(args)
    ipa_args.setdefault("ipa_path", _DEFAULT_CLIENT_IPA)
    ipa_path = _ipa_path(services, ipa_args, "ipa_path")
    result = services.vphone.install_ipa(ipa_path, bundle_id if launch else None)
    data: dict[str, Any] = {
        "ipa_path": str(ipa_path),
        "bytes": ipa_path.stat().st_size,
        "bundle_id": bundle_id,
        "launched": launch,
        "install": result,
    }
    try:
        data["foreground"] = services.vphone.apps_foreground()
    except ToolError as error:
        data["foreground_error"] = error.as_dict()
    try:
        data["app"] = services.vphone.apps_search(bundle_id)
    except ToolError as error:
        data["app_error"] = error.as_dict()
    return text_result(data)


def register(registry: Any, services: Services) -> None:
    registry.add(
        Tool(
            name="vista_app_list",
            title="List installed apps",
            description=(
                "List or search guest apps through vphoned. filter=running is useful before "
                "launching or terminating a development build."
            ),
            input_schema=schema(
                properties={
                    "filter": {
                        "type": "string",
                        "enum": ["all", "running", "user", "system"],
                        "default": "all",
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional search string; uses apps.search.",
                        "maxLength": 128,
                    },
                },
            ),
            handler=lambda args: app_list(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_app_install",
            title="Install an IPA",
            description=(
                "Upload and install an .ipa/.tipa through vphone-linux's install-ipa.py. "
                "The guest signs the app. Requires confirm:true."
            ),
            input_schema=schema(
                properties={
                    "ipa_path": {
                        "type": "string",
                        "description": "Local IPA path; relative paths resolve against the Vista workspace.",
                        "maxLength": 4096,
                    },
                    "launch_bundle_id": {
                        "type": "string",
                        "description": "Optional bundle ID to launch after installation.",
                        "maxLength": 255,
                    },
                    "confirm": confirm_property(),
                },
                required=["ipa_path", "confirm"],
            ),
            handler=lambda args: app_install(services, args),
            destructive=True,
        )
    )
    registry.add(
        Tool(
            name="vista_app_launch",
            title="Launch an app",
            description="Launch an installed app by bundle ID, optionally with an http(s) URL.",
            input_schema=schema(
                properties={
                    "bundle_id": {"type": "string", "maxLength": 255},
                    "url": {
                        "type": "string",
                        "description": "Optional http/https URL to open with the app.",
                        "maxLength": 2048,
                    },
                },
                required=["bundle_id"],
            ),
            handler=lambda args: app_launch(services, args),
        )
    )
    registry.add(
        Tool(
            name="vista_app_terminate",
            title="Terminate an app",
            description="Terminate a running guest app by bundle ID.",
            input_schema=schema(
                properties={"bundle_id": {"type": "string", "maxLength": 255}},
                required=["bundle_id"],
            ),
            handler=lambda args: app_terminate(services, args),
            destructive=True,
        )
    )
    registry.add(
        Tool(
            name="vista_app_logs",
            title="Read app logs",
            description=(
                "Collect the guest's unified log for one process through app-logs.py. "
                "Use subsystem to keep only the app's own os.Logger lines. Returns the most "
                "recent max_entries entries."
            ),
            input_schema=schema(
                properties={
                    "process": {
                        "type": "string",
                        "description": "Executable/process name, e.g. Vista.",
                        "maxLength": 128,
                    },
                    "subsystem": {"type": "string", "maxLength": 128},
                    "seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "default": 10,
                    },
                    "max_entries": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 5000,
                        "default": 500,
                    },
                },
                required=["process"],
            ),
            handler=lambda args: app_logs(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_app_crashes",
            title="List crash reports",
            description="List guest crash reports, optionally filtered by bundle ID.",
            input_schema=schema(
                properties={"bundle_id": {"type": "string", "maxLength": 255}},
            ),
            handler=lambda args: app_crashes(services, args),
            read_only=True,
        )
    )
    registry.add(
        Tool(
            name="vista_client_deploy",
            title="Install and launch the Vista iOS client",
            description=(
                "Install client/ios/xtool/Vista.ipa into the running guest and optionally launch "
                "dev.vista.client. Build the IPA first with client/ios/vphone.sh build. "
                "Requires confirm:true."
            ),
            input_schema=schema(
                properties={
                    "ipa_path": {
                        "type": "string",
                        "description": f"Default: {_DEFAULT_CLIENT_IPA}",
                        "maxLength": 4096,
                    },
                    "bundle_id": {
                        "type": "string",
                        "description": f"Default: {_DEFAULT_BUNDLE}",
                        "maxLength": 255,
                    },
                    "launch": {"type": "boolean", "default": True},
                    "confirm": confirm_property(),
                },
                required=["confirm"],
            ),
            handler=lambda args: client_deploy(services, args),
            destructive=True,
        )
    )
