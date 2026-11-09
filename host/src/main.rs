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
    eprintln!(
        "       vista-host actions --window ID [--allow-window-actions] [--allow-desktop-actions] [--allow-clipboard] [--json]"
    );
    eprintln!("       vista-host capture-consent --confirm [--json]");
    eprintln!("       vista-host capture-frame --confirm [--json]");
    eprintln!("       vista-host capture-burst --confirm [--json]");
    eprintln!("       vista-host capture-monitor --confirm [--json]");
    eprintln!("       vista-host encoders [--json]");
    eprintln!("       vista-host codec-test [--json]");
    eprintln!("       vista-host bridge-test [--json]");
    eprintln!("       vista-host sync-test [--json]");
    eprintln!(
        "       vista-host media-stream --serve [--listen ADDR] [--frames N] | --connect ADDR --key-hex HEX --session-id ID [--json]"
    );
    eprintln!("       vista-host keyframe-control-test [--json]");
    eprintln!("       vista-host capture-h264 --confirm [--window ID] [--json]");
    eprintln!("       vista-host capture-output --confirm [--json]");
    eprintln!("       vista-host pipewire-loopback [--json]");
    eprintln!("       vista-host capability-test [--json]");
    eprintln!("       vista-host lifecycle-test [--json]");
    eprintln!("       vista-host control-dispatch-test [--json]");
    eprintln!("       vista-host protocol-test [--json]");
    eprintln!("       vista-host protocol-loopback [--json]");
    eprintln!(
        "       vista-host protocol-serve --confirm [--listen ADDR] [--secret-hex HEX] [--window ID --allow-window-actions|--allow-desktop-actions|--allow-clipboard|--allow-input] [--fake-actions] [--capability-revision N] [--allow-remote] [--json]"
    );
    eprintln!("       vista-host keyframe-control-test [--json]");
    eprintln!("       vista-host media-stream --serve [--listen ADDR] [--frames N] [--json]");
    eprintln!("       vista-host media-stream --connect ADDR --secret-hex HEX [--json]");
    eprintln!(
        "       vista-host protocol-client --connect ADDR --secret-hex HEX [--allow-input] [--action ID] [--json]"
    );
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
                    "Sync recovery verified: {:?} -> {}, {} encoded buffers, {} decoded frames ({} with PTS), {} discontinuities, {} suppressed units, {} keyframe request(s), {} encoder restart(s), {} AEAD records / {} bytes, {} TCP media records / {} bytes, in {} ms.",
                    report.encoder,
                    report.decoder,
                    report.encoded_buffers,
                    report.decoded_frames,
                    report.decoded_with_pts,
                    report.discontinuities,
                    report.discarded_access_units,
                    report.keyframe_requests,
                    report.encoder_restarts,
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
        [command, confirm, flag, id]
            if command.as_str() == "capture-h264"
                && confirm.as_str() == "--confirm"
                && flag.as_str() == "--window" =>
        {
            let window_id = id
                .parse::<u64>()
                .map_err(|_| NiriError::InvalidArgument("--window must be an integer".into()))?;
            eprintln!(
                "Vista will request one window and briefly encode its frames in memory. No video is saved or transmitted; permissions are not persisted."
            );
            eprintln!(
                "Watching Niri window {window_id}; removal or a compositor restart terminates capture."
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let report = runtime
                .block_on(vista_host::live::capture_h264_probe_for_window(Some(
                    window_id,
                )))
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
                println!(
                    "Window {window_id} was watched for removal; no pixels were saved or transmitted."
                );
            }
        }
        [command] if command.as_str() == "pipewire-loopback" => {
            eprintln!(
                "Running a local synthetic PipeWire video-node loopback; no portal consent, window capture or network access."
            );
            let report = vista_host::pipewire_loopback::pipewire_loopback().map_err(|error| {
                NiriError::Unavailable(format!("PipeWire loopback failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "PipeWire loopback verified: {} RGBA frames at {}x{} ({} bytes) in {} ms from synthetic node {}; portal consent={}.",
                    report.frames,
                    report.width,
                    report.height,
                    report.bytes,
                    report.elapsed_ms,
                    report.node_name,
                    report.portal_consent
                );
            }
        }
        [command, confirm]
            if command.as_str() == "capture-output" && confirm.as_str() == "--confirm" =>
        {
            eprintln!(
                "Vista will capture the entire active output in memory via grim, encode one frame and decode it. Pixels are not saved or transmitted. This is a wlr-screencopy fallback, not portal per-window capture."
            );
            let report = vista_host::output_capture::output_capture_probe().map_err(|error| {
                NiriError::Unavailable(format!("output capture diagnostic failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Output capture verified: {}x{} RGB->{} RGBA bytes, {:?} -> {}, {} encoded / {} decoded bytes in {} ms (capture {} ms); portal consent={}.",
                    report.width,
                    report.height,
                    report.rgba_bytes,
                    report.encoder,
                    report.decoder,
                    report.encoded_bytes,
                    report.decoded_bytes,
                    report.encode_decode_ms,
                    report.capture_ms,
                    report.portal_consent
                );
            }
        }
        [command] if command.as_str() == "keyframe-control-test" => {
            eprintln!(
                "Running an authenticated localhost control request -> encoder restart test; no capture, media transport or external network."
            );
            let report =
                vista_host::keyframe_control::keyframe_control_probe().map_err(|error| {
                    NiriError::Unavailable(format!("keyframe control test failed: {error}"))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Authenticated keyframe request verified={}: server requests={}, producer requests={}, encoder restarts={}, decoded frames={}.",
                    report.verified,
                    report.server_keyframe_requests,
                    report.external_keyframe_requests,
                    report.encoder_restarts,
                    report.decoded_frames
                );
            }
        }
        [command, rest @ ..] if command.as_str() == "media-stream" => {
            let rest: Vec<&str> = rest.iter().map(|arg| arg.as_str()).collect();
            media_stream_command(&rest, json)?;
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
        [command, rest @ ..] if command.as_str() == "actions" => {
            let rest: Vec<&str> = rest.iter().map(|arg| arg.as_str()).collect();
            actions_command(&rest, json, &niri)?;
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
            protocol_serve(&rest, json, &niri)?;
        }
        [command, rest @ ..] if command.as_str() == "protocol-client" => {
            let rest: Vec<&str> = rest.iter().map(|arg| arg.as_str()).collect();
            protocol_client(&rest, json)?;
        }
        [command] if command.as_str() == "control-dispatch-test" => {
            eprintln!(
                "Running the session-bound capability/action dispatch diagnostic through an in-memory pairing; no network, capture or input injection."
            );
            let report = vista_host::control_dispatch::diagnostic().map_err(|error| {
                NiriError::Unavailable(format!("control dispatch diagnostic failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Control dispatch verified={}: protocol session={}, snapshot actions={} (grant {}), completed={}, backend calls={}, duplicate cached={}.",
                    report.verified,
                    report.protocol_session,
                    report.snapshot_actions,
                    report.snapshot_grant_epoch,
                    report.action_completed,
                    report.backend_calls,
                    report.duplicate_cached
                );
                println!(
                    "Rejected: revoked={}, stale grant={}, wrong session={}, wrong target={}, malformed={}.",
                    report.revoked_rejected,
                    report.stale_grant_rejected,
                    report.wrong_session_rejected,
                    report.wrong_target_rejected,
                    report.malformed_rejected
                );
            }
        }
        [command] if command.as_str() == "lifecycle-test" => {
            eprintln!(
                "Running the deterministic window lifecycle guard diagnostic; no compositor, capture or input access."
            );
            let report = vista_host::lifecycle::diagnostic().map_err(|error| {
                NiriError::Unavailable(format!("lifecycle guard diagnostic failed: {error}"))
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Lifecycle guard verified={} for window {}: removal={}, missing snapshot={}, portal close={}, permission revoke={}, compositor restart={}.",
                    report.verified,
                    report.window_id,
                    report.removal_terminates,
                    report.snapshot_missing_terminates,
                    report.portal_close_terminates,
                    report.permission_revoke_terminates,
                    report.compositor_restart_terminates
                );
                println!(
                    "Resize flushes={}, geometry discontinuity={}, closed session={}, idempotent termination={}.",
                    report.resize_flushes,
                    report.resize_signals_geometry_discontinuity,
                    report.terminated_session_is_closed,
                    report.idempotent_termination
                );
            }
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

fn actions_command(args: &[&str], json: bool, niri: &Niri) -> Result<(), NiriError> {
    check_known_arguments(
        args,
        &["--window"],
        &[
            "--allow-window-actions",
            "--allow-desktop-actions",
            "--allow-clipboard",
        ],
    )?;
    let id = option(args, "--window")
        .ok_or_else(|| NiriError::InvalidArgument("actions requires --window ID".into()))?;
    let id: u64 = id.parse().map_err(|_| {
        NiriError::InvalidArgument("--window must be a non-negative integer".into())
    })?;
    let window = niri
        .window(id)?
        .ok_or_else(|| NiriError::InvalidArgument(format!("window ID {id} was not found")))?;
    let grants = capability::ActionGrants {
        window_actions: args.contains(&"--allow-window-actions"),
        desktop_actions: args.contains(&"--allow-desktop-actions"),
        clipboard: args.contains(&"--allow-clipboard"),
    };
    let snapshot = capability::niri_snapshot(id, grants, 1).map_err(|error| {
        NiriError::Unavailable(format!("cannot build capability snapshot: {error}"))
    })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!(
            "Host actions for window {} ({}).",
            id,
            window.title.as_deref().unwrap_or("(untitled)")
        );
        println!(
            "Target generation {}; capability revision {}.",
            snapshot.target_generation, snapshot.capability_revision
        );
        for action in &snapshot.actions {
            let state = if !action.supported {
                "unsupported"
            } else if !action.enabled {
                "unavailable"
            } else {
                match action.authorization {
                    capability::Authorization::Granted => "allowed",
                    capability::Authorization::Required => "permission-required",
                    capability::Authorization::Denied => "denied",
                    capability::Authorization::Revoked => "revoked",
                }
            };
            println!(
                "  {:<28} {:<20} {:<19} {}",
                action.id,
                action.presentation.label,
                state,
                action.reason.as_deref().unwrap_or("")
            );
        }
        println!("Discovery only; no native action was executed.");
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
fn protocol_serve(args: &[&str], json: bool, niri: &Niri) -> Result<(), NiriError> {
    check_known_arguments(
        args,
        &[
            "--listen",
            "--secret-hex",
            "--window",
            "--capability-revision",
        ],
        &[
            "--confirm",
            "--allow-remote",
            "--allow-window-actions",
            "--allow-desktop-actions",
            "--allow-clipboard",
            "--allow-input",
            "--fake-actions",
        ],
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

    let grants = capability::ActionGrants {
        window_actions: args.contains(&"--allow-window-actions"),
        desktop_actions: args.contains(&"--allow-desktop-actions"),
        clipboard: args.contains(&"--allow-clipboard"),
    };
    let allow_input = args.contains(&"--allow-input");
    let window = match option(args, "--window") {
        Some(value) => Some(
            value
                .parse::<u64>()
                .map_err(|_| NiriError::InvalidArgument("--window must be an integer".into()))?,
        ),
        None => None,
    };
    if window.is_none()
        && (grants.window_actions || grants.desktop_actions || grants.clipboard || allow_input)
    {
        return Err(NiriError::InvalidArgument(
            "--allow-window-actions/--allow-desktop-actions/--allow-clipboard/--allow-input require --window"
                .into(),
        ));
    }
    let fake_actions = args.contains(&"--fake-actions");
    if fake_actions && window.is_none() {
        return Err(NiriError::InvalidArgument(
            "--fake-actions requires --window".into(),
        ));
    }
    let capability_revision = match option(args, "--capability-revision") {
        Some(value) => value.parse::<u64>().map_err(|_| {
            NiriError::InvalidArgument("--capability-revision must be a positive integer".into())
        })?,
        None => 1,
    };
    if capability_revision == 0 {
        return Err(NiriError::InvalidArgument(
            "--capability-revision must be non-zero".into(),
        ));
    }
    let policy = protocol::PermissionSet {
        video: true,
        input: allow_input,
        clipboard: grants.clipboard,
        audio: false,
    };
    let control = if let Some(id) = window {
        let target = if fake_actions {
            capability::TargetRef::new(format!("w{id}"), format!("fake-session#w{id}")).map_err(
                |error| NiriError::InvalidArgument(format!("invalid control target: {error}")),
            )?
        } else {
            niri.window(id)?.ok_or_else(|| {
                NiriError::InvalidArgument(format!("window ID {id} was not found"))
            })?;
            capability::TargetRef::new(format!("w{id}"), capability::niri_generation(id)).map_err(
                |error| NiriError::InvalidArgument(format!("invalid control target: {error}")),
            )?
        };
        let backend: Box<dyn capability::ActionBackend + Send> = if fake_actions {
            Box::new(capability::FakeActionBackend::default())
        } else {
            Box::new(capability::NiriActionBackend::default())
        };
        Some(vista_host::transport::ControlSetup::new(
            target,
            grants,
            capability::InputPermissions {
                pointer: allow_input,
                keyboard: allow_input,
                text: allow_input,
            },
            capability_revision,
            backend,
        ))
    } else {
        None
    };
    let control_info = control.as_ref().map(|setup| {
        serde_json::json!({
            "target_id": setup.target.target_id,
            "target_generation": setup.target.target_generation,
            "capability_revision": setup.capability_revision,
            "window_actions": setup.grants.window_actions,
            "desktop_actions": setup.grants.desktop_actions,
            "clipboard": setup.grants.clipboard,
            "input": allow_input,
            "backend": if fake_actions { "fake" } else { "niri" },
        })
    });

    let secret = match option(args, "--secret-hex") {
        Some(value) => protocol::PairingSecret::from_hex(value).map_err(|error| {
            NiriError::InvalidArgument(format!("invalid --secret-hex: {error}"))
        })?,
        None => protocol::PairingSecret::generate().map_err(|error| {
            NiriError::Unavailable(format!("cannot generate pairing secret: {error}"))
        })?,
    };
    let (server, offer) = vista_host::transport::HostControlServer::bind_with_control(
        address,
        secret.clone(),
        protocol::unix_now(),
        policy,
        control,
    )
    .map_err(|error| NiriError::Unavailable(format!("cannot bind control listener: {error}")))?;
    let invite = serde_json::json!({
        "protocol": "1.0",
        "address": offer.address.to_string(),
        "pairing_id": offer.pairing_id.clone(),
        "secret_hex": secret.to_hex(),
        "expires_at": offer.expires_at,
        "control": control_info,
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
        if let Some(info) = &control_info {
            println!(
                "Control target {} ({}), capability revision {}, grants window={} desktop={} clipboard={} input={}, backend {}.",
                info["target_id"],
                info["target_generation"],
                info["capability_revision"],
                info["window_actions"],
                info["desktop_actions"],
                info["clipboard"],
                info["input"],
                info["backend"]
            );
            println!("Action dispatch is enabled; native input injection is not implemented.");
        } else {
            println!("No action policy attached: this listener is video-only.");
        }
        io::stdout().flush()?;
    }
    let stats = server
        .accept_and_run()
        .map_err(|error| NiriError::Unavailable(format!("control session failed: {error}")))?;
    if json {
        println!("{}", serde_json::to_string(&stats)?);
    } else {
        println!(
            "Encrypted control session: {} handshake frames, {} encrypted records, {} keyframe request(s), pong sent={}, snapshot sent={}, {} action result(s) ({} completed).",
            stats.handshake_frames,
            stats.encrypted_records,
            stats.keyframe_requests,
            stats.pong_sent,
            stats.control_snapshot_sent,
            stats.action_results,
            stats.actions_completed
        );
    }
    Ok(())
}

fn protocol_client(args: &[&str], json: bool) -> Result<(), NiriError> {
    check_known_arguments(
        args,
        &["--connect", "--secret-hex", "--action"],
        &["--allow-input"],
    )?;
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

    if let Some(action) = option(args, "--action") {
        let permissions = protocol::PermissionSet {
            video: true,
            input: args.contains(&"--allow-input"),
            ..protocol::PermissionSet::default()
        };
        let probe = vista_host::transport::ControlAction::new("cli-action", action);
        let stats = vista_host::transport::connect_control(address, secret, permissions, &probe)
            .map_err(|error| {
                NiriError::Unavailable(format!("control action client failed: {error}"))
            })?;
        let verified =
            stats.reached_active && stats.snapshot_received && stats.action_result_received;
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
                "Control action client on {}: active={}, snapshot actions={} (grant {}), result={:?}, error={:?}.",
                address,
                stats.reached_active,
                stats.snapshot_actions,
                stats.grant_epoch,
                stats.action_status,
                stats.action_error
            );
        }
        return Ok(());
    }

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

fn media_stream_command(args: &[&str], json: bool) -> Result<(), NiriError> {
    let serve = args.contains(&"--serve");
    let connect = option(args, "--connect");
    if serve && connect.is_some() {
        return Err(NiriError::InvalidArgument(
            "media-stream accepts either --serve or --connect, not both".into(),
        ));
    }
    if serve {
        check_known_arguments(args, &["--listen", "--frames"], &["--serve"])?;
        let listen = option(args, "--listen").unwrap_or("127.0.0.1:0");
        let address: SocketAddr = listen.parse().map_err(|_| {
            NiriError::InvalidArgument("--listen must be an IP:port address".into())
        })?;
        let frames = match option(args, "--frames") {
            Some(value) => value.parse::<usize>().map_err(|_| {
                NiriError::InvalidArgument("--frames must be a positive integer".into())
            })?,
            None => vista_host::media_stream::DEFAULT_FRAMES,
        };
        if frames == 0 {
            return Err(NiriError::InvalidArgument(
                "--frames must be greater than zero".into(),
            ));
        }
        let (server, offer) = vista_host::media_stream::MediaServer::bind(address, frames)
            .map_err(|error| NiriError::Unavailable(format!("media server: {error}")))?;
        if json {
            println!("{}", serde_json::to_string(&offer)?);
            io::stdout().flush()?;
        } else {
            println!(
                "Media stream listening on {} ({} frames, pairing id {}).",
                offer.address, offer.frames, offer.pairing_id
            );
            println!("Pairing id: {}", offer.pairing_id);
            println!(
                "Out-of-band pairing secret (test): {}",
                offer.pairing_secret_hex
            );
            println!("Offer expires at: {}", offer.expires_at);
            io::stdout().flush()?;
        }
        let stats = server
            .accept_and_send()
            .map_err(|error| NiriError::Unavailable(format!("media send: {error}")))?;
        if json {
            println!("{}", serde_json::to_string(&stats)?);
        } else {
            println!(
                "Sent {} encrypted media records / {} bytes in {} ms; keyframe requests={}, encoder restarts={}.",
                stats.frames_sent,
                stats.bytes_sent,
                stats.elapsed_ms,
                stats.keyframe_requests,
                stats.encoder_restarts
            );
        }
        return Ok(());
    }
    let Some(connect) = connect else {
        usage();
        return Err(NiriError::InvalidArgument(
            "media-stream requires --serve or --connect".into(),
        ));
    };
    check_known_arguments(args, &["--connect", "--secret-hex"], &[])?;
    let address: SocketAddr = connect
        .parse()
        .map_err(|_| NiriError::InvalidArgument("--connect must be an IP:port address".into()))?;
    let secret = option(args, "--secret-hex")
        .ok_or_else(|| NiriError::InvalidArgument("media-stream requires --secret-hex".into()))?;
    let stats = vista_host::media_stream::connect(address, secret)
        .map_err(|error| NiriError::Unavailable(format!("media receive: {error}")))?;
    if json {
        println!("{}", serde_json::to_string(&stats)?);
    } else {
        println!(
            "Received and decoded {} encrypted media records into {} frames ({} bytes) in {} ms; keyframe requests sent={}, latency samples={}.",
            stats.frames_received,
            stats.decoded_frames,
            stats.decoded_bytes,
            stats.elapsed_ms,
            stats.keyframe_requests_sent,
            stats.latency.samples
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
