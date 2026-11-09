//! Bounded AEAD record layer and loopback transport for the Vista control protocol.
//!
//! The plaintext handshake uses the length-prefixed JSON frames from
//! `protocol`. Immediately after `pairing_accepted`, both sides switch to
//! ChaCha20-Poly1305 records keyed by the HKDF-derived directional session
//! keys. The nonce is an implicit per-direction counter; the AAD binds the
//! record to the protocol version, session id and direction.
//!
//! This is a loopback diagnostic and a transport-neutral record boundary. It
//! is **not** TLS/QUIC/WebRTC, does not protect metadata, has no discovery and
//! is not externally audited. Production transport remains a separate step.

use crate::capability::{
    ActionBackend, ActionGrants, ActionRegistry, ActionRequest, ActionStatus, InputPermissions,
    TargetRef,
};
use crate::control_dispatch::HostControlChannel;
use crate::control_lease::ControlLease;
use crate::protocol::{
    ClientSession, ControlMessage, Envelope, HostSession, HostState, MAX_CONTROL_FRAME_BYTES,
    PROTOCOL_VERSION, PairingSecret, PermissionSet, ProtocolError, SessionKeys, unix_now,
};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

pub const RECORD_TAG_BYTES: usize = 16;
pub const MAX_RECORD_BYTES: usize = MAX_CONTROL_FRAME_BYTES + RECORD_TAG_BYTES;
const NONCE_BYTES: usize = 12;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const SERVER_ACCEPT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ACTIVE_RECORDS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    ClientToHost,
    HostToClient,
}

impl Direction {
    fn tag(self) -> u8 {
        match self {
            Self::ClientToHost => 1,
            Self::HostToClient => 2,
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    Io(io::Error),
    Protocol(ProtocolError),
    Json(serde_json::Error),
    Crypto,
    RecordTooLarge,
    CounterExhausted,
    PairingState,
    Server(String),
    Control(String),
    Timeout,
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Crypto => formatter.write_str("record authentication failed"),
            Self::RecordTooLarge => formatter.write_str("record exceeds the size bound"),
            Self::CounterExhausted => formatter.write_str("record nonce counter exhausted"),
            Self::PairingState => formatter.write_str("session keys are not available"),
            Self::Server(reason) => write!(formatter, "server error: {reason}"),
            Self::Control(reason) => write!(formatter, "control policy error: {reason}"),
            Self::Timeout => formatter.write_str("transport timed out"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ProtocolError> for TransportError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<serde_json::Error> for TransportError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// One direction of the AEAD record stream. The nonce is implicit and strictly
/// incremented; replay or reordering fails authentication instead of being
/// silently accepted.
pub struct RecordCipher {
    key: LessSafeKey,
    aad: Vec<u8>,
    counter: u64,
}

impl RecordCipher {
    pub fn new(
        key: &[u8; 32],
        session_id: &str,
        direction: Direction,
    ) -> Result<Self, TransportError> {
        let unbound =
            UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| TransportError::Crypto)?;
        let mut aad = b"vista-record-v1".to_vec();
        aad.extend_from_slice(session_id.as_bytes());
        aad.push(direction.tag());
        Ok(Self {
            key: LessSafeKey::new(unbound),
            aad,
            counter: 0,
        })
    }

    fn nonce(&self) -> Result<Nonce, TransportError> {
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[4..].copy_from_slice(&self.counter.to_be_bytes());
        Ok(Nonce::assume_unique_for_key(nonce))
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, TransportError> {
        if plaintext.len() > MAX_CONTROL_FRAME_BYTES {
            return Err(TransportError::RecordTooLarge);
        }
        let nonce = self.nonce()?;
        let mut output = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::from(self.aad.as_slice()), &mut output)
            .map_err(|_| TransportError::Crypto)?;
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or(TransportError::CounterExhausted)?;
        Ok(output)
    }

    pub fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, TransportError> {
        if ciphertext.len() < RECORD_TAG_BYTES || ciphertext.len() > MAX_RECORD_BYTES {
            return Err(TransportError::RecordTooLarge);
        }
        let nonce = self.nonce()?;
        let mut input = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, Aad::from(self.aad.as_slice()), &mut input)
            .map_err(|_| TransportError::Crypto)?;
        let result = plaintext.to_vec();
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or(TransportError::CounterExhausted)?;
        Ok(result)
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }
}

struct Connection {
    stream: TcpStream,
    read: Option<RecordCipher>,
    write: Option<RecordCipher>,
}

impl Connection {
    fn new(stream: TcpStream) -> Result<Self, TransportError> {
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        Ok(Self {
            stream,
            read: None,
            write: None,
        })
    }

    fn enable(&mut self, read: RecordCipher, write: RecordCipher) {
        self.read = Some(read);
        self.write = Some(write);
    }

    fn is_plaintext(&self) -> bool {
        self.read.is_none()
    }

    fn read_envelope(&mut self) -> Result<Envelope, TransportError> {
        let mut prefix = [0u8; 4];
        self.stream.read_exact(&mut prefix)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 {
            return Err(TransportError::RecordTooLarge);
        }
        if let Some(cipher) = &mut self.read {
            if length > MAX_RECORD_BYTES {
                return Err(TransportError::RecordTooLarge);
            }
            let mut ciphertext = vec![0u8; length];
            self.stream.read_exact(&mut ciphertext)?;
            let plaintext = cipher.open(&ciphertext)?;
            Ok(serde_json::from_slice(&plaintext)?)
        } else {
            if length > MAX_CONTROL_FRAME_BYTES {
                return Err(TransportError::RecordTooLarge);
            }
            let mut body = vec![0u8; length];
            self.stream.read_exact(&mut body)?;
            let mut framed = Vec::with_capacity(4 + length);
            framed.extend_from_slice(&(length as u32).to_be_bytes());
            framed.extend_from_slice(&body);
            Ok(Envelope::decode(&framed)?)
        }
    }

    fn write_envelope(&mut self, envelope: &Envelope) -> Result<usize, TransportError> {
        let json = serde_json::to_vec(envelope)?;
        if json.len() > MAX_CONTROL_FRAME_BYTES {
            return Err(TransportError::RecordTooLarge);
        }
        let payload = if let Some(cipher) = &mut self.write {
            cipher.seal(&json)?
        } else {
            json
        };
        let length = u32::try_from(payload.len()).map_err(|_| TransportError::RecordTooLarge)?;
        self.stream.write_all(&length.to_be_bytes())?;
        self.stream.write_all(&payload)?;
        self.stream.flush()?;
        Ok(4 + payload.len())
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ServerStats {
    pub handshake_frames: usize,
    pub encrypted_records: usize,
    pub keyframe_requests: u64,
    pub pong_sent: bool,
    pub control_snapshot_sent: bool,
    pub action_results: usize,
    pub actions_completed: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct ClientStats {
    pub handshake_frames: usize,
    pub encrypted_writes: usize,
    pub encrypted_reads: usize,
    pub pong_verified: bool,
    pub reached_active: bool,
}

#[derive(Debug, Serialize)]
pub struct ServerOffer {
    pub address: SocketAddr,
    pub pairing_id: String,
    pub expires_at: u64,
}

/// Opt-in capability/action policy for one listener session. The target and
/// grants are fixed when the listener starts; the control lease is created with
/// the pairing-assigned session id once the client reaches `SessionOpened`.
pub struct ControlSetup {
    pub target: TargetRef,
    pub grants: ActionGrants,
    pub input_permissions: InputPermissions,
    pub capability_revision: u64,
    pub backend: Box<dyn ActionBackend + Send>,
}

impl ControlSetup {
    pub fn new(
        target: TargetRef,
        grants: ActionGrants,
        input_permissions: InputPermissions,
        capability_revision: u64,
        backend: Box<dyn ActionBackend + Send>,
    ) -> Self {
        Self {
            target,
            grants,
            input_permissions,
            capability_revision,
            backend,
        }
    }
}

/// One semantic action a `connect_control` client sends after reading the
/// snapshot. Parameters are bounded by the contract before they are sent.
#[derive(Clone, Debug)]
pub struct ControlAction {
    pub id: String,
    pub action: String,
    pub schema_version: u16,
    pub parameters: BTreeMap<String, Value>,
}

impl ControlAction {
    pub fn new(id: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            action: action.into(),
            schema_version: 1,
            parameters: BTreeMap::new(),
        }
    }

    pub fn with_parameters(mut self, parameters: BTreeMap<String, Value>) -> Self {
        self.parameters = parameters;
        self
    }
}

#[derive(Debug, Serialize)]
pub struct ControlClientStats {
    pub reached_active: bool,
    pub snapshot_received: bool,
    pub snapshot_actions: usize,
    pub grant_epoch: String,
    pub action_result_received: bool,
    pub action_status: Option<ActionStatus>,
    pub action_error: Option<crate::capability::ContractErrorCode>,
    pub action_state: Option<Value>,
    pub encrypted_writes: usize,
    pub encrypted_reads: usize,
}

/// One-shot control listener. It owns a host pairing state and accepts at most
/// one client. This is a control-plane server, not a media or discovery server.
pub struct HostControlServer {
    listener: TcpListener,
    host: HostSession,
    now_secs: u64,
    control: Option<ControlSetup>,
}

impl HostControlServer {
    pub fn bind(
        address: SocketAddr,
        secret: PairingSecret,
        now_secs: u64,
    ) -> Result<(Self, ServerOffer), TransportError> {
        Self::bind_with_control(address, secret, now_secs, PermissionSet::video_only(), None)
    }

    /// Bind a listener with an explicit session permission policy and optional
    /// capability/action dispatcher. `policy` must contain every permission the
    /// client may request; a `ControlSetup` with input permissions requires the
    /// input permission in `policy`.
    pub fn bind_with_control(
        address: SocketAddr,
        secret: PairingSecret,
        now_secs: u64,
        policy: PermissionSet,
        control: Option<ControlSetup>,
    ) -> Result<(Self, ServerOffer), TransportError> {
        if let Some(setup) = &control
            && (setup.capability_revision == 0)
        {
            return Err(TransportError::Control(
                "capability revision must be non-zero".into(),
            ));
        }
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let host = HostSession::new("vista-host", "Vista host", secret, policy, now_secs)?;
        let offer = ServerOffer {
            address: listener.local_addr()?,
            pairing_id: host.pairing_id().to_string(),
            expires_at: host.expires_at(),
        };
        Ok((
            Self {
                listener,
                host,
                now_secs,
                control,
            },
            offer,
        ))
    }

    pub fn accept_and_run(self) -> Result<ServerStats, TransportError> {
        self.accept_and_run_with_hook(|| {})
    }

    /// Accept one client and invoke `hook` after each authenticated
    /// `KeyframeRequest` is handled. The hook runs on the server thread.
    pub fn accept_and_run_with_hook<F>(self, mut hook: F) -> Result<ServerStats, TransportError>
    where
        F: FnMut(),
    {
        let deadline = Instant::now() + SERVER_ACCEPT_TIMEOUT;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    return serve(stream, self.host, self.now_secs, self.control, &mut hook);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(TransportError::Timeout);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(TransportError::Io(error)),
            }
        }
    }
}

fn serve<F>(
    stream: TcpStream,
    mut host: HostSession,
    now_secs: u64,
    control: Option<ControlSetup>,
    hook: &mut F,
) -> Result<ServerStats, TransportError>
where
    F: FnMut(),
{
    let mut connection = Connection::new(stream)?;
    let mut stats = ServerStats::default();

    while host.state() != HostState::Active {
        let envelope = connection.read_envelope()?;
        let plaintext = connection.is_plaintext();
        let response = host.handle(&envelope, now_secs)?;
        if plaintext {
            stats.handshake_frames += 1;
        } else {
            stats.encrypted_records += 1;
        }
        if let Some(response) = response {
            let is_acceptance = matches!(&response.body, ControlMessage::PairingAccepted { .. });
            connection.write_envelope(&response)?;
            if is_acceptance {
                let keys = host
                    .session_keys()
                    .ok_or(TransportError::PairingState)?
                    .clone();
                let session_id = host
                    .session_id()
                    .ok_or(TransportError::PairingState)?
                    .to_string();
                connection.enable(
                    RecordCipher::new(keys.client_to_host(), &session_id, Direction::ClientToHost)?,
                    RecordCipher::new(keys.host_to_client(), &session_id, Direction::HostToClient)?,
                );
            }
        }
    }

    if let Some(setup) = control {
        let session_id = host
            .session_id()
            .ok_or(TransportError::PairingState)?
            .to_string();
        let lease = ControlLease::grant(
            &session_id,
            Some(session_id.clone()),
            setup.target,
            setup.grants,
            setup.input_permissions,
        )
        .map_err(|error| TransportError::Control(error.to_string()))?;
        let channel = HostControlChannel::new(
            ActionRegistry::new(setup.backend),
            lease,
            setup.capability_revision,
        )
        .map_err(|error| TransportError::Control(error.to_string()))?;
        host.attach_control(channel)?;
        if let Some(snapshot) = host.control_snapshot()? {
            connection.write_envelope(&snapshot)?;
            stats.control_snapshot_sent = true;
        }
    }

    for _ in 0..MAX_ACTIVE_RECORDS {
        let envelope = match connection.read_envelope() {
            Ok(envelope) => envelope,
            Err(TransportError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        };
        stats.encrypted_records += 1;
        let keyframe_requests_before = host.keyframe_requests();
        let response = host.handle(&envelope, now_secs)?;
        if matches!(&envelope.body, ControlMessage::KeyframeRequest { .. })
            && host.keyframe_requests() > keyframe_requests_before
        {
            hook();
        }
        if let Some(response) = response {
            match &response.body {
                ControlMessage::Pong { .. } => stats.pong_sent = true,
                ControlMessage::ActionResult { status, .. } => {
                    stats.action_results += 1;
                    if *status == ActionStatus::Completed {
                        stats.actions_completed += 1;
                    }
                }
                _ => {}
            }
            connection.write_envelope(&response)?;
        }
        if matches!(&envelope.body, ControlMessage::SessionClose { .. }) {
            break;
        }
    }
    if let Some(control) = host.control_mut() {
        control.revoke();
    }
    stats.keyframe_requests = host.keyframe_requests();
    Ok(stats)
}

/// A client that has completed pairing and opened its requested session.
struct OpenedClient {
    connection: Connection,
    client: ClientSession,
    stats: ClientStats,
}

fn pair_and_open(
    address: SocketAddr,
    secret: PairingSecret,
    permissions: PermissionSet,
) -> Result<OpenedClient, TransportError> {
    let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)?;
    let mut connection = Connection::new(stream)?;
    let mut client = ClientSession::new(secret, "vista-client")?;
    let mut stats = ClientStats::default();

    connection.write_envelope(&client.hello()?)?;
    stats.handshake_frames += 1;
    let server_hello = connection.read_envelope()?;
    let proof = client.accept_server_hello(&server_hello)?;
    connection.write_envelope(&proof)?;
    stats.handshake_frames += 1;
    let accepted = connection.read_envelope()?;
    client.accept_pairing(&accepted)?;

    let keys: SessionKeys = client
        .session_keys()
        .ok_or(TransportError::PairingState)?
        .clone();
    let session_id = client
        .session_id()
        .ok_or(TransportError::PairingState)?
        .to_string();
    connection.enable(
        RecordCipher::new(keys.host_to_client(), &session_id, Direction::HostToClient)?,
        RecordCipher::new(keys.client_to_host(), &session_id, Direction::ClientToHost)?,
    );

    let open = client.open_session(permissions)?;
    connection.write_envelope(&open)?;
    stats.encrypted_writes += 1;
    let opened = connection.read_envelope()?;
    stats.encrypted_reads += 1;
    client.accept_session_opened(&opened)?;
    stats.reached_active = true;
    Ok(OpenedClient {
        connection,
        client,
        stats,
    })
}

/// Connects to a one-shot host, performs the full pairing and opens a
/// video-only session with an encrypted KeyframeRequest and Ping/Pong.
pub fn connect(address: SocketAddr, secret: PairingSecret) -> Result<ClientStats, TransportError> {
    let OpenedClient {
        mut connection,
        mut client,
        mut stats,
    } = pair_and_open(address, secret, PermissionSet::video_only())?;

    connection.write_envelope(&client.keyframe_request()?)?;
    stats.encrypted_writes += 1;
    connection.write_envelope(&client.ping(42)?)?;
    stats.encrypted_writes += 1;
    let pong = connection.read_envelope()?;
    stats.encrypted_reads += 1;
    client.accept_pong(&pong, 42)?;
    stats.pong_verified = true;

    connection.write_envelope(&client.session_close("loopback complete")?)?;
    stats.encrypted_writes += 1;
    Ok(stats)
}

/// Connect, read the host's grant-scoped capability snapshot, send one semantic
/// action and return the host's truthful result. Used by the action-policy
/// integration test and the `protocol-client --action` path.
pub fn connect_control(
    address: SocketAddr,
    secret: PairingSecret,
    permissions: PermissionSet,
    action: &ControlAction,
) -> Result<ControlClientStats, TransportError> {
    let OpenedClient {
        mut connection,
        mut client,
        stats,
    } = pair_and_open(address, secret, permissions)?;

    let mut control = ControlClientStats {
        reached_active: stats.reached_active,
        snapshot_received: false,
        snapshot_actions: 0,
        grant_epoch: String::new(),
        action_result_received: false,
        action_status: None,
        action_error: None,
        action_state: None,
        encrypted_writes: stats.encrypted_writes,
        encrypted_reads: stats.encrypted_reads,
    };

    let snapshot_envelope = connection.read_envelope()?;
    control.encrypted_reads += 1;
    let snapshot = client.accept_capability_snapshot(&snapshot_envelope)?;
    control.snapshot_received = true;
    control.snapshot_actions = snapshot.actions.len();
    control.grant_epoch = snapshot.grant_epoch.clone();

    let context = crate::capability::ActionContext {
        connection_epoch: snapshot.connection_epoch.clone(),
        session_id: snapshot.session_id.clone(),
        grant_epoch: snapshot.grant_epoch.clone(),
        target: snapshot.target(),
        capability_revision: snapshot.capability_revision,
        grants: ActionGrants::default(),
    };
    let request = ActionRequest::new(
        &action.id,
        &context,
        &action.action,
        action.schema_version,
        action.parameters.clone(),
    )
    .map_err(|error| TransportError::Control(error.to_string()))?;
    connection.write_envelope(&client.action_invoke(&request)?)?;
    control.encrypted_writes += 1;
    let reply = connection.read_envelope()?;
    control.encrypted_reads += 1;
    let result = client.accept_action_result(&reply)?;
    control.action_result_received = true;
    control.action_status = Some(result.status);
    control.action_error = result.error_code;
    control.action_state = result.state;

    connection.write_envelope(&client.session_close("control action complete")?)?;
    control.encrypted_writes += 1;
    Ok(control)
}

#[derive(Debug, Serialize)]
pub struct LoopbackReport {
    pub address: String,
    pub protocol: String,
    pub handshake_frames: usize,
    pub server_encrypted_records: usize,
    pub client_encrypted_writes: usize,
    pub client_encrypted_reads: usize,
    pub keyframe_requests: u64,
    pub pong_verified: bool,
    pub verified: bool,
}

/// Runs a real TCP loopback pairing and an encrypted control exchange. This is
/// local-only and never binds a public interface.
pub fn protocol_loopback() -> Result<LoopbackReport, TransportError> {
    let secret = PairingSecret::generate()?;
    let now_secs = unix_now();
    let (server, offer) = HostControlServer::bind(
        "127.0.0.1:0".parse().expect("loopback address"),
        secret.clone(),
        now_secs,
    )?;
    let handle = thread::spawn(move || server.accept_and_run());
    let client_result = connect(offer.address, secret);
    let server_result = handle
        .join()
        .map_err(|_| TransportError::Server("server thread panicked".into()))?;
    let client = client_result?;
    let server = server_result?;
    let verified = client.reached_active
        && client.pong_verified
        && server.pong_sent
        && server.keyframe_requests == 1;
    Ok(LoopbackReport {
        address: offer.address.to_string(),
        protocol: PROTOCOL_VERSION.to_string(),
        handshake_frames: server.handshake_frames,
        server_encrypted_records: server.encrypted_records,
        client_encrypted_writes: client.encrypted_writes,
        client_encrypted_reads: client.encrypted_reads,
        keyframe_requests: server.keyframe_requests,
        pong_verified: client.pong_verified,
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn record_cipher_round_trip_rejects_replay_tamper_and_direction() {
        let key = [7u8; 32];
        let mut writer = RecordCipher::new(&key, SESSION, Direction::ClientToHost).unwrap();
        let mut reader = RecordCipher::new(&key, SESSION, Direction::ClientToHost).unwrap();
        let ciphertext = writer.seal(b"hello record").unwrap();
        assert_eq!(reader.open(&ciphertext).unwrap(), b"hello record");
        assert!(
            reader.open(&ciphertext).is_err(),
            "replayed record accepted"
        );

        let mut writer = RecordCipher::new(&key, SESSION, Direction::ClientToHost).unwrap();
        let mut reader = RecordCipher::new(&key, SESSION, Direction::ClientToHost).unwrap();
        let mut ciphertext = writer.seal(b"tamper me").unwrap();
        ciphertext[0] ^= 1;
        assert!(
            reader.open(&ciphertext).is_err(),
            "tampered record accepted"
        );

        let mut writer = RecordCipher::new(&key, SESSION, Direction::ClientToHost).unwrap();
        let mut wrong_direction =
            RecordCipher::new(&key, SESSION, Direction::HostToClient).unwrap();
        let ciphertext = writer.seal(b"direction").unwrap();
        assert!(
            wrong_direction.open(&ciphertext).is_err(),
            "wrong-direction record accepted"
        );
    }

    #[test]
    fn oversized_plaintext_is_rejected() {
        let mut cipher = RecordCipher::new(&[3u8; 32], SESSION, Direction::ClientToHost).unwrap();
        assert!(matches!(
            cipher.seal(&vec![0u8; MAX_CONTROL_FRAME_BYTES + 1]),
            Err(TransportError::RecordTooLarge)
        ));
    }

    #[test]
    fn tcp_loopback_pairs_and_exchanges_encrypted_control() {
        let report = protocol_loopback().unwrap();
        assert!(report.verified, "{report:?}");
        assert_eq!(report.handshake_frames, 2);
        assert_eq!(report.keyframe_requests, 1);
        assert!(report.pong_verified);
    }
}
