use std::env;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use vista_host::{Niri, NiriError, capability, portal, protocol, watch_windows};

#[derive(serde::Serialize)]
struct HostCapabilities {
    niri: vista_host::DoctorReport,
    screen_cast: portal::ScreenCastReport,
}

fn usage() {
    eprintln!("Usage: vista-host <doctor|capabilities|windows|window ID|watch> [--json]");
    eprintln!("       vista-host capture-consent --confirm [--json]");
    eprintln!("       vista-host capture-frame --confirm [--json]");
    eprintln!("       vista-host capture-burst --confirm [--json]");
    eprintln!("       vista-host capture-monitor --confirm [--json]");
    eprintln!("       vista-host encoders [--json]");
    eprintln!("       vista-host codec-test [--json]");
    eprintln!("       vista-host bridge-test [--json]");
    eprintln!("       vista-host sync-test [--json]");
    eprintln!("       vista-host capture-h264 --confirm [--json]");
    eprintln!("       vista-host capability-test [--json]");
    eprintln!("       vista-host protocol-test [--json]");
    eprintln!("       vista-host protocol-loopback [--json]");
    eprintln!(
        "       vista-host protocol-serve --confirm [--listen ADDR] [--secret-hex HEX] [--allow-remote] [--json]"
    );
    eprintln!("       vista-host protocol-client --connect ADDR --secret-hex HEX [--json]");
}

fn run() -> Result<(), NiriError> {
    let args: Vec<_> = env::args().skip(1).collect();
    let json = args.iter().any(|arg| arg == "--json");
    let positional: Vec<_> = args.iter().filter(|arg| arg.as_str() != "--json").collect();
    let niri = Niri::default();

    match positional.as_slice() {
        [command] if command.as_str() == "doctor" => {
            let report = niri.doctor();
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Niri binary: {}", report.niri_binary);
                println!(
                    "Niri socket: {}",
                    report.niri_socket.as_deref().unwrap_or("unavailable")
                );
                println!("Compositor responding: {}", report.compositor_responding);
                if let Some(error) = &report.error {
                    eprintln!("Details: {error}");
                }
            }
            if !report.compositor_responding {
                return Err(NiriError::Unavailable(
                    "Niri compositor is not responding".into(),
                ));
            }
        }
        [command] if command.as_str() == "capabilities" => {
            let report = HostCapabilities {
                niri: niri.doctor(),
                screen_cast: portal::probe(),
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Niri responding: {}", report.niri.compositor_responding);
                println!(
                    "ScreenCast portal responding: {}",
                    report.screen_cast.portal_responding
                );
                if let Some(version) = report.screen_cast.interface_version {
                    println!("ScreenCast interface version: {version}");
                }
                if let Some(source) = &report.screen_cast.source_types {
                    println!(
                        "Portal advertised capture sources: monitor={} window={} virtual_monitor={} (mask={})",
                        source.monitor, source.window, source.virtual_monitor, source.raw
                    );
                }
                if let Some(cursor) = &report.screen_cast.cursor_modes {
                    println!(
                        "Portal advertised cursor modes: hidden={} embedded={} metadata={} (mask={})",
                        cursor.hidden, cursor.embedded, cursor.metadata, cursor.raw
                    );
                }
                for error in &report.screen_cast.errors {
                    eprintln!("ScreenCast probe: {error}");
                }
                if let Some(error) = &report.niri.error {
                    eprintln!("Niri probe: {error}");
                }
                println!(
                    "Niri per-window capture: NOT VERIFIED (advertised portal sources do not prove it works)"
                );
            }
        }
        [command, confirm]
            if command.as_str() == "capture-consent" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will ask the desktop portal to select one window. No frames are read or transmitted; the portal session is not persisted."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::capture::consent_probe())
                .map_err(|err| {
                    NiriError::Unavailable(format!(
                        "ScreenCast consent probe failed or was cancelled: {err}"
                    ))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Portal session negotiated {} stream(s)",
                    report.streams.len()
                );
                for stream in report.streams {
                    println!(
                        "PipeWire node {} (size: {:?}, position: {:?})",
                        stream.pipewire_node_id, stream.size, stream.position
                    );
                }
                println!("No pixels captured; arbitrary Niri window capture remains unverified.");
            }
        }
        [command, confirm]
            if command.as_str() == "capture-frame" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will request permission to read exactly one selected window frame. Pixels remain in memory and are not saved or transmitted; the session is not persisted."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::capture::first_frame_probe())
                .map_err(|err| {
                    NiriError::Unavailable(format!(
                        "ScreenCast frame probe failed or was cancelled: {err}"
                    ))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if let Some(frame) = report.frame {
                println!(
                    "Received PipeWire video frame from node {}: {:?}x{:?}, {} bytes, {} ms; no pixels saved or transmitted.",
                    frame.pipewire_node_id,
                    frame.width,
                    frame.height,
                    frame.bytes,
                    frame.elapsed_ms
                );
                println!("Arbitrary Niri window-ID capture remains unverified.");
            }
        }
        [command, confirm]
            if command.as_str() == "capture-burst" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will request one window and inspect up to eight transient frames. No pixels are saved or transmitted; permissions are not persisted."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::capture::frame_burst_probe())
                .map_err(|err| {
                    NiriError::Unavailable(format!(
                        "ScreenCast frame burst failed or was cancelled: {err}"
                    ))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Received {} transient window frames in {} ms ({} resize event(s)); no pixels saved or transmitted.",
                    report.frames.len(),
                    report.elapsed_ms,
                    report.resize_events
                );
                println!("Arbitrary Niri window-ID capture remains unverified.");
            }
        }
        [command, confirm]
            if command.as_str() == "capture-monitor" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will request one window and monitor transient frames for at most 3 seconds or 120 frames. No pixels are saved or transmitted; permissions are not persisted."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::capture::capture_monitor_probe())
                .map_err(|err| {
                    NiriError::Unavailable(format!(
                        "ScreenCast monitor failed or was cancelled: {err}"
                    ))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Observed {} transient window frames in {} ms ({} resize events; termination: {:?}); no pixels saved or transmitted.",
                    report.frames, report.elapsed_ms, report.resize_events, report.termination
                );
                println!("Arbitrary Niri window-ID capture remains unverified.");
            }
        }
        [command] if command.as_str() == "encoders" => {
            let report = vista_host::media::probe_encoders().map_err(NiriError::Unavailable)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "H.264 encoder factories: NVENC={} x264={} OpenH264={}",
                    report.nvidia, report.x264, report.openh264
                );
                println!(
                    "H.264 parser={} decoder factory={}",
                    report.h264_parser, report.decoder_available
                );
                println!(
                    "Preferred encoder: {:?} (availability only; encoding not verified)",
                    report.selected
                );
            }
        }
        [command] if command.as_str() == "codec-test" => {
            eprintln!(
                "Running a bounded synthetic H.264 encode/decode test; no screen capture or network access."
            );
            let report =
                vista_host::media::synthetic_h264_round_trip().map_err(NiriError::Unavailable)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "H.264 round-trip verified with {:?} -> {}: {} compressed bytes, {} decoded bytes at {}x{} in {} ms.",
                    report.encoder,
                    report.decoder,
                    report.encoded_bytes,
                    report.decoded_bytes,
                    report.width,
                    report.height,
                    report.elapsed_ms
                );
            }
        }
        [command] if command.as_str() == "bridge-test" => {
            eprintln!(
                "Running bounded synthetic RGBA -> session -> H.264 encoding; no desktop capture or network access."
            );
            let report =
                vista_host::bridge::synthetic_frame_bridge().map_err(NiriError::Unavailable)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Synthetic bridge verified: {:?}, {} input frames, {} encoded buffers, {} compressed bytes, {} dropped frames in {} ms.",
                    report.encoder,
                    report.input_frames,
                    report.encoded_buffers,
                    report.encoded_bytes,
                    report.dropped_frames,
                    report.elapsed_ms
                );
            }
        }
        [command] if command.as_str() == "sync-test" => {
            eprintln!(
                "Running bounded synthetic RGBA -> session -> H.264 -> sync gate -> decode with one injected loss; no desktop capture or network access."
            );
            let report =
                vista_host::sync::synthetic_sync_recovery().map_err(NiriError::Unavailable)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Sync recovery verified: {:?} -> {}, {} encoded buffers, {} decoded frames ({} with PTS), {} discontinuities, {} suppressed units, {} AEAD records / {} bytes, {} TCP media records / {} bytes, in {} ms.",
                    report.encoder,
                    report.decoder,
                    report.encoded_buffers,
                    report.decoded_frames,
                    report.decoded_with_pts,
                    report.discontinuities,
                    report.discarded_access_units,
                    report.aead_records,
                    report.aead_bytes,
                    report.tcp_records,
                    report.tcp_bytes,
                    report.elapsed_ms
                );
                println!(
                    "In-memory traversal latency: min {:.2} ms, median {:.2} ms, max {:.2} ms over {} samples (not capture or network latency).",
                    report.latency.min_ms,
                    report.latency.median_ms,
                    report.latency.max_ms,
                    report.latency.samples
                );
            }
        }
        [command, confirm]
            if command.as_str() == "capture-h264" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will request one window and briefly encode its frames in memory. No video is saved or transmitted; permissions are not persisted."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::live::capture_h264_probe())
                .map_err(|err| {
                    NiriError::Unavailable(format!(
                        "ScreenCast H.264 diagnostic failed or was cancelled: {err}"
                    ))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Encoded {} selected-window frames into {} H.264 buffers ({} bytes) in {} ms with {:?}; no pixels were saved or transmitted.",
                    report.input_frames,
                    report.encoded_buffers,
                    report.encoded_bytes,
                    report.elapsed_ms,
                    report.encoder
                );
                println!("Arbitrary Niri window-ID capture remains unverified.");
            }
        }
        [command] if command.as_str() == "watch" => watch_windows(json)?,
        [command] if command.as_str() == "windows" => {
            let windows = niri.windows()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&windows)?);
            } else {
                for window in windows {
                    println!(
                        "{:>6}  {:<24}  {}{}",
                        window.id,
                        window.app_id.as_deref().unwrap_or("(unknown)"),
                        window.title.as_deref().unwrap_or("(untitled)"),
                        if window.is_focused { " [focused]" } else { "" },
                    );
                }
            }
        }
        [command, id] if command.as_str() == "window" => {
            let id: u64 = id.parse().map_err(|_| {
                NiriError::InvalidArgument("window ID must be a non-negative integer".into())
            })?;
            let window = niri.window(id)?.ok_or_else(|| {
                NiriError::InvalidArgument(format!("window ID {id} was not found"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&window)?);
            } else {
                println!("ID: {}", window.id);
                println!("App: {}", window.app_id.as_deref().unwrap_or("(unknown)"));
                println!("Title: {}", window.title.as_deref().unwrap_or("(untitled)"));
                println!(
                    "PID: {}",
                    window
                        .pid
                        .map_or_else(|| "unknown".into(), |pid| pid.to_string())
                );
                println!(
                    "Workspace: {}",
                    window
                        .workspace_id
                        .map_or_else(|| "unknown".into(), |id| id.to_string())
                );
            }
        }
        [command] if command.as_str() == "protocol-loopback" => {
            eprintln!(
                "Running a localhost-only AEAD control-channel loopback; no external listener is opened."
            );
            let report = vista_host::transport::protocol_loopback().map_err(|err| {
                NiriError::Unavailable(format!("transport loopback failed: {err}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Encrypted loopback verified={}: {} handshake frames, {} server records, {} client writes/{} reads, keyframe requests={} at {}.",
                    report.verified,
                    report.handshake_frames,
                    report.server_encrypted_records,
                    report.client_encrypted_writes,
                    report.client_encrypted_reads,
                    report.keyframe_requests,
                    report.address
                );
                println!(
                    "Protocol {}; ChaCha20-Poly1305 records with implicit nonce counters and session/direction AAD; no external listener.",
                    report.protocol
                );
            }
        }
        [command] if command.as_str() == "protocol-test" => {
            eprintln!(
                "Running the in-memory protocol pairing/session diagnostic; no network, media or input access."
            );
            let report = vista_host::protocol::diagnostic().map_err(|error| {
                NiriError::Unavailable(format!("protocol diagnostic failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Protocol {} handshake verified: host={}, client={}, {} control frames / {} bytes, keys match={}, permissions video={} input={} clipboard={}.",
                    report.protocol,
                    report.host_state,
                    report.client_state,
                    report.client_frames + report.host_frames,
                    report.wire_bytes,
                    report.keys_match,
                    report.permissions.video,
                    report.permissions.input,
                    report.permissions.clipboard
                );
                println!(
                    "Negative checks: wrong secret={}, replay={}, expired offer={}, permission escalation={}, version mismatch={}.",
                    report.wrong_secret_rejected,
                    report.replayed_proof_rejected,
                    report.expired_offer_rejected,
                    report.permission_escalation_rejected,
                    report.version_mismatch_rejected
                );
            }
        }
        [command, rest @ ..] if command.as_str() == "protocol-serve" => {
            let rest: Vec<&str> = rest.iter().map(|arg| arg.as_str()).collect();
            protocol_serve(&rest, json)?;
        }
        [command, rest @ ..] if command.as_str() == "protocol-client" => {
            let rest: Vec<&str> = rest.iter().map(|arg| arg.as_str()).collect();
            protocol_client(&rest, json)?;
        }
        [command] if command.as_str() == "capability-test" => {
            eprintln!(
                "Running the backend-agnostic capability/action contract diagnostic; no network, capture or input injection."
            );
            let report = capability::diagnostic().map_err(|error| {
                NiriError::Unavailable(format!("capability contract diagnostic failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Capability contract v{} ({} backend, revision {}): {} actions.",
                    report.contract_version,
                    report.backend,
                    report.capability_revision,
                    report.action_count
                );
                println!(
                    "Allowed={}, denied={}, unsupported={}, unavailable={}, stale={}, malformed={}, duplicate-cached={}.",
                    report.allowed_completed,
                    report.denied_rejected,
                    report.unsupported_rejected,
                    report.unavailable_rejected,
                    report.stale_rejected,
                    report.malformed_rejected,
                    report.duplicate_cached
                );
                println!(
                    "Capability update without reconnect={}, reconnect clears history={}, input permission enforced={}, release-all clears held state={}, verified={}.",
                    report.update_without_reconnect,
                    report.reconnect_clears_history,
                    report.input_denied_without_permission,
                    report.release_all_clears_held_keys,
                    report.verified
                );
            }
        }
        _ => {
            usage();
            return Err(NiriError::InvalidArgument("unknown command".into()));
        }
    }
    Ok(())
}

fn option<'a>(args: &'a [&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| *arg == name)
        .and_then(|index| args.get(index + 1))
        .copied()
}

fn check_known_arguments(
    args: &[&str],
    value_flags: &[&str],
    boolean_flags: &[&str],
) -> Result<(), NiriError> {
    let mut index = 0;
    while index < args.len() {
        let argument = args[index];
        if boolean_flags.contains(&argument) {
            index += 1;
        } else if value_flags.contains(&argument) {
            if index + 1 >= args.len() {
                return Err(NiriError::InvalidArgument(format!(
                    "{argument} requires a value"
                )));
            }
            index += 2;
        } else {
            return Err(NiriError::InvalidArgument(format!(
                "unknown argument {argument}"
            )));
        }
    }
    Ok(())
}

/// One-shot control listener. It never captures or streams media, and it
/// refuses non-loopback addresses unless the caller explicitly opts in.
fn protocol_serve(args: &[&str], json: bool) -> Result<(), NiriError> {
    check_known_arguments(
        args,
        &["--listen", "--secret-hex"],
        &["--confirm", "--allow-remote"],
    )?;
    if !args.contains(&"--confirm") {
        usage();
        return Err(NiriError::InvalidArgument(
            "protocol-serve requires explicit --confirm".into(),
        ));
    }
    let listen = option(args, "--listen").unwrap_or("127.0.0.1:0");
    let address: SocketAddr = listen
        .parse()
        .map_err(|_| NiriError::InvalidArgument("--listen must be an IP:port address".into()))?;
    if !address.ip().is_loopback() && !args.contains(&"--allow-remote") {
        return Err(NiriError::InvalidArgument(
            "refusing a non-loopback control listener without --allow-remote".into(),
        ));
    }
    let secret = match option(args, "--secret-hex") {
        Some(value) => protocol::PairingSecret::from_hex(value).map_err(|error| {
            NiriError::InvalidArgument(format!("invalid --secret-hex: {error}"))
        })?,
        None => protocol::PairingSecret::generate().map_err(|error| {
            NiriError::Unavailable(format!("cannot generate pairing secret: {error}"))
        })?,
    };
    let (server, offer) = vista_host::transport::HostControlServer::bind(
        address,
        secret.clone(),
        protocol::unix_now(),
    )
    .map_err(|error| NiriError::Unavailable(format!("cannot bind control listener: {error}")))?;
    let invite = serde_json::json!({
        "protocol": "1.0",
        "address": offer.address.to_string(),
        "pairing_id": offer.pairing_id.clone(),
        "secret_hex": secret.to_hex(),
        "expires_at": offer.expires_at,
    });
    if json {
        println!("{}", serde_json::to_string(&invite)?);
        io::stdout().flush()?;
    } else {
        println!(
            "Vista control listener on {} (pairing id {}, offer expires at {}).",
            offer.address, offer.pairing_id, offer.expires_at
        );
        println!("Pairing secret (out of band): {}", secret.to_hex());
        io::stdout().flush()?;
    }
    let stats = server
        .accept_and_run()
        .map_err(|error| NiriError::Unavailable(format!("control session failed: {error}")))?;
    if json {
        println!("{}", serde_json::to_string(&stats)?);
    } else {
        println!(
            "Encrypted control session: {} handshake frames, {} encrypted records, {} keyframe request(s), pong sent={}.",
            stats.handshake_frames,
            stats.encrypted_records,
            stats.keyframe_requests,
            stats.pong_sent
        );
    }
    Ok(())
}

fn protocol_client(args: &[&str], json: bool) -> Result<(), NiriError> {
    check_known_arguments(args, &["--connect", "--secret-hex"], &[])?;
    let connect = option(args, "--connect")
        .ok_or_else(|| NiriError::InvalidArgument("protocol-client requires --connect".into()))?;
    let address: SocketAddr = connect
        .parse()
        .map_err(|_| NiriError::InvalidArgument("--connect must be an IP:port address".into()))?;
    let secret_hex = option(args, "--secret-hex").ok_or_else(|| {
        NiriError::InvalidArgument("protocol-client requires --secret-hex".into())
    })?;
    let secret = protocol::PairingSecret::from_hex(secret_hex)
        .map_err(|error| NiriError::InvalidArgument(format!("invalid --secret-hex: {error}")))?;
    let stats = vista_host::transport::connect(address, secret)
        .map_err(|error| NiriError::Unavailable(format!("control client failed: {error}")))?;
    let verified = stats.reached_active && stats.pong_verified;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "protocol": "1.0",
                "address": address.to_string(),
                "verified": verified,
                "stats": stats,
            }))?
        );
    } else {
        println!(
            "Control client connected to {}: active={}, pong verified={}, {} handshake frames, {} encrypted writes / {} reads.",
            address,
            stats.reached_active,
            stats.pong_verified,
            stats.handshake_frames,
            stats.encrypted_writes,
            stats.encrypted_reads
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vista-host: {error}");
            ExitCode::FAILURE
        }
    }
}
