//! An incremental, serializable catalog driven by Niri's newline-delimited IPC events.
//!
//! The event stream begins with a WindowsChanged snapshot. Other compositor
//! event families are ignored, allowing future Niri versions to add events.

use crate::{NiriError, Window};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CatalogUpdate {
    Snapshot { windows: Vec<Window> },
    Upsert { window: Window },
    Removed { id: u64 },
}

#[derive(Debug, Default)]
pub struct WindowCatalog {
    windows: BTreeMap<u64, Window>,
    ready: bool,
}

impl WindowCatalog {
    pub fn windows(&self) -> Vec<Window> {
        self.windows.values().cloned().collect()
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Apply exactly one JSON object from niri msg --json event-stream.
    /// Unrelated Niri events are ignored, but malformed known events fail.
    pub fn apply_line(&mut self, line: &str) -> Result<Vec<CatalogUpdate>, NiriError> {
        let event: Value = serde_json::from_str(line)?;
        let Some(object) = event.as_object() else {
            return Err(NiriError::InvalidArgument(
                "expected a JSON object in Niri event stream".into(),
            ));
        };

        if let Some(payload) = object.get("WindowsChanged") {
            #[derive(serde::Deserialize)]
            struct Snapshot {
                windows: Vec<Window>,
            }
            let snapshot: Snapshot = serde_json::from_value(payload.clone())?;
            self.windows = snapshot
                .windows
                .into_iter()
                .map(|window| (window.id, window))
                .collect();
            self.ready = true;
            return Ok(vec![CatalogUpdate::Snapshot {
                windows: self.windows(),
            }]);
        }

        if let Some(payload) = object.get("WindowOpenedOrChanged") {
            #[derive(serde::Deserialize)]
            struct Changed {
                window: Window,
            }
            let Changed { window } = serde_json::from_value(payload.clone())?;
            if self.windows.get(&window.id) == Some(&window) {
                return Ok(vec![]);
            }
            self.windows.insert(window.id, window.clone());
            return Ok(vec![CatalogUpdate::Upsert { window }]);
        }

        if let Some(payload) = object.get("WindowClosed") {
            #[derive(serde::Deserialize)]
            struct Closed {
                id: u64,
            }
            let Closed { id } = serde_json::from_value(payload.clone())?;
            return Ok(if self.windows.remove(&id).is_some() {
                vec![CatalogUpdate::Removed { id }]
            } else {
                vec![]
            });
        }

        if let Some(payload) = object.get("WindowFocusChanged") {
            #[derive(serde::Deserialize)]
            struct FocusChanged {
                id: Option<u64>,
            }
            let FocusChanged { id } = serde_json::from_value(payload.clone())?;
            let mut updates = Vec::new();
            for window in self.windows.values_mut() {
                let focused = Some(window.id) == id;
                if window.is_focused != focused {
                    window.is_focused = focused;
                    updates.push(CatalogUpdate::Upsert {
                        window: window.clone(),
                    });
                }
            }
            return Ok(updates);
        }

        if let Some(payload) = object.get("WindowUrgencyChanged") {
            #[derive(serde::Deserialize)]
            struct UrgencyChanged {
                id: u64,
                urgent: bool,
            }
            let UrgencyChanged { id, urgent } = serde_json::from_value(payload.clone())?;
            if let Some(window) = self.windows.get_mut(&id)
                && window.is_urgent != urgent
            {
                window.is_urgent = urgent;
                return Ok(vec![CatalogUpdate::Upsert {
                    window: window.clone(),
                }]);
            }
        }
        Ok(vec![])
    }
}

/// Stream events as newline-delimited JSON (stable for consumers) or human-readable text.
/// Flush every update so consumers see changes without buffering delays.
pub fn relay_events<R: BufRead, W: Write>(
    reader: R,
    mut writer: W,
    json: bool,
) -> Result<(), NiriError> {
    let mut catalog = WindowCatalog::default();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        for update in catalog.apply_line(&line)? {
            if json {
                serde_json::to_writer(&mut writer, &update)
                    .map_err(|error| NiriError::Unavailable(format!("serialize event: {error}")))?;
                writeln!(writer)?;
            } else {
                match update {
                    CatalogUpdate::Snapshot { windows } => {
                        writeln!(writer, "Window snapshot: {} windows", windows.len())?;
                        for window in &windows {
                            print_window(&mut writer, window)?;
                        }
                    }
                    CatalogUpdate::Upsert { window } => {
                        write!(writer, "Updated: ")?;
                        print_window(&mut writer, &window)?;
                    }
                    CatalogUpdate::Removed { id } => writeln!(writer, "Removed window {id}")?,
                }
            }
            writer.flush()?;
        }
    }
    Ok(())
}

fn print_window(writer: &mut impl io::Write, window: &Window) -> io::Result<()> {
    writeln!(
        writer,
        "{}  {}  {}{}",
        window.id,
        window.app_id.as_deref().unwrap_or("(unknown)"),
        window.title.as_deref().unwrap_or("(untitled)"),
        if window.is_focused { " [focused]" } else { "" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SNAPSHOT: &str = r#"{"WindowsChanged":{"windows":[{"id":9,"title":"Browser","app_id":"firefox","is_focused":true},{"id":2,"title":"Terminal","app_id":"alacritty"}]}}"#;

    #[test]
    fn initial_snapshot_is_sorted() {
        let mut catalog = WindowCatalog::default();
        assert!(!catalog.ready());
        let changes = catalog.apply_line(SNAPSHOT).unwrap();
        assert!(catalog.ready());
        assert!(
            matches!(&changes[0], CatalogUpdate::Snapshot { windows } if windows.iter().map(|w| w.id).collect::<Vec<_>>() == vec![2, 9])
        );
    }

    #[test]
    fn incremental_open_update_close_and_idempotency() {
        let mut catalog = WindowCatalog::default();
        catalog.apply_line(SNAPSHOT).unwrap();
        let open =
            r#"{"WindowOpenedOrChanged":{"window":{"id":11,"title":"Editor","app_id":"code"}}}"#;
        assert_eq!(catalog.apply_line(open).unwrap().len(), 1);
        assert!(catalog.apply_line(open).unwrap().is_empty());
        assert_eq!(catalog.windows().len(), 3);
        let removed = catalog.apply_line(r#"{"WindowClosed":{"id":11}}"#).unwrap();
        assert_eq!(removed, vec![CatalogUpdate::Removed { id: 11 }]);
        assert!(
            catalog
                .apply_line(r#"{"WindowClosed":{"id":11}}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn focus_and_urgency_events_update_only_known_windows() {
        let mut catalog = WindowCatalog::default();
        catalog.apply_line(SNAPSHOT).unwrap();
        let updates = catalog
            .apply_line(r#"{"WindowFocusChanged":{"id":2}}"#)
            .unwrap();
        assert_eq!(updates.len(), 2);
        assert!(catalog.windows().iter().any(|w| w.id == 2 && w.is_focused));
        assert!(catalog.windows().iter().any(|w| w.id == 9 && !w.is_focused));
        assert_eq!(
            catalog
                .apply_line(r#"{"WindowUrgencyChanged":{"id":2,"urgent":true}}"#)
                .unwrap()
                .len(),
            1
        );
        assert!(catalog.windows().iter().any(|w| w.id == 2 && w.is_urgent));
        assert!(
            catalog
                .apply_line(r#"{"WindowFocusChanged":{"id":999}}"#)
                .unwrap()
                .len()
                == 1
        );
    }

    #[test]
    fn ignores_unrelated_events_but_rejects_malformed_known_events() {
        let mut catalog = WindowCatalog::default();
        assert!(
            catalog
                .apply_line(r#"{"WorkspacesChanged":{"workspaces":[]}}"#)
                .unwrap()
                .is_empty()
        );
        assert!(
            catalog
                .apply_line(r#"{"WindowClosed":{"bogus":42}}"#)
                .is_err()
        );
        assert!(catalog.apply_line("not json").is_err());
    }

    #[test]
    fn relays_json_lines_and_flushes() {
        let stream = format!("{SNAPSHOT}\n{{\"WindowClosed\":{{\"id\":9}}}}\n");
        let mut output = Vec::new();
        relay_events(Cursor::new(stream), &mut output, true).unwrap();
        let output = String::from_utf8(output).unwrap();
        let parsed: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["type"], "snapshot");
        assert_eq!(parsed[1]["type"], "removed");
        assert_eq!(parsed[1]["id"], 9);
    }
}
