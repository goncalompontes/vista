use std::process::Command;

#[test]
fn lifecycle_test_reports_verified_guard() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["lifecycle-test", "--json"])
        .output()
        .expect("launch host binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    for field in [
        "removal_terminates",
        "snapshot_missing_terminates",
        "portal_close_terminates",
        "permission_revoke_terminates",
        "compositor_restart_terminates",
        "resize_flushes",
        "resize_signals_geometry_discontinuity",
        "terminated_session_is_closed",
        "idempotent_termination",
        "verified",
    ] {
        assert_eq!(report[field], serde_json::Value::Bool(true), "{field}");
    }
}

#[test]
fn lifecycle_test_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["lifecycle-test", "--confirm"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"), "{stderr}");
}

#[test]
fn capture_h264_window_flag_requires_an_integer_without_prompting() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-h264", "--confirm", "--window", "not-a-number"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--window must be an integer"), "{stderr}");
}
