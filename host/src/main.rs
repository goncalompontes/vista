use std::env;
use std::process::ExitCode;
use vista_host::{Niri, NiriError, portal, protocol, watch_windows};

#[derive(serde::Serialize)]
struct HostCapabilities {
    niri: vista_host::DoctorReport,
    screen_cast: portal::ScreenCastReport,
}

fn usage() {
    eprintln!(
        "Usage: vista-host <doctor|capabilities|windows|window ID|watch|protocol-test> [--json]"
    );
    eprintln!("       vista-host protocol-test [--json]");
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
        [command] if command.as_str() == "protocol-test" => {
            eprintln!(
                "Running an in-memory control-plane pairing and session test; no network, capture or input access."
            );
            let report = protocol::diagnostic()
                .map_err(|err| NiriError::Unavailable(format!("protocol test failed: {err}")))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Control protocol {}: host={} client={}, {} client frames / {} host frames / {} bytes; keys match={}, fingerprint={}.",
                    report.protocol,
                    report.host_state,
                    report.client_state,
                    report.client_frames,
                    report.host_frames,
                    report.wire_bytes,
                    report.keys_match,
                    report.key_fingerprint
                );
                println!(
                    "Permissions: video={} audio={} input={} clipboard={}.",
                    report.permissions.video,
                    report.permissions.audio,
                    report.permissions.input,
                    report.permissions.clipboard
                );
                println!(
                    "Negative checks: wrong-secret={} replay={} expired={} permission-escalation={} version-mismatch={}; verified={}.",
                    report.wrong_secret_rejected,
                    report.replayed_proof_rejected,
                    report.expired_offer_rejected,
                    report.permission_escalation_rejected,
                    report.version_mismatch_rejected,
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vista-host: {error}");
            ExitCode::FAILURE
        }
    }
}
