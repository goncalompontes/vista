//! Full-output capture via wlr-screencopy (`grim`) plus one-frame H.264 encode/decode.
//!
//! This is an **explicit fallback diagnostic**, not the portal per-window path.
//! It captures the entire active output into memory, never writes a file, never
//! sends pixels anywhere, and reports only dimensions/byte counts/timing.
//! It must not be used to silently replace single-window portal capture.

use crate::media::{H264Encoder, decoder_candidates, probe_encoders};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::{
    process::Command,
    time::{Duration, Instant},
};

const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;
const CODEC_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize)]
pub struct OutputCaptureReport {
    pub backend: String,
    pub width: u32,
    pub height: u32,
    pub rgb_bytes: usize,
    pub rgba_bytes: usize,
    pub encoder: H264Encoder,
    pub decoder: String,
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
    pub capture_ms: u128,
    pub encode_decode_ms: u128,
    pub elapsed_ms: u128,
    /// Always false: this path does not request the desktop portal.
    pub portal_consent: bool,
    pub verified: bool,
}

fn parse_token<'a>(bytes: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
    loop {
        while *position < bytes.len() && matches!(bytes[*position], b' ' | b'\t' | b'\r' | b'\n') {
            *position += 1;
        }
        if *position < bytes.len() && bytes[*position] == b'#' {
            while *position < bytes.len() && bytes[*position] != b'\n' {
                *position += 1;
            }
            continue;
        }
        break;
    }
    let start = *position;
    while *position < bytes.len() && !matches!(bytes[*position], b' ' | b'\t' | b'\r' | b'\n') {
        *position += 1;
    }
    (start != *position).then_some(&bytes[start..*position])
}

fn parse_u32(token: &[u8]) -> Result<u32, String> {
    std::str::from_utf8(token)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "invalid PPM numeric field".to_string())
}

fn parse_ppm(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.len() > MAX_CAPTURE_BYTES {
        return Err("capture exceeds the 64 MiB bound".into());
    }
    let mut position = 0;
    let magic = parse_token(bytes, &mut position).ok_or("missing PPM magic")?;
    if magic != b"P6" {
        return Err("capture is not a binary P6 PPM".into());
    }
    let width = parse_u32(parse_token(bytes, &mut position).ok_or("missing PPM width")?)?;
    let height = parse_u32(parse_token(bytes, &mut position).ok_or("missing PPM height")?)?;
    let maxval = parse_u32(parse_token(bytes, &mut position).ok_or("missing PPM maxval")?)?;
    if width == 0 || height == 0 || maxval != 255 {
        return Err("unsupported PPM dimensions or maxval".into());
    }
    // The PPM header ends with exactly one whitespace byte after maxval.
    if position >= bytes.len() || !bytes[position].is_ascii_whitespace() {
        return Err("PPM header is not followed by pixel data".into());
    }
    position += 1;
    if position < bytes.len() && bytes[position - 1] == b'\r' && bytes[position] == b'\n' {
        position += 1;
    }
    let pixel_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or("PPM dimensions overflow")?;
    if pixel_bytes > MAX_CAPTURE_BYTES || bytes.len() - position < pixel_bytes {
        return Err("PPM pixel data is truncated or too large".into());
    }
    Ok((
        width,
        height,
        bytes[position..position + pixel_bytes].to_vec(),
    ))
}

fn rgb_to_rgba(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or("capture dimensions overflow")?;
    if rgb.len() != pixels * 3 {
        return Err("RGB data length does not match the capture dimensions".into());
    }
    let mut rgba = vec![0u8; pixels * 4];
    for pixel in 0..pixels {
        let source = &rgb[pixel * 3..pixel * 3 + 3];
        let destination = &mut rgba[pixel * 4..pixel * 4 + 4];
        destination[..3].copy_from_slice(source);
        destination[3] = 0xff;
    }
    Ok(rgba)
}

fn capture_output() -> Result<(u32, u32, Vec<u8>), String> {
    let output = Command::new("timeout")
        .args(["10", "grim", "-t", "ppm", "-"])
        .output()
        .map_err(|error| format!("cannot execute grim: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "grim failed or timed out: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_ppm(&output.stdout)
}

fn build_pipeline(description: &str) -> Result<gst::Pipeline, String> {
    gst::parse::launch(description)
        .map_err(|error| format!("cannot construct pipeline: {error}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "GStreamer returned an unexpected element".to_string())
}

fn encode_frame(
    encoder: H264Encoder,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<Vec<u8>, String> {
    let description = format!(
        "appsrc name=frame_source is-live=false format=time \
         caps=video/x-raw,format=RGBA,width={width},height={height},framerate=30/1 ! \
         queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} {} ! \
         h264parse config-interval=-1 ! \
         video/x-h264,stream-format=byte-stream,alignment=au ! \
         appsink name=encoded_sink sync=false max-buffers=2 drop=false",
        encoder.element_name(),
        encoder.settings()
    );
    let pipeline = build_pipeline(&description)?;
    let result = (|| {
        let source = pipeline
            .by_name("frame_source")
            .ok_or("missing frame_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "frame_source is not an AppSrc")?;
        let sink = pipeline
            .by_name("encoded_sink")
            .ok_or("missing encoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "encoded_sink is not an AppSink")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start encoder: {error}"))?;
        let buffer = gst::Buffer::from_slice(rgba.to_vec());
        source
            .push_buffer(buffer)
            .map_err(|error| format!("encoder input: {error}"))?;
        source
            .end_of_stream()
            .map_err(|error| format!("encoder EOS: {error}"))?;
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_seconds(CODEC_WAIT.as_secs()))
            .ok_or("encoder produced no sample before the deadline")?;
        let caps = sample.caps().ok_or("encoded sample missing caps")?;
        if caps.structure(0).map(|structure| structure.name().as_str()) != Some("video/x-h264") {
            return Err("encoded sample is not H.264".into());
        }
        let buffer = sample.buffer().ok_or("encoded sample missing buffer")?;
        let size = buffer.size();
        if size == 0 || size > MAX_ENCODED_BYTES {
            return Err(format!("invalid encoded size: {size}"));
        }
        Ok(buffer
            .map_readable()
            .map_err(|_| "cannot map encoded buffer")?
            .as_slice()
            .to_vec())
    })();
    let stop = pipeline.set_state(gst::State::Null);
    match (result, stop) {
        (Ok(bytes), Ok(_)) => Ok(bytes),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!("encoder cleanup failed: {error}; prior={result:?}")),
    }
}

fn decode_frame(decoder: &str, width: u32, height: u32, encoded: &[u8]) -> Result<usize, String> {
    let description = format!(
        "appsrc name=h264_source is-live=false format=time \
         caps=video/x-h264,stream-format=byte-stream,alignment=au ! \
         h264parse ! {decoder} ! videoconvert ! video/x-raw,format=RGBA ! \
         appsink name=decoded_sink sync=false max-buffers=2 drop=false"
    );
    let pipeline = build_pipeline(&description)?;
    let result = (|| {
        let source = pipeline
            .by_name("h264_source")
            .ok_or("missing h264_source")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "h264_source is not an AppSrc")?;
        let sink = pipeline
            .by_name("decoded_sink")
            .ok_or("missing decoded_sink")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "decoded_sink is not an AppSink")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start decoder: {error}"))?;
        let buffer = gst::Buffer::from_slice(encoded.to_vec());
        source
            .push_buffer(buffer)
            .map_err(|error| format!("decoder input: {error}"))?;
        source
            .end_of_stream()
            .map_err(|error| format!("decoder EOS: {error}"))?;
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_seconds(CODEC_WAIT.as_secs()))
            .ok_or("decoder produced no sample before the deadline")?;
        let caps = sample.caps().ok_or("decoded sample missing caps")?;
        let structure = caps.structure(0).ok_or("decoded caps missing structure")?;
        if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA")
        {
            return Err("decoded sample is not RGBA".into());
        }
        let decoded_width = structure
            .get::<i32>("width")
            .map_err(|_| "decoded width missing")?;
        let decoded_height = structure
            .get::<i32>("height")
            .map_err(|_| "decoded height missing")?;
        if decoded_width != width as i32 || decoded_height != height as i32 {
            return Err(format!(
                "decoded dimensions {decoded_width}x{decoded_height} do not match {width}x{height}"
            ));
        }
        let size = sample
            .buffer()
            .ok_or("decoded sample missing buffer")?
            .size();
        let expected = (width as usize) * (height as usize) * 4;
        if size < expected {
            return Err("decoded buffer is truncated".into());
        }
        Ok(size)
    })();
    let stop = pipeline.set_state(gst::State::Null);
    match (result, stop) {
        (Ok(bytes), Ok(_)) => Ok(bytes),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!("decoder cleanup failed: {error}; prior={result:?}")),
    }
}

/// Captures the full output through grim, then encodes and decodes exactly one
/// frame. This requires explicit `--confirm` at the CLI and never persists
/// pixels or sends them over a network.
pub fn output_capture_probe() -> Result<OutputCaptureReport, String> {
    let started = Instant::now();
    let (width, height, rgb) = capture_output()?;
    let capture_ms = started.elapsed().as_millis();
    let rgba = rgb_to_rgba(width, height, &rgb)?;
    let capabilities = probe_encoders()?;
    if !capabilities.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoders = [
        (capabilities.nvidia, H264Encoder::Nvidia),
        (capabilities.x264, H264Encoder::X264),
        (capabilities.openh264, H264Encoder::OpenH264),
    ];
    let decoders: Vec<_> = decoder_candidates()
        .iter()
        .copied()
        .filter(|name| gst::ElementFactory::find(name).is_some())
        .collect();
    let mut errors = Vec::new();
    for (present, encoder) in encoders {
        if !present {
            continue;
        }
        for decoder in &decoders {
            let started = Instant::now();
            match encode_frame(encoder, width, height, &rgba).and_then(|encoded| {
                decode_frame(decoder, width, height, &encoded).map(|decoded| (encoded, decoded))
            }) {
                Ok((encoded, decoded_bytes)) => {
                    return Ok(OutputCaptureReport {
                        backend: "grim/wlr-screencopy".into(),
                        width,
                        height,
                        rgb_bytes: rgb.len(),
                        rgba_bytes: rgba.len(),
                        encoder,
                        decoder: decoder.to_string(),
                        encoded_bytes: encoded.len(),
                        decoded_bytes,
                        capture_ms,
                        encode_decode_ms: started.elapsed().as_millis(),
                        elapsed_ms: started.elapsed().as_millis(),
                        portal_consent: false,
                        verified: true,
                    });
                }
                Err(error) => {
                    errors.push(format!("{} -> {decoder}: {error}", encoder.element_name()))
                }
            }
        }
    }
    Err(format!(
        "no encoder/decoder path handled the output capture: {}",
        errors.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppm(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut output = format!("P6\n{width} {height}\n255\n").into_bytes();
        output.extend_from_slice(pixels);
        output
    }

    #[test]
    fn parses_binary_ppm_and_converts_to_rgba() {
        let data = ppm(2, 1, &[1, 2, 3, 4, 5, 6]);
        let (width, height, rgb) = parse_ppm(&data).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgb, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            rgb_to_rgba(width, height, &rgb).unwrap(),
            vec![1, 2, 3, 0xff, 4, 5, 6, 0xff]
        );
    }

    #[test]
    fn rejects_malformed_or_truncated_ppm() {
        assert!(parse_ppm(b"P3\n1 1\n255\n0 0 0").is_err());
        assert!(parse_ppm(b"P6\n2 1\n255\n1 2 3").is_err());
        assert!(parse_ppm(b"P6\n0 1\n255\n").is_err());
        assert!(parse_ppm(b"P6\n1 1\n254\n0 0 0").is_err());
    }

    #[test]
    #[ignore = "requires grim, a live Wayland output and H.264 codec plugins"]
    fn full_output_capture_encode_decode_on_supported_host() {
        let report = output_capture_probe().unwrap();
        assert!(report.verified);
        assert!(!report.portal_consent);
        assert!(report.width > 0 && report.height > 0);
        assert!(report.encoded_bytes > 0);
    }
}
