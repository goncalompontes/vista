//! Bounded, versioned window-catalog snapshot for the authenticated control
//! channel.
//!
//! The catalog is host-owned metadata, not a capability grant. It carries only
//! stable identifying fields and a session generation; it never includes pixels,
//! process arguments or filesystem paths. Limits are enforced on both Rust and
//! Swift sides.

use crate::{OutputInfo, Window, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CATALOG_CONTRACT_VERSION: u16 = 1;
pub const CATALOG_MESSAGE_TYPE: &str = "catalog.snapshot";
pub const MAX_CATALOG_ENTRIES: usize = 256;
pub const MAX_CATALOG_WORKSPACES: usize = 64;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_APP_ID_BYTES: usize = 128;
pub const MAX_OUTPUT_NAME_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogWireError {
    pub detail: String,
}

impl CatalogWireError {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for CatalogWireError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for CatalogWireError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub window_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<u64>,
}

impl CatalogEntry {
    pub fn from_window(window: &Window) -> Self {
        Self {
            window_id: window.id,
            title: window.title.clone(),
            app_id: window.app_id.clone(),
            workspace_id: window.workspace_id,
        }
    }

    pub fn display_name(&self) -> String {
        self.title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .or(self.app_id.as_deref())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Window {}", self.window_id))
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(title) = &self.title
            && title.len() > MAX_TITLE_BYTES
        {
            return Err(format!("title exceeds {MAX_TITLE_BYTES} bytes"));
        }
        if let Some(app_id) = &self.app_id
            && (app_id.is_empty()
                || app_id.len() > MAX_APP_ID_BYTES
                || app_id.chars().any(char::is_control))
        {
            return Err(format!("invalid app_id for window {}", self.window_id));
        }
        if let Some(title) = &self.title
            && title
                .chars()
                .any(|character| character.is_control() && character != '\n' && character != '\t')
        {
            return Err(format!(
                "title contains control characters for window {}",
                self.window_id
            ));
        }
        Ok(())
    }
}

/// One Niri workspace (virtual desktop) that a client may select for a
/// full-desktop stream. Metadata only; selecting it is a separate request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogWorkspace {
    pub workspace_id: u64,
    pub idx: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub output: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window_id: Option<u64>,
}

impl CatalogWorkspace {
    pub fn from_workspace(workspace: &Workspace) -> Self {
        Self {
            workspace_id: workspace.id,
            idx: workspace.idx,
            name: workspace.name.clone(),
            output: workspace.output.clone(),
            is_active: workspace.is_active,
            is_focused: workspace.is_focused,
            active_window_id: workspace.active_window_id,
        }
    }

    pub fn display_name(&self) -> String {
        self.name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Desktop {}", self.idx))
    }

    fn validate(&self) -> Result<(), String> {
        if self.idx == 0 {
            return Err("workspace idx must be non-zero".into());
        }
        if self.output.is_empty() || self.output.len() > MAX_OUTPUT_NAME_BYTES {
            return Err("invalid workspace output".into());
        }
        if let Some(name) = &self.name
            && (name.len() > MAX_TITLE_BYTES || name.chars().any(char::is_control))
        {
            return Err(format!("invalid workspace name for {}", self.workspace_id));
        }
        Ok(())
    }
}

/// Logical geometry of the streamed output. Desktop and window streams are
/// delivered in this coordinate space, which lets the client map normalized
/// pointer coordinates without knowing compositor details.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogOutput {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl CatalogOutput {
    pub fn from_output(output: &OutputInfo) -> Option<Self> {
        let logical = output.logical.as_ref()?;
        Some(Self {
            name: output.name.clone(),
            width: logical.width,
            height: logical.height,
            scale: logical.scale,
        })
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > MAX_OUTPUT_NAME_BYTES {
            return Err("invalid output name".into());
        }
        if self.width == 0
            || self.height == 0
            || self.width > 16_384
            || self.height > 16_384
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            return Err("invalid output geometry".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub v: u16,
    #[serde(rename = "type")]
    pub message_type: String,
    pub connection_epoch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub generation: String,
    pub revision: u64,
    pub entries: Vec<CatalogEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<CatalogWorkspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<CatalogOutput>,
}

impl CatalogSnapshot {
    pub fn from_windows(
        connection_epoch: impl Into<String>,
        session_id: Option<String>,
        generation: impl Into<String>,
        revision: u64,
        windows: &[Window],
    ) -> Result<Self, String> {
        let entries: Vec<_> = windows
            .iter()
            .take(MAX_CATALOG_ENTRIES)
            .map(CatalogEntry::from_window)
            .collect();
        let snapshot = Self {
            v: CATALOG_CONTRACT_VERSION,
            message_type: CATALOG_MESSAGE_TYPE.into(),
            connection_epoch: connection_epoch.into(),
            session_id,
            generation: generation.into(),
            revision,
            entries,
            workspaces: Vec::new(),
            output: None,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_niri(
        connection_epoch: impl Into<String>,
        session_id: Option<String>,
        generation: impl Into<String>,
        revision: u64,
        windows: &[Window],
        workspaces: &[Workspace],
        output: Option<&OutputInfo>,
    ) -> Result<Self, String> {
        let entries: Vec<_> = windows
            .iter()
            .take(MAX_CATALOG_ENTRIES)
            .map(CatalogEntry::from_window)
            .collect();
        let workspaces: Vec<_> = workspaces
            .iter()
            .take(MAX_CATALOG_WORKSPACES)
            .map(CatalogWorkspace::from_workspace)
            .collect();
        let snapshot = Self {
            v: CATALOG_CONTRACT_VERSION,
            message_type: CATALOG_MESSAGE_TYPE.into(),
            connection_epoch: connection_epoch.into(),
            session_id,
            generation: generation.into(),
            revision,
            entries,
            workspaces,
            output: output.and_then(CatalogOutput::from_output),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.v != CATALOG_CONTRACT_VERSION {
            return Err("unsupported catalog contract version".into());
        }
        if self.message_type != CATALOG_MESSAGE_TYPE {
            return Err("unexpected catalog message type".into());
        }
        if self.connection_epoch.is_empty() || self.connection_epoch.len() > 128 {
            return Err("invalid connection_epoch".into());
        }
        if let Some(session_id) = &self.session_id
            && (session_id.is_empty() || session_id.len() > 128)
        {
            return Err("invalid session_id".into());
        }
        if self.generation.is_empty() || self.generation.len() > 256 {
            return Err("invalid catalog generation".into());
        }
        if self.revision == 0 {
            return Err("catalog revision must be non-zero".into());
        }
        if self.entries.len() > MAX_CATALOG_ENTRIES {
            return Err(format!("catalog exceeds {MAX_CATALOG_ENTRIES} entries"));
        }
        if self.workspaces.len() > MAX_CATALOG_WORKSPACES {
            return Err(format!(
                "catalog exceeds {MAX_CATALOG_WORKSPACES} workspaces"
            ));
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !seen.insert(entry.window_id) {
                return Err(format!("duplicate window id {}", entry.window_id));
            }
        }
        let mut seen_workspaces = BTreeSet::new();
        for workspace in &self.workspaces {
            workspace.validate()?;
            if !seen_workspaces.insert(workspace.workspace_id) {
                return Err(format!("duplicate workspace id {}", workspace.workspace_id));
            }
        }
        if let Some(output) = &self.output {
            output.validate()?;
        }
        Ok(())
    }

    /// Stable generation seed for the current Niri session. Niri exposes no boot
    /// id through its CLI; the socket/session name is the best available change
    /// signal, matching the capability target generation.
    pub fn current_generation() -> String {
        let socket = std::env::var("NIRI_SOCKET").unwrap_or_else(|_| "niri".into());
        socket
            .rsplit('/')
            .next()
            .unwrap_or("niri")
            .trim_end_matches(".sock")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u64, title: Option<&str>, app_id: Option<&str>) -> Window {
        Window {
            id,
            title: title.map(str::to_owned),
            app_id: app_id.map(str::to_owned),
            pid: Some(42),
            workspace_id: Some(1),
            is_focused: false,
            is_floating: false,
            is_urgent: false,
        }
    }

    #[test]
    fn converts_windows_in_stable_order_and_skips_pid() {
        let windows = vec![
            window(5, Some("Browser"), Some("firefox")),
            window(2, None, None),
        ];
        let snapshot =
            CatalogSnapshot::from_windows("s1", Some("s1".into()), "niri-test", 1, &windows)
                .unwrap();
        assert_eq!(snapshot.entries[0].window_id, 5);
        assert_eq!(snapshot.entries[1].display_name(), "Window 2");
        assert_eq!(snapshot.entries.len(), 2);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json["entries"][0].get("pid").is_none());
        assert_eq!(json["type"], "catalog.snapshot");
    }

    #[test]
    fn limits_entry_count() {
        let windows: Vec<_> = (0..(MAX_CATALOG_ENTRIES + 10))
            .map(|id| window(id as u64, None, None))
            .collect();
        let snapshot = CatalogSnapshot::from_windows("s1", None, "gen", 1, &windows).unwrap();
        assert_eq!(snapshot.entries.len(), MAX_CATALOG_ENTRIES);
    }

    #[test]
    fn rejects_duplicates_long_fields_and_bad_revision() {
        let mut snapshot =
            CatalogSnapshot::from_windows("s1", None, "gen", 1, &[window(1, None, None)]).unwrap();
        snapshot
            .entries
            .push(CatalogEntry::from_window(&window(1, None, None)));
        assert!(snapshot.validate().is_err());

        let mut snapshot =
            CatalogSnapshot::from_windows("s1", None, "gen", 1, &[window(1, None, None)]).unwrap();
        snapshot.revision = 0;
        assert!(snapshot.validate().is_err());

        let long = "x".repeat(MAX_TITLE_BYTES + 1);
        let mut snapshot =
            CatalogSnapshot::from_windows("s1", None, "gen", 1, &[window(1, None, None)]).unwrap();
        snapshot.entries[0].title = Some(long);
        assert!(snapshot.validate().is_err());
    }

    #[test]
    fn empty_catalog_is_valid() {
        let snapshot = CatalogSnapshot::from_windows("s1", None, "gen", 2, &[]).unwrap();
        assert!(snapshot.entries.is_empty());
        assert!(snapshot.validate().is_ok());
    }
}
