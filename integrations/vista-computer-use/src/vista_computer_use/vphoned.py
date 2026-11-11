"""HTTP-over-vsock transport for vphoned.

Why this exists even though vphone-linux ships ``tools/vphoned.sh``: that
script keeps socat's stdin open (as vphoned requires) and therefore blocks for
the full ``VPHONED_TIMEOUT`` on every request. A 1-second floor per tap makes an
accessibility-driven loop impractical. This module follows the same wire
contract already implemented in ``tools/window/vphone-window.py`` and
``tools/app-logs.py``: HTTP/1.1 on vsock port 1339, ``Host: localhost``, one
response per connection, Content-Length bounded. It supports both the default
vhost-vsock path and the optional Unix-socket transport.
"""

from __future__ import annotations

import json
import os
import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .config import Config
from .errors import ToolError

PORT = 1339
MAX_RESPONSE_BYTES = 64 * 1024 * 1024


@dataclass(frozen=True)
class HttpResponse:
    status: int
    reason: str
    headers: dict[str, str]
    body: bytes

    def json(self) -> Any:
        try:
            return json.loads(self.body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ToolError(
                "invalid_response",
                f"vphoned returned invalid JSON: {error}",
                {"status": self.status, "body": self.body[:400].decode("utf-8", "replace")},
            ) from error


class VphonedTransport:
    def __init__(self, config: Config, cid: int) -> None:
        self.config = config
        self.cid = cid

    def request(
        self,
        method: str,
        path: str,
        *,
        body: bytes | None = None,
        timeout: float = 30.0,
    ) -> HttpResponse:
        if method not in {"GET", "POST", "PUT"}:
            raise ToolError("internal_error", f"unsupported HTTP method {method}")
        uds = self._uds_path()
        if uds is not None:
            return self._request_uds(uds, method, path, body, timeout)
        return self._request_vsock(method, path, body, timeout)

    def get_json(self, path: str, *, timeout: float = 10.0) -> Any:
        response = self.request("GET", path, timeout=timeout)
        if response.status < 200 or response.status >= 300:
            raise _http_error(response)
        return response.json()

    def rpc(
        self,
        method: str,
        params: dict[str, Any] | None = None,
        *,
        timeout: float = 30.0,
    ) -> Any:
        payload = json.dumps(
            {"method": method, "params": params or {}, "id": 1},
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode("utf-8")
        response = self.request("POST", "/v1/rpc", body=payload, timeout=timeout)
        if response.status < 200 or response.status >= 300:
            raise _http_error(response)
        data = response.json()
        if isinstance(data, dict):
            error = data.get("error")
            if error is not None:
                if not isinstance(error, dict):
                    error = {"message": str(error)}
                raise ToolError(
                    "guest_error",
                    str(error.get("message") or f"{method} failed"),
                    {"method": method, "guest_code": error.get("code")},
                )
            if "result" in data:
                return data["result"]
        return data

    def _uds_path(self) -> Path | None:
        directory = Path(os.environ.get("VSOCK_DIR", "/tmp/vphone-vsock"))
        candidate = directory / f"{self.cid}.sock"
        return candidate if candidate.is_socket() else None

    def _connect_vsock(self, timeout: float) -> socket.socket:
        if not hasattr(socket, "AF_VSOCK"):
            raise ToolError(
                "not_configured",
                "Python has no AF_VSOCK support and no Unix vsock socket was found",
            )
        sock = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        for option_name in (
            "SO_VM_SOCKETS_BUFFER_MAX_SIZE",
            "SO_VM_SOCKETS_BUFFER_SIZE",
        ):
            option = getattr(socket, option_name, None)
            if option is not None:
                try:
                    sock.setsockopt(socket.AF_VSOCK, option, 4 << 20)
                except OSError:
                    pass
        try:
            sock.connect((self.cid, PORT))
        except OSError as error:
            sock.close()
            raise ToolError(
                "unavailable",
                f"cannot connect to vphoned on vsock CID {self.cid}: {error}",
                {"cid": self.cid, "port": PORT},
            ) from error
        return sock

    def _request_vsock(
        self,
        method: str,
        path: str,
        body: bytes | None,
        timeout: float,
    ) -> HttpResponse:
        sock = self._connect_vsock(timeout)
        try:
            sock.sendall(_http_request(method, path, body))
            return _read_response(sock)
        except (OSError, TimeoutError) as error:
            raise ToolError(
                "unavailable",
                f"vphoned transport failed: {error}",
                {"cid": self.cid},
            ) from error
        finally:
            sock.close()

    def _request_uds(
        self,
        path_to_socket: Path,
        method: str,
        path: str,
        body: bytes | None,
        timeout: float,
    ) -> HttpResponse:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        try:
            sock.connect(str(path_to_socket))
            sock.sendall(b"CONNECT 1339\n")
            sock.sendall(_http_request(method, path, body))
            return _read_response(sock, skip_prefix_lines=True)
        except (OSError, TimeoutError) as error:
            raise ToolError(
                "unavailable",
                f"vphoned Unix-socket transport failed: {error}",
                {"socket": str(path_to_socket)},
            ) from error
        finally:
            sock.close()


def _http_request(method: str, path: str, body: bytes | None) -> bytes:
    lines = [
        f"{method} {path} HTTP/1.1",
        "Host: localhost",
        "Connection: close",
    ]
    if body is not None:
        lines.append("Content-Type: application/json")
        lines.append(f"Content-Length: {len(body)}")
    head = ("\r\n".join(lines) + "\r\n\r\n").encode("ascii")
    return head + (body or b"")


def _read_until(sock: socket.socket, marker: bytes, limit: int = 64 * 1024) -> bytes:
    buffer = b""
    while marker not in buffer:
        chunk = sock.recv(4096)
        if not chunk:
            raise ToolError("unavailable", "vphoned closed the connection before headers")
        buffer += chunk
        if len(buffer) > limit:
            raise ToolError("invalid_response", "vphoned response headers are too large")
    return buffer


def _read_response(sock: socket.socket, skip_prefix_lines: bool = False) -> HttpResponse:
    raw = _read_until(sock, b"\r\n\r\n")
    head, _, rest = raw.partition(b"\r\n\r\n")
    lines = head.split(b"\r\n")
    if skip_prefix_lines:
        while lines and not lines[0].startswith(b"HTTP/"):
            lines.pop(0)
    if not lines or not lines[0].startswith(b"HTTP/"):
        raise ToolError(
            "invalid_response",
            "vphoned response did not contain an HTTP status line",
            {"head": head[:400].decode("utf-8", "replace")},
        )
    status_parts = lines[0].split(b" ", 2)
    try:
        status = int(status_parts[1])
    except (IndexError, ValueError) as error:
        raise ToolError("invalid_response", "vphoned response had an invalid status") from error
    reason = status_parts[2].decode("utf-8", "replace") if len(status_parts) > 2 else ""
    headers: dict[str, str] = {}
    for line in lines[1:]:
        name, separator, value = line.partition(b":")
        if separator:
            headers[name.decode("ascii", "replace").lower()] = value.strip().decode(
                "utf-8", "replace"
            )
    length_raw = headers.get("content-length")
    if length_raw is None:
        raise ToolError("invalid_response", "vphoned response had no Content-Length")
    try:
        length = int(length_raw)
    except ValueError as error:
        raise ToolError("invalid_response", "vphoned Content-Length was invalid") from error
    if length < 0 or length > MAX_RESPONSE_BYTES:
        raise ToolError(
            "invalid_response",
            f"vphoned response length {length} is outside the allowed range",
        )
    body = rest
    while len(body) < length:
        chunk = sock.recv(min(1 << 20, length - len(body)))
        if not chunk:
            break
        body += chunk
    if len(body) < length:
        raise ToolError(
            "invalid_response",
            f"vphoned response ended after {len(body)} of {length} bytes",
        )
    return HttpResponse(status=status, reason=reason, headers=headers, body=body[:length])


def _http_error(response: HttpResponse) -> ToolError:
    message = response.body[:1000].decode("utf-8", "replace")
    try:
        data = json.loads(response.body.decode("utf-8"))
        error = data.get("error")
        if isinstance(error, dict) and error.get("message"):
            message = str(error["message"])
            return ToolError(
                "guest_error",
                message,
                {"status": response.status, "guest_code": error.get("code")},
            )
        if isinstance(error, str):
            message = error
    except (UnicodeDecodeError, json.JSONDecodeError):
        pass
    return ToolError(
        "guest_http_error",
        f"vphoned returned HTTP {response.status}: {message}",
        {"status": response.status, "reason": response.reason},
    )
