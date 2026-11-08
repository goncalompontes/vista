//! Backend-agnostic capability, semantic-action and portable-input contract.
//!
//! A host declares *what it can do* as versioned action descriptors attached to
//! a target (window, application, session or the desktop). A client renders
//! controls from that snapshot and sends semantic requests or portable input;
//! it never hardcodes a platform shortcut or assumes that a similarly named
//! operation exists on another backend.
//!
//! This module is transport-independent. It contains no sockets, capture or
//! native input injection. It does define the validation, freshness,
//! authorization, de-duplication and cleanup rules that every adapter and
//! future transport must apply before an action can have a native side effect.
//! See `docs/protocol.md` and `docs/fixtures/capabilities-v1.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
};

pub const CONTRACT_VERSION: u16 = 1;
pub const SNAPSHOT_MESSAGE_TYPE: &str = "capabilities.snapshot";
pub const ACTION_REQUEST_MESSAGE_TYPE: &str = "action.invoke";
pub const ACTION_RESULT_MESSAGE_TYPE: &str = "action.result";
pub const MENU_MESSAGE_TYPE: &str = "menu.snapshot";

pub const MAX_ACTIONS_PER_TARGET: usize = 256;
pub const MAX_PARAMETERS_PER_ACTION: usize = 64;
pub const MAX_PARAMETER_BYTES: usize = 4096;
pub const MAX_PARAMETER_DEPTH: usize = 4;
pub const MAX_IDENTIFIER_BYTES: usize = 128;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_REASON_BYTES: usize = 1024;
pub const MAX_BINDINGS: usize = 8;
pub const MAX_BINDING_BYTES: usize = 64;
pub const MAX_MENU_DEPTH: usize = 6;
pub const MAX_MENU_ITEMS: usize = 512;
pub const MAX_TEXT_COMMIT_BYTES: usize = 16 * 1024;
pub const MAX_RELATIVE_POINTER: f64 = 8192.0;
pub const MAX_ACTION_RESULTS: usize = 1024;
pub const ACTION_RESULT_TTL_SECS: u64 = 60;
pub const MAX_ACTION_REQUESTS_PER_SECOND: u32 = 20;
pub const MAX_DESTRUCTIVE_ACTIONS_PER_SECOND: u32 = 2;

pub const SCHEMA_EMPTY: &str = "vista.empty.v1";
pub const SCHEMA_MAXIMIZE_SET: &str = "vista.window.maximize.set.v1";
pub const SCHEMA_WORKSPACE_STEP: &str = "vista.desktop.workspace.step.v1";
pub const SCHEMA_EXTENSION: &str = "vista.extension.v1";

/// Structured contract failure, carried both by local validation errors and by
/// `action.result.error_code` on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractErrorCode {
    UnsupportedAction,
    InvalidParameters,
    PermissionRequired,
    PermissionDenied,
    StaleContext,
    TargetGone,
    Busy,
    RateLimited,
    BackendUnavailable,
    OutcomeUnknown,
    DuplicateRequest,
    MalformedMessage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractError {
    pub code: ContractErrorCode,
    pub detail: String,
}

impl ContractError {
    pub fn new(code: ContractErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    fn invalid(detail: impl Into<String>) -> Self {
        Self::new(ContractErrorCode::InvalidParameters, detail)
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ContractError {}

fn validate_identifier(value: &str, field: &str) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::invalid(format!("{field} must not be empty")));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(ContractError::invalid(format!(
            "{field} exceeds {MAX_IDENTIFIER_BYTES} bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::invalid(format!(
            "{field} contains a control character"
        )));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max: usize) -> Result<(), ContractError> {
    if value.len() > max {
        return Err(ContractError::invalid(format!(
            "{field} exceeds {max} bytes"
        )));
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(ContractError::invalid(format!(
            "{field} contains a forbidden control character"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorization {
    Granted,
    Required,
    Denied,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetRef {
    pub target_id: String,
    pub target_generation: String,
}

impl TargetRef {
    pub fn new(
        target_id: impl Into<String>,
        target_generation: impl Into<String>,
    ) -> Result<Self, ContractError> {
        let value = Self {
            target_id: target_id.into(),
            target_generation: target_generation.into(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        validate_identifier(&self.target_id, "target_id")?;
        validate_identifier(&self.target_generation, "target_generation")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionPresentation {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

impl ActionPresentation {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            shortcut: None,
        }
    }

    fn validate(&self) -> Result<(), ContractError> {
        validate_text(&self.label, "presentation.label", MAX_LABEL_BYTES)?;
        if self.label.is_empty() {
            return Err(ContractError::invalid(
                "presentation.label must not be empty",
            ));
        }
        if let Some(icon) = &self.icon {
            validate_identifier(icon, "presentation.icon")?;
        }
        if let Some(shortcut) = &self.shortcut {
            validate_text(shortcut, "presentation.shortcut", MAX_LABEL_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionDescriptor {
    pub id: String,
    pub schema_version: u16,
    pub supported: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub authorization: Authorization,
    pub parameters: String,
    pub presentation: ActionPresentation,
    #[serde(default)]
    pub suggested_bindings: Vec<String>,
}

impl ActionDescriptor {
    fn validate(&self) -> Result<(), ContractError> {
        validate_identifier(&self.id, "action.id")?;
        if self.schema_version == 0 {
            return Err(ContractError::invalid(
                "action.schema_version must be non-zero",
            ));
        }
        if let Some(reason) = &self.reason {
            validate_text(reason, "action.reason", MAX_REASON_BYTES)?;
        }
        validate_identifier(&self.parameters, "action.parameters")?;
        self.presentation.validate()?;
        if self.suggested_bindings.len() > MAX_BINDINGS {
            return Err(ContractError::invalid(format!(
                "action.suggested_bindings exceeds {MAX_BINDINGS} entries"
            )));
        }
        for binding in &self.suggested_bindings {
            if binding.is_empty() || binding.len() > MAX_BINDING_BYTES {
                return Err(ContractError::invalid(
                    "suggested binding must be 1..=64 bytes",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySnapshot {
    pub v: u16,
    #[serde(rename = "type")]
    pub message_type: String,
    pub connection_epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub target_id: String,
    pub target_generation: String,
    pub capability_revision: u64,
    pub grant_epoch: String,
    pub actions: Vec<ActionDescriptor>,
}

impl CapabilitySnapshot {
    pub fn new(
        connection_epoch: impl Into<String>,
        session_id: Option<String>,
        grant_epoch: impl Into<String>,
        target: &TargetRef,
        capability_revision: u64,
        actions: Vec<ActionDescriptor>,
    ) -> Result<Self, ContractError> {
        let value = Self {
            v: CONTRACT_VERSION,
            message_type: SNAPSHOT_MESSAGE_TYPE.into(),
            connection_epoch: connection_epoch.into(),
            session_id,
            target_id: target.target_id.clone(),
            target_generation: target.target_generation.clone(),
            capability_revision,
            grant_epoch: grant_epoch.into(),
            actions,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.v != CONTRACT_VERSION {
            return Err(ContractError::invalid("unsupported contract version"));
        }
        if self.message_type != SNAPSHOT_MESSAGE_TYPE {
            return Err(ContractError::invalid("unexpected snapshot message type"));
        }
        validate_identifier(&self.connection_epoch, "connection_epoch")?;
        if let Some(session_id) = &self.session_id {
            validate_identifier(session_id, "session_id")?;
        }
        TargetRef {
            target_id: self.target_id.clone(),
            target_generation: self.target_generation.clone(),
        }
        .validate()?;
        validate_identifier(&self.grant_epoch, "grant_epoch")?;
        if self.capability_revision == 0 {
            return Err(ContractError::invalid(
                "capability_revision must be non-zero",
            ));
        }
        if self.actions.len() > MAX_ACTIONS_PER_TARGET {
            return Err(ContractError::invalid(format!(
                "actions exceed {MAX_ACTIONS_PER_TARGET}"
            )));
        }
        let mut seen = BTreeSet::new();
        for action in &self.actions {
            action.validate()?;
            if !seen.insert(action.id.as_str()) {
                return Err(ContractError::invalid("duplicate action id"));
            }
        }
        Ok(())
    }

    pub fn target(&self) -> TargetRef {
        TargetRef {
            target_id: self.target_id.clone(),
            target_generation: self.target_generation.clone(),
        }
    }

    pub fn action(&self, id: &str) -> Option<&ActionDescriptor> {
        self.actions.iter().find(|action| action.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub v: u16,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub connection_epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub grant_epoch: String,
    pub target_id: String,
    pub target_generation: String,
    pub capability_revision: u64,
    pub action: String,
    pub schema_version: u16,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

impl ActionRequest {
    pub fn new(
        id: impl Into<String>,
        context: &ActionContext,
        action: impl Into<String>,
        schema_version: u16,
        parameters: BTreeMap<String, Value>,
    ) -> Result<Self, ContractError> {
        let value = Self {
            v: CONTRACT_VERSION,
            message_type: ACTION_REQUEST_MESSAGE_TYPE.into(),
            id: id.into(),
            connection_epoch: context.connection_epoch.clone(),
            session_id: context.session_id.clone(),
            grant_epoch: context.grant_epoch.clone(),
            target_id: context.target.target_id.clone(),
            target_generation: context.target.target_generation.clone(),
            capability_revision: context.capability_revision,
            action: action.into(),
            schema_version,
            parameters,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.v != CONTRACT_VERSION {
            return Err(ContractError::invalid("unsupported contract version"));
        }
        if self.message_type != ACTION_REQUEST_MESSAGE_TYPE {
            return Err(ContractError::new(
                ContractErrorCode::MalformedMessage,
                "unexpected action request message type",
            ));
        }
        validate_identifier(&self.id, "action request id")?;
        validate_identifier(&self.connection_epoch, "connection_epoch")?;
        if let Some(session_id) = &self.session_id {
            validate_identifier(session_id, "session_id")?;
        }
        validate_identifier(&self.grant_epoch, "grant_epoch")?;
        validate_identifier(&self.target_id, "target_id")?;
        validate_identifier(&self.target_generation, "target_generation")?;
        validate_identifier(&self.action, "action")?;
        if self.schema_version == 0 {
            return Err(ContractError::invalid("schema_version must be non-zero"));
        }
        validate_parameters_bounds(&self.parameters)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Completed,
    Rejected,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub v: u16,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub connection_epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: ActionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ContractErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_revision: Option<u64>,
}

impl ActionResult {
    fn from_request(
        request: &ActionRequest,
        status: ActionStatus,
        error_code: Option<ContractErrorCode>,
        state: Option<Value>,
        capability_revision: Option<u64>,
    ) -> Self {
        Self {
            v: CONTRACT_VERSION,
            message_type: ACTION_RESULT_MESSAGE_TYPE.into(),
            id: request.id.clone(),
            connection_epoch: request.connection_epoch.clone(),
            session_id: request.session_id.clone(),
            status,
            error_code,
            state,
            capability_revision,
        }
    }

    fn rejected(request: &ActionRequest, code: ContractErrorCode) -> Self {
        Self::from_request(request, ActionStatus::Rejected, Some(code), None, None)
    }
}

/// Host-side authorization snapshot used to revalidate an action at execution
/// time. Advertised capabilities are never a permission grant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionGrants {
    pub desktop_actions: bool,
    pub window_actions: bool,
    pub clipboard: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionContext {
    pub connection_epoch: String,
    pub session_id: Option<String>,
    pub grant_epoch: String,
    pub target: TargetRef,
    pub capability_revision: u64,
    pub grants: ActionGrants,
}

impl ActionContext {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_identifier(&self.connection_epoch, "connection_epoch")?;
        if let Some(session_id) = &self.session_id {
            validate_identifier(session_id, "session_id")?;
        }
        validate_identifier(&self.grant_epoch, "grant_epoch")?;
        self.target.validate()?;
        if self.capability_revision == 0 {
            return Err(ContractError::invalid(
                "capability_revision must be non-zero",
            ));
        }
        Ok(())
    }

    fn allows(&self, action: &str) -> bool {
        match action.split('.').next().unwrap_or_default() {
            "desktop" | "application" | "session" => self.grants.desktop_actions,
            "clipboard" => self.grants.clipboard,
            _ => self.grants.window_actions,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendOutcome {
    pub state: Option<Value>,
    pub capability_revision: Option<u64>,
}

impl BackendOutcome {
    pub fn completed() -> Self {
        Self {
            state: None,
            capability_revision: None,
        }
    }

    pub fn with_state(state: Value) -> Self {
        Self {
            state: Some(state),
            capability_revision: None,
        }
    }
}

/// Adapter boundary for a compositor, desktop or managed-session backend.
///
/// Implementations declare only operations whose semantics they actually
/// provide. The registry revalidates authorization and freshness before calling
/// `execute`, so an adapter must not rely on the client's snapshot.
pub trait ActionBackend {
    fn descriptors(&self) -> Vec<ActionDescriptor>;

    fn execute(
        &mut self,
        target: &TargetRef,
        action: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<BackendOutcome, ContractErrorCode>;
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    id: String,
    fingerprint: String,
    created_at: u64,
    result: ActionResult,
}

#[derive(Clone, Debug)]
struct RateWindow {
    second: u64,
    total: u32,
    destructive: u32,
}

impl Default for RateWindow {
    fn default() -> Self {
        Self {
            second: u64::MAX,
            total: 0,
            destructive: 0,
        }
    }
}

impl RateWindow {
    fn allow(&mut self, now: u64, destructive: bool) -> bool {
        if self.second != now {
            self.second = now;
            self.total = 0;
            self.destructive = 0;
        }
        if self.total >= MAX_ACTION_REQUESTS_PER_SECOND
            || (destructive && self.destructive >= MAX_DESTRUCTIVE_ACTIONS_PER_SECOND)
        {
            return false;
        }
        self.total += 1;
        if destructive {
            self.destructive += 1;
        }
        true
    }
}

pub struct ActionRegistry<B: ActionBackend> {
    backend: B,
    history: VecDeque<HistoryEntry>,
    connection_epoch: Option<String>,
    rate: RateWindow,
}

impl<B: ActionBackend> ActionRegistry<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            history: VecDeque::new(),
            connection_epoch: None,
            rate: RateWindow::default(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Start a fresh connection epoch. Request de-duplication and rate state do
    /// not cross reconnects, matching the rule that no action is replayed.
    pub fn begin_connection(&mut self, epoch: &str) {
        if self.connection_epoch.as_deref() != Some(epoch) {
            self.connection_epoch = Some(epoch.to_string());
            self.history.clear();
            self.rate = RateWindow::default();
        }
    }

    pub fn snapshot(&self, context: &ActionContext) -> Result<CapabilitySnapshot, ContractError> {
        context.validate()?;
        CapabilitySnapshot::new(
            context.connection_epoch.clone(),
            context.session_id.clone(),
            context.grant_epoch.clone(),
            &context.target,
            context.capability_revision,
            self.backend.descriptors(),
        )
    }

    /// Revalidate and execute one semantic action. `now` is a caller-supplied
    /// UNIX-style second count so tests can exercise the 60-second result
    /// window without sleeping.
    pub fn invoke(
        &mut self,
        context: &ActionContext,
        request: &ActionRequest,
        now: u64,
    ) -> ActionResult {
        if let Err(error) = context.validate() {
            return record_invalid(request, error.code);
        }
        if let Err(error) = request.validate() {
            return record_invalid(request, error.code);
        }
        self.begin_connection(&context.connection_epoch);

        let fingerprint = serde_json::to_string(request).unwrap_or_default();
        self.prune_history(now);
        if let Some(entry) = self.history.iter().find(|entry| entry.id == request.id) {
            if entry.fingerprint == fingerprint {
                return entry.result.clone();
            }
            return ActionResult::rejected(request, ContractErrorCode::DuplicateRequest);
        }
        if self.history.len() >= MAX_ACTION_RESULTS {
            return ActionResult::rejected(request, ContractErrorCode::Busy);
        }

        if request.connection_epoch != context.connection_epoch
            || request.session_id != context.session_id
            || request.grant_epoch != context.grant_epoch
            || request.target_id != context.target.target_id
            || request.target_generation != context.target.target_generation
            || request.capability_revision != context.capability_revision
        {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::StaleContext),
                now,
            );
        }

        let destructive = matches!(
            request.action.as_str(),
            "window.close" | "application.launch" | "session.stop"
        );
        if !self.rate.allow(now, destructive) {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::RateLimited),
                now,
            );
        }

        let snapshot = match self.snapshot(context) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return self.finish(
                    request,
                    fingerprint,
                    ActionResult::rejected(request, error.code),
                    now,
                );
            }
        };
        let Some(descriptor) = snapshot.action(&request.action) else {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::UnsupportedAction),
                now,
            );
        };

        if !descriptor.supported {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::UnsupportedAction),
                now,
            );
        }
        if !descriptor.enabled {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::BackendUnavailable),
                now,
            );
        }
        match descriptor.authorization {
            Authorization::Granted => {
                if !context.allows(&request.action) {
                    return self.finish(
                        request,
                        fingerprint,
                        ActionResult::rejected(request, ContractErrorCode::PermissionRequired),
                        now,
                    );
                }
            }
            Authorization::Required => {
                return self.finish(
                    request,
                    fingerprint,
                    ActionResult::rejected(request, ContractErrorCode::PermissionRequired),
                    now,
                );
            }
            Authorization::Denied | Authorization::Revoked => {
                return self.finish(
                    request,
                    fingerprint,
                    ActionResult::rejected(request, ContractErrorCode::PermissionDenied),
                    now,
                );
            }
        }
        if descriptor.schema_version != request.schema_version {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::UnsupportedAction),
                now,
            );
        }
        if validate_parameters(&descriptor.parameters, &request.parameters).is_err() {
            return self.finish(
                request,
                fingerprint,
                ActionResult::rejected(request, ContractErrorCode::InvalidParameters),
                now,
            );
        }

        let result =
            match self
                .backend
                .execute(&context.target, &request.action, &request.parameters)
            {
                Ok(outcome) => {
                    let state = outcome.state;
                    let revision = outcome
                        .capability_revision
                        .or_else(|| Some(context.capability_revision + 1));
                    ActionResult::from_request(
                        request,
                        ActionStatus::Completed,
                        None,
                        state,
                        revision,
                    )
                }
                Err(ContractErrorCode::OutcomeUnknown) => ActionResult::from_request(
                    request,
                    ActionStatus::OutcomeUnknown,
                    Some(ContractErrorCode::OutcomeUnknown),
                    None,
                    None,
                ),
                Err(code) => ActionResult::from_request(
                    request,
                    ActionStatus::Failed,
                    Some(code),
                    None,
                    None,
                ),
            };
        self.finish(request, fingerprint, result, now)
    }

    fn prune_history(&mut self, now: u64) {
        while let Some(entry) = self.history.front() {
            if now.saturating_sub(entry.created_at) < ACTION_RESULT_TTL_SECS {
                break;
            }
            self.history.pop_front();
        }
    }

    fn finish(
        &mut self,
        _request: &ActionRequest,
        fingerprint: String,
        result: ActionResult,
        now: u64,
    ) -> ActionResult {
        self.history.push_back(HistoryEntry {
            id: result.id.clone(),
            fingerprint,
            created_at: now,
            result: result.clone(),
        });
        result
    }
}

fn record_invalid(request: &ActionRequest, code: ContractErrorCode) -> ActionResult {
    ActionResult {
        v: CONTRACT_VERSION,
        message_type: ACTION_RESULT_MESSAGE_TYPE.into(),
        id: request.id.clone(),
        connection_epoch: request.connection_epoch.clone(),
        session_id: request.session_id.clone(),
        status: ActionStatus::Rejected,
        error_code: Some(code),
        state: None,
        capability_revision: None,
    }
}

fn validate_parameters_bounds(parameters: &BTreeMap<String, Value>) -> Result<(), ContractError> {
    if parameters.len() > MAX_PARAMETERS_PER_ACTION {
        return Err(ContractError::invalid(format!(
            "parameters exceed {MAX_PARAMETERS_PER_ACTION} entries"
        )));
    }
    let encoded = serde_json::to_vec(parameters).map_err(|error| {
        ContractError::invalid(format!("parameters are not valid JSON: {error}"))
    })?;
    if encoded.len() > MAX_PARAMETER_BYTES {
        return Err(ContractError::invalid(format!(
            "parameters exceed {MAX_PARAMETER_BYTES} bytes"
        )));
    }
    let mut count = 0;
    for (key, value) in parameters {
        validate_identifier(key, "parameter name")?;
        validate_parameter_value(value, 0, &mut count)?;
    }
    Ok(())
}

fn validate_parameter_value(
    value: &Value,
    depth: usize,
    count: &mut usize,
) -> Result<(), ContractError> {
    if depth > MAX_PARAMETER_DEPTH {
        return Err(ContractError::invalid("parameter nesting is too deep"));
    }
    *count += 1;
    if *count > MAX_PARAMETERS_PER_ACTION {
        return Err(ContractError::invalid("parameter value count is too large"));
    }
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(number) => {
            if let Some(value) = number.as_f64()
                && !value.is_finite()
            {
                return Err(ContractError::invalid("parameter number is not finite"));
            }
            Ok(())
        }
        Value::String(value) => {
            validate_text(value, "parameter string", MAX_REASON_BYTES)?;
            Ok(())
        }
        Value::Array(values) => {
            if values.len() > MAX_PARAMETERS_PER_ACTION {
                return Err(ContractError::invalid("parameter array is too long"));
            }
            for value in values {
                validate_parameter_value(value, depth + 1, count)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() > MAX_PARAMETERS_PER_ACTION {
                return Err(ContractError::invalid("parameter object is too large"));
            }
            for (key, value) in values {
                validate_identifier(key, "parameter name")?;
                validate_parameter_value(value, depth + 1, count)?;
            }
            Ok(())
        }
    }
}

/// Validate an action's declared parameter schema. Unknown schemas are rejected
/// rather than interpreted as executable code or permissive free-form data.
pub fn validate_parameters(
    schema: &str,
    parameters: &BTreeMap<String, Value>,
) -> Result<(), ContractError> {
    validate_parameters_bounds(parameters)?;
    match schema {
        SCHEMA_EMPTY => {
            if parameters.is_empty() {
                Ok(())
            } else {
                Err(ContractError::invalid("this action takes no parameters"))
            }
        }
        SCHEMA_MAXIMIZE_SET => {
            if parameters.len() == 1 && parameters.get("value").is_some_and(Value::is_boolean) {
                Ok(())
            } else {
                Err(ContractError::invalid(
                    "maximize requires exactly one boolean `value`",
                ))
            }
        }
        SCHEMA_WORKSPACE_STEP => {
            let valid = parameters.len() == 1
                && parameters
                    .get("direction")
                    .and_then(Value::as_str)
                    .is_some_and(|value| matches!(value, "previous" | "next"));
            if valid {
                Ok(())
            } else {
                Err(ContractError::invalid(
                    "workspace step requires direction=previous|next",
                ))
            }
        }
        SCHEMA_EXTENSION => Ok(()),
        _ => Err(ContractError::new(
            ContractErrorCode::UnsupportedAction,
            "unknown parameter schema",
        )),
    }
}

// ---------------------------------------------------------------------------
// Menu trees
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default)]
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    fn validate(&self, depth: usize, total: &mut usize) -> Result<(), ContractError> {
        if depth > MAX_MENU_DEPTH {
            return Err(ContractError::invalid("menu nesting is too deep"));
        }
        *total += 1;
        if *total > MAX_MENU_ITEMS {
            return Err(ContractError::invalid("menu has too many items"));
        }
        validate_identifier(&self.id, "menu.id")?;
        validate_text(&self.label, "menu.label", MAX_LABEL_BYTES)?;
        if self.label.is_empty() {
            return Err(ContractError::invalid("menu.label must not be empty"));
        }
        for child in &self.children {
            child.validate(depth + 1, total)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MenuSnapshot {
    pub v: u16,
    #[serde(rename = "type")]
    pub message_type: String,
    pub connection_epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub target_id: String,
    pub target_generation: String,
    pub menu_revision: u64,
    pub items: Vec<MenuItem>,
}

impl MenuSnapshot {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.v != CONTRACT_VERSION {
            return Err(ContractError::invalid("unsupported contract version"));
        }
        if self.message_type != MENU_MESSAGE_TYPE {
            return Err(ContractError::invalid("unexpected menu message type"));
        }
        validate_identifier(&self.connection_epoch, "connection_epoch")?;
        if let Some(session_id) = &self.session_id {
            validate_identifier(session_id, "session_id")?;
        }
        TargetRef {
            target_id: self.target_id.clone(),
            target_generation: self.target_generation.clone(),
        }
        .validate()?;
        if self.menu_revision == 0 {
            return Err(ContractError::invalid("menu_revision must be non-zero"));
        }
        let mut total = 0;
        let mut ids = BTreeSet::new();
        for item in &self.items {
            item.validate(0, &mut total)?;
            collect_menu_ids(item, &mut ids)?;
        }
        Ok(())
    }
}

fn collect_menu_ids(item: &MenuItem, ids: &mut BTreeSet<String>) -> Result<(), ContractError> {
    if !ids.insert(item.id.clone()) {
        return Err(ContractError::invalid("duplicate menu item id"));
    }
    for child in &item.children {
        collect_menu_ids(child, ids)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Portable input
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputPermissions {
    pub pointer: bool,
    pub keyboard: bool,
    pub text: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputContext {
    pub target: TargetRef,
    pub grant_epoch: String,
    pub geometry_epoch: u64,
    pub permissions: InputPermissions,
}

impl InputContext {
    pub fn validate(&self) -> Result<(), ContractError> {
        self.target.validate()?;
        validate_identifier(&self.grant_epoch, "grant_epoch")?;
        if self.geometry_epoch == 0 {
            return Err(ContractError::invalid("geometry_epoch must be non-zero"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollUnit {
    Pixel,
    Line,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollPhase {
    Begin,
    Update,
    End,
    Cancel,
}

/// Portable input is distinct from semantic actions: physical keys keep their
/// meaning, text is committed separately from key presses, and every event is
/// authorized against the target and grant that produced the geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputEvent {
    #[serde(rename = "pointer.absolute")]
    PointerAbsolute { x: f64, y: f64, geometry_epoch: u64 },
    #[serde(rename = "pointer.relative")]
    PointerRelative { dx: f64, dy: f64 },
    #[serde(rename = "pointer.button")]
    PointerButton {
        button: PointerButton,
        pressed: bool,
    },
    #[serde(rename = "scroll")]
    Scroll {
        dx: f64,
        dy: f64,
        unit: ScrollUnit,
        phase: ScrollPhase,
        #[serde(default)]
        momentum: bool,
        anchor_x: f64,
        anchor_y: f64,
    },
    #[serde(rename = "key.physical")]
    KeyPhysical {
        usage: u16,
        pressed: bool,
        #[serde(default)]
        repeat: bool,
    },
    #[serde(rename = "text.commit")]
    TextCommit { text: String },
    #[serde(rename = "input.cancel")]
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEffect {
    PointerAbsolute {
        x: f64,
        y: f64,
    },
    PointerRelative {
        dx: f64,
        dy: f64,
    },
    Button {
        button: PointerButton,
        pressed: bool,
    },
    Scroll {
        dx: f64,
        dy: f64,
        unit: ScrollUnit,
        phase: ScrollPhase,
        momentum: bool,
        anchor_x: f64,
        anchor_y: f64,
    },
    Key {
        usage: u16,
        pressed: bool,
        repeat: bool,
    },
    Text {
        text: String,
    },
}

/// Tracks held buttons and keys so a disconnect, revoke or cancel can release
/// everything even when the client disappears without sending key-up.
#[derive(Debug, Default)]
pub struct InputLease {
    held_buttons: BTreeSet<PointerButton>,
    held_keys: BTreeSet<u16>,
}

impl InputLease {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(
        &mut self,
        event: &InputEvent,
        context: &InputContext,
    ) -> Result<Vec<InputEffect>, ContractError> {
        context.validate()?;
        match event {
            InputEvent::PointerAbsolute {
                x,
                y,
                geometry_epoch,
            } => {
                require_pointer(context)?;
                if *geometry_epoch != context.geometry_epoch {
                    return Err(ContractError::new(
                        ContractErrorCode::StaleContext,
                        "pointer geometry does not match the active epoch",
                    ));
                }
                if !unit_coordinate(*x) || !unit_coordinate(*y) {
                    return Err(ContractError::invalid(
                        "absolute pointer coordinates must be finite and inside [0,1]",
                    ));
                }
                Ok(vec![InputEffect::PointerAbsolute { x: *x, y: *y }])
            }
            InputEvent::PointerRelative { dx, dy } => {
                require_pointer(context)?;
                if !relative_coordinate(*dx) || !relative_coordinate(*dy) {
                    return Err(ContractError::invalid(
                        "relative pointer movement must be finite and bounded",
                    ));
                }
                Ok(vec![InputEffect::PointerRelative { dx: *dx, dy: *dy }])
            }
            InputEvent::PointerButton { button, pressed } => {
                require_pointer(context)?;
                if *pressed {
                    self.held_buttons.insert(*button);
                } else {
                    self.held_buttons.remove(button);
                }
                Ok(vec![InputEffect::Button {
                    button: *button,
                    pressed: *pressed,
                }])
            }
            InputEvent::Scroll {
                dx,
                dy,
                unit,
                phase,
                momentum,
                anchor_x,
                anchor_y,
            } => {
                require_pointer(context)?;
                if !relative_coordinate(*dx)
                    || !relative_coordinate(*dy)
                    || !unit_coordinate(*anchor_x)
                    || !unit_coordinate(*anchor_y)
                {
                    return Err(ContractError::invalid(
                        "scroll delta or anchor is outside the contract bounds",
                    ));
                }
                Ok(vec![InputEffect::Scroll {
                    dx: *dx,
                    dy: *dy,
                    unit: *unit,
                    phase: *phase,
                    momentum: *momentum,
                    anchor_x: *anchor_x,
                    anchor_y: *anchor_y,
                }])
            }
            InputEvent::KeyPhysical {
                usage,
                pressed,
                repeat,
            } => {
                require_keyboard(context)?;
                if *usage == 0 {
                    return Err(ContractError::invalid("physical key usage 0 is reserved"));
                }
                if *repeat && !*pressed {
                    return Err(ContractError::invalid("a key repeat must also be a press"));
                }
                if *pressed {
                    self.held_keys.insert(*usage);
                } else {
                    self.held_keys.remove(usage);
                }
                Ok(vec![InputEffect::Key {
                    usage: *usage,
                    pressed: *pressed,
                    repeat: *repeat,
                }])
            }
            InputEvent::TextCommit { text } => {
                if !context.permissions.text {
                    return Err(ContractError::new(
                        ContractErrorCode::PermissionRequired,
                        "text input is not authorized",
                    ));
                }
                if text.is_empty() || text.len() > MAX_TEXT_COMMIT_BYTES {
                    return Err(ContractError::invalid("committed text must be 1..=16 KiB"));
                }
                Ok(vec![InputEffect::Text { text: text.clone() }])
            }
            InputEvent::Cancel => Ok(self.release_all()),
        }
    }

    /// Cleanup is never throttled or denied: disconnect, revoke and cancel must
    /// always be able to clear held state.
    pub fn release_all(&mut self) -> Vec<InputEffect> {
        let mut effects = Vec::new();
        for button in std::mem::take(&mut self.held_buttons) {
            effects.push(InputEffect::Button {
                button,
                pressed: false,
            });
        }
        for usage in std::mem::take(&mut self.held_keys) {
            effects.push(InputEffect::Key {
                usage,
                pressed: false,
                repeat: false,
            });
        }
        effects
    }

    pub fn is_empty(&self) -> bool {
        self.held_buttons.is_empty() && self.held_keys.is_empty()
    }
}

fn require_pointer(context: &InputContext) -> Result<(), ContractError> {
    if context.permissions.pointer {
        Ok(())
    } else {
        Err(ContractError::new(
            ContractErrorCode::PermissionRequired,
            "pointer input is not authorized",
        ))
    }
}

fn require_keyboard(context: &InputContext) -> Result<(), ContractError> {
    if context.permissions.keyboard {
        Ok(())
    } else {
        Err(ContractError::new(
            ContractErrorCode::PermissionRequired,
            "keyboard input is not authorized",
        ))
    }
}

fn unit_coordinate(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn relative_coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_RELATIVE_POINTER
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Command seam for the Niri adapter so tests can record native calls without a
/// running compositor.
pub trait CommandRunner {
    fn run(&mut self, arguments: &[String]) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, arguments: &[String]) -> Result<(), String> {
        let status = std::process::Command::new("niri")
            .arg("msg")
            .arg("action")
            .args(arguments)
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("niri msg action exited with {status}"))
        }
    }
}

pub struct NiriActionBackend<R: CommandRunner = SystemCommandRunner> {
    runner: R,
}

impl Default for NiriActionBackend<SystemCommandRunner> {
    fn default() -> Self {
        Self {
            runner: SystemCommandRunner,
        }
    }
}

impl<R: CommandRunner> NiriActionBackend<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    fn window_id(target: &TargetRef) -> Result<u64, ContractErrorCode> {
        target
            .target_id
            .strip_prefix('w')
            .and_then(|id| id.parse().ok())
            .ok_or(ContractErrorCode::TargetGone)
    }
}

impl<R: CommandRunner> ActionBackend for NiriActionBackend<R> {
    fn descriptors(&self) -> Vec<ActionDescriptor> {
        vec![
            descriptor(
                "window.focus",
                true,
                true,
                Authorization::Granted,
                SCHEMA_EMPTY,
                ActionPresentation::new("Focus window"),
            ),
            descriptor(
                "window.close",
                true,
                true,
                Authorization::Granted,
                SCHEMA_EMPTY,
                ActionPresentation::new("Close window"),
            ),
            descriptor(
                "desktop.workspace.step",
                true,
                true,
                Authorization::Granted,
                SCHEMA_WORKSPACE_STEP,
                ActionPresentation::new("Switch workspace"),
            ),
            ActionDescriptor {
                id: "window.maximize.set".into(),
                schema_version: 1,
                supported: false,
                enabled: false,
                reason: Some("Niri exposes fullscreen, not maximize".into()),
                authorization: Authorization::Required,
                parameters: SCHEMA_MAXIMIZE_SET.into(),
                presentation: ActionPresentation::new("Maximize"),
                suggested_bindings: Vec::new(),
            },
        ]
    }

    fn execute(
        &mut self,
        target: &TargetRef,
        action: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<BackendOutcome, ContractErrorCode> {
        let window_id = Self::window_id(target)?;
        let arguments = match action {
            "window.focus" => vec!["focus-window".into(), "--id".into(), window_id.to_string()],
            "window.close" => vec!["close-window".into(), "--id".into(), window_id.to_string()],
            "desktop.workspace.step" => match parameters.get("direction").and_then(Value::as_str) {
                Some("previous") => vec!["focus-workspace-up".into()],
                Some("next") => vec!["focus-workspace-down".into()],
                _ => return Err(ContractErrorCode::InvalidParameters),
            },
            _ => return Err(ContractErrorCode::UnsupportedAction),
        };
        self.runner
            .run(&arguments)
            .map_err(|_| ContractErrorCode::BackendUnavailable)?;
        Ok(BackendOutcome::completed())
    }
}

/// Deterministic backend used by tests and `capability-test`. It deliberately
/// includes supported, unsupported, denied and extension actions plus a
/// dynamic availability flag.
#[derive(Debug, Default)]
pub struct FakeActionBackend {
    pub maximized: bool,
    pub overview_enabled: bool,
    pub fail_with: Option<ContractErrorCode>,
    pub calls: Vec<String>,
}

impl ActionBackend for FakeActionBackend {
    fn descriptors(&self) -> Vec<ActionDescriptor> {
        vec![
            descriptor(
                "window.focus",
                true,
                true,
                Authorization::Granted,
                SCHEMA_EMPTY,
                ActionPresentation::new("Focus window"),
            ),
            descriptor(
                "window.maximize.set",
                true,
                true,
                Authorization::Granted,
                SCHEMA_MAXIMIZE_SET,
                ActionPresentation::new("Maximize"),
            ),
            ActionDescriptor {
                id: "window.minimize".into(),
                schema_version: 1,
                supported: true,
                enabled: false,
                reason: Some("No managed session is selected".into()),
                authorization: Authorization::Granted,
                parameters: SCHEMA_EMPTY.into(),
                presentation: ActionPresentation::new("Minimize"),
                suggested_bindings: Vec::new(),
            },
            descriptor(
                "desktop.workspace.step",
                true,
                true,
                Authorization::Granted,
                SCHEMA_WORKSPACE_STEP,
                ActionPresentation::new("Switch workspace"),
            ),
            ActionDescriptor {
                id: "desktop.overview.show".into(),
                schema_version: 1,
                supported: self.overview_enabled,
                enabled: self.overview_enabled,
                reason: (!self.overview_enabled)
                    .then(|| "Backend does not implement overview".into()),
                authorization: Authorization::Granted,
                parameters: SCHEMA_EMPTY.into(),
                presentation: ActionPresentation::new("Overview"),
                suggested_bindings: Vec::new(),
            },
            descriptor(
                "edit.copy",
                true,
                true,
                Authorization::Denied,
                SCHEMA_EMPTY,
                ActionPresentation::new("Copy"),
            ),
            ActionDescriptor {
                id: "example.vendor.special".into(),
                schema_version: 1,
                supported: true,
                enabled: true,
                reason: None,
                authorization: Authorization::Granted,
                parameters: SCHEMA_EXTENSION.into(),
                presentation: ActionPresentation::new("Vendor action"),
                suggested_bindings: vec!["three_finger_swipe_left".into()],
            },
        ]
    }

    fn execute(
        &mut self,
        _target: &TargetRef,
        action: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<BackendOutcome, ContractErrorCode> {
        if let Some(code) = self.fail_with {
            return Err(code);
        }
        self.calls.push(action.to_string());
        match action {
            "window.focus"
            | "window.close"
            | "desktop.workspace.step"
            | "example.vendor.special" => Ok(BackendOutcome::completed()),
            "window.maximize.set" => {
                self.maximized = parameters
                    .get("value")
                    .and_then(Value::as_bool)
                    .ok_or(ContractErrorCode::InvalidParameters)?;
                Ok(BackendOutcome::with_state(
                    serde_json::json!({ "maximized": self.maximized }),
                ))
            }
            _ => Err(ContractErrorCode::UnsupportedAction),
        }
    }
}

fn descriptor(
    id: &str,
    supported: bool,
    enabled: bool,
    authorization: Authorization,
    parameters: &str,
    presentation: ActionPresentation,
) -> ActionDescriptor {
    ActionDescriptor {
        id: id.into(),
        schema_version: 1,
        supported,
        enabled,
        reason: None,
        authorization,
        parameters: parameters.into(),
        presentation,
        suggested_bindings: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Executable diagnostic
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CapabilityReport {
    pub contract_version: u16,
    pub backend: &'static str,
    pub capability_revision: u64,
    pub action_count: usize,
    pub allowed_completed: bool,
    pub denied_rejected: bool,
    pub unsupported_rejected: bool,
    pub unavailable_rejected: bool,
    pub stale_rejected: bool,
    pub malformed_rejected: bool,
    pub duplicate_cached: bool,
    pub update_without_reconnect: bool,
    pub reconnect_clears_history: bool,
    pub input_denied_without_permission: bool,
    pub release_all_clears_held_keys: bool,
    pub verified: bool,
}

fn empty_parameters() -> BTreeMap<String, Value> {
    BTreeMap::new()
}

fn parameters(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

/// Deterministic, hardware-free proof of the contract rules used by the CLI.
pub fn diagnostic() -> Result<CapabilityReport, ContractError> {
    let target = TargetRef::new("w5", "niri-boot1-w5")?;
    let context = ActionContext {
        connection_epoch: "c1".into(),
        session_id: Some("s1".into()),
        grant_epoch: "g2".into(),
        target: target.clone(),
        capability_revision: 12,
        grants: ActionGrants {
            desktop_actions: true,
            window_actions: true,
            clipboard: false,
        },
    };
    let mut registry = ActionRegistry::new(FakeActionBackend::default());
    registry.begin_connection("c1");
    let snapshot = registry.snapshot(&context)?;

    let maximize = ActionRequest::new(
        "r17",
        &context,
        "window.maximize.set",
        1,
        parameters(&[("value", Value::Bool(true))]),
    )?;
    let allowed = registry.invoke(&context, &maximize, 1_000);
    let allowed_completed = allowed.status == ActionStatus::Completed
        && allowed
            .state
            .as_ref()
            .is_some_and(|state| state["maximized"] == true);

    let duplicate = registry.invoke(&context, &maximize, 1_000);
    let duplicate_cached = duplicate.status == ActionStatus::Completed
        && registry
            .backend()
            .calls
            .iter()
            .filter(|call| *call == "window.maximize.set")
            .count()
            == 1;

    let denied = ActionRequest::new("r18", &context, "edit.copy", 1, empty_parameters())?;
    let denied_rejected = registry.invoke(&context, &denied, 1_001).error_code
        == Some(ContractErrorCode::PermissionDenied);

    let overview = ActionRequest::new(
        "r19",
        &context,
        "desktop.overview.show",
        1,
        empty_parameters(),
    )?;
    let unsupported_rejected = registry.invoke(&context, &overview, 1_002).error_code
        == Some(ContractErrorCode::UnsupportedAction);

    let unknown = ActionRequest::new("r20", &context, "window.nonexistent", 1, empty_parameters())?;
    let unsupported_unknown = registry.invoke(&context, &unknown, 1_003).error_code
        == Some(ContractErrorCode::UnsupportedAction);
    let unsupported_rejected = unsupported_rejected && unsupported_unknown;

    let unavailable =
        ActionRequest::new("r23", &context, "window.minimize", 1, empty_parameters())?;
    let unavailable_rejected = registry.invoke(&context, &unavailable, 1_006).error_code
        == Some(ContractErrorCode::BackendUnavailable);

    let mut stale_request = maximize.clone();
    stale_request.id = "r21".into();
    stale_request.capability_revision = 11;
    let stale_rejected = registry.invoke(&context, &stale_request, 1_004).error_code
        == Some(ContractErrorCode::StaleContext);

    let malformed = ActionRequest::new(
        "r22",
        &context,
        "window.maximize.set",
        1,
        parameters(&[("value", Value::String("yes".into()))]),
    )?;
    let malformed_rejected = registry.invoke(&context, &malformed, 1_005).error_code
        == Some(ContractErrorCode::InvalidParameters);

    // Capability updates change controls without reconnecting.
    registry.backend_mut().overview_enabled = true;
    let mut updated_context = context.clone();
    updated_context.capability_revision = 13;
    let updated_snapshot = registry.snapshot(&updated_context)?;
    let updated_action = updated_snapshot.action("desktop.overview.show");
    let update_without_reconnect = updated_action.is_some_and(|action| action.enabled)
        && updated_snapshot.connection_epoch == snapshot.connection_epoch;

    // Request de-duplication and rate state belong to a connection epoch.
    registry.begin_connection("c2");
    let mut reconnected = context.clone();
    reconnected.connection_epoch = "c2".into();
    let replay = registry.invoke(&reconnected, &maximize, 2_000);
    let reconnect_clears_history = replay.status == ActionStatus::Rejected
        && replay.error_code == Some(ContractErrorCode::StaleContext);

    let view_only = InputContext {
        target: target.clone(),
        grant_epoch: "g2".into(),
        geometry_epoch: 3,
        permissions: InputPermissions::default(),
    };
    let mut lease = InputLease::new();
    let absolute = InputEvent::PointerAbsolute {
        x: 0.5,
        y: 0.5,
        geometry_epoch: 3,
    };
    let input_denied_without_permission = lease
        .apply(&absolute, &view_only)
        .is_err_and(|error| error.code == ContractErrorCode::PermissionRequired);

    let input_context = InputContext {
        target,
        grant_epoch: "g2".into(),
        geometry_epoch: 3,
        permissions: InputPermissions {
            pointer: true,
            keyboard: true,
            text: true,
        },
    };
    lease.apply(
        &InputEvent::KeyPhysical {
            usage: 4,
            pressed: true,
            repeat: false,
        },
        &input_context,
    )?;
    lease.apply(
        &InputEvent::PointerButton {
            button: PointerButton::Primary,
            pressed: true,
        },
        &input_context,
    )?;
    let released = lease.release_all();
    let release_all_clears_held_keys = released.len() == 2 && lease.is_empty();

    let verified = allowed_completed
        && denied_rejected
        && unsupported_rejected
        && stale_rejected
        && malformed_rejected
        && unavailable_rejected
        && duplicate_cached
        && update_without_reconnect
        && reconnect_clears_history
        && input_denied_without_permission
        && release_all_clears_held_keys;

    Ok(CapabilityReport {
        contract_version: CONTRACT_VERSION,
        backend: "fake",
        capability_revision: snapshot.capability_revision,
        action_count: snapshot.actions.len(),
        allowed_completed,
        denied_rejected,
        unsupported_rejected,
        unavailable_rejected,
        stale_rejected,
        malformed_rejected,
        duplicate_cached,
        update_without_reconnect,
        reconnect_clears_history,
        input_denied_without_permission,
        release_all_clears_held_keys,
        verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

    fn target() -> TargetRef {
        TargetRef::new("w5", "niri-boot1-w5").unwrap()
    }

    fn context() -> ActionContext {
        ActionContext {
            connection_epoch: "c1".into(),
            session_id: Some("s1".into()),
            grant_epoch: "g2".into(),
            target: target(),
            capability_revision: 12,
            grants: ActionGrants {
                desktop_actions: true,
                window_actions: true,
                clipboard: false,
            },
        }
    }

    fn request(action: &str, schema: u16, parameters: BTreeMap<String, Value>) -> ActionRequest {
        let id = format!("r{}", REQUEST_SEQ.fetch_add(1, Ordering::Relaxed));
        ActionRequest::new(id, &context(), action, schema, parameters).unwrap()
    }

    #[test]
    fn snapshot_rejects_duplicate_and_oversized_actions() {
        let action = descriptor(
            "window.focus",
            true,
            true,
            Authorization::Granted,
            SCHEMA_EMPTY,
            ActionPresentation::new("Focus"),
        );
        assert!(
            CapabilitySnapshot::new("c1", None, "g2", &target(), 1, vec![action.clone(), action])
                .is_err()
        );

        let actions = (0..=MAX_ACTIONS_PER_TARGET)
            .map(|index| {
                let mut value = descriptor(
                    &format!("window.focus{index}"),
                    true,
                    true,
                    Authorization::Granted,
                    SCHEMA_EMPTY,
                    ActionPresentation::new("Focus"),
                );
                value.presentation.label = "Focus".into();
                value
            })
            .collect();
        assert!(CapabilitySnapshot::new("c1", None, "g2", &target(), 1, actions).is_err());
    }

    #[test]
    fn allowed_action_completes_and_denied_action_does_not_execute() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        let maximize = request(
            "window.maximize.set",
            1,
            parameters(&[("value", Value::Bool(true))]),
        );
        let result = registry.invoke(&context(), &maximize, 1);
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(result.error_code, None);
        assert_eq!(registry.backend().calls, vec!["window.maximize.set"]);

        let denied = request("edit.copy", 1, empty_parameters());
        let result = registry.invoke(&context(), &denied, 2);
        assert_eq!(result.status, ActionStatus::Rejected);
        assert_eq!(result.error_code, Some(ContractErrorCode::PermissionDenied));
        assert_eq!(registry.backend().calls.len(), 1);
    }

    #[test]
    fn stale_context_is_rejected_before_execution() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        let mut stale = request("window.focus", 1, empty_parameters());
        stale.id = "r2".into();
        stale.capability_revision = 11;
        let result = registry.invoke(&context(), &stale, 1);
        assert_eq!(result.error_code, Some(ContractErrorCode::StaleContext));
        assert!(registry.backend().calls.is_empty());
    }

    #[test]
    fn wrong_grant_and_target_generation_are_stale() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        let mut request = request("window.focus", 1, empty_parameters());
        request.grant_epoch = "g1".into();
        assert_eq!(
            registry.invoke(&context(), &request, 1).error_code,
            Some(ContractErrorCode::StaleContext)
        );

        request.grant_epoch = "g2".into();
        request.target_generation = "niri-boot2-w5".into();
        request.id = "r-stale-generation".into();
        assert_eq!(
            registry.invoke(&context(), &request, 1).error_code,
            Some(ContractErrorCode::StaleContext)
        );
    }

    #[test]
    fn malformed_and_unknown_parameters_are_rejected() {
        let malformed = request(
            "window.maximize.set",
            1,
            parameters(&[("value", Value::String("yes".into()))]),
        );
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        assert_eq!(
            registry.invoke(&context(), &malformed, 1).error_code,
            Some(ContractErrorCode::InvalidParameters)
        );

        let unknown_schema = request("window.focus", 2, empty_parameters());
        assert_eq!(
            registry.invoke(&context(), &unknown_schema, 2).error_code,
            Some(ContractErrorCode::UnsupportedAction)
        );
    }

    #[test]
    fn duplicate_request_returns_cached_result_and_conflicts_are_rejected() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        let maximize = request(
            "window.maximize.set",
            1,
            parameters(&[("value", Value::Bool(true))]),
        );
        let first = registry.invoke(&context(), &maximize, 10);
        let second = registry.invoke(&context(), &maximize, 10);
        assert_eq!(first, second);
        assert_eq!(registry.backend().calls.len(), 1);

        let mut conflicting = maximize.clone();
        conflicting.parameters = parameters(&[("value", Value::Bool(false))]);
        let result = registry.invoke(&context(), &conflicting, 11);
        assert_eq!(result.error_code, Some(ContractErrorCode::DuplicateRequest));
        assert_eq!(registry.backend().calls.len(), 1);
    }

    #[test]
    fn result_history_expires_and_per_connection_state_clears() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        let maximize = request(
            "window.maximize.set",
            1,
            parameters(&[("value", Value::Bool(true))]),
        );
        registry.invoke(&context(), &maximize, 10);
        let result = registry.invoke(&context(), &maximize, 10 + ACTION_RESULT_TTL_SECS);
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(registry.backend().calls.len(), 2);

        let mut other = context();
        other.connection_epoch = "c2".into();
        registry.begin_connection("c2");
        let result = registry.invoke(&other, &maximize, 100);
        assert_eq!(result.error_code, Some(ContractErrorCode::StaleContext));
    }

    #[test]
    fn action_rate_limit_is_bounded() {
        let mut registry = ActionRegistry::new(FakeActionBackend::default());
        for index in 0..MAX_ACTION_REQUESTS_PER_SECOND {
            let mut request = request("window.focus", 1, empty_parameters());
            request.id = format!("r{index}");
            let result = registry.invoke(&context(), &request, 1);
            assert_eq!(result.status, ActionStatus::Completed);
        }
        let mut request = request("window.focus", 1, empty_parameters());
        request.id = "overflow".into();
        assert_eq!(
            registry.invoke(&context(), &request, 1).error_code,
            Some(ContractErrorCode::RateLimited)
        );
    }

    #[test]
    fn menu_trees_are_bounded_and_ids_are_unique() {
        let menu = MenuSnapshot {
            v: 1,
            message_type: MENU_MESSAGE_TYPE.into(),
            connection_epoch: "c1".into(),
            session_id: None,
            target_id: "w5".into(),
            target_generation: "niri-boot1-w5".into(),
            menu_revision: 1,
            items: vec![MenuItem {
                id: "file".into(),
                label: "File".into(),
                enabled: true,
                checked: None,
                children: vec![MenuItem {
                    id: "file.close".into(),
                    label: "Close".into(),
                    enabled: true,
                    checked: None,
                    children: Vec::new(),
                }],
            }],
        };
        assert!(menu.validate().is_ok());

        let mut duplicate = menu.clone();
        duplicate.items.push(duplicate.items[0].clone());
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn input_requires_permission_and_matching_geometry() {
        let mut lease = InputLease::new();
        let view_only = InputContext {
            target: target(),
            grant_epoch: "g2".into(),
            geometry_epoch: 3,
            permissions: InputPermissions::default(),
        };
        let absolute = InputEvent::PointerAbsolute {
            x: 0.5,
            y: 0.5,
            geometry_epoch: 3,
        };
        assert_eq!(
            lease.apply(&absolute, &view_only).unwrap_err().code,
            ContractErrorCode::PermissionRequired
        );

        let mut control = view_only.clone();
        control.permissions.pointer = true;
        assert_eq!(
            lease
                .apply(
                    &InputEvent::PointerAbsolute {
                        x: 0.5,
                        y: 0.5,
                        geometry_epoch: 2,
                    },
                    &control
                )
                .unwrap_err()
                .code,
            ContractErrorCode::StaleContext
        );
        assert!(lease.apply(&absolute, &control).is_ok());
    }

    #[test]
    fn cancel_and_release_all_clear_held_input() {
        let mut lease = InputLease::new();
        let context = InputContext {
            target: target(),
            grant_epoch: "g2".into(),
            geometry_epoch: 3,
            permissions: InputPermissions {
                pointer: true,
                keyboard: true,
                text: true,
            },
        };
        lease
            .apply(
                &InputEvent::PointerButton {
                    button: PointerButton::Primary,
                    pressed: true,
                },
                &context,
            )
            .unwrap();
        lease
            .apply(
                &InputEvent::KeyPhysical {
                    usage: 4,
                    pressed: true,
                    repeat: false,
                },
                &context,
            )
            .unwrap();
        let effects = lease.apply(&InputEvent::Cancel, &context).unwrap();
        assert_eq!(effects.len(), 2);
        assert!(lease.is_empty());
        assert!(lease.release_all().is_empty());
    }

    #[test]
    fn text_and_relative_pointer_bounds_are_enforced() {
        let mut lease = InputLease::new();
        let context = InputContext {
            target: target(),
            grant_epoch: "g2".into(),
            geometry_epoch: 3,
            permissions: InputPermissions {
                pointer: true,
                keyboard: true,
                text: true,
            },
        };
        assert!(
            lease
                .apply(
                    &InputEvent::TextCommit {
                        text: "x".repeat(MAX_TEXT_COMMIT_BYTES + 1)
                    },
                    &context
                )
                .is_err()
        );
        assert!(
            lease
                .apply(
                    &InputEvent::PointerRelative {
                        dx: MAX_RELATIVE_POINTER + 1.0,
                        dy: 0.0
                    },
                    &context
                )
                .is_err()
        );
        assert!(
            lease
                .apply(
                    &InputEvent::TextCommit {
                        text: "hello".into()
                    },
                    &context
                )
                .is_ok()
        );
    }

    #[test]
    fn niri_adapter_only_advertises_supported_semantics() {
        struct RecordingRunner(Vec<String>);
        impl CommandRunner for RecordingRunner {
            fn run(&mut self, arguments: &[String]) -> Result<(), String> {
                self.0.push(arguments.join(" "));
                Ok(())
            }
        }

        let backend = NiriActionBackend::new(RecordingRunner(Vec::new()));
        let descriptors = backend.descriptors();
        assert!(
            descriptors
                .iter()
                .any(|action| action.id == "window.focus" && action.supported)
        );
        assert!(
            descriptors
                .iter()
                .any(|action| { action.id == "window.maximize.set" && !action.supported })
        );
        assert!(
            !descriptors
                .iter()
                .any(|action| action.id == "desktop.launcher.open")
        );

        let mut backend = NiriActionBackend::new(RecordingRunner(Vec::new()));
        backend
            .execute(&target(), "window.focus", &empty_parameters())
            .unwrap();
        assert_eq!(backend.runner.0, vec!["focus-window --id 5"]);
        assert_eq!(
            backend
                .execute(
                    &target(),
                    "desktop.workspace.step",
                    &parameters(&[("direction", Value::String("previous".into()))])
                )
                .unwrap(),
            BackendOutcome::completed()
        );
        assert_eq!(backend.runner.0[1], "focus-workspace-up");
    }

    #[test]
    fn backend_failure_is_reported_without_claiming_success() {
        let mut registry = ActionRegistry::new(FakeActionBackend {
            fail_with: Some(ContractErrorCode::OutcomeUnknown),
            ..FakeActionBackend::default()
        });
        let result = registry.invoke(
            &context(),
            &request("window.focus", 1, empty_parameters()),
            1,
        );
        assert_eq!(result.status, ActionStatus::OutcomeUnknown);
        assert_eq!(result.error_code, Some(ContractErrorCode::OutcomeUnknown));
    }

    #[test]
    fn diagnostic_verifies_every_contract_case() {
        let report = diagnostic().unwrap();
        assert!(report.verified, "{report:?}");
    }
}
