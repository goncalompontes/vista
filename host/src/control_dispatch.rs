//! Session-bound dispatch of the capability/action contract.
//!
//! `HostControlChannel` is the join between three pieces that already exist:
//!
//! - an authenticated control session (`protocol::HostSession`) that decides
//!   which session id and connection epoch a message belongs to;
//! - a revocable `ControlLease` that owns the target, action grants and input
//!   permissions for that session;
//! - an `ActionRegistry` that revalidates freshness/authorization and executes a
//!   semantic action through a backend.
//!
//! The dispatcher never trusts the client's copy of the context. It derives the
//! `ActionContext` from the lease, rejects a request whose session/epoch does not
//! match that lease, and converts every outcome into a truthful `action.result`.
//! It contains no sockets, capture or native input injection.

use crate::capability::{
    ActionBackend, ActionGrants, ActionRegistry, ActionRequest, ActionResult, ActionStatus,
    CONTRACT_VERSION, CapabilitySnapshot, ContractError, ContractErrorCode, InputEffect,
    InputPermissions,
};
use crate::control_lease::ControlLease;
use crate::protocol::ControlMessage;
use serde::Serialize;

/// Bounded counters for the dispatcher diagnostic. They describe what actually
/// happened on the channel; they are not a success flag.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DispatchStats {
    pub snapshots: u64,
    pub actions_completed: u64,
    pub actions_rejected: u64,
    pub actions_failed: u64,
    pub wrong_session: u64,
    pub stale_grant: u64,
}

/// Session-bound capability/action dispatcher.
///
/// The default backend is a boxed adapter so the transport can choose a Niri or
/// deterministic backend at runtime without making the protocol generic.
pub struct HostControlChannel<B: ActionBackend = Box<dyn ActionBackend + Send>> {
    registry: ActionRegistry<B>,
    lease: ControlLease,
    capability_revision: u64,
    stats: DispatchStats,
}

impl<B: ActionBackend> HostControlChannel<B> {
    /// Bind a channel to one lease. The lease must already carry the session id
    /// assigned during pairing; `HostSession::attach_control` enforces that the
    /// lease and the authenticated session agree.
    pub fn new(
        registry: ActionRegistry<B>,
        lease: ControlLease,
        capability_revision: u64,
    ) -> Result<Self, ContractError> {
        if capability_revision == 0 {
            return Err(ContractError::new(
                ContractErrorCode::InvalidParameters,
                "capability_revision must be non-zero",
            ));
        }
        if !lease.is_active() {
            return Err(ContractError::new(
                ContractErrorCode::PermissionDenied,
                "control lease is not active",
            ));
        }
        // Reuses the lease's own validation so an unusable epoch/session cannot
        // be attached and then fail on the first request.
        lease.action_context(capability_revision)?;
        Ok(Self {
            registry,
            lease,
            capability_revision,
            stats: DispatchStats::default(),
        })
    }

    pub fn lease(&self) -> &ControlLease {
        &self.lease
    }

    pub fn registry(&self) -> &ActionRegistry<B> {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut ActionRegistry<B> {
        &mut self.registry
    }

    pub fn capability_revision(&self) -> u64 {
        self.capability_revision
    }

    pub fn session_id(&self) -> Option<&str> {
        self.lease.session_id()
    }

    pub fn connection_epoch(&self) -> &str {
        self.lease.connection_epoch()
    }

    pub fn stats(&self) -> &DispatchStats {
        &self.stats
    }

    /// Capability snapshot for the *current* grant. A supported action whose
    /// scope is not granted is reported as `required`.
    pub fn snapshot(&self) -> Result<CapabilitySnapshot, ContractError> {
        let context = self.lease.action_context(self.capability_revision)?;
        self.registry.snapshot(&context)
    }

    /// Wire-ready snapshot message. The caller (the host session) owns the
    /// envelope sequence number.
    pub fn snapshot_message(&mut self) -> Result<ControlMessage, ContractError> {
        let snapshot = self.snapshot()?;
        self.stats.snapshots = self.stats.snapshots.saturating_add(1);
        Ok(ControlMessage::from(snapshot))
    }

    /// Handle one client message. Returns `None` for messages this layer does
    /// not own, so the session state machine can reject them normally.
    ///
    /// `action.invoke` always produces an `action.result`; malformed requests are
    /// rejected with a structured error instead of failing the connection.
    pub fn handle(&mut self, message: &ControlMessage, now_secs: u64) -> Option<ControlMessage> {
        match message {
            ControlMessage::ActionInvoke { .. } => Some(self.handle_action(message, now_secs)),
            _ => None,
        }
    }

    /// Revoke the lease and release every held button/key. The grant epoch
    /// advances, so in-flight requests from the old grant are stale.
    pub fn revoke(&mut self) -> Vec<InputEffect> {
        self.lease.revoke()
    }

    /// Re-grant after explicit host/user approval. The epoch advances again.
    pub fn regrant(&mut self, grants: ActionGrants, input_permissions: InputPermissions) {
        self.lease.regrant(grants, input_permissions);
    }

    fn handle_action(&mut self, message: &ControlMessage, now_secs: u64) -> ControlMessage {
        let request = match ActionRequest::try_from(message) {
            Ok(request) => request,
            Err(error) => {
                self.stats.actions_rejected = self.stats.actions_rejected.saturating_add(1);
                return rejection_without_request(message, error.code);
            }
        };

        // The lease, not the request, decides which session may act.
        let context = match self.lease.action_context(self.capability_revision) {
            Ok(context) => context,
            Err(error) => {
                self.stats.actions_rejected = self.stats.actions_rejected.saturating_add(1);
                return ControlMessage::from(ActionResult::rejected(&request, error.code));
            }
        };

        if request.connection_epoch != context.connection_epoch
            || request.session_id != context.session_id
        {
            self.stats.wrong_session = self.stats.wrong_session.saturating_add(1);
            self.stats.actions_rejected = self.stats.actions_rejected.saturating_add(1);
            return ControlMessage::from(ActionResult::rejected(
                &request,
                ContractErrorCode::StaleContext,
            ));
        }

        if request.grant_epoch != context.grant_epoch {
            // A request from a revoked or superseded grant must never run, even
            // if the registry would otherwise accept its action.
            self.stats.stale_grant = self.stats.stale_grant.saturating_add(1);
            self.stats.actions_rejected = self.stats.actions_rejected.saturating_add(1);
            return ControlMessage::from(ActionResult::rejected(
                &request,
                ContractErrorCode::StaleContext,
            ));
        }

        let result = self.registry.invoke(&context, &request, now_secs);
        match result.status {
            ActionStatus::Completed => {
                self.stats.actions_completed = self.stats.actions_completed.saturating_add(1);
            }
            ActionStatus::Rejected => {
                self.stats.actions_rejected = self.stats.actions_rejected.saturating_add(1);
            }
            ActionStatus::Failed | ActionStatus::OutcomeUnknown => {
                self.stats.actions_failed = self.stats.actions_failed.saturating_add(1);
            }
        }
        ControlMessage::from(result)
    }
}

/// Echo enough of the malformed request for the client to correlate a rejection
/// without trusting unvalidated fields.
fn rejection_without_request(message: &ControlMessage, code: ContractErrorCode) -> ControlMessage {
    let ControlMessage::ActionInvoke {
        id,
        connection_epoch,
        session_id,
        ..
    } = message
    else {
        unreachable!("only action.invoke messages reach dispatch");
    };
    ControlMessage::ActionResult {
        v: CONTRACT_VERSION,
        id: id.clone(),
        connection_epoch: connection_epoch.clone(),
        session_id: session_id.clone(),
        status: ActionStatus::Rejected,
        error_code: Some(code),
        state: None,
        capability_revision: None,
    }
}

impl<B: ActionBackend> std::fmt::Debug for HostControlChannel<B> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostControlChannel")
            .field("session_id", &self.session_id())
            .field("connection_epoch", &self.connection_epoch())
            .field("grant_epoch", &self.lease.grant_epoch_string())
            .field("capability_revision", &self.capability_revision)
            .field("stats", &self.stats)
            .finish()
    }
}

/// Wire result type check used by tests and the diagnostic.
pub fn action_result(message: &ControlMessage) -> Option<ActionResult> {
    ActionResult::try_from(message).ok()
}

/// Deterministic backend wrapper that counts real executions, so the diagnostic
/// can prove the registry reached the adapter instead of only returning a state.
struct CountingBackend {
    inner: crate::capability::FakeActionBackend,
    executed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ActionBackend for CountingBackend {
    fn descriptors(&self) -> Vec<crate::capability::ActionDescriptor> {
        self.inner.descriptors()
    }

    fn execute(
        &mut self,
        target: &crate::capability::TargetRef,
        action: &str,
        parameters: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<crate::capability::BackendOutcome, ContractErrorCode> {
        self.executed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.execute(target, action, parameters)
    }
}

/// Deterministic, network-free proof that the contract is dispatched through a
/// real `HostSession`/`ClientSession` pairing rather than only through the
/// registry API. Uses the fake backend and no desktop access.
#[derive(Debug, Serialize)]
pub struct DispatchReport {
    pub protocol_session: bool,
    pub snapshot_received: bool,
    pub snapshot_actions: usize,
    pub snapshot_grant_epoch: String,
    pub action_completed: bool,
    pub backend_calls: usize,
    pub duplicate_cached: bool,
    pub stale_grant_rejected: bool,
    pub revoked_rejected: bool,
    pub wrong_session_rejected: bool,
    pub wrong_target_rejected: bool,
    pub malformed_rejected: bool,
    pub verified: bool,
}

pub fn diagnostic() -> Result<DispatchReport, crate::protocol::ProtocolError> {
    use crate::capability::{ActionContext, FakeActionBackend, TargetRef};
    use crate::protocol::{ClientSession, HostSession, PairingSecret, PermissionSet};
    use serde_json::Value;
    use std::collections::BTreeMap;

    let secret = PairingSecret::from_bytes([0x42; 32]);
    let now = 1_000;
    let policy = PermissionSet {
        video: true,
        input: true,
        ..PermissionSet::default()
    };
    let mut host = HostSession::new("vista-host", "Vista host", secret.clone(), policy, now)?;
    let mut client = ClientSession::new(secret, "vista-client")?;

    let hello = client.hello()?;
    let server_hello =
        host.handle(&hello, now)?
            .ok_or(crate::protocol::ProtocolError::InvalidTransition {
                expected: "server_hello",
                actual: "no reply",
            })?;
    let proof = client.accept_server_hello(&server_hello)?;
    let accepted =
        host.handle(&proof, now)?
            .ok_or(crate::protocol::ProtocolError::InvalidTransition {
                expected: "pairing_accepted",
                actual: "no reply",
            })?;
    client.accept_pairing(&accepted)?;
    let open = client.open_session(policy)?;
    let opened =
        host.handle(&open, now)?
            .ok_or(crate::protocol::ProtocolError::InvalidTransition {
                expected: "session_opened",
                actual: "no reply",
            })?;
    client.accept_session_opened(&opened)?;
    let protocol_session = host.session_id() == client.session_id()
        && client.state() == crate::protocol::ClientState::Active;

    let session_id = host
        .session_id()
        .ok_or(crate::protocol::ProtocolError::SessionMismatch)?
        .to_string();
    let grants = ActionGrants {
        desktop_actions: true,
        window_actions: true,
        clipboard: false,
    };
    let input_permissions = InputPermissions {
        pointer: true,
        keyboard: true,
        text: true,
    };
    let lease = ControlLease::grant(
        &session_id,
        Some(session_id.clone()),
        TargetRef::new("w5", "niri-test-w5")
            .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?,
        grants,
        input_permissions,
    )
    .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?;
    let executed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let registry = ActionRegistry::new(Box::new(CountingBackend {
        inner: FakeActionBackend::default(),
        executed: executed.clone(),
    }) as Box<dyn ActionBackend + Send>);
    let channel = HostControlChannel::new(registry, lease, 4)
        .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?;
    host.attach_control(channel)?;

    let snapshot_envelope =
        host.control_snapshot()?
            .ok_or(crate::protocol::ProtocolError::InvalidTransition {
                expected: "capabilities.snapshot",
                actual: "no dispatcher",
            })?;
    let snapshot = client.accept_capability_snapshot(&snapshot_envelope)?;
    let snapshot_actions = snapshot.actions.len();
    let snapshot_grant_epoch = snapshot.grant_epoch.clone();

    let context = ActionContext {
        connection_epoch: snapshot.connection_epoch.clone(),
        session_id: snapshot.session_id.clone(),
        grant_epoch: snapshot.grant_epoch.clone(),
        target: snapshot.target(),
        capability_revision: snapshot.capability_revision,
        grants,
    };
    let maximize = ActionRequest::new(
        "r1",
        &context,
        "window.maximize.set",
        1,
        BTreeMap::from([("value".to_string(), Value::Bool(true))]),
    )
    .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?;
    let result = exchange(&mut host, &mut client, &maximize, now)?;
    let action_completed = result.status == ActionStatus::Completed;
    let backend_calls = executed.load(std::sync::atomic::Ordering::SeqCst);

    let duplicate = exchange(&mut host, &mut client, &maximize, now)?;
    let duplicate_cached = duplicate == result;

    let mut wrong_session = maximize.clone();
    wrong_session.id = "r2".into();
    wrong_session.session_id = Some("other-session".into());
    let wrong_session_rejected = exchange(&mut host, &mut client, &wrong_session, now)?.error_code
        == Some(ContractErrorCode::StaleContext);

    host.control_mut()
        .ok_or(crate::protocol::ProtocolError::PermissionDenied)?
        .revoke();
    let mut revoked = maximize.clone();
    revoked.id = "r3".into();
    let revoked_rejected = exchange(&mut host, &mut client, &revoked, now)?.error_code
        == Some(ContractErrorCode::PermissionDenied);

    let control = host
        .control_mut()
        .ok_or(crate::protocol::ProtocolError::PermissionDenied)?;
    control.regrant(grants, input_permissions);
    let mut stale = maximize.clone();
    stale.id = "r4".into();
    let stale_grant_rejected = exchange(&mut host, &mut client, &stale, now)?.error_code
        == Some(ContractErrorCode::StaleContext);

    let control = host
        .control()
        .ok_or(crate::protocol::ProtocolError::PermissionDenied)?;
    let current = control
        .lease()
        .action_context(control.capability_revision())
        .map_err(|_| crate::protocol::ProtocolError::PermissionDenied)?;
    let mut wrong_target = ActionRequest::new("r5", &current, "window.focus", 1, BTreeMap::new())
        .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?;
    wrong_target.target_id = "w6".into();
    let wrong_target_rejected = exchange(&mut host, &mut client, &wrong_target, now)?.error_code
        == Some(ContractErrorCode::StaleContext);

    let mut malformed = ActionRequest::new(
        "r6",
        &current,
        "window.maximize.set",
        1,
        BTreeMap::from([("value".to_string(), Value::String("yes".into()))]),
    )
    .map_err(|_| crate::protocol::ProtocolError::SessionMismatch)?;
    malformed.id = "r6".into();
    let malformed_rejected = exchange(&mut host, &mut client, &malformed, now)?.error_code
        == Some(ContractErrorCode::InvalidParameters);

    let snapshot_received =
        !snapshot.actions.is_empty() && snapshot.session_id.as_deref() == Some(session_id.as_str());
    let verified = protocol_session
        && snapshot_received
        && action_completed
        && backend_calls == 1
        && duplicate_cached
        && stale_grant_rejected
        && revoked_rejected
        && wrong_session_rejected
        && wrong_target_rejected
        && malformed_rejected;

    Ok(DispatchReport {
        protocol_session,
        snapshot_received,
        snapshot_actions,
        snapshot_grant_epoch,
        action_completed,
        backend_calls,
        duplicate_cached,
        stale_grant_rejected,
        revoked_rejected,
        wrong_session_rejected,
        wrong_target_rejected,
        malformed_rejected,
        verified,
    })
}

fn exchange(
    host: &mut crate::protocol::HostSession,
    client: &mut crate::protocol::ClientSession,
    request: &ActionRequest,
    now: u64,
) -> Result<ActionResult, crate::protocol::ProtocolError> {
    let envelope = client.action_invoke(request)?;
    let reply =
        host.handle(&envelope, now)?
            .ok_or(crate::protocol::ProtocolError::InvalidTransition {
                expected: "action.result",
                actual: "no reply",
            })?;
    client.accept_action_result(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Authorization, FakeActionBackend, TargetRef};
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn target() -> TargetRef {
        TargetRef::new("w5", "niri-test-w5").unwrap()
    }

    fn grants() -> ActionGrants {
        ActionGrants {
            desktop_actions: true,
            window_actions: true,
            clipboard: false,
        }
    }

    fn input_permissions() -> InputPermissions {
        InputPermissions {
            pointer: true,
            keyboard: true,
            text: true,
        }
    }

    fn channel() -> HostControlChannel<FakeActionBackend> {
        let lease = ControlLease::grant(
            "s1",
            Some("s1".into()),
            target(),
            grants(),
            input_permissions(),
        )
        .unwrap();
        HostControlChannel::new(ActionRegistry::new(FakeActionBackend::default()), lease, 7)
            .unwrap()
    }

    fn request(id: &str, action: &str, parameters: BTreeMap<String, Value>) -> ActionRequest {
        let channel = channel();
        let context = channel.lease().action_context(7).unwrap();
        ActionRequest::new(id, &context, action, 1, parameters).unwrap()
    }

    fn maximize(value: bool) -> BTreeMap<String, Value> {
        BTreeMap::from([("value".to_string(), Value::Bool(value))])
    }

    #[test]
    fn snapshot_reports_current_grant_and_marks_ungranted_actions_required() {
        let lease = ControlLease::grant(
            "s1",
            Some("s1".into()),
            target(),
            ActionGrants {
                desktop_actions: false,
                window_actions: false,
                clipboard: false,
            },
            InputPermissions::default(),
        )
        .unwrap();
        let channel =
            HostControlChannel::new(ActionRegistry::new(FakeActionBackend::default()), lease, 3)
                .unwrap();
        let snapshot = channel.snapshot().unwrap();
        assert_eq!(snapshot.session_id.as_deref(), Some("s1"));
        assert_eq!(snapshot.grant_epoch, "g1");
        assert_eq!(snapshot.target(), target());
        assert_eq!(
            snapshot
                .action("window.maximize.set")
                .unwrap()
                .authorization,
            Authorization::Required
        );

        let mut channel = channel;
        assert!(matches!(
            channel.snapshot_message().unwrap(),
            ControlMessage::CapabilitySnapshot {
                capability_revision: 3,
                ..
            }
        ));
        assert_eq!(channel.stats().snapshots, 1);
    }

    #[test]
    fn action_invoke_completes_and_calls_the_backend() {
        let mut channel = channel();
        let message = ControlMessage::from(request("r1", "window.maximize.set", maximize(true)));
        let reply = channel
            .handle(&message, 10)
            .expect("action.invoke is owned");
        let result = action_result(&reply).unwrap();
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(result.error_code, None);
        assert_eq!(result.state, Some(serde_json::json!({ "maximized": true })));
        assert_eq!(
            channel.registry().backend().calls,
            vec!["window.maximize.set"]
        );
        assert_eq!(channel.stats().actions_completed, 1);
    }

    #[test]
    fn non_action_messages_are_not_owned() {
        let mut channel = channel();
        assert!(
            channel
                .handle(&ControlMessage::Ping { nonce: 1 }, 1)
                .is_none()
        );
        assert_eq!(channel.stats(), &DispatchStats::default());
    }

    #[test]
    fn wrong_session_and_epoch_are_rejected_without_execution() {
        let mut channel = channel();
        let mut wrong_session = request("r1", "window.focus", BTreeMap::new());
        wrong_session.session_id = Some("s2".into());
        let result = action_result(
            &channel
                .handle(&ControlMessage::from(wrong_session), 10)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(result.error_code, Some(ContractErrorCode::StaleContext));
        assert_eq!(channel.stats().wrong_session, 1);

        let mut wrong_epoch = request("r2", "window.focus", BTreeMap::new());
        wrong_epoch.grant_epoch = "g9".into();
        let result = action_result(
            &channel
                .handle(&ControlMessage::from(wrong_epoch), 10)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result.error_code, Some(ContractErrorCode::StaleContext));
        assert_eq!(channel.stats().stale_grant, 1);
        assert!(channel.registry().backend().calls.is_empty());
    }

    #[test]
    fn revoked_lease_rejects_every_request_until_regrant() {
        let mut channel = channel();
        let old = channel.lease().action_context(7).unwrap();
        let old_request =
            ActionRequest::new("r1", &old, "window.focus", 1, BTreeMap::new()).unwrap();
        channel.revoke();

        let result = action_result(
            &channel
                .handle(&ControlMessage::from(old_request.clone()), 10)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(result.error_code, Some(ContractErrorCode::PermissionDenied));

        channel.regrant(grants(), input_permissions());
        let result = action_result(
            &channel
                .handle(&ControlMessage::from(old_request), 10)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            result.error_code,
            Some(ContractErrorCode::StaleContext),
            "a request from the pre-revoke grant must remain stale after re-grant"
        );
        assert!(channel.registry().backend().calls.is_empty());
    }

    #[test]
    fn malformed_wire_message_is_rejected_not_fatal() {
        let mut channel = channel();
        let valid = request("r1", "window.focus", BTreeMap::new());
        let mut message = ControlMessage::from(valid);
        if let ControlMessage::ActionInvoke { v, .. } = &mut message {
            *v = 99;
        }
        let result = action_result(&channel.handle(&message, 10).unwrap()).unwrap();
        assert_eq!(result.id, "r1");
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(
            result.error_code,
            Some(ContractErrorCode::InvalidParameters)
        );
    }

    #[test]
    fn unknown_action_is_rejected_without_calling_the_backend() {
        let mut channel = channel();
        let message = ControlMessage::from(request("r1", "window.not_real", BTreeMap::new()));
        let result = action_result(&channel.handle(&message, 10).unwrap()).unwrap();
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(
            result.error_code,
            Some(ContractErrorCode::UnsupportedAction)
        );
        assert!(channel.registry().backend().calls.is_empty());
    }

    #[test]
    fn stale_capability_revision_is_rejected() {
        let mut channel = channel();
        let mut request = request("r1", "window.focus", BTreeMap::new());
        request.capability_revision = 8;
        let result =
            action_result(&channel.handle(&ControlMessage::from(request), 10).unwrap()).unwrap();
        assert_eq!(result.error_code, Some(ContractErrorCode::StaleContext));
        assert!(channel.registry().backend().calls.is_empty());
    }

    #[test]
    fn malformed_parameters_are_rejected_by_the_registered_schema() {
        let mut channel = channel();
        let message = ControlMessage::from(request(
            "r1",
            "window.maximize.set",
            BTreeMap::from([("value".to_string(), Value::String("yes".into()))]),
        ));
        let result = action_result(&channel.handle(&message, 10).unwrap()).unwrap();
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(
            result.error_code,
            Some(ContractErrorCode::InvalidParameters)
        );
        assert!(channel.registry().backend().calls.is_empty());
    }

    #[test]
    fn duplicate_request_returns_cached_result_and_conflicting_reuse_is_rejected() {
        let mut channel = channel();
        let first = request("r1", "window.maximize.set", maximize(true));
        let cached = action_result(
            &channel
                .handle(&ControlMessage::from(first.clone()), 10)
                .unwrap(),
        )
        .unwrap();
        let replay =
            action_result(&channel.handle(&ControlMessage::from(first), 10).unwrap()).unwrap();
        assert_eq!(cached, replay);
        assert_eq!(channel.registry().backend().calls.len(), 1);

        let conflicting = request("r1", "window.maximize.set", maximize(false));
        let result = action_result(
            &channel
                .handle(&ControlMessage::from(conflicting), 10)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result.error_code, Some(ContractErrorCode::DuplicateRequest));
    }

    #[test]
    fn zero_revision_is_rejected() {
        let lease = ControlLease::grant(
            "s1",
            Some("s1".into()),
            target(),
            grants(),
            input_permissions(),
        )
        .unwrap();
        let error =
            HostControlChannel::new(ActionRegistry::new(FakeActionBackend::default()), lease, 0)
                .unwrap_err();
        assert_eq!(error.code, ContractErrorCode::InvalidParameters);
    }

    #[test]
    fn end_to_end_dispatch_diagnostic_is_verified() {
        let report = super::diagnostic().unwrap();
        assert!(report.verified, "{report:?}");
        assert!(report.protocol_session);
        assert!(report.snapshot_received);
        assert!(report.action_completed);
        assert_eq!(report.backend_calls, 1);
        assert!(report.duplicate_cached);
        assert!(report.revoked_rejected);
        assert!(report.stale_grant_rejected);
        assert!(report.wrong_session_rejected);
        assert!(report.wrong_target_rejected);
        assert!(report.malformed_rejected);
    }
}
