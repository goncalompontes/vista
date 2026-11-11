//! Passive Linux desktop observations. No capture/session/input/clipboard methods
//! are representable by this transport. Advertisements never become grants.
use ashpd::zbus::{
    self,
    proxy::{Builder, CacheProperties, MethodFlags},
    zvariant::OwnedValue,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const INTERFACES: [&str; 3] = ["ScreenCast", "RemoteDesktop", "Clipboard"];
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeError {
    Unavailable,
    Timeout,
    InvalidReply,
    OwnerChanged,
    LimitExceeded,
}

/// Closed query vocabulary; no arbitrary method names or side-effecting calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    BusId,
    Names,
    Owner(String),
    Property {
        destination: String,
        interface: String,
        property: String,
    },
    RemoteDesktopIntrospection,
}

#[allow(async_fn_in_trait)]
pub trait ReadOnlyBus {
    async fn query(&self, query: Query) -> Result<serde_json::Value, ProbeError>;
}

pub struct SessionBus(zbus::Connection);
impl SessionBus {
    pub async fn connect() -> Result<Self, ProbeError> {
        tokio::time::timeout(TIMEOUT, zbus::Connection::session())
            .await
            .map_err(|_| ProbeError::Timeout)?
            .map(Self)
            .map_err(|_| ProbeError::Unavailable)
    }
}

impl ReadOnlyBus for SessionBus {
    async fn query(&self, query: Query) -> Result<serde_json::Value, ProbeError> {
        let work = async {
            let (destination, path, interface, method) = match &query {
                Query::BusId => (BUS, BUS_PATH, BUS, "GetId"),
                Query::Names => (BUS, BUS_PATH, BUS, "ListNames"),
                Query::Owner(_) => (BUS, BUS_PATH, BUS, "GetNameOwner"),
                Query::Property { destination, .. } => (
                    destination.as_str(),
                    PATH,
                    "org.freedesktop.DBus.Properties",
                    "Get",
                ),
                Query::RemoteDesktopIntrospection => (
                    DESKTOP,
                    PATH,
                    "org.freedesktop.DBus.Introspectable",
                    "Introspect",
                ),
            };
            // No automatic activation, property cache subscriptions, or interactive auth.
            let proxy = Builder::<zbus::Proxy<'_>>::new(&self.0)
                .destination(destination)?
                .path(path)?
                .interface(interface)?
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            let flags = MethodFlags::NoAutoStart.into();
            let value = match &query {
                Query::BusId | Query::RemoteDesktopIntrospection => {
                    let reply: Option<String> = proxy.call_with_flags(method, flags, &()).await?;
                    serde_json::json!(reply)
                }
                Query::Names => {
                    let reply: Option<Vec<String>> =
                        proxy.call_with_flags(method, flags, &()).await?;
                    serde_json::json!(reply)
                }
                Query::Owner(name) => {
                    let reply: Option<String> =
                        proxy.call_with_flags(method, flags, &(name,)).await?;
                    serde_json::json!(reply)
                }
                Query::Property {
                    interface,
                    property,
                    ..
                } => {
                    let reply: Option<OwnedValue> = proxy
                        .call_with_flags(method, flags, &(interface, property))
                        .await?;
                    let value = reply.ok_or(zbus::Error::Failure("missing reply".into()))?;
                    serde_json::json!(
                        u32::try_from(value)
                            .map_err(|_| zbus::Error::Failure("not uint32".into()))?
                    )
                }
            };
            Ok::<_, zbus::Error>(value)
        };
        // Never serialize D-Bus error text: a service can put user content in it.
        tokio::time::timeout(TIMEOUT, work)
            .await
            .map_err(|_| ProbeError::Timeout)?
            .map_err(|_| ProbeError::Unavailable)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub host_os: String,
    pub distribution: Option<String>,
    pub distribution_version: Option<String>,
    pub display_server_hint: Option<String>,
    pub desktop_hint: Option<String>,
    pub compositor_version: Option<String>,
    pub pipewire_version: Option<String>,
}

fn safe_token(value: &str) -> Option<String> {
    let value = value.trim_matches('"');
    (!value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+-:".contains(&c)))
    .then(|| value.to_string())
}
impl Environment {
    pub fn current() -> Self {
        let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let field = |key: &str| {
            release
                .lines()
                .find_map(|line| line.strip_prefix(key).and_then(safe_token))
        };
        let session = std::env::var("XDG_SESSION_TYPE")
            .ok()
            .filter(|s| matches!(s.as_str(), "wayland" | "x11" | "tty"));
        // Report only recognized hints. Arbitrary environment values may contain secrets.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok().and_then(|value| {
            let recognized: Vec<_> = value
                .split(':')
                .filter_map(|s| match s.to_ascii_lowercase().as_str() {
                    "niri" => Some("niri"),
                    "gnome" => Some("gnome"),
                    "kde" => Some("kde"),
                    "sway" => Some("sway"),
                    "hyprland" => Some("hyprland"),
                    _ => None,
                })
                .collect();
            (!recognized.is_empty()).then(|| recognized.join(":"))
        });
        Self {
            host_os: std::env::consts::OS.into(),
            distribution: field("ID="),
            distribution_version: field("VERSION_ID="),
            display_server_hint: session,
            desktop_hint: desktop,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceObservation {
    pub version: Option<u32>,
    pub source_types: Option<u32>,
    pub cursor_modes: Option<u32>,
    pub device_types: Option<u32>,
    /// Method advertisement only, never a successful EIS connection.
    pub eis_method_advertised: Option<bool>,
    pub errors: BTreeMap<String, ProbeError>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendObservation {
    pub service: String,
    pub interfaces: BTreeMap<String, InterfaceObservation>,
    /// A running provider is not evidence of frontend routing to that provider.
    pub selected_for: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterObservation {
    pub catalog_available: bool,
    pub actions_available: bool,
    pub adapter: Option<String>,
    pub reason: String,
}

/// Internal epoch keys are deliberately excluded from serialized diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub environment: Environment,
    pub frontend: BTreeMap<String, InterfaceObservation>,
    pub running_backends: Vec<BackendObservation>,
    pub adapters: AdapterObservation,
    pub errors: BTreeMap<String, ProbeError>,
    #[serde(skip)]
    pub epoch_keys: BTreeMap<String, String>,
}

async fn text_query(bus: &impl ReadOnlyBus, query: Query) -> Result<String, ProbeError> {
    bus.query(query)
        .await?
        .as_str()
        .map(ToString::to_string)
        .ok_or(ProbeError::InvalidReply)
}
async fn uint_query(
    bus: &impl ReadOnlyBus,
    destination: &str,
    interface: &str,
    property: &str,
    errors: &mut BTreeMap<String, ProbeError>,
) -> Option<u32> {
    let result = bus
        .query(Query::Property {
            destination: destination.into(),
            interface: interface.into(),
            property: property.into(),
        })
        .await
        .and_then(|v| {
            v.as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or(ProbeError::InvalidReply)
        });
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            errors.insert(property.into(), error);
            None
        }
    }
}
async fn interfaces(
    bus: &impl ReadOnlyBus,
    destination: &str,
    prefix: &str,
) -> BTreeMap<String, InterfaceObservation> {
    let mut result = BTreeMap::new();
    for name in INTERFACES {
        let interface = format!("{prefix}.{name}");
        let mut obs = InterfaceObservation::default();
        obs.version = uint_query(bus, destination, &interface, "version", &mut obs.errors).await;
        // Backend interfaces do not define the frontend Available* properties.
        if prefix == "org.freedesktop.portal" && obs.version.is_some_and(|v| v > 0) {
            if name == "ScreenCast" {
                obs.source_types = uint_query(
                    bus,
                    destination,
                    &interface,
                    "AvailableSourceTypes",
                    &mut obs.errors,
                )
                .await;
                obs.cursor_modes = uint_query(
                    bus,
                    destination,
                    &interface,
                    "AvailableCursorModes",
                    &mut obs.errors,
                )
                .await;
            }
            if name == "RemoteDesktop" {
                obs.device_types = uint_query(
                    bus,
                    destination,
                    &interface,
                    "AvailableDeviceTypes",
                    &mut obs.errors,
                )
                .await;
                // v2 defines ConnectToEIS, but introspection must actually advertise it.
                obs.eis_method_advertised =
                    match text_query(bus, Query::RemoteDesktopIntrospection).await {
                        Ok(xml) => Some(remote_desktop_eis(&xml)),
                        Err(error) => {
                            obs.errors.insert("introspection".into(), error);
                            None
                        }
                    };
            }
        }
        result.insert(name.into(), obs);
    }
    result
}

// Only inspect the RemoteDesktop interface, never a similarly named method on
// another interface. This bounded textual check is not an XML parser.
fn remote_desktop_eis(xml: &str) -> bool {
    // A deliberately conservative advertisement check: inspect only the exact
    // RemoteDesktop interface block, and never trust free text elsewhere in XML.
    if xml.len() > 128 * 1024 {
        return false;
    }
    for marker in [
        "<interface name=\"org.freedesktop.portal.RemoteDesktop\"",
        "<interface name='org.freedesktop.portal.RemoteDesktop'",
    ] {
        if let Some((_, tail)) = xml.split_once(marker)
            && let Some((block, _)) = tail.split_once("</interface>")
        {
            return block.contains("<method name=\"ConnectToEIS\"")
                || block.contains("<method name='ConnectToEIS'");
        }
    }
    false
}

pub async fn observe_with(
    bus: &impl ReadOnlyBus,
    environment: Environment,
    adapters: AdapterObservation,
) -> Observation {
    let mut obs = Observation {
        environment,
        adapters,
        ..Observation::default()
    };
    match text_query(bus, Query::BusId).await {
        Ok(id) => {
            obs.epoch_keys.insert("bus".into(), id);
        }
        Err(error) => {
            obs.errors.insert("bus".into(), error);
        }
    }
    match text_query(bus, Query::Owner(DESKTOP.into())).await {
        Ok(owner) => {
            obs.epoch_keys.insert(DESKTOP.into(), owner);
            obs.frontend = interfaces(bus, DESKTOP, "org.freedesktop.portal").await;
        }
        Err(error) => {
            obs.errors.insert("frontend".into(), error);
        }
    }
    match bus.query(Query::Names).await.and_then(|v| {
        serde_json::from_value::<Vec<String>>(v).map_err(|_| ProbeError::InvalidReply)
    }) {
        Ok(names) => {
            let mut names: Vec<_> = names
                .into_iter()
                .filter(|n| {
                    n.starts_with("org.freedesktop.impl.portal.desktop.")
                        && n.len() < 128
                        && n.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                })
                .collect();
            names.sort();
            names.dedup();
            if names.len() > 16 {
                obs.errors
                    .insert("backends".into(), ProbeError::LimitExceeded);
            }
            for service in names.into_iter().take(16) {
                if let Ok(owner) = text_query(bus, Query::Owner(service.clone())).await {
                    obs.epoch_keys.insert(service.clone(), owner);
                    let interfaces = interfaces(bus, &service, "org.freedesktop.impl.portal").await;
                    obs.running_backends.push(BackendObservation {
                        service,
                        interfaces,
                        selected_for: vec![],
                    });
                }
            }
        }
        Err(error) => {
            obs.errors.insert("backends".into(), error);
        }
    }
    // Discard a mixed snapshot if any service changed during discovery.
    let keys = obs.epoch_keys.clone();
    for (service, previous) in keys {
        let query = if service == "bus" {
            Query::BusId
        } else {
            Query::Owner(service.clone())
        };
        if text_query(bus, query).await.as_ref() != Ok(&previous) {
            obs.errors
                .insert("owner_changed".into(), ProbeError::OwnerChanged);
            obs.frontend.clear();
            obs.running_backends.clear();
            obs.epoch_keys.clear();
            break;
        }
    }
    obs
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Advertised,
    Unsupported,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    NeedsUserConsent,
    TemporarilyUnavailable,
    Unsupported,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    NotRequested,
    Granted,
    Denied,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub supported: Support,
    pub availability: Availability,
    pub consent_required: bool,
    pub permission: Permission,
    pub verified: bool,
    pub scope: String,
    pub reason: String,
}
impl Feature {
    fn passive(support: Support, scope: &str, reason: &str) -> Self {
        let availability = match support {
            Support::Advertised => Availability::NeedsUserConsent,
            Support::Unsupported => Availability::Unsupported,
            Support::Unknown => Availability::TemporarilyUnavailable,
        };
        Self {
            supported: support,
            availability,
            consent_required: true,
            permission: Permission::NotRequested,
            verified: false,
            scope: scope.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub operation: String,
    pub capability_revision: u64,
    pub generation: u64,
    pub observation: Observation,
    pub capabilities: BTreeMap<String, Feature>,
    pub selection_mode: String,
    pub selected_backend_reason: String,
    pub audio_sources: Vec<String>,
    pub unobserved: Vec<String>,
}

fn build_snapshot(obs: Observation) -> Snapshot {
    let sc = obs.frontend.get("ScreenCast");
    let rd = obs.frontend.get("RemoteDesktop");
    let clip = obs.frontend.get("Clipboard");
    let source = sc.and_then(|i| i.source_types);
    let support_bit = |mask: Option<u32>, bit: u32| match mask {
        Some(m) if m & bit != 0 => Support::Advertised,
        Some(_) => Support::Unsupported,
        None => Support::Unknown,
    };
    let interface_support = |i: Option<&InterfaceObservation>| match i.and_then(|i| i.version) {
        Some(v) if v > 0 => Support::Advertised,
        Some(_) => Support::Unsupported,
        None => Support::Unknown,
    };
    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "window_capture".into(),
        Feature::passive(
            support_bit(source, 2),
            "portal_selected_window",
            "advertisement_is_not_frame_verification",
        ),
    );
    capabilities.insert(
        "monitor_capture".into(),
        Feature::passive(
            support_bit(source, 1),
            "monitor",
            "separate_explicit_consent_required",
        ),
    );
    let devices = rd.and_then(|i| i.device_types);
    capabilities.insert(
        "remote_desktop".into(),
        Feature::passive(
            interface_support(rd),
            "shared_desktop",
            "scope_is_not_window_isolation",
        ),
    );
    for (name, bit) in [("keyboard", 1), ("pointer", 2), ("touchscreen", 4)] {
        capabilities.insert(
            name.into(),
            Feature::passive(
                support_bit(devices, bit),
                "shared_desktop",
                "device_advertisement_is_not_input_verification",
            ),
        );
    }
    let eis = match rd.and_then(|i| i.eis_method_advertised) {
        Some(true) if rd.and_then(|i| i.version).is_some_and(|v| v >= 2) => Support::Advertised,
        Some(_) => Support::Unsupported,
        None => Support::Unknown,
    };
    capabilities.insert(
        "eis".into(),
        Feature::passive(eis, "shared_desktop", "connection_not_attempted"),
    );
    let mut input = Feature::passive(
        Support::Unsupported,
        "unknown",
        "no_verified_scoped_input_adapter_no_uinput_fallback",
    );
    input.consent_required = true;
    capabilities.insert("scoped_input".into(), input);
    capabilities.insert(
        "clipboard".into(),
        Feature::passive(
            interface_support(clip),
            "remote_desktop_session",
            "separate_grant_and_session_required_no_data_read",
        ),
    );
    for (name, scope, reason) in [
        ("audio", "unknown", "no_verified_audio_routing_adapter"),
        (
            "private_session",
            "owned_compositor",
            "not_implemented_no_shared_desktop_fallback",
        ),
    ] {
        capabilities.insert(
            name.into(),
            Feature::passive(Support::Unsupported, scope, reason),
        );
    }
    for (name, available) in [
        ("catalog", obs.adapters.catalog_available),
        ("actions", obs.adapters.actions_available),
    ] {
        capabilities.insert(
            name.into(),
            Feature {
                supported: if available {
                    Support::Advertised
                } else {
                    Support::Unknown
                },
                availability: if available {
                    Availability::Available
                } else {
                    Availability::TemporarilyUnavailable
                },
                consent_required: name == "actions",
                permission: Permission::NotRequested,
                verified: false,
                scope: "compositor_metadata".into(),
                reason: obs.adapters.reason.clone(),
            },
        );
    }
    // Adapter reachability is not authority to execute actions.
    if let Some(actions) = capabilities.get_mut("actions")
        && actions.availability == Availability::Available
    {
        actions.availability = Availability::NeedsUserConsent;
    }
    let selection_mode = if source.is_some_and(|s| s & 2 != 0) {
        if obs.adapters.catalog_available {
            "catalog_unbound"
        } else {
            "portal_picker_only"
        }
    } else {
        "unavailable"
    };
    Snapshot {
        schema_version: 1,
        operation: "probe_only".into(),
        capability_revision: 1,
        generation: 1,
        capabilities,
        selection_mode: selection_mode.into(),
        observation: obs,
        selected_backend_reason: "frontend_does_not_expose_authoritative_per_interface_routing"
            .into(),
        audio_sources: vec![],
        unobserved: vec![
            "portal_frontend_package_version".into(),
            "portal_backend_package_versions".into(),
            "pipewire_runtime_version".into(),
            "audio_sources_not_enumerated".into(),
            "permission_persistence_not_verified".into(),
        ],
    }
}

/// Per-monitor lifecycle. Epoch identity changes invalidate every local grant;
/// capability revisions also advance for property and availability changes.
#[derive(Default)]
pub struct ProbeTracker {
    previous: Option<Snapshot>,
}
impl ProbeTracker {
    pub fn update(&mut self, obs: Observation) -> Option<Snapshot> {
        if self.previous.as_ref().is_some_and(|p| p.observation == obs) {
            return None;
        }
        let mut snapshot = build_snapshot(obs);
        if let Some(previous) = &self.previous {
            snapshot.capability_revision = previous.capability_revision + 1;
            let restarted = snapshot.observation.epoch_keys != previous.observation.epoch_keys;
            snapshot.generation = previous.generation + u64::from(restarted);
            // A passive observation cannot recover or transfer authorization.
            // All output permissions remain NotRequested; revocation must come
            // from a separate, authenticated lease/grant lifecycle provider.
        }
        self.previous = Some(snapshot.clone());
        Some(snapshot)
    }
    pub fn current(&self) -> Option<&Snapshot> {
        self.previous.as_ref()
    }
}

pub async fn observe() -> Observation {
    let mut environment = Environment::current();
    let (adapters, epoch) = probe_niri(&mut environment);
    let mut result = match SessionBus::connect().await {
        Ok(bus) => observe_with(&bus, environment, adapters).await,
        Err(error) => Observation {
            environment,
            adapters,
            errors: BTreeMap::from([("session_bus".into(), error)]),
            ..Observation::default()
        },
    };
    if let Some(epoch) = epoch {
        result.epoch_keys.insert("compositor".into(), epoch);
    }
    result
}

fn probe_niri(environment: &mut Environment) -> (AdapterObservation, Option<String>) {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::{fs::MetadataExt, net::UnixStream},
    };
    let unavailable = || {
        (
            AdapterObservation {
                reason: "no_reachable_native_adapter".into(),
                ..AdapterObservation::default()
            },
            None,
        )
    };
    let Ok(path) = std::env::var("NIRI_SOCKET") else {
        return unavailable();
    };
    // Only Version is requested: never list window titles or invoke actions.
    let result = (|| {
        let metadata = std::fs::metadata(&path).ok()?;
        let mut socket = UnixStream::connect(&path).ok()?;
        socket.set_read_timeout(Some(TIMEOUT)).ok()?;
        socket.set_write_timeout(Some(TIMEOUT)).ok()?;
        socket.write_all(b"\"Version\"\n").ok()?;
        let mut line = String::new();
        BufReader::new(socket)
            .take(4097)
            .read_line(&mut line)
            .ok()?;
        if line.len() > 4096 || !line.ends_with('\n') {
            return None;
        }
        let reply: serde_json::Value = serde_json::from_str(&line).ok()?;
        let version = safe_token(reply.get("Ok")?.get("Version")?.as_str()?)?;
        Some((
            version,
            format!(
                "{}:{}:{}:{}",
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec()
            ),
        ))
    })();
    match result {
        Some((version, epoch)) => {
            environment.compositor_version = Some(version);
            (
                AdapterObservation {
                    catalog_available: false,
                    actions_available: false,
                    adapter: Some("niri".into()),
                    reason: "version_responded_catalog_and_actions_not_verified".into(),
                },
                Some(epoch),
            )
        }
        None => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        desktop: String,
        screen_cast_version: Option<u32>,
        source_types: Option<u32>,
        cursor_modes: Option<u32>,
        remote_version: Option<u32>,
        device_types: Option<u32>,
        clipboard_version: Option<u32>,
        catalog: bool,
        expected_window: Support,
        expected_keyboard: Support,
        selection: String,
    }

    #[derive(Deserialize)]
    struct Cases {
        scenarios: Vec<Case>,
    }

    #[test]
    fn multi_desktop_capability_matrix_is_fail_closed() {
        let fixtures: Cases =
            serde_json::from_str(include_str!("../../docs/fixtures/linux-desktop-probe.json"))
                .unwrap();
        assert!(fixtures.scenarios.len() >= 10);
        for case in fixtures.scenarios {
            let mut obs = Observation::default();
            obs.environment.desktop_hint = Some(case.desktop);
            obs.frontend.insert(
                "ScreenCast".into(),
                InterfaceObservation {
                    version: case.screen_cast_version,
                    source_types: case.source_types,
                    cursor_modes: case.cursor_modes,
                    ..InterfaceObservation::default()
                },
            );
            obs.frontend.insert(
                "RemoteDesktop".into(),
                InterfaceObservation {
                    version: case.remote_version,
                    device_types: case.device_types,
                    eis_method_advertised: None,
                    ..InterfaceObservation::default()
                },
            );
            obs.frontend.insert(
                "Clipboard".into(),
                InterfaceObservation {
                    version: case.clipboard_version,
                    ..InterfaceObservation::default()
                },
            );
            obs.adapters.catalog_available = case.catalog;
            let snapshot = build_snapshot(obs);
            assert_eq!(
                snapshot.capabilities["window_capture"].supported, case.expected_window,
                "{}",
                case.name
            );
            assert_eq!(
                snapshot.capabilities["keyboard"].supported, case.expected_keyboard,
                "{}",
                case.name
            );
            assert_eq!(snapshot.selection_mode, case.selection, "{}", case.name);
            assert_eq!(snapshot.operation, "probe_only", "{}", case.name);
            for feature in snapshot.capabilities.values() {
                assert!(!feature.verified, "{}", case.name);
                assert_eq!(
                    feature.permission,
                    Permission::NotRequested,
                    "{}",
                    case.name
                );
                if feature.consent_required {
                    assert_ne!(
                        feature.availability,
                        Availability::Available,
                        "{}",
                        case.name
                    );
                }
            }
            assert_eq!(
                snapshot.capabilities["scoped_input"].supported,
                Support::Unsupported
            );
        }
    }

    #[test]
    fn eis_introspection_requires_exact_interface() {
        assert!(!remote_desktop_eis(
            "<node><interface name='other'><method name='ConnectToEIS'/></interface></node>"
        ));
        assert!(remote_desktop_eis(
            "<node><interface name='org.freedesktop.portal.RemoteDesktop'><method name='ConnectToEIS'/></interface></node>"
        ));
        assert!(!remote_desktop_eis(
            "<node><interface name='org.freedesktop.portal.RemoteDesktop'></interface><interface name='other'><method name='ConnectToEIS'/></interface></node>"
        ));
        assert!(!remote_desktop_eis(&"x".repeat(128 * 1024 + 1)));
    }

    #[test]
    fn tracker_changes_revision_and_generation_without_ever_granting() {
        let mut tracker = ProbeTracker::default();
        let mut obs = Observation::default();
        obs.epoch_keys.insert("bus".into(), "bus-1".into());
        obs.frontend.insert(
            "ScreenCast".into(),
            InterfaceObservation {
                version: Some(4),
                source_types: Some(2),
                ..InterfaceObservation::default()
            },
        );
        let first = tracker.update(obs.clone()).unwrap();
        assert_eq!((first.capability_revision, first.generation), (1, 1));
        assert!(tracker.update(obs.clone()).is_none());
        obs.frontend.get_mut("ScreenCast").unwrap().source_types = Some(1);
        let changed = tracker.update(obs.clone()).unwrap();
        assert_eq!((changed.capability_revision, changed.generation), (2, 1));
        assert_eq!(
            changed.capabilities["window_capture"].supported,
            Support::Unsupported
        );
        obs.epoch_keys.insert("bus".into(), "bus-2".into());
        let restarted = tracker.update(obs).unwrap();
        assert_eq!(
            (restarted.capability_revision, restarted.generation),
            (3, 2)
        );
        assert_eq!(
            restarted.capabilities["window_capture"].permission,
            Permission::NotRequested
        );
    }

    struct FakeBus {
        changing_owner: bool,
        owners_seen: std::sync::atomic::AtomicUsize,
    }
    impl ReadOnlyBus for FakeBus {
        async fn query(&self, query: Query) -> Result<serde_json::Value, ProbeError> {
            match query {
                Query::BusId => Ok(serde_json::json!("bus-a")),
                Query::Names => Ok(serde_json::json!([
                    "org.freedesktop.portal.Desktop",
                    "org.freedesktop.impl.portal.desktop.gnome",
                    "org.freedesktop.impl.portal.desktop.kde"
                ])),
                Query::Owner(ref service) if service == DESKTOP => {
                    let n = self
                        .owners_seen
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(serde_json::json!(if self.changing_owner && n > 0 {
                        ":1.99"
                    } else {
                        ":1.5"
                    }))
                }
                Query::Owner(service) if service.ends_with(".gnome") => {
                    Ok(serde_json::json!(":1.6"))
                }
                Query::Owner(service) if service.ends_with(".kde") => Ok(serde_json::json!(":1.7")),
                Query::Property {
                    destination,
                    interface,
                    property,
                } if property == "version" => {
                    let value = if destination == DESKTOP {
                        5
                    } else if interface.contains("Clipboard") {
                        1
                    } else {
                        2
                    };
                    Ok(serde_json::json!(value))
                }
                Query::Property { property, .. } => {
                    Ok(serde_json::json!(match property.as_str() {
                        "AvailableSourceTypes" => 2,
                        "AvailableCursorModes" => 3,
                        "AvailableDeviceTypes" => 0,
                        _ => return Err(ProbeError::InvalidReply),
                    }))
                }
                Query::RemoteDesktopIntrospection => Ok(serde_json::json!(
                    "<node><interface name='org.freedesktop.portal.RemoteDesktop'></interface></node>"
                )),
                _ => Err(ProbeError::InvalidReply),
            }
        }
    }

    #[tokio::test]
    async fn composite_backends_are_inventory_not_routing_claims() {
        let bus = FakeBus {
            changing_owner: false,
            owners_seen: std::sync::atomic::AtomicUsize::new(0),
        };
        let observation =
            observe_with(&bus, Environment::default(), AdapterObservation::default()).await;
        assert_eq!(observation.running_backends.len(), 2);
        assert!(
            observation
                .running_backends
                .iter()
                .all(|b| b.selected_for.is_empty())
        );
        let snapshot = build_snapshot(observation);
        assert_eq!(snapshot.selection_mode, "portal_picker_only");
        assert_eq!(
            snapshot.capabilities["keyboard"].supported,
            Support::Unsupported
        );
        assert_eq!(snapshot.capabilities["eis"].supported, Support::Unsupported);
    }

    #[tokio::test]
    async fn owner_race_discards_mixed_portal_snapshot() {
        let bus = FakeBus {
            changing_owner: true,
            owners_seen: std::sync::atomic::AtomicUsize::new(0),
        };
        let observation =
            observe_with(&bus, Environment::default(), AdapterObservation::default()).await;
        assert!(observation.errors.contains_key("owner_changed"));
        assert!(observation.frontend.is_empty());
        assert!(observation.running_backends.is_empty());
        assert_eq!(
            build_snapshot(observation).capabilities["window_capture"].supported,
            Support::Unknown
        );
    }

    #[test]
    fn raw_error_text_never_enters_report() {
        let obs = Observation {
            errors: BTreeMap::from([("frontend".into(), ProbeError::Unavailable)]),
            ..Observation::default()
        };
        let output = serde_json::to_string(&build_snapshot(obs)).unwrap();
        assert!(!output.contains("window title"));
        assert!(!output.contains("secret"));
    }
}
