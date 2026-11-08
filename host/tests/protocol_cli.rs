use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

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

#[test]
fn protocol_serve_requires_confirmation() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-serve", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--confirm"), "{stderr}");
}

#[test]
fn protocol_client_requires_connection_details() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["protocol-client", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--connect"), "{stderr}");
}

#[test]
fn protocol_serve_and_client_complete_encrypted_session() {
    let mut server = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args([
            "protocol-serve",
            "--confirm",
            "--listen",
            "127.0.0.1:0",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start control server");
    let stdout = server.stdout.take().expect("server stdout");
    let mut lines = BufReader::new(stdout).lines();
    let offer_line = lines
        .next()
        .expect("server offer line")
        .expect("server offer content");
    let offer: serde_json::Value = serde_json::from_str(&offer_line).expect("offer JSON");
    let address = offer["address"].as_str().expect("offer address");
    let secret = offer["secret_hex"].as_str().expect("offer secret");

    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args([
            "protocol-client",
            "--connect",
            address,
            "--secret-hex",
            secret,
            "--json",
        ])
        .output()
        .expect("run control client");
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("client JSON");
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["stats"]["pong_verified"], true, "{report}");
    assert_eq!(report["stats"]["reached_active"], true, "{report}");

    let status = server.wait().expect("wait for control server");
    if !status.success() {
        let mut text = String::new();
        if let Some(mut stderr) = server.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut text);
        }
        panic!("server failed: {text}");
    }
}
