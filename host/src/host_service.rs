//! Long-lived host service: pair a client, publish windows and virtual
//! desktops, stream the selected target, and apply semantic actions/input.
//!
//! This is the first end-to-end host path. It deliberately keeps the existing
//! control protocol and AEAD record layer, adds stream-selection and input
//! messages, and uses `grim` for pixels because the compositor's PipeWire
//! screencast currently exposes dmabuf-only buffers that this GStreamer build
//! cannot consume. Window streaming focuses and fullscreens the requested
//! window before capturing the output; desktop and workspace streaming capture
//! the focused output directly.
//!
//! What is real here:
//! - authenticated pairing and encrypted control records;
//! - live catalog of windows, workspaces and output geometry;
//! - live H.264 production from real desktop pixels over the authenticated
//!   media connection;
//! - semantic actions through Niri IPC;
//! - pointer/keyboard/text injection through a private ydotool daemon when
//!   `/dev/uinput` is available.
//!
//! What is not real here: TLS/QUIC/WebRTC, discovery, congestion control,
//! multiple simultaneous clients, audio, or clipboard.

use crate::{
    Niri,
    capability::{
        ActionGrants, ActionRegistry, InputPermissions, NiriTargetScope, ScopedNiriBackend,
        TargetRef,
    },
    catalog_wire::CatalogSnapshot,
    control_dispatch::HostControlChannel,
    control_lease::ControlLease,
    input_inject::InputInjector,
    live_media::{DEFAULT_FPS, MAX_FPS, Producer, SharedMedia},
    media_stream,
    media_transport::{MediaError, media_cipher, read_record, write_record},
    protocol::{
        ClientSession, ControlMessage, Envelope, HostSession, HostState, MAX_CONTROL_FRAME_BYTES,
        PairingSecret, PermissionSet, SessionKeys, StreamTarget, unix_now,
    },
    transport::{Direction, MAX_RECORD_BYTES, RecordCipher},
};
use serde::Serialize;
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const CONTROL_POLL: Duration = Duration::from_millis(50);
const MEDIA_ACCEPT_TIMEOUT: Duration = Duration::from_secs(20);
const CATALOG_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub listen: SocketAddr,
    pub secret: Option<PairingSecret>,
    pub allow_remote: bool,
    pub grants: ActionGrants,
    pub allow_input: bool,
    pub fps: u32,
    pub once: bool,
}

impl ServeOptions {
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            listen,
            secret: None,
            allow_remote: false,
            grants: ActionGrants::default(),
            allow_input: false,
            fps: DEFAULT_FPS,
            once: false,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !self.listen.ip().is_loopback() && !self.allow_remote {
            return Err("refusing a non-loopback listener without --allow-remote".into());
        }
        if self.fps == 0 || self.fps > MAX_FPS {
            return Err(format!("--fps must be 1..={MAX_FPS}"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HostInvite {
    pub protocol: String,
    pub address: String,
    pub secret_hex: String,
    pub host_name: String,
    pub input: bool,
    pub fps: u32,
}

#[derive(Debug, Serialize)]
pub struct SessionReport {
    pub client_id: String,
    pub catalog_revision: u64,
    pub stream_target: Option<StreamTarget>,
    pub frames: u64,
    pub bytes: u64,
    pub dropped: u64,
    pub actions_completed: u64,
    pub input_events: u64,
    pub reason: String,
}

/// A long-lived listener. It accepts one client at a time and returns the
/// client's media connection to the next holder after each session ends.
pub struct HostService {
    listener: TcpListener,
    secret: PairingSecret,
    options: ServeOptions,
    injector: Option<Arc<InputInjector>>,
}

impl HostService {
    pub fn bind(options: ServeOptions) -> Result<(Self, HostInvite), String> {
        options.validate()?;
        let secret = match options.secret.clone() {
            Some(secret) => secret,
            None => {
                PairingSecret::generate().map_err(|error| format!("pairing secret: {error}"))?
            }
        };
        let listener = TcpListener::bind(options.listen)
            .map_err(|error| format!("cannot bind control listener: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read listener address: {error}"))?;
        let permissions = InputPermissions {
            pointer: options.allow_input,
            keyboard: options.allow_input,
            text: options.allow_input,
        };
        let injector = if options.allow_input {
            match InputInjector::probe(permissions) {
                Ok(injector) => Some(Arc::new(injector)),
                Err(error) => {
                    eprintln!("vista-host: input disabled: {error}");
                    None
                }
            }
        } else {
            None
        };
        let invite = HostInvite {
            protocol: "1.0".into(),
            address: address.to_string(),
            secret_hex: secret.to_hex(),
            host_name: "Vista host".into(),
            input: injector.is_some(),
            fps: options.fps,
        };
        Ok((
            Self {
                listener,
                secret,
                options,
                injector,
            },
            invite,
        ))
    }

    pub fn run(self) -> Result<(), String> {
        let secret = self.secret;
        loop {
            let (stream, peer) = match self.listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("control accept failed: {error}")),
            };
            eprintln!("vista-host: client connected from {peer}");
            match serve_client(stream, secret.clone(), &self.options, self.injector.clone()) {
                Ok(report) => {
                    let json = serde_json::to_string(&report).unwrap_or_else(|_| "{}".into());
                    println!("{json}");
                }
                Err(error) => eprintln!("vista-host: session failed: {error}"),
            }
            if self.options.once {
                return Ok(());
            }
        }
    }
}

struct Wire {
    stream: TcpStream,
    read: Option<RecordCipher>,
    read_limit: usize,
    read_buffer: Vec<u8>,
    write: Option<RecordCipher>,
}

impl Wire {
    fn new(stream: TcpStream) -> Result<Self, String> {
        stream
            .set_read_timeout(Some(CONTROL_POLL))
            .map_err(|error| format!("cannot set read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("cannot set write timeout: {error}"))?;
        Ok(Self {
            stream,
            read: None,
            read_limit: MAX_CONTROL_FRAME_BYTES,
            read_buffer: Vec::new(),
            write: None,
        })
    }

    fn enable(&mut self, read: RecordCipher, write: RecordCipher, read_limit: usize) {
        self.read = Some(read);
        self.write = Some(write);
        self.read_limit = read_limit;
    }

    fn read_envelope(&mut self) -> Result<Option<Envelope>, String> {
        if !self.read_until(4)? {
            return Ok(None);
        }
        let length = u32::from_be_bytes(self.read_buffer[..4].try_into().unwrap()) as usize;
        if length == 0 || length > self.read_limit {
            return Err(format!("invalid control frame length: {length}"));
        }
        if !self.read_until(4 + length)? {
            return Ok(None);
        }
        let frame = std::mem::take(&mut self.read_buffer);
        let body = &frame[4..];
        if let Some(cipher) = &mut self.read {
            let plaintext = cipher
                .open(body)
                .map_err(|error| format!("control open failed: {error}"))?;
            Ok(Some(serde_json::from_slice(&plaintext).map_err(
                |error| format!("control decode failed: {error}"),
            )?))
        } else {
            Ok(Some(Envelope::decode(&frame).map_err(|error| {
                format!("control decode failed: {error}")
            })?))
        }
    }

    /// Preserve partial frames across polling timeouts; TCP may split even the
    /// four-byte prefix across reads.
    fn read_until(&mut self, target: usize) -> Result<bool, String> {
        while self.read_buffer.len() < target {
            let mut chunk = [0u8; 8192];
            let remaining = (target - self.read_buffer.len()).min(chunk.len());
            match self.stream.read(&mut chunk[..remaining]) {
                Ok(0) => return Err("peer disconnected".into()),
                Ok(count) => self.read_buffer.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(false);
                }
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {
                    return Err("peer disconnected".into());
                }
                Err(error) => return Err(format!("control read failed: {error}")),
            }
        }
        Ok(true)
    }

    fn read_envelope_blocking(&mut self) -> Result<Envelope, String> {
        self.stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("cannot set handshake timeout: {error}"))?;
        let result = self.read_envelope();
        self.stream
            .set_read_timeout(Some(CONTROL_POLL))
            .map_err(|error| format!("cannot restore read timeout: {error}"))?;
        match result {
            Ok(Some(envelope)) => Ok(envelope),
            Ok(None) => Err("handshake timed out".into()),
            Err(error) => Err(error),
        }
    }

    fn write_envelope(&mut self, envelope: &Envelope) -> Result<(), String> {
        let json = serde_json::to_vec(envelope).map_err(|error| format!("encode: {error}"))?;
        if json.len() > MAX_CONTROL_FRAME_BYTES {
            return Err("control envelope exceeds the frame bound".into());
        }
        let payload = if let Some(cipher) = &mut self.write {
            cipher
                .seal(&json)
                .map_err(|error| format!("control seal failed: {error}"))?
        } else {
            json
        };
        let length = u32::try_from(payload.len()).map_err(|_| "control frame too large")?;
        self.stream
            .write_all(&length.to_be_bytes())
            .map_err(|error| format!("control write failed: {error}"))?;
        self.stream
            .write_all(&payload)
            .map_err(|error| format!("control write failed: {error}"))?;
        self.stream
            .flush()
            .map_err(|error| format!("control flush failed: {error}"))
    }

    fn set_server_cipher(&mut self, keys: &SessionKeys, session_id: &str) -> Result<(), String> {
        self.enable(
            RecordCipher::new(keys.client_to_host(), session_id, Direction::ClientToHost)
                .map_err(|error| format!("read cipher: {error}"))?,
            RecordCipher::new(keys.host_to_client(), session_id, Direction::HostToClient)
                .map_err(|error| format!("write cipher: {error}"))?,
            MAX_RECORD_BYTES,
        );
        Ok(())
    }

    fn set_client_cipher(&mut self, keys: &SessionKeys, session_id: &str) -> Result<(), String> {
        self.enable(
            RecordCipher::new(keys.host_to_client(), session_id, Direction::HostToClient)
                .map_err(|error| format!("read cipher: {error}"))?,
            RecordCipher::new(keys.client_to_host(), session_id, Direction::ClientToHost)
                .map_err(|error| format!("write cipher: {error}"))?,
            MAX_RECORD_BYTES,
        );
        Ok(())
    }

    fn close(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[allow(unused_assignments)]
fn serve_client(
    stream: TcpStream,
    secret: PairingSecret,
    options: &ServeOptions,
    injector: Option<Arc<InputInjector>>,
) -> Result<SessionReport, String> {
    let mut wire = Wire::new(stream)?;
    let session_input = options.allow_input && injector.is_some();
    let policy = PermissionSet {
        video: true,
        input: session_input,
        ..PermissionSet::default()
    };
    let now = unix_now();
    let mut host = HostSession::new("vista-host", "Vista host", secret.clone(), policy, now)
        .map_err(|error| format!("host session: {error}"))?;
    host_handshake(&mut wire, &mut host)?;
    let client_id = "client".to_string();
    let session_id = host
        .session_id()
        .ok_or("authenticated session has no id")?
        .to_string();

    // Announce a fresh media endpoint for this control session. The endpoint
    // derives its own media keys from the same out-of-band secret.
    let media = MediaEndpoint::bind(options.listen.ip(), secret)
        .map_err(|error| format!("media endpoint: {error}"))?;
    let offer = media.offer();
    let envelope = host
        .emit_media_offer(offer.port, offer.pairing_id.clone(), offer.expires_at)
        .map_err(|error| format!("media offer: {error}"))?;
    wire.write_envelope(&envelope)?;

    let shared = Arc::new(SharedMedia::new());
    let media_thread = media.spawn(shared.clone());

    let mut revision = 0u64;
    send_catalog(&mut host, &mut wire, &session_id, &mut revision)?;

    let mut capability_revision = 1u64;
    attach_target(
        &mut host,
        &session_id,
        StreamTarget::Desktop,
        options,
        capability_revision,
    )?;
    if let Some(snapshot) = host
        .control_snapshot()
        .map_err(|error| format!("capability snapshot: {error}"))?
    {
        wire.write_envelope(&snapshot)?;
    }

    let mut producer: Option<Producer> = None;
    let mut target: Option<StreamTarget> = None;
    let mut started_epoch = 0u64;
    let mut input_events = 0u64;
    let mut actions_completed = 0u64;
    let mut last_catalog = Instant::now();
    let mut reason = String::new();

    loop {
        match wire.read_envelope() {
            Ok(Some(envelope)) => {
                let body = envelope.body.clone();
                let keyframe_before = host.keyframe_requests();
                let response = host
                    .handle(&envelope, unix_now())
                    .map_err(|error| format!("control message rejected: {error}"))?;
                if let Some(response) = response {
                    if matches!(&response.body, ControlMessage::ActionResult { .. }) {
                        actions_completed = actions_completed.saturating_add(1);
                    }
                    wire.write_envelope(&response)?;
                }
                if host.keyframe_requests() > keyframe_before {
                    shared.request_keyframe();
                }
                match body {
                    ControlMessage::StreamSelect {
                        target: selected, ..
                    } => {
                        if let Some(mut old) = producer.take() {
                            old.stop();
                        }
                        if let Some(control) = host.control_mut() {
                            let releases = control.revoke();
                            if let Some(injector) = &injector {
                                injector.release_effects(&releases);
                                injector.clear_geometry();
                            }
                        }
                        target = None;
                        shared.begin();
                        if let Err(error) = validate_target(&selected) {
                            let envelope = host
                                .emit(ControlMessage::StreamError {
                                    v: 1,
                                    session_id: session_id.clone(),
                                    code: "target_missing".into(),
                                    message: error,
                                })
                                .map_err(|error| format!("stream error: {error}"))?;
                            wire.write_envelope(&envelope)?;
                            continue;
                        }
                        capability_revision = capability_revision.saturating_add(1);
                        attach_target(
                            &mut host,
                            &session_id,
                            selected,
                            options,
                            capability_revision,
                        )?;
                        if let Some(snapshot) = host
                            .control_snapshot()
                            .map_err(|error| format!("capability snapshot: {error}"))?
                        {
                            wire.write_envelope(&snapshot)?;
                        }
                        match Producer::start(selected, shared.clone(), options.fps) {
                            Ok(started) => {
                                producer = Some(started);
                                target = Some(selected);
                                started_epoch = 0;
                            }
                            Err(error) => {
                                if let Some(control) = host.control_mut() {
                                    let releases = control.revoke();
                                    if let Some(injector) = &injector {
                                        injector.release_effects(&releases);
                                        injector.clear_geometry();
                                    }
                                }
                                let envelope = host
                                    .emit(ControlMessage::StreamError {
                                        v: 1,
                                        session_id: session_id.clone(),
                                        code: "capture_unavailable".into(),
                                        message: error,
                                    })
                                    .map_err(|error| format!("stream error: {error}"))?;
                                wire.write_envelope(&envelope)?;
                            }
                        }
                    }
                    ControlMessage::StreamStop { .. } => {
                        if let Some(mut old) = producer.take() {
                            old.stop();
                        }
                        target = None;
                        shared.begin();
                        if let Some(control) = host.control_mut() {
                            let releases = control.revoke();
                            if let Some(injector) = &injector {
                                injector.release_effects(&releases);
                                injector.clear_geometry();
                            }
                        }
                        let envelope = host
                            .emit(ControlMessage::StreamStopped {
                                v: 1,
                                session_id: session_id.clone(),
                                reason: "client requested stop".into(),
                            })
                            .map_err(|error| format!("stream stopped: {error}"))?;
                        wire.write_envelope(&envelope)?;
                    }
                    ControlMessage::CatalogRequest { .. } => {
                        send_catalog(&mut host, &mut wire, &session_id, &mut revision)?;
                    }
                    ControlMessage::InputEvent { event, .. } => {
                        if let Some(injector) = &injector {
                            input_events = input_events.saturating_add(1);
                            let result = apply_live_input(
                                host.control_mut(),
                                &event,
                                injector.geometry_epoch(),
                                |effects| injector.apply_effects(effects),
                                |effects| injector.release_effects(effects),
                            );
                            if let Err(error) = result {
                                eprintln!("vista-host: input rejected: {error}");
                            }
                        } else {
                            let envelope = host
                                .emit(ControlMessage::StreamError {
                                    v: 1,
                                    session_id: session_id.clone(),
                                    code: "input_unavailable".into(),
                                    message: "input injection is not enabled".into(),
                                })
                                .map_err(|error| format!("input error: {error}"))?;
                            wire.write_envelope(&envelope)?;
                        }
                    }
                    ControlMessage::ActionResult { .. } => {
                        actions_completed = actions_completed.saturating_add(1);
                        send_catalog(&mut host, &mut wire, &session_id, &mut revision)?;
                    }
                    ControlMessage::SessionClose { .. } => {
                        reason = "client closed the session".into();
                        break;
                    }
                    _ => {}
                }
            }
            Ok(None) => {}
            Err(error) => {
                reason = error;
                break;
            }
        }

        if let Some((geometry, encoder)) = shared.started()
            && geometry.epoch != started_epoch
        {
            started_epoch = geometry.epoch;
            if let Some(selected) = target {
                let envelope = host
                    .emit(ControlMessage::StreamStarted {
                        v: 1,
                        session_id: session_id.clone(),
                        target: selected,
                        width: geometry.width,
                        height: geometry.height,
                        geometry_epoch: geometry.epoch,
                        encoder,
                    })
                    .map_err(|error| format!("stream started: {error}"))?;
                wire.write_envelope(&envelope)?;
                if let Some(injector) = &injector {
                    injector.set_geometry(crate::input_inject::StreamGeometry {
                        width: geometry.width,
                        height: geometry.height,
                        epoch: geometry.epoch,
                    });
                }
            }
        }
        if let Some(error) = shared.take_error() {
            if let Some(mut old) = producer.take() {
                old.stop();
            }
            target = None;
            if let Some(control) = host.control_mut() {
                let releases = control.revoke();
                if let Some(injector) = &injector {
                    injector.release_effects(&releases);
                    injector.clear_geometry();
                }
            }
            let envelope = host
                .emit(ControlMessage::StreamError {
                    v: 1,
                    session_id: session_id.clone(),
                    code: "capture_failed".into(),
                    message: error,
                })
                .map_err(|error| format!("stream error: {error}"))?;
            wire.write_envelope(&envelope)?;
        }
        if last_catalog.elapsed() >= CATALOG_INTERVAL {
            last_catalog = Instant::now();
            if let Err(error) = send_catalog(&mut host, &mut wire, &session_id, &mut revision) {
                reason = error;
                break;
            }
        }
    }

    if let Some(mut old) = producer.take() {
        old.stop();
    }
    if let Some(control) = host.control_mut() {
        let releases = control.revoke();
        if let Some(injector) = &injector {
            injector.release_effects(&releases);
            injector.clear_geometry();
        }
    }
    shared.close();
    let (frames, bytes, dropped) = shared.stats();
    if reason.is_empty() {
        reason = "connection ended".into();
    }
    let media_result = media_thread.join();
    if let Ok(Err(error)) = media_result {
        eprintln!("vista-host: media session ended: {error}");
    }
    wire.close();
    Ok(SessionReport {
        client_id,
        catalog_revision: revision,
        stream_target: target,
        frames,
        bytes,
        dropped,
        actions_completed,
        input_events,
        reason,
    })
}

/// The sole live input path: authorization is performed by the authenticated
/// channel lease before the injector callback can observe any effects.
fn apply_live_input(
    control: Option<&mut HostControlChannel>,
    event: &crate::capability::InputEvent,
    geometry_epoch: Option<u64>,
    mut inject: impl FnMut(&[crate::capability::InputEffect]) -> Result<(), String>,
    mut release: impl FnMut(&[crate::capability::InputEffect]),
) -> Result<(), String> {
    let geometry_epoch = geometry_epoch.ok_or_else(|| "no active stream geometry".to_string())?;
    let control = control.ok_or_else(|| "no active control lease".to_string())?;
    let effects = control
        .apply_input(event, geometry_epoch)
        .map_err(|error| format!("authorization failed: {error}"))?;
    if let Err(error) = inject(&effects) {
        let releases = control.revoke();
        release(&releases);
        return Err(format!("native input injection failed: {error}"));
    }
    Ok(())
}

fn host_handshake(wire: &mut Wire, host: &mut HostSession) -> Result<(), String> {
    let now = unix_now();
    while host.state() != HostState::Active {
        let envelope = wire.read_envelope_blocking()?;
        let plaintext = wire.read.is_none();
        let response = host
            .handle(&envelope, now)
            .map_err(|error| format!("control handshake: {error}"))?;
        if let Some(response) = response {
            let acceptance = matches!(&response.body, ControlMessage::PairingAccepted { .. });
            wire.write_envelope(&response)?;
            if acceptance && plaintext {
                let keys = host
                    .session_keys()
                    .ok_or("session keys are unavailable")?
                    .clone();
                let session_id = host
                    .session_id()
                    .ok_or("session id is unavailable")?
                    .to_string();
                wire.set_server_cipher(&keys, &session_id)?;
            }
        }
    }
    Ok(())
}

fn validate_target(target: &StreamTarget) -> Result<(), String> {
    let niri = Niri::default();
    match target {
        StreamTarget::Desktop => Ok(()),
        StreamTarget::Window { id } => niri
            .window(*id)
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .ok_or_else(|| format!("window {id} is not available")),
        StreamTarget::Workspace { id } => niri
            .workspace(*id)
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .ok_or_else(|| format!("workspace {id} is not available")),
    }
}

fn attach_target(
    host: &mut HostSession,
    session_id: &str,
    target: StreamTarget,
    options: &ServeOptions,
    capability_revision: u64,
) -> Result<(), String> {
    let scope = match target {
        StreamTarget::Window { .. } => NiriTargetScope::Window,
        StreamTarget::Workspace { .. } => NiriTargetScope::Workspace,
        StreamTarget::Desktop => NiriTargetScope::Desktop,
    };
    let generation = match target {
        StreamTarget::Window { id } => crate::capability::niri_generation(id),
        _ => format!(
            "{}#{}",
            CatalogSnapshot::current_generation(),
            target.kind()
        ),
    };
    let target_ref = TargetRef::new(target.identifier(), generation)
        .map_err(|error| format!("target: {error}"))?;
    let grants = options.grants;
    let input_permissions = InputPermissions {
        pointer: options.allow_input,
        keyboard: options.allow_input,
        text: options.allow_input,
    };
    let lease = ControlLease::grant(
        session_id,
        Some(session_id.to_string()),
        target_ref,
        grants,
        input_permissions,
    )
    .map_err(|error| format!("control lease: {error}"))?;
    let backend: Box<dyn crate::capability::ActionBackend + Send> =
        Box::new(ScopedNiriBackend::new(scope));
    let channel = HostControlChannel::new(
        ActionRegistry::new(backend),
        lease,
        capability_revision.max(1),
    )
    .map_err(|error| format!("control channel: {error}"))?;
    host.attach_control(channel)
        .map_err(|error| format!("attach control: {error}"))
}

fn send_catalog(
    host: &mut HostSession,
    wire: &mut Wire,
    session_id: &str,
    revision: &mut u64,
) -> Result<(), String> {
    *revision = revision.saturating_add(1);
    let niri = Niri::default();
    let windows = niri.windows().map_err(|error| error.to_string())?;
    let workspaces = niri.workspaces().map_err(|error| error.to_string())?;
    let output = niri.focused_output().ok().flatten();
    let snapshot = CatalogSnapshot::from_niri(
        session_id.to_string(),
        Some(session_id.to_string()),
        CatalogSnapshot::current_generation(),
        *revision,
        &windows,
        &workspaces,
        output.as_ref(),
    )
    .map_err(|error| format!("catalog: {error}"))?;
    let envelope = host
        .emit_catalog(snapshot)
        .map_err(|error| format!("catalog emit: {error}"))?;
    wire.write_envelope(&envelope)
}

#[derive(Clone, Debug, Serialize)]
pub struct MediaOfferInfo {
    pub port: u16,
    pub pairing_id: String,
    pub expires_at: u64,
}

struct MediaEndpoint {
    listener: TcpListener,
    host: HostSession,
    now: u64,
    offer: MediaOfferInfo,
}

impl MediaEndpoint {
    fn bind(ip: IpAddr, secret: PairingSecret) -> Result<Self, String> {
        let listener = TcpListener::bind(SocketAddr::new(ip, 0))
            .map_err(|error| format!("cannot bind media listener: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read media address: {error}"))?;
        let now = unix_now();
        let host = HostSession::new(
            "vista-media-host",
            "Vista media host",
            secret,
            PermissionSet::video_only(),
            now,
        )
        .map_err(|error| format!("media host session: {error}"))?;
        let offer = MediaOfferInfo {
            port: address.port(),
            pairing_id: host.pairing_id().to_string(),
            expires_at: host.expires_at(),
        };
        Ok(Self {
            listener,
            host,
            now,
            offer,
        })
    }

    fn offer(&self) -> &MediaOfferInfo {
        &self.offer
    }

    fn spawn(self, shared: Arc<SharedMedia>) -> JoinHandle<Result<MediaReport, String>> {
        thread::spawn(move || self.accept_and_stream(shared))
    }

    fn accept_and_stream(self, shared: Arc<SharedMedia>) -> Result<MediaReport, String> {
        self.listener
            .set_nonblocking(true)
            .map_err(|error| format!("media listener: {error}"))?;
        let deadline = Instant::now() + MEDIA_ACCEPT_TIMEOUT;
        let mut stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline || shared.is_closed() {
                        return Err("media client did not connect".into());
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(format!("media accept failed: {error}")),
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|error| format!("media stream: {error}"))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("media read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|error| format!("media write timeout: {error}"))?;
        let mut host = self.host;
        let (keys, session_id) = media_stream::host_handshake(&mut stream, &mut host, self.now)?;
        let control_stream = stream
            .try_clone()
            .map_err(|error| format!("cannot clone media stream: {error}"))?;
        let control_cipher =
            media_cipher(keys.client_to_host(), &session_id, Direction::ClientToHost)
                .map_err(|error| format!("media control cipher: {error}"))?;
        let control_shared = shared.clone();
        let control_thread = thread::spawn(move || {
            read_media_control(control_stream, host, control_cipher, control_shared)
        });

        let mut cipher = media_cipher(keys.host_to_client(), &session_id, Direction::HostToClient)
            .map_err(|error| format!("media cipher: {error}"))?;
        let mut frames = 0u64;
        let mut bytes = 0u64;
        let started = Instant::now();
        while let Some((record, _, _)) = shared.next() {
            match write_record(&mut stream, &mut cipher, &record) {
                Ok(written) => {
                    frames = frames.saturating_add(1);
                    bytes = bytes.saturating_add(written as u64);
                }
                Err(error) if shared.is_closed() => {
                    let _ = error;
                    break;
                }
                Err(error) => return Err(format!("media write failed: {error}")),
            }
        }
        let _ = control_thread.join();
        Ok(MediaReport {
            frames,
            bytes,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

fn read_media_control(
    mut stream: TcpStream,
    mut host: HostSession,
    mut cipher: RecordCipher,
    shared: Arc<SharedMedia>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(CONTROL_POLL))
        .map_err(|error| format!("control read timeout: {error}"))?;
    while !shared.is_closed() {
        let mut prefix = [0u8; 4];
        match stream.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                break;
            }
            Err(error) => return Err(format!("media control read failed: {error}")),
        }
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 || length > MAX_CONTROL_FRAME_BYTES + 16 {
            return Err(format!("invalid media control length: {length}"));
        }
        let mut body = vec![0u8; length];
        stream
            .read_exact(&mut body)
            .map_err(|error| format!("media control read failed: {error}"))?;
        let plaintext = cipher
            .open(&body)
            .map_err(|error| format!("media control open failed: {error}"))?;
        let Ok(envelope) = serde_json::from_slice::<Envelope>(&plaintext) else {
            continue;
        };
        let _ = host.handle(&envelope, unix_now());
        match envelope.body {
            ControlMessage::KeyframeRequest { .. } => shared.request_keyframe(),
            ControlMessage::SessionClose { .. } => {
                shared.close();
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct MediaReport {
    pub frames: u64,
    pub bytes: u64,
    pub elapsed_ms: u128,
}

/// Client-side helper for the reference CLI and tests.
pub struct LiveClient {
    wire: Wire,
    pub(crate) client: ClientSession,
    pub media: Option<MediaOfferInfo>,
    pub last_geometry_epoch: Option<u64>,
}

impl LiveClient {
    pub fn connect(
        address: SocketAddr,
        secret: PairingSecret,
        permissions: PermissionSet,
        client_id: &str,
    ) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
            .map_err(|error| format!("control connect failed: {error}"))?;
        let mut wire = Wire::new(stream)?;
        let mut client = ClientSession::new(secret.clone(), client_id)
            .map_err(|error| format!("client session: {error}"))?;
        wire.write_envelope(&client.hello().map_err(|error| error.to_string())?)?;
        let server_hello = wire.read_envelope_blocking()?;
        let proof = client
            .accept_server_hello(&server_hello)
            .map_err(|error| format!("server hello: {error}"))?;
        wire.write_envelope(&proof)?;
        let accepted = wire.read_envelope_blocking()?;
        client
            .accept_pairing(&accepted)
            .map_err(|error| format!("pairing: {error}"))?;
        let keys = client
            .session_keys()
            .ok_or("client has no session keys")?
            .clone();
        let session_id = client
            .session_id()
            .ok_or("client has no session id")?
            .to_string();
        wire.set_client_cipher(&keys, &session_id)?;
        wire.write_envelope(
            &client
                .open_session(permissions)
                .map_err(|error| format!("session open: {error}"))?,
        )?;
        let opened = wire.read_envelope_blocking()?;
        client
            .accept_session_opened(&opened)
            .map_err(|error| format!("session opened: {error}"))?;
        let mut media = None;
        while media.is_none() {
            let envelope = wire.read_envelope_blocking()?;
            match &envelope.body {
                ControlMessage::MediaOffer {
                    port,
                    pairing_id,
                    expires_at,
                    ..
                } => {
                    client
                        .accept_media_offer(&envelope)
                        .map_err(|error| format!("media offer: {error}"))?;
                    media = Some(MediaOfferInfo {
                        port: *port,
                        pairing_id: pairing_id.clone(),
                        expires_at: *expires_at,
                    });
                }
                ControlMessage::CatalogSnapshot { .. } => {
                    client
                        .accept_catalog_snapshot(&envelope)
                        .map_err(|error| format!("catalog: {error}"))?;
                }
                ControlMessage::CapabilitySnapshot { .. } => {
                    client
                        .accept_capability_snapshot(&envelope)
                        .map_err(|error| format!("capabilities: {error}"))?;
                }
                other => {
                    return Err(format!("unexpected message {}", other.name()));
                }
            }
        }
        Ok(Self {
            wire,
            client,
            media,
            last_geometry_epoch: None,
        })
    }

    pub fn select(&mut self, target: StreamTarget) -> Result<(), String> {
        let envelope = self
            .client
            .select_stream(target)
            .map_err(|error| error.to_string())?;
        self.wire.write_envelope(&envelope)
    }

    pub fn send(&mut self, envelope: Envelope) -> Result<(), String> {
        self.wire.write_envelope(&envelope)
    }

    pub fn poll_control(&mut self) -> Result<Option<Envelope>, String> {
        self.wire.read_envelope()
    }

    pub fn send_input(&mut self, event: crate::capability::InputEvent) -> Result<(), String> {
        let envelope = self
            .client
            .input_event(event)
            .map_err(|error| error.to_string())?;
        self.wire.write_envelope(&envelope)
    }

    pub fn close(&mut self, reason: &str) {
        if let Ok(envelope) = self.client.session_close(reason) {
            let _ = self.wire.write_envelope(&envelope);
        }
        self.wire.close();
    }
}

impl Drop for LiveClient {
    fn drop(&mut self) {
        self.wire.close();
    }
}

/// Connect the media socket announced by a control session and return the
/// handshake material used to read access units.
pub fn connect_media(
    address: SocketAddr,
    secret: &PairingSecret,
) -> Result<(TcpStream, SessionKeys, String), String> {
    let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
        .map_err(|error| format!("media connect failed: {error}"))?;
    let mut stream = stream;
    let (client, keys, session_id) = media_stream::client_handshake(&mut stream, secret.clone())?;
    drop(client);
    Ok((stream, keys, session_id))
}

pub fn read_media_record(
    stream: &mut TcpStream,
    cipher: &mut RecordCipher,
) -> Result<Option<crate::media_transport::MediaRecord>, String> {
    match read_record(stream, cipher) {
        Ok(record) => Ok(Some(record)),
        Err(MediaError::Truncated) => Ok(None),
        Err(error) => Err(format!("media read failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability::{
            ActionGrants, ActionRegistry, FakeActionBackend, InputPermissions, TargetRef,
        },
        control_lease::ControlLease,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn input_control() -> HostControlChannel {
        let lease = ControlLease::grant(
            "session-1",
            Some("session-1".into()),
            TargetRef::new("w5", "generation-5").unwrap(),
            ActionGrants::default(),
            InputPermissions {
                pointer: true,
                keyboard: true,
                text: true,
            },
        )
        .unwrap();
        let backend: Box<dyn crate::capability::ActionBackend + Send> =
            Box::new(FakeActionBackend::default());
        HostControlChannel::new(ActionRegistry::new(backend), lease, 1).unwrap()
    }

    fn key_down() -> crate::capability::InputEvent {
        crate::capability::InputEvent::KeyPhysical {
            usage: 4,
            pressed: true,
            repeat: false,
        }
    }

    #[test]
    fn live_input_without_lease_or_geometry_has_no_injection_side_effects() {
        let calls = AtomicUsize::new(0);
        let mut inject = |_effects: &[crate::capability::InputEffect]| -> Result<(), String> {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let mut release = |_effects: &[crate::capability::InputEffect]| {};
        assert!(apply_live_input(None, &key_down(), Some(3), &mut inject, &mut release).is_err());
        let mut control = input_control();
        assert!(
            apply_live_input(
                Some(&mut control),
                &key_down(),
                None,
                &mut inject,
                &mut release
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn revoked_and_stale_geometry_input_never_reaches_injector() {
        let calls = AtomicUsize::new(0);
        let mut inject = |_effects: &[crate::capability::InputEffect]| -> Result<(), String> {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let mut release = |_effects: &[crate::capability::InputEffect]| {};
        let mut revoked = input_control();
        revoked.revoke();
        assert!(
            apply_live_input(
                Some(&mut revoked),
                &key_down(),
                Some(3),
                &mut inject,
                &mut release
            )
            .is_err()
        );
        let mut stale = input_control();
        let event = crate::capability::InputEvent::PointerAbsolute {
            x: 0.5,
            y: 0.5,
            geometry_epoch: 2,
        };
        assert!(
            apply_live_input(Some(&mut stale), &event, Some(3), &mut inject, &mut release).is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn injection_failure_revokes_and_releases_every_held_input() {
        let mut control = input_control();
        let injected = AtomicUsize::new(0);
        let released = AtomicUsize::new(0);
        let mut inject = |effects: &[crate::capability::InputEffect]| {
            injected.fetch_add(effects.len(), Ordering::SeqCst);
            Err("fake native failure".to_string())
        };
        let mut release = |effects: &[crate::capability::InputEffect]| {
            released.fetch_add(effects.len(), Ordering::SeqCst);
        };
        assert!(
            apply_live_input(
                Some(&mut control),
                &key_down(),
                Some(3),
                &mut inject,
                &mut release
            )
            .is_err()
        );
        assert_eq!(injected.load(Ordering::SeqCst), 1);
        assert_eq!(released.load(Ordering::SeqCst), 1);
        assert!(!control.lease().is_active());
        assert!(!control.lease().held_input());
    }

    #[test]
    fn serve_rejects_public_listener_without_opt_in() {
        let options = ServeOptions::new("0.0.0.0:7443".parse().unwrap());
        assert!(options.validate().is_err());
    }

    #[test]
    fn serve_bounds_fps() {
        let mut options = ServeOptions::new("127.0.0.1:0".parse().unwrap());
        options.fps = 0;
        assert!(options.validate().is_err());
        options.fps = MAX_FPS + 1;
        assert!(options.validate().is_err());
        options.fps = DEFAULT_FPS;
        assert!(options.validate().is_ok());
    }

    #[test]
    fn control_reader_preserves_partial_frames_across_timeouts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (receiver, _) = listener.accept().unwrap();
        let mut wire = Wire::new(receiver).unwrap();
        let envelope = Envelope::new(0, ControlMessage::Ping { nonce: 42 });
        let frame = envelope.encode().unwrap();

        sender.write_all(&frame[..2]).unwrap();
        assert!(wire.read_envelope().unwrap().is_none());
        sender.write_all(&frame[2..6]).unwrap();
        assert!(wire.read_envelope().unwrap().is_none());
        sender.write_all(&frame[6..]).unwrap();
        assert_eq!(wire.read_envelope().unwrap(), Some(envelope));
    }
}
