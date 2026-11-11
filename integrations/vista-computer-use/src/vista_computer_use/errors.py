"""Error types shared by the MCP transport and the tool implementations."""

from __future__ import annotations

from typing import Any


class ToolError(Exception):
    """A bounded, user-facing tool failure.

    ``code`` is stable enough for clients and tests to rely on. ``details`` must
    stay JSON-serializable and must never contain passcodes or pairing secrets.
    """

    def __init__(
        self,
        code: str,
        message: str,
        details: dict[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}

    def as_dict(self) -> dict[str, Any]:
        data: dict[str, Any] = {"code": self.code, "message": self.message}
        if self.details:
            data["details"] = self.details
        return data
