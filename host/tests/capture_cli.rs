use std::process::Command;

#[test]
fn monitor_command_requires_confirmation_and_rejects_extra_flags() {
    for args in [
        vec!["capture-monitor", "--json"],
        vec!["capture-monitor", "--confirm", "--unexpected"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
            .args(args)
            .output()
            .expect("launch host binary");
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("capture-monitor --confirm"));
        assert!(stderr.contains("unknown command"));
    }
}

/// Never show the portal chooser or ask for screen access without opt-in.
#[test]
fn capture_command_rejects_missing_confirmation() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-consent", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostics");
    assert!(stderr.contains("capture-consent --confirm"));
    assert!(stderr.contains("unknown command"));
}

#[test]
fn frame_command_rejects_missing_confirmation() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-frame", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostics");
    assert!(stderr.contains("capture-frame --confirm"));
    assert!(stderr.contains("unknown command"));
}

#[test]
fn frame_command_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-frame", "--confirm", "--unexpected"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
}

#[test]
fn burst_command_rejects_missing_confirmation() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-burst", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostics");
    assert!(stderr.contains("capture-burst --confirm"));
    assert!(stderr.contains("unknown command"));
}

#[test]
fn burst_command_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-burst", "--confirm", "--unexpected"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
}

#[test]
fn capture_command_rejects_unexpected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["capture-consent", "--confirm", "--unexpected"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
}
