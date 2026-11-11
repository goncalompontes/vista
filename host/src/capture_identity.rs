//! Capture identity boundary between compositor catalogs and portal grants.
//!
//! A catalog target is a request hint, never proof of the source returned by the
//! desktop chooser. Only a provider-issued opaque correlation token can bind the
//! two. In its absence, the approved source remains a picker-only target.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CaptureScope {
    pub host_id: String,
    pub backend_id: String,
    pub session_id: String,
    pub generation: u64,
    pub permission_epoch: u64,
}

impl CaptureScope {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.host_id.is_empty() || self.backend_id.is_empty() || self.session_id.is_empty() {
            return Err("capture scope identifiers must be non-empty");
        }
        if self.generation == 0 || self.permission_epoch == 0 {
            return Err("capture scope generations must be non-zero");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CatalogTargetRef {
    pub scope: CaptureScope,
    pub target_id: String,
    pub target_generation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct PortalSelectedSourceRef {
    pub scope: CaptureScope,
    pub portal_session: String,
    pub source_type: PortalSourceType,
    pub pipewire_node_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipewire_serial: Option<u64>,
    /// Opaque, provider-attested stable identity. Never populate from UI metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_identity: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalSourceType {
    Window,
    Monitor,
    Virtual,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CaptureGrantRef {
    pub scope: CaptureScope,
    pub grant_id: String,
    pub portal_session: String,
    pub source_type: PortalSourceType,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct VerifiedTargetBinding {
    pub scope: CaptureScope,
    pub target: CatalogTargetRef,
    pub source_identity: String,
    pub grant_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamTarget {
    Catalog {
        target: CatalogTargetRef,
        binding: VerifiedTargetBinding,
    },
    PickerOnly {
        source: PortalSelectedSourceRef,
        grant: CaptureGrantRef,
        lifecycle_generation: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchOutcome {
    Verified(Box<VerifiedTargetBinding>),
    PickerOnly,
    Rejected(&'static str),
}

/// Match only on a provider-attested identity token. Titles, PIDs, app IDs,
/// geometry, focus, ordering, node IDs and serials are not catalog identity.
pub fn match_portal_source(
    target: &CatalogTargetRef,
    source: &PortalSelectedSourceRef,
    grant: &CaptureGrantRef,
) -> MatchOutcome {
    if target.scope != source.scope || source.scope != grant.scope {
        return MatchOutcome::Rejected("capture scope mismatch");
    }
    if source.portal_session != grant.portal_session {
        return MatchOutcome::Rejected("portal session mismatch");
    }
    if source.source_type != grant.source_type {
        return MatchOutcome::Rejected("grant source type mismatch");
    }
    if source.source_type != PortalSourceType::Window {
        return MatchOutcome::Rejected(
            "window capture cannot fall back to monitor or virtual source",
        );
    }
    if target.scope.validate().is_err()
        || source.scope.validate().is_err()
        || target.target_id.is_empty()
        || target.target_generation.is_empty()
        || grant.grant_id.is_empty()
        || source.pipewire_node_id == 0
    {
        return MatchOutcome::Rejected("invalid capture identity or grant");
    }
    let Some(identity) = source
        .verified_identity
        .as_ref()
        .filter(|value| !value.is_empty())
    else {
        return MatchOutcome::PickerOnly;
    };
    MatchOutcome::Verified(Box::new(VerifiedTargetBinding {
        scope: target.scope.clone(),
        target: target.clone(),
        source_identity: identity.clone(),
        grant_id: grant.grant_id.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs() -> (CatalogTargetRef, PortalSelectedSourceRef, CaptureGrantRef) {
        let scope = CaptureScope {
            host_id: "host-a".into(),
            backend_id: "portal-xdg".into(),
            session_id: "session-1".into(),
            generation: 4,
            permission_epoch: 9,
        };
        (
            CatalogTargetRef {
                scope: scope.clone(),
                target_id: "niri:42".into(),
                target_generation: "boot-4:42".into(),
            },
            PortalSelectedSourceRef {
                scope: scope.clone(),
                portal_session: "portal-1".into(),
                source_type: PortalSourceType::Window,
                pipewire_node_id: 77,
                pipewire_serial: Some(1001),
                verified_identity: None,
            },
            CaptureGrantRef {
                scope,
                grant_id: "grant-1".into(),
                portal_session: "portal-1".into(),
                source_type: PortalSourceType::Window,
            },
        )
    }

    #[test]
    fn absent_correlation_is_picker_only_not_catalog_binding() {
        let (target, source, grant) = refs();
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::PickerOnly
        );
    }

    #[test]
    fn provider_attested_identity_can_bind_within_exact_scope() {
        let (target, mut source, grant) = refs();
        source.verified_identity = Some("provider-object:abc".into());
        let MatchOutcome::Verified(binding) = match_portal_source(&target, &source, &grant) else {
            panic!("expected verified binding")
        };
        assert_eq!(binding.target, target);
        assert_eq!(binding.source_identity, "provider-object:abc");
    }

    #[test]
    fn matching_metadata_or_reused_pipewire_node_never_establishes_identity() {
        let (target, mut source, grant) = refs();
        source.pipewire_node_id = 77;
        source.pipewire_serial = Some(1001);
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::PickerOnly
        );
        source.pipewire_node_id = 88;
        source.pipewire_serial = Some(1002);
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::PickerOnly
        );
    }

    #[test]
    fn title_pid_or_geometry_are_not_identity_inputs() {
        let (target, source, grant) = refs();
        // Catalog metadata deliberately does not exist in this identity API.
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::PickerOnly
        );
    }

    #[test]
    fn stale_generation_or_permission_epoch_is_rejected() {
        let (target, mut source, grant) = refs();
        source.scope.generation += 1;
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::Rejected("capture scope mismatch")
        );
        let (target, mut source, grant) = refs();
        source.scope.permission_epoch += 1;
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::Rejected("capture scope mismatch")
        );
    }

    #[test]
    fn window_request_refuses_monitor_and_virtual_fallback() {
        for source_type in [
            PortalSourceType::Monitor,
            PortalSourceType::Virtual,
            PortalSourceType::Unknown,
        ] {
            let (target, mut source, mut grant) = refs();
            source.source_type = source_type.clone();
            grant.source_type = source_type;
            assert_eq!(
                match_portal_source(&target, &source, &grant),
                MatchOutcome::Rejected(
                    "window capture cannot fall back to monitor or virtual source"
                )
            );
        }
    }

    #[test]
    fn mismatched_portal_session_or_grant_scope_is_rejected() {
        let (target, mut source, grant) = refs();
        source.portal_session = "other".into();
        assert_eq!(
            match_portal_source(&target, &source, &grant),
            MatchOutcome::Rejected("portal session mismatch")
        );
    }

    #[test]
    fn picker_only_wire_target_is_distinct_from_catalog_target() {
        let (_target, source, grant) = refs();
        let target = StreamTarget::PickerOnly {
            source,
            grant,
            lifecycle_generation: 1,
        };
        let json = serde_json::to_value(target).unwrap();
        assert_eq!(json["kind"], "picker_only");
        assert!(json.get("target").is_none());
        assert_ne!(json["kind"], "catalog");
    }
}
