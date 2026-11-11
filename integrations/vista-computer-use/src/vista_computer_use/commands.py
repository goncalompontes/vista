"""Pure command builders.

Every external program is invoked with an argv list, never through a shell.
Keeping the builders pure makes request validation and command construction
testable without a running VM, host service, or QMP socket.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Sequence

# qmp.py maps absolute pointer coordinates onto its fixed 1290x2796 display
# grid. The VM may run at another resolution, so callers pre-scale points into
# this grid; qmp.py then normalizes to QEMU's 0..32767 axis exactly as before.
QMP_GRID_WIDTH = 1290
QMP_GRID_HEIGHT = 2796


def python_script_argv(
    python: str,
    script: Path,
    args: Sequence[str],
) -> list[str]:
    return [python, str(script), *map(str, args)]


def qmp_argv(
    python: str,
    qmp_script: Path,
    socket_path: Path,
    operation: str,
    args: Sequence[object] = (),
) -> list[str]:
    return [
        python,
        str(qmp_script),
        str(socket_path),
        operation,
        *[str(arg) for arg in args],
    ]


def format_coord(value: float) -> str:
    """Stable decimal formatting for qmp.py's float parser."""
    return f"{float(value):.4f}".rstrip("0").rstrip(".") or "0"


def point_to_qmp_grid(x: float, y: float, width: int, height: int) -> tuple[float, float]:
    if width <= 0 or height <= 0:
        raise ValueError("screen width and height must be positive")
    return (
        float(x) * QMP_GRID_WIDTH / float(width),
        float(y) * QMP_GRID_HEIGHT / float(height),
    )


def qmp_tap_argv(
    python: str,
    qmp_script: Path,
    socket_path: Path,
    x: float,
    y: float,
    screen_width: int,
    screen_height: int,
) -> list[str]:
    gx, gy = point_to_qmp_grid(x, y, screen_width, screen_height)
    return qmp_argv(
        python,
        qmp_script,
        socket_path,
        "tap",
        (format_coord(gx), format_coord(gy)),
    )


def qmp_swipe_argv(
    python: str,
    qmp_script: Path,
    socket_path: Path,
    x1: float,
    y1: float,
    x2: float,
    y2: float,
    screen_width: int,
    screen_height: int,
    duration_ms: int,
    steps: int,
) -> list[str]:
    gx1, gy1 = point_to_qmp_grid(x1, y1, screen_width, screen_height)
    gx2, gy2 = point_to_qmp_grid(x2, y2, screen_width, screen_height)
    return qmp_argv(
        python,
        qmp_script,
        socket_path,
        "swipe",
        (
            format_coord(gx1),
            format_coord(gy1),
            format_coord(gx2),
            format_coord(gy2),
            str(duration_ms),
            str(steps),
        ),
    )


def qmp_key_argv(
    python: str,
    qmp_script: Path,
    socket_path: Path,
    qcodes: Sequence[str],
) -> list[str]:
    return qmp_argv(python, qmp_script, socket_path, "key", qcodes)


def qmp_home_argv(python: str, qmp_script: Path, socket_path: Path) -> list[str]:
    return qmp_argv(python, qmp_script, socket_path, "home")


def qmp_screenshot_argv(
    python: str,
    qmp_script: Path,
    socket_path: Path,
    output_path: Path,
) -> list[str]:
    return qmp_argv(python, qmp_script, socket_path, "shot", (str(output_path),))


def install_ipa_argv(
    python: str,
    install_script: Path,
    ipa_path: Path,
    cid: int,
    launch_bundle_id: str | None,
) -> list[str]:
    args = [str(ipa_path), "--cid", str(cid)]
    if launch_bundle_id:
        args.extend(["--launch", launch_bundle_id])
    return python_script_argv(python, install_script, args)


def app_logs_argv(
    python: str,
    app_logs_script: Path,
    process: str,
    cid: int,
    seconds: int,
    subsystem: str | None,
    json_lines: bool = True,
) -> list[str]:
    args = [process, "--seconds", str(seconds), "--cid", str(cid)]
    if subsystem:
        args.extend(["--subsystem", subsystem])
    if json_lines:
        args.append("--json")
    return python_script_argv(python, app_logs_script, args)


def start_vm_argv(
    bash: str,
    start_script: Path,
    name: str | None,
    cid: int | None,
    headless: bool,
    unlock: bool,
) -> list[str]:
    argv = [bash, str(start_script)]
    if cid is not None:
        argv.extend(["--cid", str(cid)])
    if headless:
        argv.append("--no-window")
    if not unlock:
        argv.append("--no-unlock")
    if name:
        argv.append(name)
    return argv


def stop_vm_argv(bash: str, start_script: Path, name: str) -> list[str]:
    return [bash, str(start_script), "--stop", name]


def host_serve_argv(
    host_bin: Path,
    listen: str,
    fps: int,
    allow_remote: bool,
    allow_window_actions: bool,
    allow_desktop_actions: bool,
    allow_clipboard: bool,
    allow_input: bool,
    once: bool,
    secret_hex: str | None,
) -> list[str]:
    argv = [
        str(host_bin),
        "serve",
        "--confirm",
        "--json",
        "--listen",
        listen,
        "--fps",
        str(fps),
    ]
    if allow_remote:
        argv.append("--allow-remote")
    if allow_window_actions:
        argv.append("--allow-window-actions")
    if allow_desktop_actions:
        argv.append("--allow-desktop-actions")
    if allow_clipboard:
        argv.append("--allow-clipboard")
    if allow_input:
        argv.append("--allow-input")
    if once:
        argv.append("--once")
    if secret_hex:
        argv.extend(["--secret-hex", secret_hex])
    return argv


def host_windows_argv(host_bin: Path) -> list[str]:
    return [str(host_bin), "windows", "--json"]


def host_live_client_argv(
    host_bin: Path,
    address: str,
    secret_hex: str,
    target: str,
    frames: int,
    h264_path: Path | None,
    pointer: str | None,
    text: str | None,
    allow_input: bool,
) -> list[str]:
    argv = [
        str(host_bin),
        "live-client",
        "--connect",
        address,
        "--secret-hex",
        secret_hex,
        "--target",
        target,
        "--frames",
        str(frames),
        "--json",
    ]
    if h264_path is not None:
        argv.extend(["--h264", str(h264_path)])
    if pointer:
        argv.extend(["--pointer", pointer])
    if text:
        argv.extend(["--text", text])
    if allow_input:
        argv.append("--allow-input")
    return argv


def hmp_hostfwd_add(netdev: str, host_port: int, guest_port: int) -> str:
    return f"hostfwd_add {netdev} tcp:127.0.0.1:{host_port}-:{guest_port}"


def hmp_hostfwd_remove(netdev: str, host_port: int) -> str:
    return f"hostfwd_remove {netdev} tcp:127.0.0.1:{host_port}"


def default_python() -> str:
    return sys.executable or "python3"
