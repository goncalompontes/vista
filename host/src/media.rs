//! H.264 encoder capability negotiation and bounded pipeline configuration.
//!
//! Discovery does not start a capture session, allocate an encoder, or transmit pixels.
//! Only a separately consented ScreenCast source may eventually feed the encoder.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum H264Encoder {
    Nvidia,
    X264,
    OpenH264,
}

impl H264Encoder {
    pub fn element_name(self) -> &'static str {
        match self {
            Self::Nvidia => "nvh264enc",
            Self::X264 => "x264enc",
            Self::OpenH264 => "openh264enc",
        }
    }

    pub(crate) fn settings(self) -> &'static str {
        match self {
            Self::Nvidia => "preset=p1 bframes=0 bitrate=3000",
            Self::X264 => "tune=zerolatency speed-preset=ultrafast bitrate=3000 bframes=0",
            Self::OpenH264 => "bitrate=3000000",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EncoderCapabilities {
    pub nvidia: bool,
    pub x264: bool,
    pub openh264: bool,
    pub h264_parser: bool,
    pub decoder_available: bool,
    /// Availability of a GStreamer element is not proof that GPU initialization works.
    pub selected: Option<H264Encoder>,
}

pub fn probe_encoders() -> Result<EncoderCapabilities, String> {
    gst::init().map_err(|e| format!("GStreamer initialization: {e}"))?;
    let available = |name| gst::ElementFactory::find(name).is_some();
    let nvidia = available("nvh264enc");
    let x264 = available("x264enc");
    let openh264 = available("openh264enc");
    let h264_parser = available("h264parse");
    let decoder_available =
        available("nvh264dec") || available("avdec_h264") || available("openh264dec");
    let selected = select_encoder(nvidia, x264, openh264, h264_parser);
    Ok(EncoderCapabilities {
        nvidia,
        x264,
        openh264,
        h264_parser,
        decoder_available,
        selected,
    })
}

fn select_encoder(nvidia: bool, x264: bool, openh264: bool, parser: bool) -> Option<H264Encoder> {
    if !parser {
        return None;
    }
    if nvidia {
        Some(H264Encoder::Nvidia)
    } else if x264 {
        Some(H264Encoder::X264)
    } else if openh264 {
        Some(H264Encoder::OpenH264)
    } else {
        None
    }
}

/// A synthetic test-source pipeline, never a screen capture pipeline.
/// Encoded samples are consumed in memory and limited to two pending buffers.
/// Hardware encoder availability alone does not establish a working media path.
pub fn synthetic_h264_pipeline_description(encoder: H264Encoder) -> String {
    format!(
        "videotestsrc num-buffers=30 is-live=true ! video/x-raw,width=640,height=360,framerate=30/1 ! videoconvert ! {} {} ! h264parse ! video/x-h264,alignment=au ! appsink name=encoded_sink sync=false max-buffers=2 drop=true",
        encoder.element_name(),
        encoder.settings()
    )
}

/// A bounded, permission-free codec smoke test. No PipeWire source is involved.
#[derive(Debug, Serialize)]
pub struct H264RoundTripReport {
    pub encoder: H264Encoder,
    pub decoder: String,
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
    pub width: i32,
    pub height: i32,
    pub elapsed_ms: u128,
    pub verified: bool,
}

const CODEC_WAIT: Duration = Duration::from_secs(8);

pub(crate) fn decoder_candidates() -> &'static [&'static str] {
    &["nvh264dec", "avdec_h264", "openh264dec"]
}

fn codec_candidates(capabilities: &EncoderCapabilities) -> Vec<H264Encoder> {
    [
        (capabilities.nvidia, H264Encoder::Nvidia),
        (capabilities.x264, H264Encoder::X264),
        (capabilities.openh264, H264Encoder::OpenH264),
    ]
    .into_iter()
    .filter_map(|(available, encoder)| available.then_some(encoder))
    .collect()
}

fn validate_encoded(sample: &gst::Sample) -> Result<usize, String> {
    let caps = sample.caps().ok_or("encoded sample missing caps")?;
    if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
        return Err("encoded sample is not H.264".into());
    }
    let bytes = sample
        .buffer()
        .ok_or("encoded sample missing buffer")?
        .size();
    if bytes == 0 || bytes > 16 * 1024 * 1024 {
        return Err(format!("invalid encoded buffer size: {bytes}"));
    }
    Ok(bytes)
}

fn validate_decoded(sample: &gst::Sample) -> Result<(i32, i32, usize), String> {
    let caps = sample.caps().ok_or("decoded sample missing caps")?;
    let structure = caps.structure(0).ok_or("decoded caps missing structure")?;
    if structure.name() != "video/x-raw" || structure.get::<&str>("format").ok() != Some("RGBA") {
        return Err("decoded sample is not RGBA video".into());
    }
    let width = structure
        .get::<i32>("width")
        .map_err(|_| "missing decoded width")?;
    let height = structure
        .get::<i32>("height")
        .map_err(|_| "missing decoded height")?;
    if width != 640 || height != 360 {
        return Err(format!("unexpected decoded dimensions: {width}x{height}"));
    }
    let bytes = sample
        .buffer()
        .ok_or("decoded sample missing buffer")?
        .size();
    let expected = (width as usize) * (height as usize) * 4;
    if bytes < expected || bytes > 16 * 1024 * 1024 {
        return Err(format!("invalid decoded buffer size: {bytes}"));
    }
    Ok((width, height, bytes))
}

/// Tests a synthetic 30-frame source, bounded H.264 encoder and decoder, and
/// verifies real compressed and decompressed buffers. Always attempts shutdown.
fn try_synthetic_round_trip(
    encoder: H264Encoder,
    decoder: &str,
) -> Result<H264RoundTripReport, String> {
    // Both choices originate from fixed, compiled-in plugin names.
    let description = format!(
        "videotestsrc num-buffers=30 is-live=true ! video/x-raw,width=640,height=360,framerate=30/1 ! videoconvert ! {} {} ! h264parse ! video/x-h264,alignment=au ! tee name=video_split \
         video_split. ! queue max-size-buffers=2 leaky=downstream ! appsink name=encoded_sink sync=false max-buffers=2 drop=true \
         video_split. ! queue max-size-buffers=2 leaky=downstream ! {} ! videoconvert ! video/x-raw,format=RGBA ! appsink name=decoded_sink sync=false max-buffers=2 drop=true",
        encoder.element_name(),
        encoder.settings(),
        decoder
    );
    let element = gst::parse::launch(&description)
        .map_err(|e| format!("cannot construct codec pipeline: {e}"))?;
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "codec pipeline is not a GStreamer pipeline".to_string())?;

    let result = (|| {
        let encoded_sink = pipeline
            .by_name("encoded_sink")
            .ok_or("encoded sink not found")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "encoded sink is not an AppSink")?;
        let decoded_sink = pipeline
            .by_name("decoded_sink")
            .ok_or("decoded sink not found")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "decoded sink is not an AppSink")?;
        let started = Instant::now();
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("codec startup failed: {e}"))?;
        let deadline = started + CODEC_WAIT;
        let remaining = || {
            gst::ClockTime::from_nseconds(
                u64::try_from(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_nanos(),
                )
                .unwrap_or(u64::MAX),
            )
        };
        // Both branches run concurrently; bounded queues prevent stalled consumers.
        let decoded = decoded_sink
            .try_pull_sample(remaining())
            .ok_or_else(|| "codec produced no decoded sample before deadline".to_string())?;
        let (width, height, decoded_bytes) = validate_decoded(&decoded)?;
        let encoded = encoded_sink
            .try_pull_sample(remaining())
            .ok_or_else(|| "codec produced no encoded sample before deadline".to_string())?;
        let encoded_bytes = validate_encoded(&encoded)?;
        Ok(H264RoundTripReport {
            encoder,
            decoder: decoder.to_string(),
            encoded_bytes,
            decoded_bytes,
            width,
            height,
            elapsed_ms: started.elapsed().as_millis(),
            verified: true,
        })
    })();

    let cleanup = pipeline
        .set_state(gst::State::Null)
        .map_err(|e| format!("codec pipeline cleanup failed: {e}"));
    match (result, cleanup) {
        (Ok(report), Ok(_)) => Ok(report),
        (Err(err), Ok(_)) => Err(err),
        (Ok(_), Err(err)) => Err(err),
        (Err(err), Err(cleanup_err)) => Err(format!("{err}; {cleanup_err}")),
    }
}

/// Tries installed encoders and decoders, including software fallback when
/// NVIDIA startup fails. Plugin discovery is not reported as successful encoding.
pub fn synthetic_h264_round_trip() -> Result<H264RoundTripReport, String> {
    let caps = probe_encoders()?;
    if !caps.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoders = codec_candidates(&caps);
    let decoders: Vec<_> = decoder_candidates()
        .iter()
        .copied()
        .filter(|name| gst::ElementFactory::find(name).is_some())
        .collect();
    if encoders.is_empty() || decoders.is_empty() {
        return Err("no usable H.264 encoder/decoder factories are installed".into());
    }
    let mut failures = Vec::new();
    for encoder in encoders {
        for decoder in &decoders {
            match try_synthetic_round_trip(encoder, decoder) {
                Ok(report) => return Ok(report),
                Err(error) => {
                    failures.push(format!("{} -> {decoder}: {error}", encoder.element_name()))
                }
            }
        }
    }
    Err(format!(
        "all H.264 codec paths failed: {}",
        failures.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_hardware_then_software_without_claiming_runtime_support() {
        assert_eq!(
            select_encoder(true, true, true, true),
            Some(H264Encoder::Nvidia)
        );
        assert_eq!(
            select_encoder(false, true, true, true),
            Some(H264Encoder::X264)
        );
        assert_eq!(
            select_encoder(false, false, true, true),
            Some(H264Encoder::OpenH264)
        );
        assert_eq!(select_encoder(true, true, true, false), None);
        assert_eq!(select_encoder(false, false, false, true), None);
    }

    #[test]
    fn synthetic_descriptions_are_bounded_and_use_video_not_desktop_source() {
        for encoder in [
            H264Encoder::Nvidia,
            H264Encoder::X264,
            H264Encoder::OpenH264,
        ] {
            let desc = synthetic_h264_pipeline_description(encoder);
            assert!(desc.contains("videotestsrc num-buffers=30"));
            assert!(desc.contains(encoder.element_name()));
            assert!(desc.contains("h264parse"));
            assert!(desc.contains("max-buffers=2 drop=true"));
            assert!(!desc.contains("pipewiresrc"));
        }
    }

    #[test]
    fn codec_fallback_order_follows_available_factories() {
        let caps = EncoderCapabilities {
            nvidia: false,
            x264: true,
            openh264: true,
            h264_parser: true,
            decoder_available: true,
            selected: Some(H264Encoder::X264),
        };
        assert_eq!(
            codec_candidates(&caps),
            vec![H264Encoder::X264, H264Encoder::OpenH264]
        );
        let no_encoder = EncoderCapabilities {
            x264: false,
            openh264: false,
            ..caps
        };
        assert!(codec_candidates(&no_encoder).is_empty());
    }

    #[test]
    #[ignore = "requires installed and functioning hardware/software codec plugins"]
    fn synthetic_codec_integration_on_supported_host() {
        let result = synthetic_h264_round_trip().expect("actual synthetic codec round trip");
        assert!(result.verified);
        assert!(result.encoded_bytes > 0);
        assert_eq!((result.width, result.height), (640, 360));
        assert!(result.decoded_bytes >= 640 * 360 * 4);
    }

    #[test]
    fn capability_probe_only_discovers_element_factories() {
        let report = probe_encoders().unwrap();
        if let Some(encoder) = report.selected {
            assert!(report.h264_parser);
            assert!(match encoder {
                H264Encoder::Nvidia => report.nvidia,
                H264Encoder::X264 => report.x264,
                H264Encoder::OpenH264 => report.openh264,
            });
        }
    }
}
