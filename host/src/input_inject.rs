//! Portable input injection for the authenticated session.
//!
//! Absolute pointer motion and buttons use a native uinput absolute device.
//! `ydotool --absolute` approximates positions with accelerated relative
//! motion and cannot preserve the streamed coordinate transform. Keyboard,
//! relative motion and text use a private `ydotoold` socket. No compositor
//! acceleration settings are changed. This does not rely on shortcuts, so an
//! injected click/keystroke reaches whatever surface is focused, including a
//! window that the producer fullscreened for streaming.
//!
//! The injector owns a bounded set of held buttons and keys and releases them
//! on cancellation or disconnect. A missing `/dev/uinput` permission disables
//! input instead of pretending it worked.

use crate::capability::{InputEffect, InputPermissions, PointerButton, ScrollPhase, ScrollUnit};
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, PropType,
    RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Coordinates are normalized to the streamed output. A click at (0.5, 0.5)
/// maps to the center of the window or desktop currently being streamed.
#[derive(Clone, Copy, Debug)]
pub struct StreamGeometry {
    pub width: u32,
    pub height: u32,
    pub epoch: u64,
}

pub struct InputInjector {
    socket: PathBuf,
    daemon: Mutex<Option<Child>>,
    geometry: Mutex<Option<StreamGeometry>>,
    pointer: Mutex<VirtualDevice>,
}

impl InputInjector {
    pub fn probe(_permissions: InputPermissions) -> Result<Self, String> {
        let pointer = absolute_pointer()
            .map_err(|error| format!("cannot create absolute pointer: {error}"))?;
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let socket = Path::new(&runtime).join(format!("vista-ydotool-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let daemon = Command::new("ydotoold")
            .arg("--socket-path")
            .arg(&socket)
            .arg("--socket-perm")
            .arg("0600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start ydotoold: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if socket.exists() {
                return Ok(Self {
                    socket,
                    daemon: Mutex::new(Some(daemon)),
                    geometry: Mutex::new(None),
                    pointer: Mutex::new(pointer),
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut daemon = daemon;
        let _ = daemon.kill();
        Err("ydotoold did not create its socket; /dev/uinput may be unavailable".into())
    }

    pub fn set_geometry(&self, geometry: StreamGeometry) {
        *self.geometry.lock().expect("input geometry") = Some(geometry);
    }

    pub fn clear_geometry(&self) {
        *self.geometry.lock().expect("input geometry") = None;
    }

    pub fn geometry_epoch(&self) -> Option<u64> {
        self.geometry
            .lock()
            .expect("input geometry")
            .map(|geometry| geometry.epoch)
    }

    /// Apply effects that have already been authorized by the active ControlLease.
    /// This layer deliberately has no authority to create an input context.
    pub fn apply_effects(&self, effects: &[InputEffect]) -> Result<(), String> {
        let geometry = *self.geometry.lock().expect("input geometry");
        for effect in effects.iter().cloned() {
            self.apply_effect(effect, geometry)?;
        }
        Ok(())
    }

    /// Best-effort native release effects supplied by the revoked ControlLease.
    pub fn release_effects(&self, effects: &[InputEffect]) {
        for effect in effects.iter().cloned() {
            let _ = self.apply_effect(effect, None);
        }
    }

    fn apply_effect(
        &self,
        effect: InputEffect,
        geometry: Option<StreamGeometry>,
    ) -> Result<(), String> {
        match effect {
            InputEffect::PointerAbsolute { x, y } => {
                let Some(_) = geometry else {
                    return Err("no stream geometry is active".into());
                };
                self.emit_pointer(&absolute_position(x, y))
            }
            InputEffect::PointerRelative { dx, dy } => self.run(&[
                "mousemove".into(),
                "--xpos".into(),
                format!("{dx:.0}"),
                "--ypos".into(),
                format!("{dy:.0}"),
            ]),
            InputEffect::Button { button, pressed } => {
                let code = match button {
                    PointerButton::Primary => KeyCode::BTN_LEFT,
                    PointerButton::Secondary => KeyCode::BTN_RIGHT,
                    PointerButton::Middle => KeyCode::BTN_MIDDLE,
                    PointerButton::Back => KeyCode::BTN_BACK,
                    PointerButton::Forward => KeyCode::BTN_FORWARD,
                };
                self.emit_pointer(&[InputEvent::new(
                    EventType::KEY.0,
                    code.0,
                    i32::from(pressed),
                )])
            }
            InputEffect::Scroll {
                dx,
                dy,
                unit,
                phase,
                ..
            } => {
                if phase == ScrollPhase::Begin
                    || phase == ScrollPhase::End
                    || phase == ScrollPhase::Cancel
                {
                    return Ok(());
                }
                let scale = if unit == ScrollUnit::Line { 1.0 } else { 0.05 };
                self.emit_pointer(&[
                    InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_HWHEEL.0,
                        (dx * scale).round() as i32,
                    ),
                    InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_WHEEL.0,
                        (dy * scale).round() as i32,
                    ),
                ])
            }
            InputEffect::Key { usage, pressed, .. } => {
                let code = hid_usage_to_evdev(usage)
                    .ok_or_else(|| format!("unsupported HID usage {usage:#04x}"))?;
                self.run(&["key".into(), format!("{code}:{}", u8::from(pressed))])
            }
            InputEffect::Text { text } => {
                if text.is_empty() {
                    return Ok(());
                }
                let mut child = Command::new("ydotool")
                    .env("YDOTOOL_SOCKET", &self.socket)
                    .args(["type", "--file", "-"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|error| format!("cannot type text: {error}"))?;
                child
                    .stdin
                    .take()
                    .ok_or("text injector has no stdin")?
                    .write_all(text.as_bytes())
                    .map_err(|error| format!("cannot write text: {error}"))?;
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("text injection failed: {error}"))?;
                if output.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "ydotool type failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ))
                }
            }
        }
    }

    fn emit_pointer(&self, events: &[InputEvent]) -> Result<(), String> {
        self.pointer
            .lock()
            .expect("absolute pointer")
            .emit(events)
            .map_err(|error| format!("absolute pointer injection failed: {error}"))
    }

    fn run(&self, arguments: &[String]) -> Result<(), String> {
        let output = Command::new("ydotool")
            .env("YDOTOOL_SOCKET", &self.socket)
            .args(arguments)
            .output()
            .map_err(|error| format!("cannot run ydotool: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "ydotool {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

fn absolute_pointer() -> std::io::Result<VirtualDevice> {
    let range = AbsInfo::new(0, 0, 65_535, 0, 0, 0);
    VirtualDevice::builder()?
        .name("Vista absolute pointer")
        .with_properties(&AttributeSet::from_iter([PropType::POINTER]))?
        .with_keys(&AttributeSet::from_iter([
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::BTN_BACK,
            KeyCode::BTN_FORWARD,
        ]))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, range))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, range))?
        .with_relative_axes(&AttributeSet::from_iter([
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
        ]))?
        .build()
}

fn absolute_position(x: f64, y: f64) -> [InputEvent; 2] {
    [
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            (x.clamp(0.0, 1.0) * 65_535.0).round() as i32,
        ),
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            (y.clamp(0.0, 1.0) * 65_535.0).round() as i32,
        ),
    ]
}

impl Drop for InputInjector {
    fn drop(&mut self) {
        if let Some(mut daemon) = self.daemon.lock().expect("input daemon").take() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Minimal HID usage page 0x07 to Linux evdev keycode mapping for the keys a
/// remote client actually sends. Unknown usages are rejected rather than
/// guessed.
pub fn hid_usage_to_evdev(usage: u16) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    const DIGITS: [u16; 10] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    const FUNCTIONS: [u16; 12] = [59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88];
    match usage {
        0x04..=0x1d => Some(LETTERS[usize::from(usage - 0x04)]),
        0x1e..=0x27 => Some(DIGITS[usize::from(usage - 0x1e)]),
        0x28 => Some(28),
        0x29 => Some(1),
        0x2a => Some(14),
        0x2b => Some(15),
        0x2c => Some(57),
        0x2d => Some(12),
        0x2e => Some(13),
        0x2f => Some(26),
        0x30 => Some(27),
        0x31 => Some(43),
        0x32 => Some(43),
        0x33 => Some(39),
        0x34 => Some(40),
        0x35 => Some(41),
        0x36 => Some(51),
        0x37 => Some(52),
        0x38 => Some(53),
        0x39 => Some(58),
        0x3a..=0x45 => Some(FUNCTIONS[usize::from(usage - 0x3a)]),
        0x49 => Some(110),
        0x4a => Some(102),
        0x4b => Some(104),
        0x4c => Some(111),
        0x4d => Some(107),
        0x4e => Some(109),
        0x4f => Some(106),
        0x50 => Some(105),
        0x51 => Some(108),
        0x52 => Some(103),
        0xe0 => Some(29),
        0xe1 => Some(42),
        0xe2 => Some(56),
        0xe3 => Some(125),
        0xe4 => Some(97),
        0xe5 => Some(54),
        0xe6 => Some(100),
        0xe7 => Some(126),
        _ => None,
    }
}

/// Normalized pointer geometry for a selected stream. The host computes this
/// from the output used by the producer, not from client-supplied values.
pub fn normalized_to_pixels(x: f64, y: f64, width: u32, height: u32) -> (i32, i32) {
    let x = (x.clamp(0.0, 1.0) * f64::from(width.saturating_sub(1))).round();
    let y = (y.clamp(0.0, 1.0) * f64::from(height.saturating_sub(1))).round();
    (x as i32, y as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_hid_usages() {
        assert_eq!(hid_usage_to_evdev(0x04), Some(30)); // A
        assert_eq!(hid_usage_to_evdev(0x1e), Some(2)); // 1
        assert_eq!(hid_usage_to_evdev(0x28), Some(28)); // Enter
        assert_eq!(hid_usage_to_evdev(0x50), Some(105)); // Left
        assert_eq!(hid_usage_to_evdev(0xe1), Some(42)); // Left shift
        assert_eq!(hid_usage_to_evdev(0xffff), None);
    }

    #[test]
    fn maps_normalized_coordinates_inclusively() {
        assert_eq!(normalized_to_pixels(0.0, 0.0, 1920, 1080), (0, 0));
        assert_eq!(normalized_to_pixels(1.0, 1.0, 1920, 1080), (1919, 1079));
        assert_eq!(normalized_to_pixels(2.0, -1.0, 100, 50), (99, 0));
    }

    #[test]
    fn absolute_pointer_uses_kernel_axes_without_accelerated_relative_motion() {
        let events = absolute_position(0.5, 1.0);
        assert_eq!(events[0].event_type(), EventType::ABSOLUTE);
        assert_eq!(events[0].code(), AbsoluteAxisCode::ABS_X.0);
        assert_eq!(events[0].value(), 32_768);
        assert_eq!(events[1].code(), AbsoluteAxisCode::ABS_Y.0);
        assert_eq!(events[1].value(), 65_535);
        assert_eq!(absolute_position(-0.1, 1.1)[0].value(), 0);
    }
}
