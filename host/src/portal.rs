//! Read-only xdg-desktop-portal ScreenCast capability inspection.
//!
//! A portal's advertised source types do not guarantee that Niri supports
//! capturing a particular window, or that consent has been granted. This
//! module never creates a ScreenCast session or requests any pixels.

use serde::Serialize;
use std::env;
use std::process::Command;

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.ScreenCast";

/// Bit flags returned by ScreenCast.AvailableSourceTypes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceTypes {
    pub raw: u32,
    pub monitor: bool,
    pub window: bool,
    pub virtual_monitor: bool,
    pub other_bits: u32,
}

impl From<u32> for SourceTypes {
    fn from(raw: u32) -> Self {
        Self {
            raw,
            monitor: raw & 1 != 0,
            window: raw & 2 != 0,
            virtual_monitor: raw & 4 != 0,
            other_bits: raw & !7,
        }
    }
}

/// Bit flags returned by ScreenCast.AvailableCursorModes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CursorModes {
    pub raw: u32,
    pub hidden: bool,
    pub embedded: bool,
    pub metadata: bool,
    pub other_bits: u32,
}

impl From<u32> for CursorModes {
    fn from(raw: u32) -> Self {
        Self {
            raw,
            hidden: raw & 1 != 0,
            embedded: raw & 2 != 0,
            metadata: raw & 4 != 0,
            other_bits: raw & !7,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenCastReport {
    /// At least one ScreenCast property was returned by the desktop portal.
    pub portal_responding: bool,
    pub interface_version: Option<u32>,
    pub source_types: Option<SourceTypes>,
    pub cursor_modes: Option<CursorModes>,
    pub errors: Vec<String>,
}

pub trait PortalProperties {
    fn get_unsigned(&self, property: &str) -> Result<u32, String>;
}

/// Uses only busctl get-property; never invokes ScreenCast methods.
pub struct BusctlPortal {
    address: String,
}

impl BusctlPortal {
    pub fn from_environment() -> Result<Self, String> {
        let address = env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                env::var("XDG_RUNTIME_DIR")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|dir| format!("unix:path={dir}/bus"))
            })
            .ok_or_else(|| {
                "no session D-Bus address (set DBUS_SESSION_BUS_ADDRESS or XDG_RUNTIME_DIR)"
                    .to_string()
            })?;
        Ok(Self { address })
    }
}

impl PortalProperties for BusctlPortal {
    fn get_unsigned(&self, property: &str) -> Result<u32, String> {
        let output = Command::new("busctl")
            .arg(format!("--address={}", self.address))
            .arg("--timeout=2s")
            .args([
                "get-property",
                DESTINATION,
                OBJECT_PATH,
                INTERFACE,
                property,
            ])
            .output()
            .map_err(|err| format!("busctl unavailable: {err}"))?;
        if !output.status.success() {
            return Err(format!(
                "busctl failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let output = std::str::from_utf8(&output.stdout)
            .map_err(|err| format!("non-UTF8 busctl property: {err}"))?;
        parse_unsigned(output)
    }
}

fn parse_unsigned(output: &str) -> Result<u32, String> {
    // busctl get-property emits `u 7` for a D-Bus UINT32 value.
    let parts: Vec<&str> = output.split_whitespace().collect();
    match parts.as_slice() {
        ["u", value] => value
            .parse::<u32>()
            .map_err(|err| format!("invalid D-Bus UINT32 '{value}': {err}")),
        _ => Err(format!("unexpected D-Bus UINT32 reply: {output:?}")),
    }
}

/// Query properties individually to preserve partial results and errors.
pub fn probe_with(transport: &impl PortalProperties) -> ScreenCastReport {
    let mut errors = Vec::new();
    let mut get = |property: &str| match transport.get_unsigned(property) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{property}: {error}"));
            None
        }
    };
    let source_types = get("AvailableSourceTypes").map(SourceTypes::from);
    let cursor_modes = get("AvailableCursorModes").map(CursorModes::from);
    let interface_version = get("version");
    let portal_responding =
        source_types.is_some() || cursor_modes.is_some() || interface_version.is_some();

    ScreenCastReport {
        portal_responding,
        interface_version,
        source_types,
        cursor_modes,
        errors,
    }
}

pub fn probe() -> ScreenCastReport {
    match BusctlPortal::from_environment() {
        Ok(client) => probe_with(&client),
        Err(error) => ScreenCastReport {
            portal_responding: false,
            interface_version: None,
            source_types: None,
            cursor_modes: None,
            errors: vec![error],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePortal {
        source: Result<u32, &'static str>,
        cursor: Result<u32, &'static str>,
        version: Result<u32, &'static str>,
    }

    impl PortalProperties for FakePortal {
        fn get_unsigned(&self, property: &str) -> Result<u32, String> {
            let result = match property {
                "AvailableSourceTypes" => &self.source,
                "AvailableCursorModes" => &self.cursor,
                "version" => &self.version,
                _ => panic!("unexpected property: {property}"),
            };
            (*result).map_err(ToString::to_string)
        }
    }

    #[test]
    fn decodes_portal_flags_without_inferring_capture_permissions() {
        let report = probe_with(&FakePortal {
            source: Ok(7),
            cursor: Ok(5),
            version: Ok(5),
        });
        assert!(report.portal_responding);
        let source = report.source_types.unwrap();
        assert!(source.monitor && source.window && source.virtual_monitor);
        assert_eq!(source.other_bits, 0);
        let cursor = report.cursor_modes.unwrap();
        assert!(cursor.hidden && cursor.metadata && !cursor.embedded);
        assert_eq!(report.interface_version, Some(5));
        assert!(report.errors.is_empty());
    }

    #[test]
    fn preserves_unrecognized_future_flag_bits() {
        let flags = SourceTypes::from(0x81);
        assert!(flags.monitor);
        assert!(!flags.window);
        assert_eq!(flags.other_bits, 0x80);
    }

    #[test]
    fn reports_partial_portal_failures_without_claiming_unavailability() {
        let report = probe_with(&FakePortal {
            source: Ok(2),
            cursor: Err("not supported"),
            version: Err("timeout"),
        });
        assert!(report.portal_responding);
        assert!(report.source_types.unwrap().window);
        assert!(report.cursor_modes.is_none());
        assert_eq!(report.errors.len(), 2);
    }

    #[test]
    fn reports_an_unreachable_portal() {
        let report = probe_with(&FakePortal {
            source: Err("unreachable"),
            cursor: Err("unreachable"),
            version: Err("unreachable"),
        });
        assert!(!report.portal_responding);
        assert!(report.source_types.is_none());
    }

    #[test]
    fn parses_exact_busctl_uint32_reply() {
        assert_eq!(parse_unsigned("u 7\n").unwrap(), 7);
        assert!(parse_unsigned("i 7").is_err());
        assert!(parse_unsigned("u -1").is_err());
        assert!(parse_unsigned("u 4294967296").is_err());
        assert!(parse_unsigned("u 7 more").is_err());
    }
}
