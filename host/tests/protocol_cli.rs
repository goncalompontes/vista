use std::process::Command;

#[test]
fn protocol_test_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-test", "--confirm"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"), "{stderr}");
}

#[test]
fn protocol_test_reports_verified_in_memory_handshake() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-test", "--json"])
        .output()
        .expect("launch host binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verified"], true);
    assert_eq!(report["keys_match"], true);
    assert_eq!(report["permissions"]["video"], true);
    assert_eq!(report["permissions"]["input"], false);
    assert_eq!(report["permissions"]["clipboard"], false);
    assert_eq!(report["wrong_secret_rejected"], true);
    assert_eq!(report["permission_escalation_rejected"], true);
}

#[test]
fn protocol_loopback_reports_verified_encrypted_session() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-loopback", "--json"])
        .output()
        .expect("launch host binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["pong_verified"], true, "{report}");
    assert_eq!(report["keyframe_requests"], 1, "{report}");
    assert_eq!(report["handshake_frames"], 2, "{report}");
}

#[test]
fn protocol_loopback_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-loopback", "--confirm"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"), "{stderr}");
}
