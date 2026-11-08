use std::process::Command;

#[test]
fn keyframe_control_test_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["keyframe-control-test", "--confirm"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"), "{stderr}");
}

#[test]
#[ignore = "requires H.264 GStreamer plugins and localhost networking"]
fn keyframe_control_test_reports_authenticated_restart() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["keyframe-control-test", "--json"])
        .output()
        .expect("launch host binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verified"], true, "{report}");
    assert!(report["server_keyframe_requests"].as_u64().unwrap() >= 1);
    assert!(report["external_keyframe_requests"].as_u64().unwrap() >= 1);
    assert!(report["encoder_restarts"].as_u64().unwrap() >= 1);
}
