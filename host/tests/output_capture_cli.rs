use std::process::Command;

#[test]
fn capture_output_rejects_missing_confirmation() {
    for args in [
        vec!["capture-output"],
        vec!["capture-output", "--json"],
        vec!["capture-output", "--confirm", "--unexpected"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
            .args(args)
            .output()
            .expect("launch host binary");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("capture-output --confirm"), "{stderr}");
        assert!(stderr.contains("unknown command"), "{stderr}");
    }
}

#[test]
#[ignore = "requires grim, a live Wayland output and H.264 codec plugins"]
fn capture_output_reports_real_compositor_pixels() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-output", "--confirm", "--json"])
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
    assert!(report["encoded_bytes"].as_u64().unwrap() > 0, "{report}");
}
