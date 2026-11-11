#!/usr/bin/env python3
"""Disposable GTK target for opt-in Android input verification.

Run on the host with --log <private temporary file>. The log contains only
this test window's input, including intentionally typed test text. No desktop
screenshots, clipboard data or other applications are read.
"""
import argparse
import json
import signal
import time
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk

parser = argparse.ArgumentParser()
parser.add_argument("--log", required=True, type=Path)
args = parser.parse_args()


def report(event, **data):
    with args.log.open("a") as output:
        output.write(json.dumps({"event": event, "time": time.monotonic(), **data}) + "\n")


class Target(Gtk.Application):
    def __init__(self):
        super().__init__(application_id="dev.vista.InputVerification")
        self.connect("activate", self.activate)
        self.held = set()

    def activate(self, app):
        window = Gtk.ApplicationWindow(application=app, title="Vista input verification")
        window.set_default_size(1000, 700)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        box.set_margin_top(24)
        box.set_margin_bottom(24)
        box.set_margin_start(24)
        box.set_margin_end(24)
        window.set_child(box)
        heading = Gtk.Label(label="Vista · input verification")
        heading.add_css_class("title-1")
        box.append(heading)
        entry = Gtk.Entry(placeholder_text="Committed test text appears here")
        entry.set_size_request(-1, 70)
        entry.connect("changed", lambda field: report("text", value=field.get_text()))
        box.append(entry)
        box.append(Gtk.Label(label="Drag, scroll and secondary-click below. The keyboard stays in the test field."))
        pad = Gtk.DrawingArea()
        pad.set_vexpand(True)
        pad.set_halign(Gtk.Align.FILL)
        pad.set_can_target(True)
        pad.set_focusable(True)
        def draw(area, canvas, width, height):
            canvas.set_source_rgb(0.12, 0.18, 0.28)
            canvas.paint()
        pad.set_draw_func(draw)
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", lambda obj, x, y: report("motion", x=x, y=y))
        pad.add_controller(motion)
        box.append(pad)
        click = Gtk.GestureClick()
        click.set_button(0)
        click.connect("pressed", lambda obj, count, x, y: report("button_down", button=obj.get_current_button(), x=x, y=y))
        click.connect("released", lambda obj, count, x, y: report("button_up", button=obj.get_current_button(), x=x, y=y))
        pad.add_controller(click)
        drag = Gtk.GestureDrag()
        drag.connect("drag-begin", lambda obj, x, y: report("drag_begin", x=x, y=y))
        drag.connect("drag-end", lambda obj, x, y: report("drag_end", dx=x, dy=y))
        pad.add_controller(drag)
        scroll = Gtk.EventControllerScroll.new(Gtk.EventControllerScrollFlags.BOTH_AXES)
        scroll.connect("scroll", lambda obj, x, y: report("scroll", dx=x, dy=y) or True)
        pad.add_controller(scroll)
        keys = Gtk.EventControllerKey()
        keys.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
        def key_down(obj, keyval, keycode, state):
            self.held.add(keyval)
            report("key_down", key=keyval, held=sorted(self.held))
            return False
        def key_up(obj, keyval, keycode, state):
            self.held.discard(keyval)
            report("key_up", key=keyval, held=sorted(self.held))
        keys.connect("key-pressed", key_down)
        keys.connect("key-released", key_up)
        window.add_controller(keys)
        window.present()
        GLib.idle_add(lambda: (entry.grab_focus(), False)[1])
        report("ready")


app = Target()
signal.signal(signal.SIGTERM, lambda *_: app.quit())
app.run([])
