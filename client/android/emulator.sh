#!/usr/bin/env bash
# Launch the VistaPixel Android emulator with settings that avoid the SystemUI
# ANRs seen with the stock AVD (2.5 GB RAM, software GPU, cold boot every time).
#
# Usage:
#   ./emulator.sh                 # windowed, host GPU, quickboot snapshot
#   ./emulator.sh --wait          # also wait for boot and disable animations
#   ./emulator.sh --headless      # no window; swiftshader (agent/CI use)
#   ./emulator.sh --cold          # ignore the quickboot snapshot
#   ./emulator.sh --stop          # stop the running emulator
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK="${ANDROID_HOME:-$HOME/Android/Sdk}"
EMULATOR="$SDK/emulator/emulator"
ADB="$SDK/platform-tools/adb"
AVD="${VISTA_AVD:-VistaPixel}"
AVD_DIR="$HOME/.android/avd/$AVD.avd"
CONFIG="$AVD_DIR/config.ini"
LOG="${VISTA_EMULATOR_LOG:-/tmp/vista-emulator.log}"

usage() {
    sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
}

die() {
    echo "emulator.sh: $*" >&2
    exit 1
}

MODE=windowed
WAIT=0
COLD=0
while [ $# -gt 0 ]; do
    case "$1" in
        --headless) MODE=headless ;;
        --wait) WAIT=1 ;;
        --cold) COLD=1 ;;
        --stop)
            "$ADB" emu kill 2>/dev/null || true
            exit 0
            ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown argument: $1 (try --help)" ;;
    esac
    shift
done

[ -x "$EMULATOR" ] || die "emulator not found at $EMULATOR (set ANDROID_HOME)"
[ -x "$ADB" ] || die "adb not found at $ADB"
if [ ! -f "$CONFIG" ]; then
    cat >&2 <<EOF
AVD '$AVD' does not exist. Create it once, then rerun:

  sdkmanager 'platform-tools' 'emulator' 'platforms;android-36' \\
    'build-tools;36.0.0' 'system-images;android-36;google_apis;x86_64'
  echo no | avdmanager create avd -n $AVD \\
    -k 'system-images;android-36;google_apis;x86_64' -d pixel_7
EOF
    exit 1
fi

# The stock AVD is 2.5 GB with the GPU disabled; the Google-APIs image then
# starves SystemUI and shows "System UI isn't responding" during cold boot.
# Apply the tuned values idempotently. -gpu host plus a real 4 GB guest is the
# difference between a 2-minute ANR-prone boot and a ~15-second clean one.
python3 - "$CONFIG" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
settings = {
    "hw.ramSize": "4096",
    "hw.cpu.ncore": "6",
    "vm.heapSize": "512",
    "hw.gpu.enabled": "yes",
    "hw.gpu.mode": "host",
    "hw.keyboard": "yes",
    "showDeviceFrame": "no",
    "disk.dataPartition.path": str(path.parent / "userdata-qemu.img"),
    "userdata.useQcow2": "yes",
    "fastboot.forceFastBoot": "yes",
    "fastboot.forceColdBoot": "no",
}
lines = path.read_text().splitlines()
seen = set()
out = []
for line in lines:
    key = line.split("=", 1)[0] if "=" in line else None
    if key in settings:
        out.append(f"{key}={settings[key]}")
        seen.add(key)
    else:
        out.append(line)
for key, value in settings.items():
    if key not in seen:
        out.append(f"{key}={value}")
path.write_text("\n".join(out) + "\n")
PY
# hardware-qemu.ini is regenerated from config.ini; a stale copy keeps the old
# RAM/GPU values even after config.ini changes.
rm -f "$AVD_DIR/hardware-qemu.ini"

# Emulator 37 ignores -scale; use the window's own zoom control instead.
args=(-avd "$AVD" -memory 4096 -cores 6 -no-audio -no-boot-anim)
if [ "$MODE" = headless ]; then
    # Host GL is not reliable without a window; swiftshader is slower but stable.
    args+=(-no-window -gpu swiftshader_indirect)
else
    # Wayland/XWayland values for the interactive desktop session.
    export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
    export DISPLAY="${DISPLAY:-:0}"
    export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
    args+=(-gpu host)
fi
[ "$COLD" = 1 ] && args+=(-no-snapshot-load)

nohup "$EMULATOR" "${args[@]}" >"$LOG" 2>&1 &
echo "emulator: $AVD ($MODE) pid $!; log $LOG"

if [ "$WAIT" = 1 ]; then
    "$ADB" wait-for-device
    for _ in $(seq 1 120); do
        [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ] && break
        sleep 1
    done
    # Remove the two things that make the emulator look "buggy" in normal use:
    # slow animations and blocking crash/ANR dialogs.
    "$ADB" shell settings put global window_animation_scale 0
    "$ADB" shell settings put global transition_animation_scale 0
    "$ADB" shell settings put global animator_duration_scale 0
    "$ADB" shell settings put global hide_error_dialogs 1
    "$ADB" shell settings put system screen_off_timeout 1800000
    echo "emulator: boot completed"
fi
