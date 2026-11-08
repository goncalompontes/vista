use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[test]
fn media_stream_requires_a_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args(["media-stream", "--json"])
        .output()
        .expect("launch host binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("media-stream"), "{stderr}");
}

#[test]
#[ignore = "requires H.264 GStreamer plugins and localhost networking"]
fn media_stream_serves_and_decodes_across_processes() {
    let mut server = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args([
            "media-stream",
            "--serve",
            "--listen",
            "127.0.0.1:0",
            "--frames",
            "12",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start media server");
    let stdout = server.stdout.take().expect("server stdout");
    let mut lines = BufReader::new(stdout).lines();
    let offer_line = lines
        .next()
        .expect("media offer line")
        .expect("media offer content");
    let offer: serde_json::Value = serde_json::from_str(&offer_line).expect("offer JSON");
    let address = offer["address"].as_str().expect("offer address");
    let secret = offer["pairing_secret_hex"]
        .as_str()
        .expect("offer pairing secret");

    let output = Command::new(env!("CARGO_BIN_EXE_vista-host"))
        .args([
            "media-stream",
            "--connect",
            address,
            "--secret-hex",
            secret,
            "--json",
        ])
        .output()
        .expect("run media client");
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("client JSON");
    assert_eq!(report["verified"], true, "{report}");
    assert!(report["frames_received"].as_u64().unwrap() > 0);
    assert!(report["decoded_frames"].as_u64().unwrap() > 0);
    assert_eq!(report["keyframe_requests_sent"], 1, "{report}");

    let status = server.wait().expect("wait for media server");
    assert!(status.success(), "server failed");
}
