use std::process::Command;

#[test]
fn pipewire_loopback_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["pipewire-loopback", "--confirm"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"), "{stderr}");
}

#[test]
#[ignore = "requires PipeWire and the GStreamer pipewiresrc/pipewiresink elements"]
fn pipewire_loopback_reports_verified_local_frames() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["pipewire-loopback", "--json"])
        .output()
        .expect("launch host binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["portal_consent"], false, "{report}");
    assert_eq!(report["frames"], 5, "{report}");
    assert_eq!(report["width"], 320, "{report}");
    assert_eq!(report["height"], 180, "{report}");
}
