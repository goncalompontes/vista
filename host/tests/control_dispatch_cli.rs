use std::process::Command;

#[test]
fn control_dispatch_test_reports_verified_session_bound_dispatch() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["control-dispatch-test", "--json"])
        .output()
        .expect("launch host binary");
    assert!(output.status.success(), "{:?}", output);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON report");
    for field in [
        "protocol_session",
        "snapshot_received",
        "action_completed",
        "duplicate_cached",
        "stale_grant_rejected",
        "revoked_rejected",
        "wrong_session_rejected",
        "wrong_target_rejected",
        "malformed_rejected",
        "verified",
    ] {
        assert_eq!(report[field], serde_json::Value::Bool(true), "{field}");
    }
    assert_eq!(report["backend_calls"], 1);
    assert!(
        report["snapshot_actions"].as_u64().unwrap_or(0) > 0,
        "snapshot should advertise at least one action"
    );
}

#[test]
fn control_dispatch_test_rejects_unexpected_arguments() {
    for args in [
        vec!["control-dispatch-test", "--confirm"],
        vec!["control-dispatch-test", "extra"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
            .args(args)
            .output()
            .expect("launch host binary");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("unknown command"), "{stderr}");
    }
}
