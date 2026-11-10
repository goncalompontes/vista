"""Small, explicit validators for tool input.

The server does not ship a general JSON Schema implementation. Each tool
declares a strict schema for discovery and calls these helpers for the checks
that matter: required fields, unknown fields, bounded values, enums, paths, and
explicit confirmation for destructive operations.
"""

from __future__ import annotations

import ipaddress
import math
import os
import re
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from .config import Config
from .errors import ToolError

_MISSING = object()

_BUNDLE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,254}$")
_SECRET_HEX = re.compile(r"^[0-9a-fA-F]{64}$")
_PROCESS = re.compile(r"^[A-Za-z0-9._+-]{1,128}$")
_SUBSYSTEM = re.compile(r"^[A-Za-z0-9._-]{1,128}$")
_KEY_NAME = re.compile(r"^[A-Za-z0-9._-]{1,64}$")
_NETDEV = re.compile(r"^[A-Za-z0-9_-]{1,32}$")
_TARGET = re.compile(r"^(desktop|window:[0-9]{1,20}|workspace:[0-9]{1,20})$")
_SCHEME = re.compile(r"^[a-z][a-z0-9+.-]{0,31}$")
_DOMAIN = re.compile(r"^[A-Za-z0-9._-]{1,253}$")


def _fail(message: str, **details: Any) -> None:
    raise ToolError("invalid_arguments", message, details or None)


def ensure_object(value: Any, name: str = "arguments") -> dict[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{name} must be a JSON object")
    for key in value:
        if not isinstance(key, str):
            _fail(f"{name} keys must be strings")
    return value


def check_keys(
    args: Mapping[str, Any],
    allowed: Iterable[str],
    required: Iterable[str] = (),
) -> None:
    allowed_set = set(allowed)
    unknown = sorted(set(args) - allowed_set)
    if unknown:
        _fail("unknown argument(s): " + ", ".join(unknown), unknown=unknown)
    missing = sorted(set(required) - set(args))
    if missing:
        _fail("missing required argument(s): " + ", ".join(missing), missing=missing)


def _present(args: Mapping[str, Any], key: str, default: Any = _MISSING) -> Any:
    value = args.get(key, _MISSING)
    if value is _MISSING:
        if default is _MISSING:
            _fail(f"{key} is required")
        return default
    return value


def str_value(
    args: Mapping[str, Any],
    key: str,
    *,
    default: Any = _MISSING,
    min_len: int = 1,
    max_len: int = 4096,
    pattern: re.Pattern[str] | None = None,
    multiline: bool = False,
    allow_empty: bool = False,
) -> str:
    value = _present(args, key, default)
    if not isinstance(value, str):
        _fail(f"{key} must be a string")
    if not allow_empty and len(value) < min_len:
        _fail(f"{key} must be at least {min_len} character(s)")
    if len(value) > max_len:
        _fail(f"{key} must be at most {max_len} character(s)")
    if any(ord(char) == 0 for char in value):
        _fail(f"{key} must not contain NUL")
    if not multiline and any(ord(char) < 0x20 or ord(char) == 0x7F for char in value):
        _fail(f"{key} must not contain control characters")
    if pattern is not None and not pattern.fullmatch(value):
        _fail(f"{key} has an invalid format")
    return value


def bool_value(
    args: Mapping[str, Any],
    key: str,
    *,
    default: Any = _MISSING,
) -> bool:
    value = _present(args, key, default)
    if not isinstance(value, bool):
        _fail(f"{key} must be a boolean")
    return value


def int_value(
    args: Mapping[str, Any],
    key: str,
    *,
    default: Any = _MISSING,
    minimum: int | None = None,
    maximum: int | None = None,
) -> int:
    value = _present(args, key, default)
    if isinstance(value, bool) or not isinstance(value, int):
        _fail(f"{key} must be an integer")
    if minimum is not None and value < minimum:
        _fail(f"{key} must be at least {minimum}")
    if maximum is not None and value > maximum:
        _fail(f"{key} must be at most {maximum}")
    return value


def number_value(
    args: Mapping[str, Any],
    key: str,
    *,
    default: Any = _MISSING,
    minimum: float | None = None,
    maximum: float | None = None,
) -> float:
    value = _present(args, key, default)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        _fail(f"{key} must be a number")
    number = float(value)
    if not math.isfinite(number):
        _fail(f"{key} must be finite")
    if minimum is not None and number < minimum:
        _fail(f"{key} must be at least {minimum}")
    if maximum is not None and number > maximum:
        _fail(f"{key} must be at most {maximum}")
    return number


def enum_value(
    args: Mapping[str, Any],
    key: str,
    choices: Sequence[str],
    *,
    default: Any = _MISSING,
) -> str:
    value = _present(args, key, default)
    if not isinstance(value, str):
        _fail(f"{key} must be a string")
    if value not in choices:
        _fail(f"{key} must be one of: " + ", ".join(choices))
    return value


def confirm_value(args: Mapping[str, Any], key: str = "confirm") -> bool:
    value = bool_value(args, key, default=False)
    if not value:
        raise ToolError(
            "confirmation_required",
            f"this operation requires {key}: true",
            {"argument": key},
        )
    return value


def secret_hex_value(
    args: Mapping[str, Any],
    key: str,
    *,
    default: Any = _MISSING,
) -> str:
    value = str_value(args, key, default=default, min_len=64, max_len=64)
    if not _SECRET_HEX.fullmatch(value):
        _fail(f"{key} must be 64 hexadecimal characters")
    return value.lower()


def bundle_id_value(args: Mapping[str, Any], key: str, *, default: Any = _MISSING) -> str:
    value = str_value(args, key, default=default, min_len=1, max_len=255)
    if not _BUNDLE_ID.fullmatch(value):
        _fail(f"{key} is not a valid bundle identifier")
    return value


def process_value(args: Mapping[str, Any], key: str = "process") -> str:
    value = str_value(args, key, min_len=1, max_len=128)
    if not _PROCESS.fullmatch(value):
        _fail(f"{key} must contain only letters, digits, . _ + -")
    return value


def subsystem_value(args: Mapping[str, Any], key: str = "subsystem") -> str:
    value = str_value(args, key, min_len=1, max_len=128)
    if not _SUBSYSTEM.fullmatch(value):
        _fail(f"{key} must contain only letters, digits, . _ -")
    return value


def key_name_value(args: Mapping[str, Any], key: str = "name") -> str:
    value = str_value(args, key, min_len=1, max_len=64)
    if not _KEY_NAME.fullmatch(value):
        _fail(f"{key} must contain only letters, digits, . _ -")
    return value


def netdev_value(args: Mapping[str, Any], key: str = "netdev") -> str:
    value = str_value(args, key, min_len=1, max_len=32)
    if not _NETDEV.fullmatch(value):
        _fail(f"{key} must contain only letters, digits, _ -")
    return value


def url_value(args: Mapping[str, Any], key: str = "url") -> str:
    value = str_value(args, key, min_len=1, max_len=2048)
    scheme, separator, _ = value.partition(":")
    if not separator or not _SCHEME.fullmatch(scheme.lower()):
        _fail(f"{key} must be an absolute URL")
    if scheme.lower() not in {"http", "https"}:
        _fail(f"{key} only accepts http and https URLs")
    if not value.lower().startswith(scheme.lower() + "://"):
        _fail(f"{key} must include :// after the scheme")
    return value


def domain_value(args: Mapping[str, Any], key: str = "host") -> str:
    value = str_value(args, key, min_len=1, max_len=253)
    if not _DOMAIN.fullmatch(value):
        _fail(f"{key} must be a hostname or IP address")
    return value


def target_value(args: Mapping[str, Any], key: str = "target", *, default: str = "desktop") -> str:
    value = str_value(args, key, default=default, min_len=1, max_len=64)
    if not _TARGET.fullmatch(value):
        _fail(f"{key} must be desktop, window:ID, or workspace:ID")
    return value


def pointer_value(args: Mapping[str, Any], key: str = "pointer") -> str:
    value = str_value(args, key, min_len=3, max_len=64)
    x_raw, separator, y_raw = value.partition(":")
    if not separator:
        _fail(f"{key} must be X:Y")
    try:
        x, y = float(x_raw), float(y_raw)
    except ValueError:
        _fail(f"{key} must be X:Y with numeric coordinates")
    if not (math.isfinite(x) and math.isfinite(y)) or not (0.0 <= x <= 1.0):
        _fail(f"{key} coordinates must be finite and between 0 and 1")
    if not 0.0 <= y <= 1.0:
        _fail(f"{key} coordinates must be finite and between 0 and 1")
    return f"{x:g}:{y:g}"


def selector_params(args: Mapping[str, Any]) -> dict[str, Any]:
    """Build the vphoned selector object from a tool call."""
    allowed = {"text", "identifier", "role", "match", "index"}
    check_keys(args, allowed)
    params: dict[str, Any] = {}
    if "text" in args:
        params["text"] = str_value(args, "text", min_len=1, max_len=512)
    if "identifier" in args:
        params["identifier"] = str_value(args, "identifier", min_len=1, max_len=512)
    if "role" in args:
        params["role"] = str_value(args, "role", min_len=1, max_len=64)
    if not any(key in params for key in ("text", "identifier", "role")):
        _fail("one of text, identifier, or role is required")
    if "match" in args:
        params["match"] = enum_value(args, "match", ("contains", "exact"))
    if "index" in args:
        params["index"] = int_value(args, "index", minimum=0, maximum=1000)
    return params


def screen_geometry(screen: Mapping[str, Any]) -> tuple[int, int, float]:
    width = screen.get("width")
    height = screen.get("height")
    scale = screen.get("scale", 1.0)
    if isinstance(width, bool) or not isinstance(width, (int, float)) or width <= 0:
        raise ToolError("invalid_response", "device.screen did not report a usable width")
    if isinstance(height, bool) or not isinstance(height, (int, float)) or height <= 0:
        raise ToolError("invalid_response", "device.screen did not report a usable height")
    if isinstance(scale, bool) or not isinstance(scale, (int, float)) or scale <= 0:
        scale = 1.0
    return int(width), int(height), float(scale)


def resolve_point(
    args: Mapping[str, Any],
    screen: Mapping[str, Any] | None,
    *,
    x_key: str = "x",
    y_key: str = "y",
) -> tuple[float, float]:
    if screen is None:
        # Without a guest answer we can still reject obviously unsafe values.
        x = number_value(args, x_key, minimum=0, maximum=100_000)
        y = number_value(args, y_key, minimum=0, maximum=100_000)
        return x, y
    width, height, scale = screen_geometry(screen)
    space = enum_value(args, "space", ("points", "pixels"), default="points")
    divisor = scale if space == "pixels" else 1.0
    if divisor <= 0:
        raise ToolError("invalid_response", "guest screen scale must be positive")
    max_x = float(width) * divisor
    max_y = float(height) * divisor
    x = number_value(args, x_key, minimum=0, maximum=max_x)
    y = number_value(args, y_key, minimum=0, maximum=max_y)
    return _clamp_point(x, y, width, height, divisor, space)


def _clamp_point(
    x: float,
    y: float,
    width: int,
    height: int,
    divisor: float,
    space: str,
) -> tuple[float, float]:
    points_x = x / divisor if space == "pixels" else x
    points_y = y / divisor if space == "pixels" else y
    if points_x < 0 or points_x >= width or points_y < 0 or points_y >= height:
        _fail(
            f"({x:g}, {y:g}) is outside the guest screen "
            f"({width}x{height} points)"
        )
    return points_x, points_y


def resolve_point_pair(
    args: Mapping[str, Any],
    screen: Mapping[str, Any] | None,
    x1_key: str,
    y1_key: str,
    x2_key: str,
    y2_key: str,
) -> tuple[tuple[float, float], tuple[float, float]]:
    space = enum_value(args, "space", ("points", "pixels"), default="points")
    if screen is None:
        return (
            (
                number_value(args, x1_key, minimum=0, maximum=100_000),
                number_value(args, y1_key, minimum=0, maximum=100_000),
            ),
            (
                number_value(args, x2_key, minimum=0, maximum=100_000),
                number_value(args, y2_key, minimum=0, maximum=100_000),
            ),
        )
    width, height, scale = screen_geometry(screen)
    divisor = scale if space == "pixels" else 1.0
    if divisor <= 0:
        raise ToolError("invalid_response", "guest screen scale must be positive")
    max_x = float(width) * divisor
    max_y = float(height) * divisor
    first = _clamp_point(
        number_value(args, x1_key, minimum=0, maximum=max_x),
        number_value(args, y1_key, minimum=0, maximum=max_y),
        width,
        height,
        divisor,
        space,
    )
    second = _clamp_point(
        number_value(args, x2_key, minimum=0, maximum=max_x),
        number_value(args, y2_key, minimum=0, maximum=max_y),
        width,
        height,
        divisor,
        space,
    )
    return first, second


def path_value(
    args: Mapping[str, Any],
    key: str,
    config: Config,
    *,
    must_exist: bool = False,
    for_write: bool = False,
    allowed_suffixes: Sequence[str] | None = None,
    max_bytes: int | None = None,
    base: Path | None = None,
) -> Path:
    raw = str_value(args, key, min_len=1, max_len=4096, multiline=False)
    path = Path(raw).expanduser()
    if not path.is_absolute():
        path = (base or config.workspace) / path
    resolved = path.resolve(strict=False)
    if for_write and not _is_allowed_write(resolved, config):
        raise ToolError(
            "unsafe_path",
            f"{key} must be inside an allowed write root",
            {"allowed_roots": [str(root) for root in config.allowed_write_roots]},
        )
    if allowed_suffixes is not None:
        suffix = resolved.suffix.lower()
        if suffix not in {item.lower() for item in allowed_suffixes}:
            _fail(f"{key} must end with one of: " + ", ".join(allowed_suffixes))
    if must_exist:
        if not resolved.is_file():
            raise ToolError("not_found", f"{key} does not exist or is not a file", {"path": str(resolved)})
        if max_bytes is not None:
            size = resolved.stat().st_size
            if size > max_bytes:
                raise ToolError(
                    "file_too_large",
                    f"{key} is larger than the {max_bytes} byte limit",
                    {"path": str(resolved), "bytes": size},
                )
    return resolved


def _is_allowed_write(path: Path, config: Config) -> bool:
    roots = list(config.allowed_write_roots) + [config.screenshot_dir, config.state_dir]
    for root in roots:
        try:
            path.relative_to(root.resolve())
            return True
        except ValueError:
            continue
    return False


def output_path(
    args: Mapping[str, Any],
    config: Config,
    *,
    default_dir: Path,
    default_name: str,
    allowed_suffixes: Sequence[str],
    key: str = "path",
) -> Path:
    raw = args.get(key)
    if raw is None:
        directory = default_dir
        name = default_name
    else:
        if not isinstance(raw, str):
            _fail(f"{key} must be a string")
        candidate = Path(raw).expanduser()
        if candidate.is_absolute():
            directory = candidate.parent
            name = candidate.name
        else:
            directory = default_dir / candidate.parent
            name = candidate.name
    if not name:
        _fail(f"{key} must name a file")
    suffix = Path(name).suffix.lower()
    allowed = {item.lower() for item in allowed_suffixes}
    if suffix and suffix not in allowed:
        _fail(f"{key} must end with one of: " + ", ".join(allowed_suffixes))
    if not suffix:
        name = name + allowed_suffixes[0]
    path = (directory / name).resolve(strict=False)
    if not _is_allowed_write(path, config):
        raise ToolError(
            "unsafe_path",
            f"{key} must be inside an allowed write root",
            {"allowed_roots": [str(root) for root in config.allowed_write_roots]},
        )
    return path


def listen_value(args: Mapping[str, Any], key: str = "listen", *, default: str = "127.0.0.1:7443") -> str:
    value = str_value(args, key, default=default, min_len=3, max_len=128)
    host, separator, port_raw = value.rpartition(":")
    if not separator or not host:
        _fail(f"{key} must be HOST:PORT")
    if host.startswith("[") and host.endswith("]"):
        host = host[1:-1]
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        if host == "localhost":
            address = ipaddress.ip_address("127.0.0.1")
        else:
            _fail(f"{key} must use a numeric IP address or localhost")
    try:
        port = int(port_raw, 10)
    except ValueError:
        _fail(f"{key} port must be an integer")
    if not 0 <= port <= 65535:
        _fail(f"{key} port must be between 0 and 65535")
    rendered = f"[{address}]" if address.version == 6 else str(address)
    return f"{rendered}:{port}"


def address_is_loopback(listen: str) -> bool:
    host, _, _ = listen.rpartition(":")
    if host.startswith("[") and host.endswith("]"):
        host = host[1:-1]
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def parse_host_port(address: str) -> tuple[str, int]:
    host, _, port_raw = address.rpartition(":")
    if host.startswith("[") and host.endswith("]"):
        host = host[1:-1]
    try:
        port = int(port_raw, 10)
    except ValueError as error:
        raise ToolError("invalid_response", f"invalid listener address {address!r}") from error
    return host, port


def max_image_bytes_from_env() -> int:
    raw = os.environ.get("VISTA_CU_MAX_IMAGE_BYTES")
    if not raw:
        return 3_000_000
    try:
        return int(raw, 10)
    except ValueError:
        return 3_000_000
