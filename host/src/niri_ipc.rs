//! Minimal, independently-defined Niri JSON IPC transport.
//!
//! Keep this wire model local to Vista: do not link the GPL-licensed
//! `niri-ipc` crate. Niri's public protocol permits additive fields/variants,
//! so response payloads are retained as JSON until decoded by a provider.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

const MAX_REPLY_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("NIRI_SOCKET is not set")]
    MissingSocket,
    #[error("cannot connect to Niri socket {path}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Niri IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Niri returned an error: {0}")]
    Remote(String),
    #[error("invalid Niri IPC JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Niri IPC reply exceeded {MAX_REPLY_BYTES} bytes")]
    ReplyTooLarge,
    #[error("Niri IPC connection closed without a reply")]
    EmptyReply,
}

#[derive(Clone, Debug)]
pub struct NiriIpc {
    socket: PathBuf,
    timeout: Duration,
}

impl NiriIpc {
    pub fn from_env() -> Result<Self, IpcError> {
        let socket = env::var_os("NIRI_SOCKET").ok_or(IpcError::MissingSocket)?;
        Ok(Self::new(PathBuf::from(socket)))
    }

    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            timeout: Duration::from_secs(3),
        }
    }

    fn connect(&self) -> Result<UnixStream, IpcError> {
        let stream = UnixStream::connect(&self.socket).map_err(|source| IpcError::Connect {
            path: self.socket.clone(),
            source,
        })?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        Ok(stream)
    }

    /// Send one typed protocol request and unwrap Niri's Ok/Err envelope.
    pub fn request_value(&self, request: &Value) -> Result<Value, IpcError> {
        let mut stream = self.connect()?;
        serde_json::to_writer(&mut stream, request)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let value = read_json_line(&mut BufReader::new(stream))?;
        unwrap_reply(value)
    }

    pub fn request<T: DeserializeOwned>(&self, request: &Value) -> Result<T, IpcError> {
        Ok(serde_json::from_value(self.request_value(request)?)?)
    }

    /// Open a dedicated event-stream socket. It must not be shared with
    /// request/response traffic because Niri streams indefinitely on it.
    pub fn event_stream(&self) -> Result<BufReader<UnixStream>, IpcError> {
        let mut stream = self.connect()?;
        stream.write_all(b"\"EventStream\"\n")?;
        stream.flush()?;
        // Event streams are intentionally long-lived; the request timeout must
        // not turn an idle compositor into a false disconnect.
        stream.set_read_timeout(None)?;
        Ok(BufReader::new(stream))
    }
}

fn read_json_line(reader: &mut impl BufRead) -> Result<Value, IpcError> {
    let mut bytes = Vec::new();
    let count = reader
        .take(MAX_REPLY_BYTES + 1)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > MAX_REPLY_BYTES {
        return Err(IpcError::ReplyTooLarge);
    }
    if count == 0 {
        return Err(IpcError::EmptyReply);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn unwrap_reply(value: Value) -> Result<Value, IpcError> {
    let object = value
        .as_object()
        .ok_or_else(|| IpcError::Remote("reply envelope is not an object".into()))?;
    if let Some(ok) = object.get("Ok") {
        return Ok(ok.clone());
    }
    if let Some(err) = object.get("Err") {
        return Err(IpcError::Remote(err.to_string()));
    }
    Err(IpcError::Remote("reply omitted Ok/Err envelope".into()))
}

/// The supported query request variants used by Vista's host catalog.
#[derive(Clone, Copy, Debug)]
pub enum Query {
    Windows,
    Workspaces,
    Outputs,
    FocusedOutput,
}

impl Query {
    pub fn wire(self) -> Value {
        match self {
            Self::Windows => json!("Windows"),
            Self::Workspaces => json!("Workspaces"),
            Self::Outputs => json!("Outputs"),
            Self::FocusedOutput => json!("FocusedOutput"),
        }
    }
}

/// Allowlisted action requests. No client-provided JSON or command string is
/// accepted at this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    FocusWindow { id: u64 },
    CloseWindow { id: u64 },
    FocusWorkspaceIndex { index: u8 },
    FocusWorkspaceId { id: u64 },
    FocusWorkspaceUp,
    FocusWorkspaceDown,
    FullscreenWindow,
}

impl Action {
    pub fn wire(&self) -> Value {
        let action = match self {
            Self::FocusWindow { id } => json!({"FocusWindow": {"id": id}}),
            Self::CloseWindow { id } => json!({"CloseWindow": {"id": id}}),
            Self::FocusWorkspaceIndex { index } => {
                json!({"FocusWorkspace": {"reference": {"Index": index}}})
            }
            Self::FocusWorkspaceId { id } => json!({"FocusWorkspace": {"reference": {"Id": id}}}),
            Self::FocusWorkspaceUp => json!("FocusWorkspaceUp"),
            Self::FocusWorkspaceDown => json!("FocusWorkspaceDown"),
            Self::FullscreenWindow => json!("FullscreenWindow"),
        };
        json!({"Action": action})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread};

    #[test]
    fn unwraps_ok_and_err_envelopes_and_preserves_unknown_payload_fields() {
        let value = unwrap_reply(json!({"Ok":{"Windows":[],"future_field":42}})).unwrap();
        assert_eq!(value["future_field"], 42);
        assert!(matches!(
            unwrap_reply(json!({"Err":"no"})),
            Err(IpcError::Remote(_))
        ));
    }

    #[test]
    fn action_requests_are_typed_allowlisted_json() {
        assert_eq!(
            Action::FocusWindow { id: 9 }.wire(),
            json!({"Action":{"FocusWindow":{"id":9}}})
        );
        assert_eq!(
            Action::FocusWorkspaceIndex { index: 2 }.wire(),
            json!({"Action":{"FocusWorkspace":{"reference":{"Index":2}}}})
        );
        assert_eq!(
            Action::FocusWorkspaceId { id: 8 }.wire(),
            json!({"Action":{"FocusWorkspace":{"reference":{"Id":8}}}})
        );
    }

    #[test]
    fn socket_request_writes_one_json_line_and_reads_wrapped_reply() {
        let dir = std::env::temp_dir().join(format!("vista-niri-ipc-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let listener = UnixListener::bind(&dir).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap(),
                json!("Windows")
            );
            stream
                .write_all(b"{\"Ok\":{\"Windows\":[],\"new\":true}}\n")
                .unwrap();
        });
        let ipc = NiriIpc::new(dir.clone());
        let reply = ipc.request_value(&Query::Windows.wire()).unwrap();
        assert_eq!(reply["new"], true);
        server.join().unwrap();
        std::fs::remove_file(dir).unwrap();
    }
}
