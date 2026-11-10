//! Niri IPC-backed host discovery for the Vista streaming platform.
//!
//! Niri identifies windows but does not supply their pixels through window listing.
//! Capture and input are deliberately kept separate from compositor discovery.

pub mod bridge;
pub mod capability;
pub mod capture;
pub mod catalog;
pub mod catalog_wire;
pub mod control_dispatch;
pub mod control_lease;
pub mod decoder_sync;
pub mod desktop_probe;
pub mod host_service;
pub mod input_inject;
pub mod keyframe_control;
pub mod lifecycle;
pub mod live;
pub mod live_media;
pub mod media;
pub mod media_stream;
pub mod media_transport;
pub mod output_capture;
pub mod pipewire_loopback;
pub mod portal;
pub mod protocol;
pub mod session;
pub mod sync;
pub mod transport;

use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::io::{self, BufReader};
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum NiriError {
    Io(io::Error),
    Json(serde_json::Error),
    Unavailable(String),
    InvalidArgument(String),
}

impl fmt::Display for NiriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "invalid Niri response: {error}"),
            Self::Unavailable(reason) => write!(f, "Niri unavailable: {reason}"),
            Self::InvalidArgument(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for NiriError {}

impl From<io::Error> for NiriError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for NiriError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Stable, publicly useful fields of Niri's window listing.
/// Forward compatibility: additional Niri fields are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Window {
    pub id: u64,
    pub title: Option<String>,
    pub app_id: Option<String>,
    pub pid: Option<u32>,
    pub workspace_id: Option<u64>,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub is_floating: bool,
    #[serde(default)]
    pub is_urgent: bool,
}

/// Stable fields of Niri's workspace listing. A workspace is the compositor's
/// virtual desktop; `idx` is the user-facing reference accepted by
/// `niri msg action focus-workspace`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Workspace {
    pub id: u64,
    pub idx: u8,
    #[serde(default)]
    pub name: Option<String>,
    pub output: String,
    #[serde(default)]
    pub is_urgent: bool,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub active_window_id: Option<u64>,
}

/// Logical geometry of one connected output. Niri reports physical mode
/// dimensions separately; capture and pointer coordinates use logical pixels.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LogicalOutput {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    #[serde(default)]
    pub transform: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct OutputInfo {
    pub name: String,
    #[serde(default)]
    pub logical: Option<LogicalOutput>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    pub niri_binary: String,
    pub niri_socket: Option<String>,
    pub compositor_responding: bool,
    pub error: Option<String>,
}

/// Isolated seam for testing IPC without launching the compositor.
pub trait NiriTransport {
    fn request(&self, command: &str) -> Result<String, NiriError>;
}

#[derive(Default)]
pub struct CommandTransport;

impl NiriTransport for CommandTransport {
    fn request(&self, command: &str) -> Result<String, NiriError> {
        let output = Command::new("niri")
            .args(["msg", "--json", command])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NiriError::Unavailable(format!(
                "niri msg {command} failed ({}): {}",
                output.status,
                stderr.trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| NiriError::Unavailable(format!("non-UTF8 output: {error}")))
    }
}

pub struct Niri<T: NiriTransport = CommandTransport> {
    transport: T,
}

impl Default for Niri<CommandTransport> {
    fn default() -> Self {
        Self {
            transport: CommandTransport,
        }
    }
}

impl<T: NiriTransport> Niri<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    pub fn windows(&self) -> Result<Vec<Window>, NiriError> {
        let response = self.transport.request("windows")?;
        let mut windows: Vec<Window> = serde_json::from_str(&response)?;
        windows.sort_by_key(|window| window.id);
        Ok(windows)
    }

    pub fn window(&self, id: u64) -> Result<Option<Window>, NiriError> {
        Ok(self.windows()?.into_iter().find(|window| window.id == id))
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>, NiriError> {
        let response = self.transport.request("workspaces")?;
        let mut workspaces: Vec<Workspace> = serde_json::from_str(&response)?;
        workspaces.sort_by_key(|workspace| (workspace.output.clone(), workspace.idx));
        Ok(workspaces)
    }

    pub fn workspace(&self, id: u64) -> Result<Option<Workspace>, NiriError> {
        Ok(self
            .workspaces()?
            .into_iter()
            .find(|workspace| workspace.id == id))
    }

    /// Returns outputs in stable name order. Niri's JSON is an object keyed by
    /// connector name; disabled outputs have no logical geometry.
    pub fn outputs(&self) -> Result<Vec<OutputInfo>, NiriError> {
        let response = self.transport.request("outputs")?;
        let mut outputs: Vec<OutputInfo> =
            serde_json::from_str::<std::collections::BTreeMap<String, OutputInfo>>(&response)?
                .into_values()
                .collect();
        outputs.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(outputs)
    }

    /// The output that receives new windows. Falls back to the only enabled
    /// output when Niri does not report a focused one.
    pub fn focused_output(&self) -> Result<Option<OutputInfo>, NiriError> {
        let fallback = |niri: &Self| {
            Ok(niri
                .outputs()?
                .into_iter()
                .find(|output| output.logical.is_some()))
        };
        match self.transport.request("focused-output") {
            Ok(response) => match serde_json::from_str::<OutputInfo>(&response) {
                Ok(output) if output.logical.is_some() => Ok(Some(output)),
                _ => fallback(self),
            },
            Err(_) => fallback(self),
        }
    }

    pub fn doctor(&self) -> DoctorReport {
        let result = self.transport.request("windows").and_then(|response| {
            serde_json::from_str::<Vec<Window>>(&response).map_err(Into::into)
        });
        DoctorReport {
            niri_binary: "niri".into(),
            niri_socket: env::var("NIRI_SOCKET").ok(),
            compositor_responding: result.is_ok(),
            error: result.err().map(|error| error.to_string()),
        }
    }
}

/// Follow Niri's event stream and emit a resumable, snapshot-first window catalog.
/// This intentionally does not request any screen/window pixels.
pub fn watch_windows(json: bool) -> Result<(), NiriError> {
    let mut child = Command::new("niri")
        .args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        NiriError::Unavailable("Niri event stream stdout was not available".into())
    })?;
    let result = catalog::relay_events(BufReader::new(stdout), io::stdout().lock(), json);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    result?;
    if !status.success() {
        return Err(NiriError::Unavailable(format!(
            "Niri event stream terminated with {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(&'static str);
    impl NiriTransport for Fake {
        fn request(&self, command: &str) -> Result<String, NiriError> {
            assert_eq!(command, "windows");
            Ok(self.0.into())
        }
    }

    const SAMPLE: &str = r#"[
        {"id":9,"title":"Browser","app_id":"firefox","pid":100,"workspace_id":2,"is_focused":true,"new_field":"ignored"},
        {"id":2,"title":null,"app_id":null,"pid":null,"workspace_id":null}
    ]"#;

    #[test]
    fn discovers_windows_and_sorts_by_id() {
        let niri = Niri::with_transport(Fake(SAMPLE));
        let windows = niri.windows().unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, 2);
        assert!(!windows[0].is_focused);
        assert_eq!(windows[1].app_id.as_deref(), Some("firefox"));
        assert!(windows[1].is_focused);
    }

    #[test]
    fn searches_by_stable_window_id() {
        let niri = Niri::with_transport(Fake(SAMPLE));
        assert_eq!(
            niri.window(9).unwrap().unwrap().title.as_deref(),
            Some("Browser")
        );
        assert!(niri.window(5).unwrap().is_none());
    }

    #[test]
    fn diagnoses_malformed_ipc_response() {
        let niri = Niri::with_transport(Fake("invalid"));
        assert!(!niri.doctor().compositor_responding);
        assert!(matches!(niri.windows(), Err(NiriError::Json(_))));
    }
}
