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

use crate::control_dispatch::HostControlChannel;

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
pub const MAX_CONTROL_FRAME_BYTES: usize = 64 * 1024;
pub const PAIRING_OFFER_TTL_SECS: u64 = 120;
/// Minimum time between accepted consumer keyframe requests.
pub const MIN_KEYFRAME_INTERVAL_SECS: u64 = 1;
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Optional authenticated media endpoint announced after the control
    /// session becomes active. The client reuses the pairing secret to perform
    /// a second handshake on this endpoint; media keys are still derived from
    /// that media session and are never sent on the wire.
    #[serde(rename = "media.offer")]
    MediaOffer {
        v: u16,
        port: u16,
        pairing_id: String,
        expires_at: u64,
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
    /// Capability/action contract messages carried over the authenticated control
    /// channel. The capability module owns validation; these variants only
    /// define the wire shape so the contract is not limited to a local diagnostic.
    #[serde(rename = "capabilities.snapshot")]
    CapabilitySnapshot {
        v: u16,
        connection_epoch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        target_id: String,
        target_generation: String,
        capability_revision: u64,
        grant_epoch: String,
        actions: Vec<crate::capability::ActionDescriptor>,
    },
    #[serde(rename = "action.invoke")]
    ActionInvoke {
        v: u16,
        id: String,
        connection_epoch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        grant_epoch: String,
        target_id: String,
        target_generation: String,
        capability_revision: u64,
        action: String,
        schema_version: u16,
        #[serde(default)]
        parameters: std::collections::BTreeMap<String, serde_json::Value>,
    },
    #[serde(rename = "action.result")]
    ActionResult {
        v: u16,
        id: String,
        connection_epoch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        status: crate::capability::ActionStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<crate::capability::ContractErrorCode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability_revision: Option<u64>,
    },
    /// Bounded host-owned window catalog. Metadata only; no capability grant.
    #[serde(rename = "catalog.snapshot")]
    CatalogSnapshot {
        v: u16,
        connection_epoch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        generation: String,
        revision: u64,
        entries: Vec<crate::catalog_wire::CatalogEntry>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        workspaces: Vec<crate::catalog_wire::CatalogWorkspace>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<crate::catalog_wire::CatalogOutput>,
    },
    /// Ask the host for a fresh catalog (for example after a window closes).
    #[serde(rename = "catalog.request")]
    CatalogRequest {
        v: u16,
        session_id: String,
    },
    /// Select the window, virtual desktop, or active output that should be
    /// captured and encoded. Selection is separate from capability grants.
    #[serde(rename = "stream.select")]
    StreamSelect {
        v: u16,
        session_id: String,
        target: StreamTarget,
    },
    #[serde(rename = "stream.stop")]
    StreamStop {
        v: u16,
        session_id: String,
    },
    /// The producer has emitted its first encoded frame at the reported size.
    #[serde(rename = "stream.started")]
    StreamStarted {
        v: u16,
        session_id: String,
        target: StreamTarget,
        width: u32,
        height: u32,
        geometry_epoch: u64,
        encoder: String,
    },
    #[serde(rename = "stream.stopped")]
    StreamStopped {
        v: u16,
        session_id: String,
        reason: String,
    },
    #[serde(rename = "stream.error")]
    StreamError {
        v: u16,
        session_id: String,
        code: String,
        message: String,
    },
    /// Portable pointer/keyboard/text input, authorized by the session input
    /// permission and the currently selected stream geometry.
    #[serde(rename = "input.event")]
    InputEvent {
        v: u16,
        session_id: String,
        event: crate::capability::InputEvent,
    },
}

/// One selectable stream source. `Desktop` means "whatever the focused output
/// currently shows"; `Workspace` is a Niri virtual desktop and `Window` is a
/// single Niri window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamTarget {
    Desktop,
    Workspace { id: u64 },
    Window { id: u64 },
}

impl StreamTarget {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Workspace { .. } => "workspace",
            Self::Window { .. } => "window",
        }
    }

    pub fn identifier(&self) -> String {
        match self {
            Self::Desktop => "desktop".into(),
            Self::Workspace { id } => format!("ws{id}"),
            Self::Window { id } => format!("w{id}"),
        }
    }
}

impl ControlMessage {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::ClientHello { .. } => "client_hello",
            Self::ServerHello { .. } => "server_hello",
            Self::PairingRequest { .. } => "pairing_request",
            Self::PairingAccepted { .. } => "pairing_accepted",
            Self::PairingRejected { .. } => "pairing_rejected",
            Self::SessionOpen { .. } => "session_open",
            Self::SessionOpened { .. } => "session_opened",
            Self::MediaOffer { .. } => "media.offer",
            Self::KeyframeRequest { .. } => "keyframe_request",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::SessionClose { .. } => "session_close",
            Self::Error { .. } => "error",
            Self::CapabilitySnapshot { .. } => "capabilities.snapshot",
            Self::ActionInvoke { .. } => "action.invoke",
            Self::ActionResult { .. } => "action.result",
            Self::CatalogSnapshot { .. } => "catalog.snapshot",
            Self::CatalogRequest { .. } => "catalog.request",
            Self::StreamSelect { .. } => "stream.select",
            Self::StreamStop { .. } => "stream.stop",
            Self::StreamStarted { .. } => "stream.started",
            Self::StreamStopped { .. } => "stream.stopped",
            Self::StreamError { .. } => "stream.error",
            Self::InputEvent { .. } => "input.event",
        }
    }
}

impl From<crate::capability::CapabilitySnapshot> for ControlMessage {
    fn from(snapshot: crate::capability::CapabilitySnapshot) -> Self {
        Self::CapabilitySnapshot {
            v: snapshot.v,
            connection_epoch: snapshot.connection_epoch,
            session_id: snapshot.session_id,
            target_id: snapshot.target_id,
            target_generation: snapshot.target_generation,
            capability_revision: snapshot.capability_revision,
            grant_epoch: snapshot.grant_epoch,
            actions: snapshot.actions,
        }
    }
}

impl TryFrom<&ControlMessage> for crate::capability::CapabilitySnapshot {
    type Error = crate::capability::ContractError;

    fn try_from(message: &ControlMessage) -> Result<Self, Self::Error> {
        let ControlMessage::CapabilitySnapshot {
            v,
            connection_epoch,
            session_id,
            target_id,
            target_generation,
            capability_revision,
            grant_epoch,
            actions,
        } = message
        else {
            return Err(crate::capability::ContractError::new(
                crate::capability::ContractErrorCode::MalformedMessage,
                "not a capabilities.snapshot message",
            ));
        };
        let snapshot = crate::capability::CapabilitySnapshot {
            v: *v,
            message_type: crate::capability::SNAPSHOT_MESSAGE_TYPE.into(),
            connection_epoch: connection_epoch.clone(),
            session_id: session_id.clone(),
            target_id: target_id.clone(),
            target_generation: target_generation.clone(),
            capability_revision: *capability_revision,
            grant_epoch: grant_epoch.clone(),
            actions: actions.clone(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl From<crate::capability::ActionRequest> for ControlMessage {
    fn from(request: crate::capability::ActionRequest) -> Self {
        Self::ActionInvoke {
            v: request.v,
            id: request.id,
            connection_epoch: request.connection_epoch,
            session_id: request.session_id,
            grant_epoch: request.grant_epoch,
            target_id: request.target_id,
            target_generation: request.target_generation,
            capability_revision: request.capability_revision,
            action: request.action,
            schema_version: request.schema_version,
            parameters: request.parameters,
        }
    }
}

impl TryFrom<&ControlMessage> for crate::capability::ActionRequest {
    type Error = crate::capability::ContractError;

    fn try_from(message: &ControlMessage) -> Result<Self, Self::Error> {
        let ControlMessage::ActionInvoke {
            v,
            id,
            connection_epoch,
            session_id,
            grant_epoch,
            target_id,
            target_generation,
            capability_revision,
            action,
            schema_version,
            parameters,
        } = message
        else {
            return Err(crate::capability::ContractError::new(
                crate::capability::ContractErrorCode::MalformedMessage,
                "not an action.invoke message",
            ));
        };
        let request = crate::capability::ActionRequest {
            v: *v,
            message_type: crate::capability::ACTION_REQUEST_MESSAGE_TYPE.into(),
            id: id.clone(),
            connection_epoch: connection_epoch.clone(),
            session_id: session_id.clone(),
            grant_epoch: grant_epoch.clone(),
            target_id: target_id.clone(),
            target_generation: target_generation.clone(),
            capability_revision: *capability_revision,
            action: action.clone(),
            schema_version: *schema_version,
            parameters: parameters.clone(),
        };
        request.validate()?;
        Ok(request)
    }
}

impl From<crate::capability::ActionResult> for ControlMessage {
    fn from(result: crate::capability::ActionResult) -> Self {
        Self::ActionResult {
            v: result.v,
            id: result.id,
            connection_epoch: result.connection_epoch,
            session_id: result.session_id,
            status: result.status,
            error_code: result.error_code,
            state: result.state,
            capability_revision: result.capability_revision,
        }
    }
}

impl TryFrom<&ControlMessage> for crate::capability::ActionResult {
    type Error = crate::capability::ContractError;

    fn try_from(message: &ControlMessage) -> Result<Self, Self::Error> {
        let ControlMessage::ActionResult {
            v,
            id,
            connection_epoch,
            session_id,
            status,
            error_code,
            state,
            capability_revision,
        } = message
        else {
            return Err(crate::capability::ContractError::new(
                crate::capability::ContractErrorCode::MalformedMessage,
                "not an action.result message",
            ));
        };
        Ok(crate::capability::ActionResult {
            v: *v,
            message_type: crate::capability::ACTION_RESULT_MESSAGE_TYPE.into(),
            id: id.clone(),
            connection_epoch: connection_epoch.clone(),
            session_id: session_id.clone(),
            status: *status,
            error_code: *error_code,
            state: state.clone(),
            capability_revision: *capability_revision,
        })
    }
}

impl From<crate::catalog_wire::CatalogSnapshot> for ControlMessage {
    fn from(snapshot: crate::catalog_wire::CatalogSnapshot) -> Self {
        Self::CatalogSnapshot {
            v: snapshot.v,
            connection_epoch: snapshot.connection_epoch,
            session_id: snapshot.session_id,
            generation: snapshot.generation,
            revision: snapshot.revision,
            entries: snapshot.entries,
            workspaces: snapshot.workspaces,
            output: snapshot.output,
        }
    }
}

impl TryFrom<&ControlMessage> for crate::catalog_wire::CatalogSnapshot {
    type Error = crate::catalog_wire::CatalogWireError;

    fn try_from(message: &ControlMessage) -> Result<Self, Self::Error> {
        let ControlMessage::CatalogSnapshot {
            v,
            connection_epoch,
            session_id,
            generation,
            revision,
            entries,
            workspaces,
            output,
        } = message
        else {
            return Err(crate::catalog_wire::CatalogWireError::new(
                "not a catalog.snapshot message",
            ));
        };
        let snapshot = Self {
            v: *v,
            message_type: crate::catalog_wire::CATALOG_MESSAGE_TYPE.into(),
            connection_epoch: connection_epoch.clone(),
            session_id: session_id.clone(),
            generation: generation.clone(),
            revision: *revision,
            entries: entries.clone(),
            workspaces: workspaces.clone(),
            output: output.clone(),
        };
        snapshot
            .validate()
            .map_err(crate::catalog_wire::CatalogWireError::new)?;
        Ok(snapshot)
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
    last_keyframe_at: Option<u64>,
    rate_limited_keyframe_requests: u64,
    control: Option<HostControlChannel>,
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
            last_keyframe_at: None,
            rate_limited_keyframe_requests: 0,
            control: None,
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

    pub fn rate_limited_keyframe_requests(&self) -> u64 {
        self.rate_limited_keyframe_requests
    }

    /// Attach the session-bound capability/action dispatcher. The channel must
    /// target the authenticated session: its connection epoch and session id are
    /// both the pairing-assigned session id, so a message cannot cross sessions.
    /// Attaching after `SessionOpened` keeps the grant explicit and revocable.
    pub fn attach_control(&mut self, channel: HostControlChannel) -> Result<(), ProtocolError> {
        if self.state != HostState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let session_id = self
            .session_id
            .as_deref()
            .ok_or(ProtocolError::SessionMismatch)?;
        if channel.session_id() != Some(session_id) || channel.connection_epoch() != session_id {
            return Err(ProtocolError::SessionMismatch);
        }
        self.control = Some(channel);
        Ok(())
    }

    /// Capability snapshot bound to the current lease grant. Returns `None` when
    /// no dispatcher is attached. The caller sends the envelope; the sequence
    /// number stays consistent with every other host message.
    pub fn control_snapshot(&mut self) -> Result<Option<Envelope>, ProtocolError> {
        let Some(control) = self.control.as_mut() else {
            return Ok(None);
        };
        let message = control
            .snapshot_message()
            .map_err(|_| ProtocolError::PermissionDenied)?;
        Ok(Some(self.reply(message)))
    }

    pub fn control(&self) -> Option<&HostControlChannel> {
        self.control.as_ref()
    }

    pub fn control_mut(&mut self) -> Option<&mut HostControlChannel> {
        self.control.as_mut()
    }

    /// Emit a bounded catalog snapshot for the authenticated session. The
    /// snapshot must target this session; it never grants capabilities.
    pub fn emit_catalog(
        &mut self,
        snapshot: crate::catalog_wire::CatalogSnapshot,
    ) -> Result<Envelope, ProtocolError> {
        let session_id = self
            .session_id
            .as_deref()
            .ok_or(ProtocolError::SessionMismatch)?;
        if snapshot.session_id.as_deref() != Some(session_id)
            || snapshot.connection_epoch != session_id
        {
            return Err(ProtocolError::SessionMismatch);
        }
        snapshot
            .validate()
            .map_err(|_| ProtocolError::InvalidTransition {
                expected: "a valid catalog snapshot",
                actual: "an invalid catalog snapshot",
            })?;
        Ok(self.reply(ControlMessage::from(snapshot)))
    }

    /// Emit an authenticated media endpoint announcement for the active
    /// control session. The endpoint is deliberately opt-in because the
    /// current producer is a bounded H.264 diagnostic rather than a full
    /// production capture service.
    pub fn emit_media_offer(
        &mut self,
        port: u16,
        pairing_id: impl Into<String>,
        expires_at: u64,
    ) -> Result<Envelope, ProtocolError> {
        if self.state != HostState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let pairing_id = pairing_id.into();
        validate_identifier(&pairing_id)?;
        Ok(self.reply(ControlMessage::MediaOffer {
            v: 1,
            port,
            pairing_id,
            expires_at,
        }))
    }

    /// Emit an authenticated, sequence-correct message to the active client.
    /// Callers are responsible for only sending messages valid in the current
    /// session; this is used by the live service for stream and catalog events.
    pub fn emit(&mut self, body: ControlMessage) -> Result<Envelope, ProtocolError> {
        if self.state != HostState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        Ok(self.reply(body))
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
                if let Some(last) = self.last_keyframe_at
                    && now_secs.saturating_sub(last) < MIN_KEYFRAME_INTERVAL_SECS
                {
                    self.rate_limited_keyframe_requests =
                        self.rate_limited_keyframe_requests.saturating_add(1);
                    return Ok(None);
                }
                self.last_keyframe_at = Some(now_secs);
                self.keyframe_requests = self.keyframe_requests.saturating_add(1);
                Ok(None)
            }
            (HostState::Active, ControlMessage::ActionInvoke { .. }) => {
                let Some(control) = self.control.as_mut() else {
                    return Err(ProtocolError::InvalidTransition {
                        expected: "a capability-enabled session",
                        actual: "action.invoke",
                    });
                };
                let reply = control.handle(&envelope.body, now_secs);
                Ok(reply.map(|body| self.reply(body)))
            }
            (
                HostState::Active,
                ControlMessage::StreamSelect { session_id, .. }
                | ControlMessage::StreamStop { session_id, .. }
                | ControlMessage::CatalogRequest { session_id, .. }
                | ControlMessage::InputEvent { session_id, .. },
            ) => {
                if self.session_id.as_deref() != Some(session_id.as_str()) {
                    return Err(ProtocolError::SessionMismatch);
                }
                if !self.permissions.video {
                    return Ok(Some(self.reply(ControlMessage::Error {
                        code: "permission_denied".into(),
                        message: "the session does not grant video".into(),
                    })));
                }
                // The live service performs capture, injection and catalog work.
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

    /// Consume one host `catalog.snapshot` and advance the host sequence.
    pub fn accept_catalog_snapshot(
        &mut self,
        envelope: &Envelope,
    ) -> Result<crate::catalog_wire::CatalogSnapshot, ProtocolError> {
        self.check_host_sequence(envelope)?;
        crate::catalog_wire::CatalogSnapshot::try_from(&envelope.body).map_err(|_| {
            ProtocolError::InvalidTransition {
                expected: "catalog.snapshot",
                actual: envelope.body.name(),
            }
        })
    }

    /// Consume one optional media endpoint announcement and advance the host
    /// sequence just like every other host-owned message.
    pub fn accept_media_offer(&mut self, envelope: &Envelope) -> Result<(), ProtocolError> {
        self.check_host_sequence(envelope)?;
        match &envelope.body {
            ControlMessage::MediaOffer { .. } => Ok(()),
            other => Err(ProtocolError::InvalidTransition {
                expected: "media.offer",
                actual: other.name(),
            }),
        }
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

    pub fn select_stream(&mut self, target: StreamTarget) -> Result<Envelope, ProtocolError> {
        self.require_active()?;
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::StreamSelect {
                v: 1,
                session_id,
                target,
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn stop_stream(&mut self) -> Result<Envelope, ProtocolError> {
        self.require_active()?;
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::StreamStop {
                v: 1,
                session_id,
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn request_catalog(&mut self) -> Result<Envelope, ProtocolError> {
        self.require_active()?;
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::CatalogRequest {
                v: 1,
                session_id,
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    pub fn input_event(
        &mut self,
        event: crate::capability::InputEvent,
    ) -> Result<Envelope, ProtocolError> {
        self.require_active()?;
        let session_id = self
            .session_id
            .clone()
            .ok_or(ProtocolError::SessionMismatch)?;
        let envelope = Envelope::new(
            self.next_client_seq,
            ControlMessage::InputEvent {
                v: 1,
                session_id,
                event,
            },
        );
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    /// Consume `stream.started`, returning target, dimensions, geometry epoch
    /// and encoder name. The geometry epoch changes on every new stream start
    /// so input can never be applied with stale coordinates.
    pub fn accept_stream_started(
        &mut self,
        envelope: &Envelope,
    ) -> Result<(StreamTarget, u32, u32, u64, String), ProtocolError> {
        self.check_host_sequence(envelope)?;
        match &envelope.body {
            ControlMessage::StreamStarted {
                target,
                width,
                height,
                geometry_epoch,
                encoder,
                ..
            } => Ok((*target, *width, *height, *geometry_epoch, encoder.clone())),
            other => Err(ProtocolError::InvalidTransition {
                expected: "stream.started",
                actual: other.name(),
            }),
        }
    }

    pub fn accept_stream_stopped(&mut self, envelope: &Envelope) -> Result<String, ProtocolError> {
        self.check_host_sequence(envelope)?;
        match &envelope.body {
            ControlMessage::StreamStopped { reason, .. } => Ok(reason.clone()),
            other => Err(ProtocolError::InvalidTransition {
                expected: "stream.stopped",
                actual: other.name(),
            }),
        }
    }

    pub fn accept_stream_error(&mut self, envelope: &Envelope) -> Result<String, ProtocolError> {
        self.check_host_sequence(envelope)?;
        match &envelope.body {
            ControlMessage::StreamError { code, message, .. } => {
                Ok(format!("{code}: {message}"))
            }
            other => Err(ProtocolError::InvalidTransition {
                expected: "stream.error",
                actual: other.name(),
            }),
        }
    }

    fn require_active(&self) -> Result<(), ProtocolError> {
        if self.state != ClientState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        Ok(())
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

    /// Send one semantic action request over the authenticated session.
    /// The host revalidates the request against its own lease; a client-supplied
    /// context is never treated as authorization.
    pub fn action_invoke(
        &mut self,
        request: &crate::capability::ActionRequest,
    ) -> Result<Envelope, ProtocolError> {
        if self.state != ClientState::Active {
            return Err(ProtocolError::InvalidTransition {
                expected: "an active session",
                actual: "another state",
            });
        }
        let envelope = Envelope::new(self.next_client_seq, ControlMessage::from(request.clone()));
        self.next_client_seq = self.next_client_seq.wrapping_add(1);
        Ok(envelope)
    }

    /// Consume one host `action.result` and advance the host sequence.
    pub fn accept_action_result(
        &mut self,
        envelope: &Envelope,
    ) -> Result<crate::capability::ActionResult, ProtocolError> {
        self.check_host_sequence(envelope)?;
        crate::capability::ActionResult::try_from(&envelope.body).map_err(|_| {
            ProtocolError::InvalidTransition {
                expected: "action.result",
                actual: envelope.body.name(),
            }
        })
    }

    /// Consume one host `capabilities.snapshot` and advance the host sequence.
    pub fn accept_capability_snapshot(
        &mut self,
        envelope: &Envelope,
    ) -> Result<crate::capability::CapabilitySnapshot, ProtocolError> {
        self.check_host_sequence(envelope)?;
        crate::capability::CapabilitySnapshot::try_from(&envelope.body).map_err(|_| {
            ProtocolError::InvalidTransition {
                expected: "capabilities.snapshot",
                actual: envelope.body.name(),
            }
        })
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
    fn keyframe_requests_are_rate_limited() {
        let (mut host, mut client, _) = paired();
        let open = client.open_session(PermissionSet::video_only()).unwrap();
        let opened = host.handle(&open, 1_000_000).unwrap().unwrap();
        client.accept_session_opened(&opened).unwrap();

        let first = client.keyframe_request().unwrap();
        host.handle(&first, 1_000_001).unwrap();
        let second = client.keyframe_request().unwrap();
        host.handle(&second, 1_000_001).unwrap();
        assert_eq!(host.keyframe_requests(), 1);
        assert_eq!(host.rate_limited_keyframe_requests(), 1);

        let third = client.keyframe_request().unwrap();
        host.handle(&third, 1_000_002).unwrap();
        assert_eq!(host.keyframe_requests(), 2);
        assert_eq!(host.rate_limited_keyframe_requests(), 1);
    }

    #[test]
    fn capability_messages_round_trip_over_control_frames() {
        use crate::capability::{
            ActionDescriptor, ActionPresentation, ActionRequest, ActionResult, ActionStatus,
            Authorization, CapabilitySnapshot, TargetRef,
        };

        let target = TargetRef::new("w5", "niri-boot1-w5").unwrap();
        let snapshot = CapabilitySnapshot::new(
            "c1",
            Some("s1".into()),
            "g2",
            &target,
            12,
            vec![ActionDescriptor {
                id: "window.focus".into(),
                schema_version: 1,
                supported: true,
                enabled: true,
                reason: None,
                authorization: Authorization::Granted,
                parameters: "vista.empty.v1".into(),
                presentation: ActionPresentation::new("Focus window"),
                suggested_bindings: Vec::new(),
            }],
        )
        .unwrap();

        let envelope = Envelope::new(0, ControlMessage::from(snapshot.clone()));
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "capabilities.snapshot");
        assert_eq!(json["actions"][0]["id"], "window.focus");
        let bytes = envelope.encode().unwrap();
        assert!(bytes.len() <= MAX_CONTROL_FRAME_BYTES + 4);
        let decoded = Envelope::decode(&bytes).unwrap();
        let round: CapabilitySnapshot = (&decoded.body).try_into().unwrap();
        assert_eq!(round, snapshot);

        let context = crate::capability::ActionContext {
            connection_epoch: "c1".into(),
            session_id: Some("s1".into()),
            grant_epoch: "g2".into(),
            target: target.clone(),
            capability_revision: 12,
            grants: crate::capability::ActionGrants::default(),
        };
        let request = ActionRequest::new(
            "r17",
            &context,
            "window.focus",
            1,
            std::collections::BTreeMap::new(),
        )
        .unwrap();
        let envelope = Envelope::new(1, ControlMessage::from(request.clone()));
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "action.invoke");
        assert_eq!(json["action"], "window.focus");
        let round: ActionRequest = (&envelope.body).try_into().unwrap();
        assert_eq!(round, request);

        let result = ActionResult {
            v: 1,
            message_type: crate::capability::ACTION_RESULT_MESSAGE_TYPE.into(),
            id: "r17".into(),
            connection_epoch: "c1".into(),
            session_id: Some("s1".into()),
            status: ActionStatus::Completed,
            error_code: None,
            state: Some(serde_json::json!({ "maximized": true })),
            capability_revision: Some(13),
        };
        let envelope = Envelope::new(2, ControlMessage::from(result.clone()));
        let round: ActionResult = (&envelope.body).try_into().unwrap();
        assert_eq!(round, result);

        assert!(CapabilitySnapshot::try_from(&ControlMessage::Ping { nonce: 1 }).is_err());
    }

    #[test]
    fn shared_pairing_vector_matches_the_swift_implementation() {
        #[derive(Deserialize)]
        struct Policy {
            video: bool,
            audio: bool,
            input: bool,
            clipboard: bool,
        }
        #[derive(Deserialize)]
        struct Vector {
            secret_hex: String,
            client_id: String,
            client_nonce: String,
            host_id: String,
            host_nonce: String,
            pairing_id: String,
            selected: ProtocolVersion,
            policy: Policy,
            transcript_hex: String,
            client_tag: String,
            session_id: String,
            host_tag: String,
            client_to_host: String,
            host_to_client: String,
        }

        let vector: Vector =
            serde_json::from_str(include_str!("../../docs/fixtures/pairing-v1.json")).unwrap();
        let secret = PairingSecret::from_hex(&vector.secret_hex).unwrap();
        let policy = PermissionSet {
            video: vector.policy.video,
            audio: vector.policy.audio,
            input: vector.policy.input,
            clipboard: vector.policy.clipboard,
        };
        let transcript = transcript(
            vector.selected,
            &vector.client_id,
            &vector.client_nonce,
            &vector.host_id,
            &vector.host_nonce,
            &vector.pairing_id,
            policy,
        );
        assert_eq!(hex::encode(&transcript), vector.transcript_hex);
        assert_eq!(
            hex::encode(tag(&secret, b"client-confirm\n", &transcript)),
            vector.client_tag
        );
        assert_eq!(
            hex::encode(host_tag(&secret, &transcript, &vector.session_id)),
            vector.host_tag
        );
        let keys =
            SessionKeys::derive(&secret, &vector.pairing_id, &transcript, &vector.session_id)
                .unwrap();
        assert_eq!(hex::encode(keys.client_to_host()), vector.client_to_host);
        assert_eq!(hex::encode(keys.host_to_client()), vector.host_to_client);
    }

    #[test]
    fn catalog_snapshot_round_trips_after_session_open() {
        let (mut host, mut client, _) = paired();
        let open = client.open_session(PermissionSet::video_only()).unwrap();
        let opened = host.handle(&open, 1_000_001).unwrap().unwrap();
        client.accept_session_opened(&opened).unwrap();
        let session_id = host.session_id().unwrap().to_string();
        let window = crate::Window {
            id: 5,
            title: Some("Browser".into()),
            app_id: Some("firefox".into()),
            pid: Some(42),
            workspace_id: Some(2),
            is_focused: false,
            is_floating: false,
            is_urgent: false,
        };
        let snapshot = crate::catalog_wire::CatalogSnapshot::from_windows(
            &session_id,
            Some(session_id.clone()),
            "niri-test",
            1,
            &[window],
        )
        .unwrap();
        let envelope = host.emit_catalog(snapshot.clone()).unwrap();
        let decoded = client.accept_catalog_snapshot(&envelope).unwrap();
        assert_eq!(decoded.entries.len(), 1);
        assert_eq!(decoded.entries[0].window_id, 5);
        assert_eq!(decoded.entries[0].display_name(), "Browser");

        let mut wrong = snapshot;
        wrong.connection_epoch = "other-session".into();
        wrong.session_id = Some("other-session".into());
        assert_eq!(
            host.emit_catalog(wrong).unwrap_err(),
            ProtocolError::SessionMismatch
        );
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

    // --- capability/action dispatch through the authenticated session ---------

    use crate::capability::{
        ActionContext, ActionGrants, ActionRegistry, InputPermissions, TargetRef,
    };
    use crate::control_dispatch::HostControlChannel;

    fn active_with_input() -> (HostSession, ClientSession) {
        let secret = PairingSecret::from_bytes([9u8; SECRET_BYTES]);
        let now = 1_000_000;
        let policy = PermissionSet {
            video: true,
            input: true,
            ..PermissionSet::default()
        };
        let mut host =
            HostSession::new("test-host", "Test host", secret.clone(), policy, now).unwrap();
        let mut client = ClientSession::new(secret, "test-client").unwrap();
        let hello = client.hello().unwrap();
        let challenge = host.handle(&hello, now).unwrap().unwrap();
        let proof = client.accept_server_hello(&challenge).unwrap();
        let accepted = host.handle(&proof, now).unwrap().unwrap();
        client.accept_pairing(&accepted).unwrap();
        let open = client.open_session(policy).unwrap();
        let opened = host.handle(&open, now).unwrap().unwrap();
        client.accept_session_opened(&opened).unwrap();
        (host, client)
    }

    fn channel_for(session_id: &str, revision: u64) -> HostControlChannel {
        let lease = crate::control_lease::ControlLease::grant(
            session_id,
            Some(session_id.to_string()),
            TargetRef::new("w5", "niri-test-w5").unwrap(),
            ActionGrants {
                desktop_actions: false,
                window_actions: true,
                clipboard: false,
            },
            InputPermissions::default(),
        )
        .unwrap();
        HostControlChannel::new(
            ActionRegistry::new(Box::new(crate::capability::FakeActionBackend::default())
                as Box<dyn crate::capability::ActionBackend + Send>),
            lease,
            revision,
        )
        .unwrap()
    }

    #[test]
    fn attach_control_requires_the_authenticated_session() {
        let (mut host, _client) = active_with_input();
        let session_id = host.session_id().unwrap().to_string();
        let error = host
            .attach_control(channel_for("other-session", 2))
            .unwrap_err();
        assert_eq!(error, ProtocolError::SessionMismatch);
        host.attach_control(channel_for(&session_id, 2)).unwrap();
        assert!(host.control().is_some());
    }

    #[test]
    fn action_invoke_routes_through_the_session_and_advances_sequences() {
        let (mut host, mut client) = active_with_input();
        let session_id = host.session_id().unwrap().to_string();
        host.attach_control(channel_for(&session_id, 2)).unwrap();

        let snapshot_envelope = host.control_snapshot().unwrap().unwrap();
        let snapshot = client
            .accept_capability_snapshot(&snapshot_envelope)
            .unwrap();
        assert_eq!(snapshot.session_id.as_deref(), Some(session_id.as_str()));
        assert!(snapshot.action("window.maximize.set").is_some());

        let context = ActionContext {
            connection_epoch: snapshot.connection_epoch.clone(),
            session_id: snapshot.session_id.clone(),
            grant_epoch: snapshot.grant_epoch.clone(),
            target: snapshot.target(),
            capability_revision: snapshot.capability_revision,
            grants: ActionGrants {
                desktop_actions: false,
                window_actions: true,
                clipboard: false,
            },
        };
        let request = crate::capability::ActionRequest::new(
            "r1",
            &context,
            "window.maximize.set",
            1,
            std::collections::BTreeMap::from([("value".to_string(), serde_json::json!(true))]),
        )
        .unwrap();
        let envelope = client.action_invoke(&request).unwrap();
        let reply = host.handle(&envelope, 1_000_001).unwrap().unwrap();
        let result = client.accept_action_result(&reply).unwrap();
        assert_eq!(result.status, crate::capability::ActionStatus::Completed);
        assert_eq!(client.state(), ClientState::Active);
        assert_eq!(host.state(), HostState::Active);
    }

    #[test]
    fn action_invoke_without_a_dispatcher_is_a_transition_error() {
        let (mut host, client) = active_with_input();
        // The client still holds an active session; use a minimally valid
        // action.invoke so only the missing dispatcher can reject it.
        let envelope = Envelope::new(
            client.next_client_seq(),
            ControlMessage::ActionInvoke {
                v: 1,
                id: "r1".into(),
                connection_epoch: host.session_id().unwrap().to_string(),
                session_id: host.session_id().map(str::to_string),
                grant_epoch: "g1".into(),
                target_id: "w5".into(),
                target_generation: "niri-test-w5".into(),
                capability_revision: 1,
                action: "window.focus".into(),
                schema_version: 1,
                parameters: Default::default(),
            },
        );
        let error = host.handle(&envelope, 1_000_001).unwrap_err();
        assert!(matches!(
            error,
            ProtocolError::InvalidTransition {
                expected: "a capability-enabled session",
                ..
            }
        ));
    }
}
