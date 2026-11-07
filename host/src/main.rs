use std::env;
use std::process::ExitCode;
use vista_host::{Niri, NiriError, portal, watch_windows};

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
