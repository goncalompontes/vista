//! Per-session control lease: granted action/input scopes, held-input cleanup
//! and revoke semantics.
//!
//! The transport authenticates a session and the host UI grants control. This
//! module keeps that grant explicit and revocable so a disconnect, local
//! interrupt or permission change cannot leave stuck keys or accept an action
//! from an old epoch. It contains no input injection or network code.

use crate::capability::{
    ActionContext, ActionGrants, ContractError, ContractErrorCode, InputEffect, InputEvent,
    InputLease, InputPermissions, TargetRef,
};

#[derive(Debug)]
pub struct ControlLease {
    connection_epoch: String,
    session_id: Option<String>,
    target: TargetRef,
    grant_epoch: u64,
    grants: ActionGrants,
    input_permissions: InputPermissions,
    active: bool,
    input: InputLease,
}

impl ControlLease {
    /// Grant action and input scopes for one target/session.
    pub fn grant(
        connection_epoch: impl Into<String>,
        session_id: Option<String>,
        target: TargetRef,
        grants: ActionGrants,
        input_permissions: InputPermissions,
    ) -> Result<Self, ContractError> {
        let connection_epoch = connection_epoch.into();
        if connection_epoch.is_empty() {
            return Err(ContractError::new(
                ContractErrorCode::InvalidParameters,
                "connection_epoch must not be empty",
            ));
        }
        target.validate()?;
        Ok(Self {
            connection_epoch,
            session_id,
            target,
            grant_epoch: 1,
            grants,
            input_permissions,
            active: true,
            input: InputLease::new(),
        })
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn connection_epoch(&self) -> &str {
        &self.connection_epoch
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn grant_epoch(&self) -> u64 {
        self.grant_epoch
    }

    /// Stable wire value for the current grant epoch. Revocation increments it
    /// so any request carrying the previous value is stale.
    pub fn grant_epoch_string(&self) -> String {
        format!("g{}", self.grant_epoch)
    }

    pub fn grants(&self) -> ActionGrants {
        self.grants
    }

    pub fn input_permissions(&self) -> InputPermissions {
        self.input_permissions
    }

    pub fn target(&self) -> &TargetRef {
        &self.target
    }

    /// Build the authorization context for an action request. A revoked lease
    /// never produces one.
    pub fn action_context(&self, capability_revision: u64) -> Result<ActionContext, ContractError> {
        if !self.active {
            return Err(ContractError::new(
                ContractErrorCode::PermissionDenied,
                "control lease is revoked",
            ));
        }
        Ok(ActionContext {
            connection_epoch: self.connection_epoch.clone(),
            session_id: self.session_id.clone(),
            grant_epoch: self.grant_epoch_string(),
            target: self.target.clone(),
            capability_revision,
            grants: self.grants,
        })
    }

    /// Apply one portable input event under the lease. Cleanup (`cancel` or
    /// `release_all`) is handled by the inner lease and is never throttled.
    pub fn apply_input(
        &mut self,
        event: &InputEvent,
        geometry_epoch: u64,
    ) -> Result<Vec<InputEffect>, ContractError> {
        if !self.active {
            return Err(ContractError::new(
                ContractErrorCode::PermissionDenied,
                "control lease is revoked",
            ));
        }
        let context = crate::capability::InputContext {
            target: self.target.clone(),
            grant_epoch: self.grant_epoch_string(),
            geometry_epoch,
            permissions: self.input_permissions,
        };
        self.input.apply(event, &context)
    }

    /// Revoke the lease, drop every grant and release all held buttons/keys.
    /// The new epoch prevents queued requests from the old grant from replaying.
    pub fn revoke(&mut self) -> Vec<InputEffect> {
        self.active = false;
        self.grant_epoch = self.grant_epoch.saturating_add(1);
        self.grants = ActionGrants::default();
        self.input_permissions = InputPermissions::default();
        self.input.release_all()
    }

    /// Re-grant after an explicit host/user approval. The epoch advances again
    /// so nothing from the revoked lease is accepted.
    pub fn regrant(&mut self, grants: ActionGrants, input_permissions: InputPermissions) {
        self.grant_epoch = self.grant_epoch.saturating_add(1);
        self.grants = grants;
        self.input_permissions = input_permissions;
        self.active = true;
    }

    pub fn held_input(&self) -> bool {
        !self.input.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{PointerButton, TargetRef};

    fn target() -> TargetRef {
        TargetRef::new("w5", "niri-boot1-w5").unwrap()
    }

    fn lease() -> ControlLease {
        ControlLease::grant(
            "c1",
            Some("s1".into()),
            target(),
            ActionGrants {
                desktop_actions: true,
                window_actions: true,
                clipboard: false,
            },
            InputPermissions {
                pointer: true,
                keyboard: true,
                text: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn active_lease_builds_matching_action_context() {
        let lease = lease();
        let context = lease.action_context(12).unwrap();
        assert_eq!(context.connection_epoch, "c1");
        assert_eq!(context.session_id.as_deref(), Some("s1"));
        assert_eq!(context.grant_epoch, "g1");
        assert_eq!(context.target, target());
        assert_eq!(context.capability_revision, 12);
        assert!(context.grants.window_actions);
        assert!(!context.grants.clipboard);
    }

    #[test]
    fn revoke_releases_held_input_and_invalidates_old_epoch() {
        let mut lease = lease();
        let geometry = 3;
        lease
            .apply_input(
                &InputEvent::KeyPhysical {
                    usage: 4,
                    pressed: true,
                    repeat: false,
                },
                geometry,
            )
            .unwrap();
        lease
            .apply_input(
                &InputEvent::PointerButton {
                    button: PointerButton::Primary,
                    pressed: true,
                },
                geometry,
            )
            .unwrap();
        assert!(lease.held_input());

        let old_epoch = lease.grant_epoch_string();
        let effects = lease.revoke();
        assert_eq!(effects.len(), 2);
        assert!(!lease.held_input());
        assert!(!lease.is_active());
        assert_ne!(lease.grant_epoch_string(), old_epoch);
        assert_eq!(
            lease.action_context(12).unwrap_err().code,
            ContractErrorCode::PermissionDenied
        );
        assert_eq!(
            lease
                .apply_input(
                    &InputEvent::TextCommit {
                        text: "hello".into()
                    },
                    3
                )
                .unwrap_err()
                .code,
            ContractErrorCode::PermissionDenied
        );
    }

    #[test]
    fn regrant_advances_epoch_and_restores_scoped_control() {
        let mut lease = lease();
        let old_epoch = lease.grant_epoch_string();
        lease.revoke();
        lease.regrant(
            ActionGrants {
                desktop_actions: false,
                window_actions: true,
                clipboard: false,
            },
            InputPermissions {
                pointer: true,
                keyboard: false,
                text: false,
            },
        );
        assert!(lease.is_active());
        assert_ne!(lease.grant_epoch_string(), old_epoch);
        let context = lease.action_context(13).unwrap();
        assert!(context.grants.window_actions);
        assert!(!context.grants.desktop_actions);
        assert_eq!(
            lease
                .apply_input(
                    &InputEvent::KeyPhysical {
                        usage: 4,
                        pressed: true,
                        repeat: false,
                    },
                    3
                )
                .unwrap_err()
                .code,
            ContractErrorCode::PermissionRequired
        );
    }

    #[test]
    fn input_geometry_mismatch_is_rejected() {
        let mut lease = lease();
        let error = lease
            .apply_input(
                &InputEvent::PointerAbsolute {
                    x: 0.5,
                    y: 0.5,
                    geometry_epoch: 2,
                },
                3,
            )
            .unwrap_err();
        assert_eq!(error.code, ContractErrorCode::StaleContext);
    }
}
