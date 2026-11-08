use serde_json::Value;
use vista_host::capability::{
    ActionRequest, ActionResult, ActionStatus, CapabilitySnapshot, InputContext, InputEvent,
    InputLease, InputPermissions, MenuSnapshot, TargetRef,
};

const FIXTURE: &str = include_str!("../../docs/fixtures/capabilities-v1.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("capability fixture is valid JSON")
}

#[test]
fn swift_compatible_snapshot_and_request_decode_and_validate() {
    let fixture = fixture();
    let snapshot: CapabilitySnapshot =
        serde_json::from_value(fixture["capability_snapshot"].clone()).expect("snapshot decodes");
    snapshot.validate().expect("snapshot validates");
    assert_eq!(snapshot.v, 1);
    assert_eq!(snapshot.message_type, "capabilities.snapshot");
    assert_eq!(
        snapshot.action("window.focus").unwrap().authorization,
        vista_host::capability::Authorization::Granted
    );
    assert!(!snapshot.action("desktop.overview.show").unwrap().supported);
    assert!(snapshot.action("window.minimize").unwrap().supported);
    assert!(!snapshot.action("window.minimize").unwrap().enabled);
    assert_eq!(
        snapshot
            .action("example.vendor.special")
            .unwrap()
            .suggested_bindings,
        vec!["three_finger_swipe_left".to_string()]
    );

    let request: ActionRequest =
        serde_json::from_value(fixture["action_request"].clone()).expect("request decodes");
    request.validate().expect("request validates");
    assert_eq!(request.action, "window.maximize.set");
    assert_eq!(request.parameters["value"], Value::Bool(true));

    let result: ActionResult =
        serde_json::from_value(fixture["action_result"].clone()).expect("result decodes");
    assert_eq!(result.status, ActionStatus::Completed);
    assert_eq!(result.capability_revision, Some(13));
    assert_eq!(result.state.unwrap()["maximized"], Value::Bool(true));
}

#[test]
fn menu_fixture_is_bounded_and_recursively_validated() {
    let menu: MenuSnapshot =
        serde_json::from_value(fixture()["menu_snapshot"].clone()).expect("menu decodes");
    menu.validate().expect("menu validates");
    assert_eq!(menu.items[0].children.len(), 2);
}

#[test]
fn input_fixture_applies_and_releases_every_held_input() {
    let fixture = fixture();
    let events: Vec<InputEvent> =
        serde_json::from_value(fixture["input_events"].clone()).expect("input events decode");
    let context = InputContext {
        target: TargetRef::new("w5", "niri-boot1-w5").unwrap(),
        grant_epoch: "g2".into(),
        geometry_epoch: 3,
        permissions: InputPermissions {
            pointer: true,
            keyboard: true,
            text: true,
        },
    };
    let mut lease = InputLease::new();
    let mut effects = 0;
    for event in &events {
        effects += lease
            .apply(event, &context)
            .expect("authorized input")
            .len();
    }
    // absolute pointer + button down + key down + committed text, then two releases.
    assert_eq!(effects, 6);
    assert!(lease.is_empty());
}

#[test]
fn fixture_cases_document_truthful_rejection_outcomes() {
    let cases = fixture()["cases"].as_array().expect("cases array").clone();
    let ids: Vec<&str> = cases
        .iter()
        .filter_map(|case| case["id"].as_str())
        .collect();
    for expected in [
        "allowed_action",
        "denied_action",
        "stale_target",
        "unsupported_action",
        "unavailable_action",
        "malformed_parameters",
        "view_only_input",
        "disconnect_after_close",
    ] {
        assert!(ids.contains(&expected), "missing case {expected}");
    }
    let codes: Vec<&str> = cases
        .iter()
        .filter_map(|case| case["expect"]["error_code"].as_str())
        .collect();
    for expected in [
        "permission_denied",
        "stale_context",
        "unsupported_action",
        "backend_unavailable",
        "invalid_parameters",
        "permission_required",
    ] {
        assert!(codes.contains(&expected), "missing error code {expected}");
    }
}

#[test]
fn malformed_snapshot_payloads_are_rejected_before_use() {
    let mut payload = fixture();
    payload["capability_snapshot"]["v"] = Value::from(2);
    let snapshot: CapabilitySnapshot =
        serde_json::from_value(payload["capability_snapshot"].clone()).expect("decodes");
    assert!(snapshot.validate().is_err());

    let mut payload = fixture();
    payload["capability_snapshot"]["actions"][1]["id"] =
        payload["capability_snapshot"]["actions"][0]["id"].clone();
    let snapshot: CapabilitySnapshot =
        serde_json::from_value(payload["capability_snapshot"].clone()).expect("decodes");
    assert!(snapshot.validate().is_err());
}
