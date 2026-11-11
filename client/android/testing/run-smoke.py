#!/usr/bin/env python3
"""Opt-in emulator + real Niri host check. Temporarily focuses a disposable
window and injects test input; keep host focus unchanged until this run exits.
Build/install the app and androidTest APK and boot an emulator first.
"""
import argparse
import json
import os
import re
import signal
import subprocess
import tempfile
import time
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--niri-socket", required=True)
parser.add_argument("--wayland-display", default="wayland-1")
parser.add_argument("--adb", default=str(Path.home() / "Android/Sdk/platform-tools/adb"))
parser.add_argument("--playback-only", action="store_true", help="Debug video recovery without injecting host input")
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
output = Path(tempfile.mkdtemp(prefix="vista-android-smoke-"))
environment = dict(os.environ, NIRI_SOCKET=args.niri_socket, WAYLAND_DISPLAY=args.wayland_display)
server = None
target = None
test = None
host_output = (output / "host.jsonl").open("a", buffering=1)
host_error = (output / "host.log").open("a", buffering=1)

def adb(*arguments, check=True):
    return subprocess.run([args.adb, *arguments], check=check, capture_output=True, text=True)


def start_host(secret=None):
    command = [str(root / "target/debug/vista-host"), "serve", "--confirm", "--listen", "127.0.0.1:7443", "--allow-input", "--json"]
    if secret:
        command += ["--secret-hex", secret]
    return subprocess.Popen(command, env=environment, stdout=host_output, stderr=host_error)


def stop_host(process):
    if process is None:
        return
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    # The injector daemon is an owned child using this exact per-host socket.
    socket = f"{environment.get('XDG_RUNTIME_DIR', '/tmp')}/vista-ydotool-{process.pid}.sock"
    for path in Path('/proc').iterdir():
        if not path.name.isdigit():
            continue
        try:
            argv = (path / 'cmdline').read_bytes().split(b'\0')
            if argv[:3] == [b'ydotoold', b'--socket-path', socket.encode()]:
                os.kill(int(path.name), signal.SIGTERM)
        except (OSError, PermissionError):
            pass


try:
    target = subprocess.Popen(["python3", str(root / "client/android/testing/input-target.py"),
                               "--log", str(output / "input-events.jsonl")], env=environment,
                              stdout=subprocess.DEVNULL, stderr=(output / "target.log").open("w"))
    deadline = time.monotonic() + 10
    window = None
    while target.poll() is None and time.monotonic() < deadline:
        windows = json.loads(subprocess.check_output(["niri", "msg", "-j", "windows"], env=environment))
        window = next((item for item in windows if item.get("app_id") == "dev.vista.InputVerification"), None)
        if window:
            break
        time.sleep(0.1)
    assert window is not None, "Disposable target did not open"
    server = start_host()
    deadline = time.monotonic() + 10
    lines = []
    while server.poll() is None and time.monotonic() < deadline:
        lines = (output / "host.jsonl").read_text().splitlines()
        if lines:
            break
        time.sleep(0.1)
    assert lines, "Host did not publish an invitation"
    invite = json.loads(lines[0])
    assert invite["input"], "Host input backend is unavailable"
    subprocess.run(["niri", "msg", "action", "focus-window", "--id", str(window["id"])], env=environment, check=True, stdout=subprocess.DEVNULL)
    adb("push", str(output / "host.jsonl"), "/data/local/tmp/vista-smoke-invitation.json")
    adb("shell", "rm", "-r", "/sdcard/Android/data/dev.vista.client/files/verification", check=False)
    adb("shell", "run-as", "dev.vista.client", "rm", "-f", "files/restart-request")
    log_path = output / "instrumentation.log"
    with log_path.open("w") as log:
        test = subprocess.Popen([args.adb, "shell", "am", "instrument", "-w", "-e", "class", "vista.module.LiveHostSmokeTest",
                                 "-e", "live", "true", "-e", "invitation", "/data/local/tmp/vista-smoke-invitation.json",
                                 "-e", "window", str(window["id"]), "-e", "restart", "true",
                                 "-e", "input", str(not args.playback_only).lower(),
                                 "dev.vista.client.test/androidx.test.runner.AndroidJUnitRunner"], stdout=log, stderr=log)
        deadline = time.monotonic() + 120
        restarted = False
        while test.poll() is None and time.monotonic() < deadline:
            marker = adb("shell", "run-as", "dev.vista.client", "test", "-f", "files/restart-request", check=False)
            if marker.returncode == 0 and not restarted:
                stop_host(server)
                time.sleep(2)  # Exercise retries against a refused connection.
                server = start_host(invite["secret_hex"])
                restarted = True
            time.sleep(0.2)
        if test.poll() is None:
            test.terminate()
            raise AssertionError("Instrumented run exceeded 120 seconds")
    result = log_path.read_text()
    print(result, flush=True)
    assert re.search(r"OK \(2 tests\)", result), "Instrumented UI checks failed"
    assert restarted, "Host restart was not exercised"
    if not args.playback_only:
        subprocess.run(["python3", str(root / "client/android/testing/verify-input-evidence.py"),
                        str(output / "input-events.jsonl")], check=True)
    adb("pull", "/sdcard/Android/data/dev.vista.client/files/verification", str(output / "screenshots"))
    print("Playback, lifecycle, rotation, source switching and restart passed." if args.playback_only else
          "Real-host effects, lifecycle, rotation, source switching and restart passed.")
finally:
    if test is not None and test.poll() is None:
        test.terminate()
    stop_host(server)
    if target is not None and target.poll() is None:
        target.terminate()
        target.wait(timeout=5)
    adb("shell", "rm", "-f", "/data/local/tmp/vista-smoke-invitation.json", check=False)
    adb("shell", "run-as", "dev.vista.client", "rm", "-f", "files/restart-request", check=False)
    host_output.close()
    host_error.close()
    print(f"Private verification artifacts: {output}")
