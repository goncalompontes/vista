"""Command-line entry point for the Vista computer-use MCP server."""

from __future__ import annotations

import argparse
import json
import sys

from . import __version__
from .config import Config
from .mcp import McpServer
from .tools import build_registry, build_services


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="vista-computer-use-mcp",
        description="MCP server for the Vista vphone/QEMU computer-use workflow.",
    )
    parser.add_argument("--version", action="store_true", help="print the version and exit")
    parser.add_argument(
        "--list-tools",
        action="store_true",
        help="print the tool descriptors as JSON and exit",
    )
    parser.add_argument(
        "--config-json",
        action="store_true",
        help="print resolved configuration as JSON and exit",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="run the read-only vista_status tool and print a short report",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if args.version:
        print(__version__)
        return 0
    try:
        config = Config.from_env()
    except ValueError as error:
        print(f"configuration error: {error}", file=sys.stderr)
        return 2
    if args.config_json:
        print(json.dumps(config.public(), indent=2, sort_keys=True))
        return 0
    services = build_services(config)
    registry = build_registry(services)
    if args.list_tools:
        print(json.dumps(registry.list(), indent=2, sort_keys=True))
        return 0
    if args.check:
        result = registry.call("vista_status", {})
        for item in result.content:
            if item.get("type") == "text":
                print(item.get("text", ""))
        return 0 if not result.is_error else 1
    server = McpServer(registry)
    server.serve_stdio()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
