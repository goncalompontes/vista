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

struct ControlServer {
    child: std::process::Child,
    lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
    address: String,
    secret: String,
    control: Option<serde_json::Value>,
}

fn start_control_server(extra: &[&str]) -> ControlServer {
    let mut args = vec![
        "protocol-serve",
        "--confirm",
        "--listen",
        "127.0.0.1:0",
        "--json",
    ];
    args.extend_from_slice(extra);
    let mut child = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start control server");
    let stdout = child.stdout.take().expect("server stdout");
    let mut lines = BufReader::new(stdout).lines();
    let offer_line = lines
        .next()
        .expect("server offer line")
        .expect("server offer content");
    let offer: serde_json::Value = serde_json::from_str(&offer_line).expect("offer JSON");
    ControlServer {
        address: offer["address"]
            .as_str()
            .expect("offer address")
            .to_string(),
        secret: offer["secret_hex"]
            .as_str()
            .expect("offer secret")
            .to_string(),
        control: offer
            .get("control")
            .filter(|value| !value.is_null())
            .cloned(),
        child,
        lines,
    }
}

fn run_client(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(args)
        .output()
        .expect("run control client");
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("client JSON")
}

fn finish_control_server(mut server: ControlServer) -> serde_json::Value {
    let stats_line = server
        .lines
        .next()
        .expect("server stats line")
        .expect("server stats content");
    let stats: serde_json::Value = serde_json::from_str(&stats_line).expect("server stats JSON");
    let status = server.child.wait().expect("wait for control server");
    if !status.success() {
        let mut text = String::new();
        if let Some(mut stderr) = server.child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut text);
        }
        panic!("server failed: {text}");
    }
    stats
}

#[test]
fn protocol_serve_and_client_complete_encrypted_session() {
    let server = start_control_server(&[]);
    assert!(
        server.control.is_none(),
        "default listener must be video-only"
    );

    let report = run_client(&[
        "protocol-client",
        "--connect",
        &server.address,
        "--secret-hex",
        &server.secret,
        "--json",
    ]);
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["stats"]["pong_verified"], true, "{report}");
    assert_eq!(report["stats"]["reached_active"], true, "{report}");

    let stats = finish_control_server(server);
    assert_eq!(stats["control_snapshot_sent"], false, "{stats}");
    assert_eq!(stats["action_results"], 0, "{stats}");
}

#[test]
fn protocol_serve_dispatches_a_granted_action_over_the_encrypted_channel() {
    let server = start_control_server(&[
        "--window",
        "5",
        "--allow-window-actions",
        "--allow-input",
        "--fake-actions",
    ]);
    let control = server.control.as_ref().expect("control policy in offer");
    assert_eq!(control["backend"], "fake", "{control}");
    assert_eq!(control["window_actions"], true, "{control}");
    assert_eq!(control["input"], true, "{control}");

    let report = run_client(&[
        "protocol-client",
        "--connect",
        &server.address,
        "--secret-hex",
        &server.secret,
        "--allow-input",
        "--action",
        "window.focus",
        "--json",
    ]);
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["stats"]["reached_active"], true, "{report}");
    assert_eq!(report["stats"]["snapshot_received"], true, "{report}");
    assert!(
        report["stats"]["snapshot_actions"].as_u64().unwrap_or(0) > 0,
        "{report}"
    );
    assert_eq!(report["stats"]["action_result_received"], true, "{report}");
    assert_eq!(report["stats"]["action_status"], "completed", "{report}");

    let stats = finish_control_server(server);
    assert_eq!(stats["control_snapshot_sent"], true, "{stats}");
    assert_eq!(stats["action_results"], 1, "{stats}");
    assert_eq!(stats["actions_completed"], 1, "{stats}");
}

#[test]
fn protocol_serve_rejects_an_action_without_the_grant() {
    let server = start_control_server(&["--window", "5", "--allow-input", "--fake-actions"]);
    let control = server.control.as_ref().expect("control policy in offer");
    assert_eq!(control["window_actions"], false, "{control}");

    let report = run_client(&[
        "protocol-client",
        "--connect",
        &server.address,
        "--secret-hex",
        &server.secret,
        "--allow-input",
        "--action",
        "window.focus",
        "--json",
    ]);
    assert_eq!(report["verified"], true, "{report}");
    assert_eq!(report["stats"]["action_status"], "rejected", "{report}");
    assert_eq!(
        report["stats"]["action_error"], "permission_required",
        "{report}"
    );

    let stats = finish_control_server(server);
    assert_eq!(stats["action_results"], 1, "{stats}");
    assert_eq!(stats["actions_completed"], 0, "{stats}");
}

#[test]
fn protocol_serve_requires_a_window_for_action_policy_flags() {
    for args in [
        vec!["protocol-serve", "--confirm", "--allow-window-actions"],
        vec![
            "protocol-serve",
            "--confirm",
            "--window",
            "5",
            "--capability-revision",
            "0",
            "--fake-actions",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
            .args(args)
            .output()
            .expect("launch host binary");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--window") || stderr.contains("non-zero"),
            "{stderr}"
        );
    }
}
