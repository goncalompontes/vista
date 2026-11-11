"""Subprocess execution with bounded time and no shell."""

from __future__ import annotations

import os
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence


@dataclass(frozen=True)
class CommandResult:
    argv: tuple[str, ...]
    returncode: int
    stdout: bytes
    stderr: bytes
    duration_ms: int
    timed_out: bool = False

    @property
    def stdout_text(self) -> str:
        return self.stdout.decode("utf-8", "replace")

    @property
    def stderr_text(self) -> str:
        return self.stderr.decode("utf-8", "replace")

    def summary(self) -> dict[str, object]:
        return {
            "argv": list(self.argv),
            "returncode": self.returncode,
            "timed_out": self.timed_out,
            "duration_ms": self.duration_ms,
            "stdout": self.stdout_text[-4000:],
            "stderr": self.stderr_text[-4000:],
        }


class Runner:
    """Seam for tests and for callers that need deterministic commands."""

    def run(
        self,
        argv: Sequence[str],
        *,
        timeout: float,
        env: Mapping[str, str] | None = None,
        cwd: Path | None = None,
        stdin: bytes | None = None,
    ) -> CommandResult:
        raise NotImplementedError


class SubprocessRunner(Runner):
    def run(
        self,
        argv: Sequence[str],
        *,
        timeout: float,
        env: Mapping[str, str] | None = None,
        cwd: Path | None = None,
        stdin: bytes | None = None,
    ) -> CommandResult:
        argv_tuple = tuple(map(str, argv))
        merged_env = dict(os.environ)
        if env:
            merged_env.update({str(key): str(value) for key, value in env.items()})
        started = time.monotonic()
        try:
            completed = subprocess.run(
                argv_tuple,
                input=stdin,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout,
                env=merged_env,
                cwd=str(cwd) if cwd is not None else None,
                check=False,
            )
            elapsed = int((time.monotonic() - started) * 1000)
            return CommandResult(
                argv=argv_tuple,
                returncode=completed.returncode,
                stdout=completed.stdout or b"",
                stderr=completed.stderr or b"",
                duration_ms=elapsed,
            )
        except subprocess.TimeoutExpired as error:
            elapsed = int((time.monotonic() - started) * 1000)
            return CommandResult(
                argv=argv_tuple,
                returncode=124,
                stdout=error.stdout or b"",
                stderr=error.stderr or b"",
                duration_ms=elapsed,
                timed_out=True,
            )
        except FileNotFoundError as error:
            elapsed = int((time.monotonic() - started) * 1000)
            return CommandResult(
                argv=argv_tuple,
                returncode=127,
                stdout=b"",
                stderr=str(error).encode("utf-8", "replace"),
                duration_ms=elapsed,
            )
