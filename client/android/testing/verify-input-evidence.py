#!/usr/bin/env python3
"""Check the disposable GTK target's observed native effects, not send counts."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("log", type=Path)
args = parser.parse_args()
rows = [json.loads(line) for line in args.log.read_text().splitlines()]
checks = {
    "drag": any(row["event"] == "drag_end" and abs(row["dx"]) > 20 for row in rows),
    "secondary_click": any(row["event"] == "button_down" and row["button"] == 3 for row in rows),
    "scroll": any(row["event"] == "scroll" and row["dy"] != 0 for row in rows),
    "committed_text": any(row["event"] == "text" and row["value"] == "vista mvp verified 20261111" for row in rows),
}
keys = [row for row in rows if row["event"] == "key_up"]
checks["all_keys_released"] = bool(keys) and keys[-1]["held"] == []
held_buttons = set()
for row in rows:
    if row["event"] == "button_down":
        held_buttons.add(row["button"])
    elif row["event"] == "button_up":
        held_buttons.discard(row["button"])
checks["all_buttons_released"] = not held_buttons
shift_down = None
shift_releases = []
for row in rows:
    if row["event"] == "key_down" and row["key"] == 65505:
        shift_down = row["time"]
    elif row["event"] == "key_up" and row["key"] == 65505 and shift_down is not None:
        shift_releases.append(row["time"] - shift_down)
        shift_down = None
checks["physical_and_background_modifier_release"] = (
    len(shift_releases) >= 2 and shift_down is None and max(shift_releases) < 1.5
)
counts = {event: sum(row["event"] == event for row in rows) for event in {row["event"] for row in rows}}
print(json.dumps({"checks": checks, "event_counts": counts, "shift_release_seconds": shift_releases}, indent=2))
raise SystemExit(0 if all(checks.values()) else 1)
