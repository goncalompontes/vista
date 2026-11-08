use std::process::Command;

#[test]
fn capability_test_reports_verified_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capability-test", "--json"])
        .output()
        .expect("launch host binary");
    assert!(output.status.success(), "{:?}", output);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["contract_version"], 1);
    assert_eq!(report["backend"], "fake");
    for field in [
        "allowed_completed",
        "denied_rejected",
        "unsupported_rejected",
        "unavailable_rejected",
        "stale_rejected",
        "malformed_rejected",
        "duplicate_cached",
        "update_without_reconnect",
        "reconnect_clears_history",
        "input_denied_without_permission",
        "release_all_clears_held_keys",
        "verified",
    ] {
        assert_eq!(report[field], serde_json::Value::Bool(true), "{field}");
    }
}

#[test]
fn capability_test_rejects_unexpected_arguments() {
    for args in [
        vec!["capability-test", "--confirm"],
        vec!["capability-test", "extra"],
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

#[test]
fn actions_requires_window_and_rejects_unknown_flags() {
    for args in [
        vec!["actions"],
        vec!["actions", "--window"],
        vec!["actions", "--window", "not-a-number"],
        vec!["actions", "--window", "1", "--bogus"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
            .args(args)
            .output()
            .expect("launch host binary");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("actions --window")
                || stderr.contains("--window")
                || stderr.contains("unknown argument"),
            "{stderr}"
        );
    }
}
