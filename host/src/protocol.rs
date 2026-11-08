//! Versioned, transport-independent control protocol and pairing for Vista.
//!
//! This module defines bounded control frames, protocol version negotiation, an
//! out-of-band-secret pairing handshake, short-lived session keys and explicit
//! per-session permissions. It intentionally contains no sockets, discovery,
//! TLS, WebRTC, media packetization or input injection.
//!
//! Security notes (see `docs/protocol.md`):
//! - The pairing secret is 256 bits and must be transferred out of band, for
//!   example in a QR code. A short human code is deliberately not accepted: an
//!   eavesdropper could otherwise mount an offline guessing attack.
//! - HMAC-SHA256 confirms knowledge of the secret in both directions and HKDF
//!   derives independent client-to-host and host-to-client keys.
//! - This construction does **not** provide forward secrecy. The transport must
//!   still encrypt frames with the derived keys; that layer is not implemented.
//! - This is a first-party protocol, not an audited one.

use ring::{
    hkdf, hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
pub const MAX_CONTROL_FRAME_BYTES: usize = 64 * 1024;
pub const PAIRING_OFFER_TTL_SECS: u64 = 120;
const MAX_IDENTIFIER_BYTES: usize = 128;
const NONCE_BYTES: usize = 32;
const PAIRING_ID_BYTES: usize = 16;
const SESSION_ID_BYTES: usize = 16;
const TAG_BYTES: usize = 32;
const SECRET_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: ProtocolVersion,
    pub max: ProtocolVersion,
}

impl VersionRange {
    pub fn current() -> Self {
        Self {
            min: PROTOCOL_VERSION,
            max: PROTOCOL_VERSION,
        }
    }

    fn supports(self, version: ProtocolVersion) -> bool {
        self.min <= version && version <= self.max
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionSet {
    pub video: bool,
    pub audio: bool,
    pub input: bool,
    pub clipboard: bool,
}

impl PermissionSet {
    pub fn video_only() -> Self {
        Self {
            video: true,
            ..Self::default()
        }
    }

    fn to_bits(self) -> u8 {
        u8::from(self.video)
            | (u8::from(self.audio) << 1)
            | (u8::from(self.input) << 2)
            | (u8::from(self.clipboard) << 3)
    }

    /// True when every permission in `self` is also present in `grant`.
    pub fn is_subset_of(self, grant: Self) -> bool {
        (!self.video || grant.video)
            && (!self.audio || grant.audio)
            && (!self.input || grant.input)
            && (!self.clipboard || grant.clipboard)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Strictly increasing per direction and starting at zero.
    pub seq: u64,
    #[serde(flatten)]
    pub body: ControlMessage,
}

impl Envelope {
    pub fn new(seq: u64, body: ControlMessage) -> Self {
        Self { seq, body }
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        encode_frame(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        decode_frame(bytes)
    }
}

/// Version 1 control messages. JSON field names are stable and intended to map
/// directly to Swift `Codable` types. Binary values are lowercase hex strings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    ClientHello {
        versions: VersionRange,
        client_id: String,
        client_nonce: String,
    },
    ServerHello {
        selected: ProtocolVersion,
        host_id: String,
        host_name: String,
        host_nonce: String,
        pairing_id: String,
        expires_at: u64,
        policy: PermissionSet,
    },
    PairingRequest {
        pairing_id: String,
        tag: String,
    },
    PairingAccepted {
        tag: String,
        session_id: String,
        permissions: PermissionSet,
    },
    PairingRejected {
        reason: String,
    },
    SessionOpen {
        session_id: String,
        permissions: PermissionSet,
    },
    SessionOpened {
        session_id: String,
        permissions: PermissionSet,
    },
    KeyframeRequest {
        session_id: String,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    SessionClose {
        session_id: Option<String>,
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ControlMessage {
    fn name(&self) -> &'static str {
        match self {
            Self::ClientHello { .. } => "client_hello",
            Self::ServerHello { .. } => "server_hello",
            Self::PairingRequest { .. } => "pairing_request",
            Self::PairingAccepted { .. } => "pairing_accepted",
            Self::PairingRejected { .. } => "pairing_rejected",
            Self::SessionOpen { .. } => "session_open",
            Self::SessionOpened { .. } => "session_opened",
            Self::KeyframeRequest { .. } => "keyframe_request",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::SessionClose { .. } => "session_close",
            Self::Error { .. } => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    FrameTooLarge,
    FrameTruncated,
    FrameLengthMismatch,
    MalformedJson,
    UnsupportedVersion,
    UnexpectedSequence {
        expected: u64,
        actual: u64,
    },
    InvalidTransition {
        expected: &'static str,
        actual: &'static str,
    },
    InvalidIdentifier,
    InvalidNonce,
    ExpiredOffer,
    UnknownPairing,
    AuthenticationFailed,
    PermissionDenied,
    SessionMismatch,
    EntropyUnavailable,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge => formatter.write_str("control frame exceeds the size limit"),
            Self::FrameTruncated => formatter.write_str("control frame is truncated"),
            Self::FrameLengthMismatch => formatter.write_str("control frame length does not match"),
            Self::MalformedJson => formatter.write_str("control frame is not valid JSON"),
            Self::UnsupportedVersion => {
                formatter.write_str("no supported protocol version overlap")
            }
            Self::UnexpectedSequence { expected, actual } => {
                write!(
                    formatter,
                    "control sequence expected {expected}, received {actual}"
                )
            }
            Self::InvalidTransition { expected, actual } => {
                write!(formatter, "expected {expected}, received {actual}")
            }
            Self::InvalidIdentifier => formatter.write_str("invalid identifier"),
            Self::InvalidNonce => formatter.write_str("invalid nonce or pairing identifier"),
            Self::ExpiredOffer => formatter.write_str("pairing offer expired"),
            Self::UnknownPairing => formatter.write_str("unknown pairing identifier"),
            Self::AuthenticationFailed => formatter.write_str("pairing authentication failed"),
            Self::PermissionDenied => formatter.write_str("permission request exceeds the grant"),
            Self::SessionMismatch => formatter.write_str("session identifier does not match"),
            Self::EntropyUnavailable => formatter.write_str("secure randomness is unavailable"),
        }
    }
}

impl std::error::Error for ProtocolError {}

pub fn encode_frame(value: &impl Serialize) -> Result<Vec<u8>, ProtocolError> {
    let json = serde_json::to_vec(value).map_err(|_| ProtocolError::MalformedJson)?;
    if json.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let mut framed = Vec::with_capacity(4 + json.len());
    framed.extend_from_slice(&(json.len() as u32).to_be_bytes());
    framed.extend_from_slice(&json);
    Ok(framed)
}

pub fn decode_frame<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, ProtocolError> {
    if bytes.len() < 4 {
        return Err(ProtocolError::FrameTruncated);
    }
    let length = u32::from_be_bytes(bytes[..4].try_into().expect("four bytes")) as usize;
    if length > MAX_CONTROL_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    if bytes.len() != 4 + length {
        return Err(ProtocolError::FrameLengthMismatch);
    }
    serde_json::from_slice(&bytes[4..]).map_err(|_| ProtocolError::MalformedJson)
}

/// A 256-bit pairing secret transferred out of band. It never appears on the wire.
#[derive(Clone)]
pub struct PairingSecret([u8; SECRET_BYTES]);

impl PairingSecret {
    pub fn generate() -> Result<Self, ProtocolError> {
        let mut bytes = [0u8; SECRET_BYTES];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| ProtocolError::EntropyUnavailable)?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(value: &str) -> Result<Self, ProtocolError> {
        let decoded = hex::decode(value).map_err(|_| ProtocolError::InvalidNonce)?;
        let bytes: [u8; SECRET_BYTES] = decoded
            .try_into()
            .map_err(|_| ProtocolError::InvalidNonce)?;
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for PairingSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingSecret(redacted)")
    }
}

impl Drop for PairingSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionKeys {
    client_to_host: [u8; 32],
    host_to_client: [u8; 32],
}

impl SessionKeys {
    pub fn client_to_host(&self) -> &[u8; 32] {
        &self.client_to_host
    }

    pub fn host_to_client(&self) -> &[u8; 32] {
        &self.host_to_client
    }

    pub fn fingerprint(&self) -> String {
        hex::encode(&self.client_to_host[..8])
    }

    fn derive(
        secret: &PairingSecret,
        pairing_id: &str,
        transcript: &[u8],
        session_id: &str,
    ) -> Result<Self, ProtocolError> {
        let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, pairing_id.as_bytes());
        let prk = salt.extract(secret.as_bytes());
        let mut info = b"vista-session-v1\n".to_vec();
        info.extend_from_slice(pairing_id.as_bytes());
        info.push(b'\n');
        info.extend_from_slice(session_id.as_bytes());
        info.push(b'\n');
        info.extend_from_slice(transcript);
        let info_slices: [&[u8]; 1] = [&info];
        let okm = prk
            .expand(&info_slices, KeyLen(64))
            .map_err(|_| ProtocolError::EntropyUnavailable)?;
        let mut output = [0u8; 64];
        okm.fill(&mut output)
            .map_err(|_| ProtocolError::EntropyUnavailable)?;
        let mut client_to_host = [0u8; 32];
        let mut host_to_client = [0u8; 32];
        client_to_host.copy_from_slice(&output[..32]);
        host_to_client.copy_from_slice(&output[32..]);
        Ok(Self {
            client_to_host,
            host_to_client,
        })
    }
}

impl fmt::Debug for SessionKeys {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionKeys")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

impl Drop for SessionKeys {
    fn drop(&mut self) {
        self.client_to_host.zeroize();
        self.host_to_client.zeroize();
    }
}

struct KeyLen(usize);

impl hkdf::KeyType for KeyLen {
    fn len(&self) -> usize {
        self.0
    }
}

fn random_hex(bytes: usize) -> Result<String, ProtocolError> {
    let mut value = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| ProtocolError::EntropyUnavailable)?;
    Ok(hex::encode(value))
}

fn tag(secret: &PairingSecret, label: &[u8], transcript: &[u8]) -> [u8; TAG_BYTES] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut data = Vec::with_capacity(label.len() + transcript.len());
    data.extend_from_slice(label);
    data.extend_from_slice(transcript);
    let value = hmac::sign(&key, &data);
    let mut out = [0u8; TAG_BYTES];
    out.copy_from_slice(value.as_ref());
    out
}

fn verify_tag(secret: &PairingSecret, label: &[u8], transcript: &[u8], provided: &[u8]) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut data = Vec::with_capacity(label.len() + transcript.len());
    data.extend_from_slice(label);
    data.extend_from_slice(transcript);
    hmac::verify(&key, &data, provided).is_ok()
}

/// The host confirmation additionally binds the session identifier so a
/// spliced acceptance cannot change which session the keys belong to.
fn host_tag(secret: &PairingSecret, transcript: &[u8], session_id: &str) -> [u8; TAG_BYTES] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut data = Vec::with_capacity(32 + transcript.len() + session_id.len());
    data.extend_from_slice(b"host-confirm\n");
    data.extend_from_slice(transcript);
    data.extend_from_slice(b"\nsession=");
    data.extend_from_slice(session_id.as_bytes());
    let value = hmac::sign(&key, &data);
    let mut out = [0u8; TAG_BYTES];
    out.copy_from_slice(value.as_ref());
    out
}

fn verify_host_tag(
    secret: &PairingSecret,
    transcript: &[u8],
    session_id: &str,
    provided: &[u8],
) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut data = Vec::with_capacity(32 + transcript.len() + session_id.len());
    data.extend_from_slice(b"host-confirm\n");
    data.extend_from_slice(transcript);
    data.extend_from_slice(b"\nsession=");
    data.extend_from_slice(session_id.as_bytes());
    hmac::verify(&key, &data, provided).is_ok()
}

fn validate_identifier(value: &str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(ProtocolError::InvalidIdentifier);
    }
    Ok(())
}

fn decode_hex_fixed(value: &str, bytes: usize) -> Result<Vec<u8>, ProtocolError> {
    let decoded = hex::decode(value).map_err(|_| ProtocolError::InvalidNonce)?;
    if decoded.len() != bytes {
        return Err(ProtocolError::InvalidNonce);
    }
    Ok(decoded)
}

fn transcript(
    selected: ProtocolVersion,
    client_id: &str,
    client_nonce: &str,
    host_id: &str,
    host_nonce: &str,
    pairing_id: &str,
    policy: PermissionSet,
) -> Vec<u8> {
    format!(
        "vista-pairing-v1\nversion={selected}\nclient_id={client_id}\nclient_nonce={client_nonce}\nhost_id={host_id}\nhost_nonce={host_nonce}\npairing_id={pairing_id}\npolicy={}\n",
        policy.to_bits()
    )
    .into_bytes()
}

fn select_version(range: VersionRange) -> Result<ProtocolVersion, ProtocolError> {
    if range.min > range.max || !range.supports(PROTOCOL_VERSION) {
        return Err(ProtocolError::UnsupportedVersion);
    }
    Ok(PROTOCOL_VERSION)
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostState {
    AwaitingHello,
    AwaitingPairing,
    Paired,
    Active,
    Closed,
    Failed,
}

pub struct HostSession {
    secret: PairingSecret,
    host_id: String,
    host_name: String,
    policy: PermissionSet,
    state: HostState,
    next_client_seq: u64,
    next_host_seq: u64,
    pairing_id: String,
    host_nonce: String,
    client_id: Option<String>,
    client_nonce: Option<String>,
    selected: Option<ProtocolVersion>,
    expires_at: u64,
    transcript: Vec<u8>,
    keys: Option<SessionKeys>,
    session_id: Option<String>,
    permissions: PermissionSet,
    keyframe_requests: u64,
}

impl HostSession {
    pub fn new(
        host_id: impl Into<String>,
        host_name: impl Into<String>,
        secret: PairingSecret,
        policy: PermissionSet,
        now_secs: u64,
    ) -> Result<Self, ProtocolError> {
        let host_id = host_id.into();
        let host_name = host_name.into();
        validate_identifier(&host_id)?;
        validate_identifier(&host_name)?;
        Ok(Self {
            secret,
            host_id,
            host_name,
            policy,
            state: HostState::AwaitingHello,
            next_client_seq: 0,
            next_host_seq: 0,
            pairing_id: random_hex(PAIRING_ID_BYTES)?,
            host_nonce: random_hex(NONCE_BYTES)?,
            client_id: None,
            client_nonce: None,
            selected: None,
            expires_at: now_secs.saturating_add(PAIRING_OFFER_TTL_SECS),
            transcript: Vec::new(),
            keys: None,
            session_id: None,
            permissions: PermissionSet::default(),
            keyframe_requests: 0,
        })
    }

    pub fn state(&self) -> HostState {
        self.state
    }

    pub fn pairing_id(&self) -> &str {
        &self.pairing_id
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn session_keys(&self) -> Option<&SessionKeys> {
        self.keys.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn permissions(&self) -> PermissionSet {
        self.permissions
    }

    pub fn keyframe_requests(&self) -> u64 {
        self.keyframe_requests
    }

    fn reply(&mut self, body: ControlMessage) -> Envelope {
        let envelope = Envelope::new(self.next_host_seq, body);
        self.next_host_seq = self.next_host_seq.wrapping_add(1);
        envelope
    }

    fn fail(&mut self) -> ProtocolError {
        self.state = HostState::Failed;
        self.keys = None;
        ProtocolError::AuthenticationFailed
    }

    pub fn handle(
        &mut self,
        envelope: &Envelope,
        now_secs: u64,
    ) -> Result<Option<Envelope>, ProtocolError> {
        if matches!(self.state, HostState::Closed | HostState::Failed) {
            return Err(ProtocolError::InvalidTransition {
                expected: "an open handshake",
                actual: "a closed session",
            });
        }
        if envelope.seq != self.next_client_seq {
            let error = ProtocolError::UnexpectedSequence {
                expected: self.next_client_seq,
                actual: envelope.seq,
            };
            self.state = HostState::Failed;
            return Err(error);
        }
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        let state = self.state;
        match (state, &envelope.body) {
            (
                HostState::AwaitingHello,
                ControlMessage::ClientHello {
                    versions,
                    client_id,
                    client_nonce,
                },
            ) => {
                validate_identifier(client_id)?;
                let selected = select_version(*versions)?;
                decode_hex_fixed(client_nonce, NONCE_BYTES)?;
                self.client_id = Some(client_id.clone());
                self.client_nonce = Some(client_nonce.clone());
                self.selected = Some(selected);
                self.transcript = transcript(
                    selected,
                    client_id,
                    client_nonce,
                    &self.host_id,
                    &self.host_nonce,
                    &self.pairing_id,
                    self.policy,
                );
                self.state = HostState::AwaitingPairing;
                Ok(Some(self.reply(ControlMessage::ServerHello {
                    selected,
                    host_id: self.host_id.clone(),
                    host_name: self.host_name.clone(),
                    host_nonce: self.host_nonce.clone(),
                    pairing_id: self.pairing_id.clone(),
                    expires_at: self.expires_at,
                    policy: self.policy,
                })))
            }
            (
                HostState::AwaitingPairing,
                ControlMessage::PairingRequest {
                    pairing_id,
                    tag: provided,
                },
            ) => {
                if now_secs > self.expires_at {
                    self.state = HostState::Failed;
                    return Err(ProtocolError::ExpiredOffer);
                }
                if pairing_id != &self.pairing_id {
                    self.state = HostState::Failed;
                    return Err(ProtocolError::UnknownPairing);
                }
                let provided = decode_hex_fixed(provided, TAG_BYTES)?;
                if !verify_tag(
                    &self.secret,
                    b"client-confirm\n",
                    &self.transcript,
                    &provided,
                ) {
                    return Err(self.fail());
                }
                let session_id = random_hex(SESSION_ID_BYTES)?;
                let host_tag = host_tag(&self.secret, &self.transcript, &session_id);
                let keys = SessionKeys::derive(
                    &self.secret,
                    &self.pairing_id,
                    &self.transcript,
                    &session_id,
                )?;
                self.keys = Some(keys);
                self.session_id = Some(session_id.clone());
                self.permissions = self.policy;
                self.state = HostState::Paired;
                Ok(Some(self.reply(ControlMessage::PairingAccepted {
                    tag: hex::encode(host_tag),
                    session_id,
                    permissions: self.policy,
                })))
            }
            (
                HostState::Paired,
                ControlMessage::SessionOpen {
                    session_id,
                    permissions,
                },
            ) => {
                if !permissions.is_subset_of(self.policy) {
                    self.state = HostState::Failed;
                    return Err(ProtocolError::PermissionDenied);
                }
                if self.session_id.as_deref() != Some(session_id.as_str()) {
                    self.state = HostState::Failed;
                    return Err(ProtocolError::SessionMismatch);
                }
                self.permissions = *permissions;
                self.state = HostState::Active;
                Ok(Some(self.reply(ControlMessage::SessionOpened {
                    session_id: session_id.clone(),
                    permissions: *permissions,
                })))
            }
            (HostState::Active, ControlMessage::KeyframeRequest { session_id }) => {
                if self.session_id.as_deref() != Some(session_id.as_str()) {
                    return Err(ProtocolError::SessionMismatch);
                }
                self.keyframe_requests = self.keyframe_requests.saturating_add(1);
                Ok(None)
            }
            (HostState::Active, ControlMessage::Ping { nonce }) => {
                Ok(Some(self.reply(ControlMessage::Pong { nonce: *nonce })))
            }
            (HostState::Active, ControlMessage::SessionClose { session_id, .. }) => {
                if let Some(session_id) = session_id
                    && self.session_id.as_deref() != Some(session_id.as_str())
                {
                    return Err(ProtocolError::SessionMismatch);
                }
                self.state = HostState::Closed;
                self.keys = None;
                Ok(None)
            }
            (_, message) => Err(ProtocolError::InvalidTransition {
                expected: match state {
                    HostState::AwaitingHello => "client_hello",
                    HostState::AwaitingPairing => "pairing_request",
                    HostState::Paired => "session_open",
                    HostState::Active => "an active-session message",
                    HostState::Closed => "a closed session",
                    HostState::Failed => "a failed session",
                },
                actual: message.name(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientState {
    AwaitingHello,
    AwaitingServerHello,
    AwaitingPairingAccepted,
    Paired,
    Active,
    Closed,
    Failed,
}

pub struct ClientSession {
    secret: PairingSecret,
    client_id: String,
    client_nonce: String,
    versions: VersionRange,
    state: ClientState,
    next_client_seq: u64,
    expected_host_seq: u64,
    pairing_id: Option<String>,
    policy: Option<PermissionSet>,
    session_id: Option<String>,
    transcript: Vec<u8>,
    keys: Option<SessionKeys>,
    requested: Option<PermissionSet>,
}

impl ClientSession {
    pub fn new(secret: PairingSecret, client_id: impl Into<String>) -> Result<Self, ProtocolError> {
        Self::with_versions(secret, client_id, VersionRange::current())
    }

    pub fn with_versions(
        secret: PairingSecret,
        client_id: impl Into<String>,
        versions: VersionRange,
    ) -> Result<Self, ProtocolError> {
        let client_id = client_id.into();
        validate_identifier(&client_id)?;
        if versions.min > versions.max {
            return Err(ProtocolError::UnsupportedVersion);
        }
        Ok(Self {
            secret,
            client_id,
            client_nonce: random_hex(NONCE_BYTES)?,
            versions,
            state: ClientState::AwaitingHello,
            next_client_seq: 0,
            expected_host_seq: 0,
            pairing_id: None,
            policy: None,
            session_id: None,
            transcript: Vec::new(),
            keys: None,
            requested: None,
        })
    }

    pub fn state(&self) -> ClientState {
        self.state
    }

    pub fn next_client_seq(&self) -> u64 {
        self.next_client_seq
    }

    pub fn session_keys(&self) -> Option<&SessionKeys> {
        self.keys.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    fn check_host_sequence(&mut self, envelope: &Envelope) -> Result<(), ProtocolError> {
        if envelope.seq != self.expected_host_seq {
            self.state = ClientState::Failed;
            return Err(ProtocolError::UnexpectedSequence {
                expected: self.expected_host_seq,
                actual: envelope.seq,
            });
        }
        self.expected_host_seq = self.expected_host_seq.wrapping_add(1);
        Ok(())
    }

    pub fn hello(&mut self) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::AwaitingHello {
            return Err(ProtocolError::InvalidTransition {
                expected: "a fresh client",
                actual: "an advanced client",
            });
        }
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::ClientHello {
                versions: self.versions,
                client_id: self.client_id.clone(),
                client_nonce: self.client_nonce.clone(),
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        self.state = ClientState::AwaitingServerHello;
        Ok(envelope)
    }

    pub fn accept_server_hello(&mut self, envelope: &Envelope) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::AwaitingServerHello {
            return Err(ProtocolError::InvalidTransition {
                expected: "server_hello",
                actual: "another state",
            });
        }
        self.check_host_sequence(envelope)?;
        let ControlMessage::ServerHello {
            selected,
            host_id,
            host_name: _,
            host_nonce,
            pairing_id,
            expires_at: _,
            policy,
        } = &envelope.body
        else {
            self.state = ClientState::Failed;
            return Err(ProtocolError::InvalidTransition {
                expected: "server_hello",
                actual: envelope.body.name(),
            });
        };
        if !self.versions.supports(*selected) {
            self.state = ClientState::Failed;
            return Err(ProtocolError::UnsupportedVersion);
        }
        validate_identifier(host_id)?;
        decode_hex_fixed(host_nonce, NONCE_BYTES)?;
        decode_hex_fixed(pairing_id, PAIRING_ID_BYTES)?;
        self.transcript = transcript(
            *selected,
            &self.client_id,
            &self.client_nonce,
            host_id,
            host_nonce,
            pairing_id,
            *policy,
        );
        let value = tag(&self.secret, b"client-confirm\n", &self.transcript);
        self.pairing_id = Some(pairing_id.clone());
        self.policy = Some(*policy);
        self.state = ClientState::AwaitingPairingAccepted;
        let proof = Envelope::new(
            self.next_client_seq,
            ControlMessage::PairingRequest {
                pairing_id: pairing_id.clone(),
                tag: hex::encode(value),
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(proof)
    }

    pub fn accept_pairing(&mut self, envelope: &Envelope) -> Result<SessionKeys, ProtocolError> {
        if self.state != ClientState::AwaitingPairingAccepted {
            return Err(ProtocolError::InvalidTransition {
                expected: "pairing_accepted",
                actual: "another state",
            });
        }
        self.check_host_sequence(envelope)?;
        let ControlMessage::PairingAccepted {
            tag: provided,
            session_id,
            permissions,
        } = &envelope.body
        else {
            self.state = ClientState::Failed;
            return Err(ProtocolError::InvalidTransition {
                expected: "pairing_accepted",
                actual: envelope.body.name(),
            });
        };
        decode_hex_fixed(session_id, SESSION_ID_BYTES)?;
        let provided = decode_hex_fixed(provided, TAG_BYTES)?;
        if !verify_host_tag(&self.secret, &self.transcript, session_id, &provided) {
            self.state = ClientState::Failed;
            return Err(ProtocolError::AuthenticationFailed);
        }
        let pairing_id = self
            .pairing_id
            .as_deref()
            .ok_or(ProtocolError::UnknownPairing)?;
        let keys = SessionKeys::derive(&self.secret, pairing_id, &self.transcript, session_id)?;
        self.policy = Some(*permissions);
        self.session_id = Some(session_id.clone());
        self.keys = Some(keys.clone());
        self.state = ClientState::Paired;
        Ok(keys)
    }

    pub fn open_session(&mut self, permissions: PermissionSet) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::Paired {
            return Err(ProtocolError::InvalidTransition {
                expected: "a paired client",
                actual: "another state",
            });
        }
        if let Some(policy) = self.policy
            && !permissions.is_subset_of(policy)
        {
            return Err(ProtocolError::PermissionDenied);
        }
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        self.requested = Some(permissions);
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::SessionOpen {
                session_id,
                permissions,
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn accept_session_opened(&mut self, envelope: &Envelope) -> Result<(), ProtocolError> {
        if self.state != ClientState::Paired {
            return Err(ProtocolError::InvalidTransition {
                expected: "session_opened",
                actual: "another state",
            });
        }
        self.check_host_sequence(envelope)?;
        let ControlMessage::SessionOpened {
            session_id,
            permissions,
        } = &envelope.body
        else {
            return Err(ProtocolError::InvalidTransition {
                expected: "session_opened",
                actual: envelope.body.name(),
            });
        };
        if self.session_id.as_deref() != Some(session_id.as_str()) {
            self.state = ClientState::Failed;
            return Err(ProtocolError::SessionMismatch);
        }
        let requested = self.requested.ok_or(ProtocolError::PermissionDenied)?;
        if !permissions.is_subset_of(requested) {
            self.state = ClientState::Failed;
            return Err(ProtocolError::PermissionDenied);
        }
        self.state = ClientState::Active;
        Ok(())
    }

    pub fn keyframe_request(&mut self) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::KeyframeRequest { session_id },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn ping(&mut self, nonce: u64) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let envelope = Envelope::new(self.next_client_seq, ControlMessage::Ping { nonce });
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn accept_pong(&mut self, envelope: &Envelope, nonce: u64) -> Result<(), ProtocolError> {
        self.check_host_sequence(envelope)?;
        match &envelope.body {
            ControlMessage::Pong { nonce: received } if *received == nonce => Ok(()),
            ControlMessage::Pong { .. } => Err(ProtocolError::InvalidTransition {
                expected: "matching pong",
                actual: "different pong",
            }),
            other => Err(ProtocolError::InvalidTransition {
                expected: "pong",
                actual: other.name(),
            }),
        }
    }

    pub fn session_close(&mut self, reason: &str) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::SessionClose {
                session_id: self.session_id.clone(),
                reason: reason.to_owned(),
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        self.state = ClientState::Closed;
        Ok(envelope)
    }
}

#[derive(Debug, Serialize)]
pub struct ProtocolReport {
    pub protocol: String,
    pub pairing_id: String,
    pub client_frames: usize,
    pub host_frames: usize,
    pub wire_bytes: usize,
    pub keys_match: bool,
    pub key_fingerprint: String,
    pub permissions: PermissionSet,
    pub host_state: String,
    pub client_state: String,
    pub wrong_secret_rejected: bool,
    pub replayed_proof_rejected: bool,
    pub expired_offer_rejected: bool,
    pub permission_escalation_rejected: bool,
    pub version_mismatch_rejected: bool,
    pub verified: bool,
}

fn state_name_host(state: HostState) -> String {
    format!("{state:?}")
}

fn state_name_client(state: ClientState) -> String {
    format!("{state:?}")
}

/// Runs a complete in-memory handshake plus negative checks. No sockets or
/// media are involved; this is evidence for the control-plane state machine.
pub fn diagnostic() -> Result<ProtocolReport, ProtocolError> {
    let now = unix_now();
    let secret = PairingSecret::generate()?;
    let mut host = HostSession::new(
        "vista-host-diagnostic",
        "Vista protocol diagnostic",
        secret.clone(),
        PermissionSet::video_only(),
        now,
    )?;
    let mut client = ClientSession::new(secret.clone(), "vista-client-diagnostic")?;
    let mut client_frames = 0;
    let mut host_frames = 0;
    let mut wire_bytes = 0;

    let hello_bytes = client.hello()?.encode()?;
    wire_bytes += hello_bytes.len();
    client_frames += 1;
    let hello = Envelope::decode(&hello_bytes)?;
    let challenge = host
        .handle(&hello, now)?
        .ok_or(ProtocolError::InvalidTransition {
            expected: "server_hello",
            actual: "no response",
        })?;
    let challenge_bytes = challenge.encode()?;
    wire_bytes += challenge_bytes.len();
    host_frames += 1;
    let challenge = Envelope::decode(&challenge_bytes)?;
    let pairing_request_bytes = client.accept_server_hello(&challenge)?.encode()?;
    wire_bytes += pairing_request_bytes.len();
    client_frames += 1;
    let pairing_request = Envelope::decode(&pairing_request_bytes)?;
    let pairing_accepted =
        host.handle(&pairing_request, now)?
            .ok_or(ProtocolError::InvalidTransition {
                expected: "pairing_accepted",
                actual: "no response",
            })?;
    let accepted_bytes = pairing_accepted.encode()?;
    wire_bytes += accepted_bytes.len();
    host_frames += 1;
    let pairing_accepted = Envelope::decode(&accepted_bytes)?;
    client.accept_pairing(&pairing_accepted)?;
    let open_bytes = client.open_session(PermissionSet::video_only())?.encode()?;
    wire_bytes += open_bytes.len();
    client_frames += 1;
    let open = Envelope::decode(&open_bytes)?;
    let opened = host
        .handle(&open, now)?
        .ok_or(ProtocolError::InvalidTransition {
            expected: "session_opened",
            actual: "no response",
        })?;
    let opened_bytes = opened.encode()?;
    wire_bytes += opened_bytes.len();
    host_frames += 1;
    let opened = Envelope::decode(&opened_bytes)?;
    client.accept_session_opened(&opened)?;

    let keys_match = client.session_keys() == host.session_keys();
    let fingerprint = host
        .session_keys()
        .map(SessionKeys::fingerprint)
        .unwrap_or_default();

    // Wrong secret must fail host authentication.
    let wrong_secret = PairingSecret::generate()?;
    let mut wrong_host = HostSession::new(
        "vista-host-negative",
        "Negative host",
        secret.clone(),
        PermissionSet::video_only(),
        now,
    )?;
    let mut wrong_client = ClientSession::new(wrong_secret, "wrong-client")?;
    let hello = wrong_client.hello()?;
    let challenge = wrong_host
        .handle(&hello, now)?
        .ok_or(ProtocolError::AuthenticationFailed)?;
    let proof = wrong_client.accept_server_hello(&challenge)?;
    let wrong_secret_rejected = matches!(
        wrong_host.handle(&proof, now),
        Err(ProtocolError::AuthenticationFailed)
    );

    // Replaying a valid proof must fail the sequence check.
    let mut replay_host = HostSession::new(
        "vista-host-replay",
        "Replay host",
        secret.clone(),
        PermissionSet::video_only(),
        now,
    )?;
    let mut replay_client = ClientSession::new(secret.clone(), "replay-client")?;
    let hello = replay_client.hello()?;
    let challenge = replay_host
        .handle(&hello, now)?
        .ok_or(ProtocolError::InvalidTransition {
            expected: "server_hello",
            actual: "no response",
        })?;
    let proof = replay_client.accept_server_hello(&challenge)?;
    let first = replay_host.handle(&proof, now)?;
    let replayed_proof_rejected = first.is_some()
        && matches!(
            replay_host.handle(&proof, now),
            Err(ProtocolError::UnexpectedSequence { .. })
        );

    // An expired offer must not authenticate.
    let mut expired_host = HostSession::new(
        "vista-host-expired",
        "Expired host",
        secret.clone(),
        PermissionSet::video_only(),
        now,
    )?;
    let mut expired_client = ClientSession::new(secret.clone(), "expired-client")?;
    let hello = expired_client.hello()?;
    let challenge = expired_host
        .handle(&hello, now)?
        .ok_or(ProtocolError::AuthenticationFailed)?;
    let proof = expired_client.accept_server_hello(&challenge)?;
    let expired_offer_rejected = matches!(
        expired_host.handle(&proof, now + PAIRING_OFFER_TTL_SECS + 1),
        Err(ProtocolError::ExpiredOffer)
    );

    // A client must not be able to escalate to input after a video-only grant.
    let mut escalation_host = HostSession::new(
        "vista-host-escalation",
        "Escalation host",
        secret.clone(),
        PermissionSet::video_only(),
        now,
    )?;
    let mut escalation_client = ClientSession::new(secret.clone(), "escalation-client")?;
    let hello = escalation_client.hello()?;
    let challenge = escalation_host
        .handle(&hello, now)?
        .ok_or(ProtocolError::AuthenticationFailed)?;
    let proof = escalation_client.accept_server_hello(&challenge)?;
    let accepted = escalation_host
        .handle(&proof, now)?
        .ok_or(ProtocolError::AuthenticationFailed)?;
    escalation_client.accept_pairing(&accepted)?;
    let requested = PermissionSet {
        video: true,
        input: true,
        ..PermissionSet::default()
    };
    let escalation = Envelope::new(
        escalation_client.next_client_seq(),
        ControlMessage::SessionOpen {
            session_id: escalation_client
                .session_id()
                .unwrap_or_default()
                .to_string(),
            permissions: requested,
        },
    );
    let permission_escalation_rejected = matches!(
        escalation_host.handle(&escalation, now),
        Err(ProtocolError::PermissionDenied)
    );

    // A client that does not support version 1 must be rejected before pairing.
    let mut version_host = HostSession::new(
        "vista-host-version",
        "Version host",
        secret,
        PermissionSet::video_only(),
        now,
    )?;
    let unsupported = VersionRange {
        min: ProtocolVersion { major: 2, minor: 0 },
        max: ProtocolVersion { major: 2, minor: 0 },
    };
    let hello = Envelope::new(
        0,
        ControlMessage::ClientHello {
            versions: unsupported,
            client_id: "future-client".into(),
            client_nonce: "11".repeat(NONCE_BYTES),
        },
    );
    let version_mismatch_rejected = matches!(
        version_host.handle(&hello, now),
        Err(ProtocolError::UnsupportedVersion)
    );

    let verified = keys_match
        && host.state() == HostState::Active
        && client.state() == ClientState::Active
        && wrong_secret_rejected
        && replayed_proof_rejected
        && expired_offer_rejected
        && permission_escalation_rejected
        && version_mismatch_rejected;

    Ok(ProtocolReport {
        protocol: PROTOCOL_VERSION.to_string(),
        pairing_id: host.pairing_id().to_string(),
        client_frames,
        host_frames,
        wire_bytes,
        keys_match,
        key_fingerprint: fingerprint,
        permissions: host.permissions(),
        host_state: state_name_host(host.state()),
        client_state: state_name_client(client.state()),
        wrong_secret_rejected,
        replayed_proof_rejected,
        expired_offer_rejected,
        permission_escalation_rejected,
        version_mismatch_rejected,
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paired() -> (HostSession, ClientSession, ProtocolVersion) {
        let secret = PairingSecret::from_bytes([7u8; SECRET_BYTES]);
        let now = 1_000_000;
        let mut host = HostSession::new(
            "test-host",
            "Test host",
            secret.clone(),
            PermissionSet::video_only(),
            now,
        )
        .unwrap();
        let mut client = ClientSession::new(secret, "test-client").unwrap();
        let hello = client.hello().unwrap();
        let challenge = host.handle(&hello, now).unwrap().unwrap();
        let proof = client.accept_server_hello(&challenge).unwrap();
        let accepted = host.handle(&proof, now).unwrap().unwrap();
        client.accept_pairing(&accepted).unwrap();
        let selected = match challenge.body {
            ControlMessage::ServerHello { selected, .. } => selected,
            _ => unreachable!(),
        };
        (host, client, selected)
    }

    #[test]
    fn full_pairing_derives_matching_session_keys_and_opens_video() {
        let (mut host, mut client, version) = paired();
        assert_eq!(version, PROTOCOL_VERSION);
        assert_eq!(host.state(), HostState::Paired);
        assert_eq!(client.state(), ClientState::Paired);
        assert_eq!(host.session_keys().unwrap(), client.session_keys().unwrap());
        let open = client.open_session(PermissionSet::video_only()).unwrap();
        let opened = host.handle(&open, 1_000_001).unwrap().unwrap();
        client.accept_session_opened(&opened).unwrap();
        assert_eq!(host.state(), HostState::Active);
        assert_eq!(client.state(), ClientState::Active);
        assert!(host.permissions().video);
        assert!(!host.permissions().input);
        assert!(!host.permissions().clipboard);
    }

    #[test]
    fn wrong_secret_cannot_complete_pairing() {
        let now = 1_000_000;
        let secret = PairingSecret::from_bytes([1u8; SECRET_BYTES]);
        let wrong = PairingSecret::from_bytes([2u8; SECRET_BYTES]);
        let mut host =
            HostSession::new("host", "Host", secret, PermissionSet::video_only(), now).unwrap();
        let mut client = ClientSession::new(wrong, "client").unwrap();
        let hello = client.hello().unwrap();
        let challenge = host.handle(&hello, now).unwrap().unwrap();
        let proof = client.accept_server_hello(&challenge).unwrap();
        assert_eq!(
            host.handle(&proof, now),
            Err(ProtocolError::AuthenticationFailed)
        );
        assert_eq!(host.state(), HostState::Failed);
        assert!(host.session_keys().is_none());
    }

    #[test]
    fn replayed_or_out_of_order_client_frames_are_rejected() {
        let (mut host, _client, _) = paired();
        // The client is paired, so replaying the pairing proof is an ordering error.
        let replay = Envelope::new(
            1,
            ControlMessage::PairingRequest {
                pairing_id: host.pairing_id().to_string(),
                tag: "00".repeat(TAG_BYTES),
            },
        );
        assert!(matches!(
            host.handle(&replay, 1_000_001),
            Err(ProtocolError::UnexpectedSequence {
                expected: 2,
                actual: 1
            })
        ));
        // A skipped sequence value is rejected on a separate, still-paired host.
        let (mut host, client, _) = paired();
        let skipped = Envelope::new(
            4,
            ControlMessage::SessionOpen {
                session_id: client.session_id().unwrap().to_string(),
                permissions: PermissionSet::video_only(),
            },
        );
        assert!(matches!(
            host.handle(&skipped, 1_000_001),
            Err(ProtocolError::UnexpectedSequence {
                expected: 2,
                actual: 4
            })
        ));
    }

    #[test]
    fn expired_offer_is_rejected() {
        let now = 1_000_000;
        let secret = PairingSecret::from_bytes([3u8; SECRET_BYTES]);
        let mut host = HostSession::new(
            "host",
            "Host",
            secret.clone(),
            PermissionSet::video_only(),
            now,
        )
        .unwrap();
        let mut client = ClientSession::new(secret, "client").unwrap();
        let hello = client.hello().unwrap();
        let challenge = host.handle(&hello, now).unwrap().unwrap();
        let proof = client.accept_server_hello(&challenge).unwrap();
        assert_eq!(
            host.handle(&proof, now + PAIRING_OFFER_TTL_SECS + 1),
            Err(ProtocolError::ExpiredOffer)
        );
        assert_eq!(host.state(), HostState::Failed);
    }

    #[test]
    fn unsupported_version_is_rejected_before_pairing() {
        let secret = PairingSecret::from_bytes([4u8; SECRET_BYTES]);
        let mut host = HostSession::new(
            "host",
            "Host",
            secret,
            PermissionSet::video_only(),
            1_000_000,
        )
        .unwrap();
        let hello = Envelope::new(
            0,
            ControlMessage::ClientHello {
                versions: VersionRange {
                    min: ProtocolVersion { major: 2, minor: 0 },
                    max: ProtocolVersion { major: 2, minor: 0 },
                },
                client_id: "client".into(),
                client_nonce: "11".repeat(NONCE_BYTES),
            },
        );
        assert_eq!(
            host.handle(&hello, 1_000_000),
            Err(ProtocolError::UnsupportedVersion)
        );
    }

    #[test]
    fn input_and_clipboard_require_an_explicit_grant() {
        let (mut host, client, _) = paired();
        let session_id = client.session_id().unwrap().to_string();
        // Input is not in the default policy even though video is.
        let escalated = Envelope::new(
            2,
            ControlMessage::SessionOpen {
                session_id: session_id.clone(),
                permissions: PermissionSet {
                    video: true,
                    input: true,
                    ..PermissionSet::default()
                },
            },
        );
        assert_eq!(
            host.handle(&escalated, 1_000_001),
            Err(ProtocolError::PermissionDenied)
        );
        assert_eq!(host.state(), HostState::Failed);

        // Clipboard is likewise denied by default.
        let (mut clipboard_host, clipboard_client, _) = paired();
        let clipboard_request = Envelope::new(
            2,
            ControlMessage::SessionOpen {
                session_id: clipboard_client.session_id().unwrap().to_string(),
                permissions: PermissionSet {
                    video: true,
                    clipboard: true,
                    ..PermissionSet::default()
                },
            },
        );
        assert_eq!(
            clipboard_host.handle(&clipboard_request, 1_000_001),
            Err(ProtocolError::PermissionDenied)
        );
    }

    #[test]
    fn messages_cannot_skip_the_handshake_states() {
        let secret = PairingSecret::from_bytes([6u8; SECRET_BYTES]);
        let mut host = HostSession::new(
            "host",
            "Host",
            secret,
            PermissionSet::video_only(),
            1_000_000,
        )
        .unwrap();
        let open = Envelope::new(
            0,
            ControlMessage::SessionOpen {
                session_id: "00".repeat(SESSION_ID_BYTES),
                permissions: PermissionSet::video_only(),
            },
        );
        assert!(matches!(
            host.handle(&open, 1_000_000),
            Err(ProtocolError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn frames_are_bounded_and_round_trip() {
        let envelope = Envelope::new(7, ControlMessage::Ping { nonce: 42 });
        let bytes = envelope.encode().unwrap();
        assert_eq!(Envelope::decode(&bytes).unwrap(), envelope);
        assert_eq!(
            Envelope::decode(&bytes[..3]),
            Err(ProtocolError::FrameTruncated)
        );
        let mut truncated = bytes.clone();
        truncated.pop();
        assert_eq!(
            Envelope::decode(&truncated),
            Err(ProtocolError::FrameLengthMismatch)
        );
        let oversized = (MAX_CONTROL_FRAME_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(
            Envelope::decode(&oversized),
            Err(ProtocolError::FrameTooLarge)
        );
        let bad_json = [0, 0, 0, 2, b'{', b'}'];
        assert_eq!(
            Envelope::decode(&bad_json),
            Err(ProtocolError::MalformedJson)
        );
    }

    #[test]
    fn identifiers_and_nonces_are_validated() {
        assert!(validate_identifier("valid id").is_ok());
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier(&"x".repeat(MAX_IDENTIFIER_BYTES + 1)).is_err());
        assert!(validate_identifier("line\nbreak").is_err());
        assert!(PairingSecret::from_hex(&"00".repeat(SECRET_BYTES)).is_ok());
        assert!(PairingSecret::from_hex("00").is_err());
    }

    #[test]
    fn diagnostic_covers_the_negative_paths() {
        let report = diagnostic().unwrap();
        assert!(report.verified, "{report:?}");
        assert!(report.keys_match);
        assert!(report.permission_escalation_rejected);
        assert!(report.replayed_proof_rejected);
        assert!(report.expired_offer_rejected);
        assert!(report.wrong_secret_rejected);
        assert!(report.version_mismatch_rejected);
        assert_eq!(report.protocol, PROTOCOL_VERSION.to_string());
        assert!(report.permissions.video);
        assert!(!report.permissions.input);
    }
}
