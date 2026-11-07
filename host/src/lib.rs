//! Niri IPC-backed host discovery for the Vista streaming platform.
//!
//! Niri identifies windows but does not supply their pixels through window listing.
//! Capture and input are deliberately kept separate from compositor discovery.

pub mod capture;
pub mod catalog;
pub mod media;
pub mod portal;

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
