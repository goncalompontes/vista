//! Live window/desktop capture and H.264 production for the authenticated
//! control service.
//!
//! The producer uses `grim`'s wlr-screencopy path to read the focused output.
//! A window selection focuses and fullscreens that window first, so the output
//! shows exactly the requested window. A workspace selection focuses that Niri
//! virtual desktop. This deliberately does not use the ScreenCast portal, which
//! requires a manual desktop chooser and cannot be addressed by a client.
//!
//! The producer never persists pixels. Access units enter a bounded queue and
//! are written by the authenticated media connection. Any consumer that falls
//! behind receives a discontinuity flag and must wait for a fresh IDR.

use crate::{
    media::{H264Encoder, probe_encoders},
    media_transport::MediaRecord,
    protocol::StreamTarget,
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const DEFAULT_FPS: u32 = 15;
pub const MAX_FPS: u32 = 30;
const QUEUE_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamGeometry {
    pub width: u32,
    pub height: u32,
    pub epoch: u64,
}

#[derive(Debug)]
struct QueueState {
    records: VecDeque<MediaRecord>,
    closed: bool,
    discontinuity_pending: bool,
    keyframe_requested: bool,
    geometry: Option<StreamGeometry>,
    encoder: Option<String>,
    started: bool,
    error: Option<String>,
    frames: u64,
    bytes: u64,
    dropped: u64,
}

/// Shared bounded handoff between the capture producer and one media consumer.
pub struct SharedMedia {
    state: Mutex<QueueState>,
    ready: Condvar,
    epoch: AtomicU64,
}

impl Default for SharedMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedMedia {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                records: VecDeque::with_capacity(QUEUE_DEPTH),
                closed: false,
                discontinuity_pending: true,
                keyframe_requested: false,
                geometry: None,
                encoder: None,
                started: false,
                error: None,
                frames: 0,
                bytes: 0,
                dropped: 0,
            }),
            ready: Condvar::new(),
            epoch: AtomicU64::new(1),
        }
    }

    /// Start a new target. The next delivered access unit is marked
    /// discontinuous so the decoder discards reference state even if the old
    /// target had the same dimensions.
    pub fn begin(&self) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        let mut state = self.state.lock().expect("media state");
        state.records.clear();
        state.discontinuity_pending = true;
        state.keyframe_requested = true;
        state.geometry = None;
        state.encoder = None;
        state.started = false;
        state.error = None;
        self.ready.notify_all();
        epoch
    }

    pub fn close(&self) {
        let mut state = self.state.lock().expect("media state");
        state.closed = true;
        state.records.clear();
        self.ready.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().expect("media state").closed
    }

    pub fn request_keyframe(&self) {
        let mut state = self.state.lock().expect("media state");
        state.keyframe_requested = true;
        self.ready.notify_all();
    }

    fn take_keyframe(&self) -> bool {
        let mut state = self.state.lock().expect("media state");
        std::mem::take(&mut state.keyframe_requested)
    }

    fn push(&self, record: MediaRecord, geometry: StreamGeometry, encoder: &str) {
        let mut state = self.state.lock().expect("media state");
        if state.closed {
            return;
        }
        if state.records.len() == QUEUE_DEPTH {
            state.records.pop_front();
            state.dropped = state.dropped.saturating_add(1);
            state.discontinuity_pending = true;
        }
        state.geometry = Some(geometry);
        state.encoder = Some(encoder.to_string());
        state.started = true;
        state.frames = state.frames.saturating_add(1);
        state.bytes = state.bytes.saturating_add(record.bytes.len() as u64);
        state.records.push_back(record);
        self.ready.notify_all();
    }

    fn fail(&self, message: impl Into<String>) {
        let mut state = self.state.lock().expect("media state");
        if state.error.is_none() {
            state.error = Some(message.into());
        }
        self.ready.notify_all();
    }

    /// A consumer blocks until a record is available or the queue closes.
    pub fn next(&self) -> Option<(MediaRecord, StreamGeometry, String)> {
        let mut state = self.state.lock().expect("media state");
        loop {
            if let Some(mut record) = state.records.pop_front() {
                if state.discontinuity_pending {
                    record.discontinuity = true;
                    state.discontinuity_pending = false;
                }
                let geometry = state.geometry.unwrap_or(StreamGeometry {
                    width: 1,
                    height: 1,
                    epoch: self.epoch.load(Ordering::SeqCst),
                });
                let encoder = state.encoder.clone().unwrap_or_else(|| "unknown".into());
                return Some((record, geometry, encoder));
            }
            if state.closed {
                return None;
            }
            state = self.ready.wait(state).expect("media state");
        }
    }

    pub fn started(&self) -> Option<(StreamGeometry, String)> {
        let state = self.state.lock().expect("media state");
        if !state.started {
            return None;
        }
        state.geometry.map(|geometry| {
            (
                geometry,
                state.encoder.clone().unwrap_or_else(|| "unknown".into()),
            )
        })
    }

    pub fn take_error(&self) -> Option<String> {
        self.state.lock().expect("media state").error.take()
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        let state = self.state.lock().expect("media state");
        (state.frames, state.bytes, state.dropped)
    }
}

/// A running producer. Dropping it stops capture and restores any window
/// fullscreen state it changed.
pub struct Producer {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    target: StreamTarget,
    shared: Arc<SharedMedia>,
}

impl Producer {
    pub fn start(target: StreamTarget, shared: Arc<SharedMedia>, fps: u32) -> Result<Self, String> {
        let fps = fps.clamp(1, MAX_FPS);
        let stop = Arc::new(AtomicBool::new(false));
        let producer_stop = stop.clone();
        let producer_shared = shared.clone();
        let producer_target = target;
        let join = thread::Builder::new()
            .name(format!("vista-capture-{}", target.identifier()))
            .spawn(move || {
                if let Err(error) =
                    run_producer(producer_target, &producer_shared, &producer_stop, fps)
                {
                    producer_shared.fail(error);
                }
            })
            .map_err(|error| format!("cannot start capture thread: {error}"))?;
        Ok(Self {
            stop,
            join: Some(join),
            target,
            shared,
        })
    }

    pub fn target(&self) -> StreamTarget {
        self.target
    }

    pub fn shared(&self) -> &Arc<SharedMedia> {
        &self.shared
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_producer(
    target: StreamTarget,
    shared: &SharedMedia,
    stop: &AtomicBool,
    fps: u32,
) -> Result<(), String> {
    match target {
        StreamTarget::Window { id } => {
            niri_action(&["focus-window", "--id", &id.to_string()])?;
            niri_action(&["fullscreen-window"])?;
        }
        StreamTarget::Workspace { id } => {
            let workspace = crate::Niri::default()
                .workspace(id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("workspace {id} is not available"))?;
            niri_action(&["focus-workspace", &workspace.idx.to_string()])?;
        }
        StreamTarget::Desktop => {}
    }

    let result = capture_loop(shared, stop, fps);

    // Restore the desktop only after the producer has fully stopped capturing.
    if let StreamTarget::Window { id } = target {
        let _ = niri_action(&["focus-window", "--id", &id.to_string()]);
        let _ = niri_action(&["fullscreen-window"]);
    }
    result
}

fn niri_action(args: &[&str]) -> Result<(), String> {
    let output = Command::new("niri")
        .arg("msg")
        .arg("action")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run niri: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "niri msg action {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn capture_loop(shared: &SharedMedia, stop: &AtomicBool, fps: u32) -> Result<(), String> {
    gst::init().map_err(|error| format!("GStreamer initialization: {error}"))?;
    let capabilities = probe_encoders()?;
    if !capabilities.h264_parser {
        return Err("GStreamer h264parse is unavailable".into());
    }
    let encoder = capabilities
        .selected
        .ok_or_else(|| crate::media::encoder_unavailable_remediation(&capabilities))?;
    let mut pipeline = EncoderPipeline::new(encoder)?;
    let frame_interval = Duration::from_micros(1_000_000 / u64::from(fps));
    let started = Instant::now();
    let mut next_frame = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        let frame = capture_ppm()?;
        let geometry = StreamGeometry {
            width: frame.width,
            height: frame.height,
            epoch: 0,
        };

        if pipeline.geometry() != Some((frame.width, frame.height)) || shared.take_keyframe() {
            // Recreate the encoder so the next access unit starts with SPS/PPS.
            pipeline.stop();
            pipeline = EncoderPipeline::new(encoder)?;
        }

        let pts = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        pipeline.push(frame, pts)?;
        while let Some((record, keyframe)) = pipeline.pull()? {
            let geometry = StreamGeometry {
                epoch: shared.epoch.load(Ordering::SeqCst),
                ..geometry
            };
            let keyframe = keyframe || record_keyframe(&record);
            let record =
                MediaRecord::new(record.pts_ns, keyframe, record.discontinuity, record.bytes)
                    .map_err(|error| format!("media record: {error}"))?;
            shared.push(record, geometry, encoder.element_name());
        }

        next_frame += frame_interval;
        let now = Instant::now();
        if next_frame > now {
            thread::sleep(next_frame - now);
        } else {
            next_frame = now;
        }
    }

    pipeline.stop();
    Ok(())
}

fn record_keyframe(record: &MediaRecord) -> bool {
    record.keyframe
}

pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// Run one `grim` capture and parse its binary PPM output.
pub fn capture_ppm() -> Result<RgbFrame, String> {
    let mut child = Command::new("grim")
        .args(["-t", "ppm", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start grim: {error}"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("grim produced no stdout")?
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read grim output: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot wait for grim: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "grim failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_ppm(&bytes)
}

pub fn parse_ppm(bytes: &[u8]) -> Result<RgbFrame, String> {
    let (width, height, offset) = parse_ppm_header(bytes)?;
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or("PPM dimensions overflow")?;
    let end = offset.checked_add(pixels).ok_or("PPM size overflow")?;
    if end > bytes.len() {
        return Err(format!(
            "truncated PPM: expected {pixels} pixel bytes after offset {offset}, got {}",
            bytes.len().saturating_sub(offset)
        ));
    }
    Ok(RgbFrame {
        width,
        height,
        bytes: bytes[offset..end].to_vec(),
    })
}

fn parse_ppm_header(bytes: &[u8]) -> Result<(u32, u32, usize), String> {
    let mut position = 0;
    let mut tokens = Vec::with_capacity(4);
    while tokens.len() < 4 {
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        if position < bytes.len() && bytes[position] == b'#' {
            while position < bytes.len() && bytes[position] != b'\n' {
                position += 1;
            }
            continue;
        }
        let start = position;
        while position < bytes.len() && !bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        if start == position {
            return Err("incomplete PPM header".into());
        }
        tokens.push(&bytes[start..position]);
    }
    if tokens[0] != b"P6" {
        return Err("grim output is not binary PPM (P6)".into());
    }
    let parse = |token: &[u8], field: &str| {
        std::str::from_utf8(token)
            .map_err(|_| format!("invalid PPM {field}"))?
            .parse::<u32>()
            .map_err(|_| format!("invalid PPM {field}"))
    };
    let width = parse(tokens[1], "width")?;
    let height = parse(tokens[2], "height")?;
    let maxval = parse(tokens[3], "maxval")?;
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 || maxval != 255 {
        return Err("unsupported PPM geometry or maxval".into());
    }
    // Exactly one whitespace byte terminates the maxval token.
    if position >= bytes.len() || !bytes[position].is_ascii_whitespace() {
        return Err("missing PPM raster separator".into());
    }
    Ok((width, height, position + 1))
}

struct EncoderPipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    sink: gst_app::AppSink,
    width: u32,
    height: u32,
}

impl EncoderPipeline {
    fn new(encoder: H264Encoder) -> Result<Self, String> {
        let pipeline = gst::Pipeline::new();
        let appsrc = gst::ElementFactory::make("appsrc")
            .name("raw_source")
            .build()
            .map_err(|error| format!("appsrc: {error}"))?;
        let queue = gst::ElementFactory::make("queue")
            .property_from_str("max-size-buffers", "2")
            .property_from_str("leaky", "downstream")
            .build()
            .map_err(|error| format!("queue: {error}"))?;
        let convert = gst::ElementFactory::make("videoconvert")
            .build()
            .map_err(|error| format!("videoconvert: {error}"))?;
        let encoder_element = gst::ElementFactory::make(encoder.element_name())
            .build()
            .map_err(|error| format!("encoder: {error}"))?;
        apply_settings(&encoder_element, encoder);
        let parser = gst::ElementFactory::make("h264parse")
            .build()
            .map_err(|error| format!("h264parse: {error}"))?;
        parser.set_property("config-interval", -1i32);
        let caps = gst::Caps::builder("video/x-h264")
            .field("stream-format", "byte-stream")
            .field("alignment", "au")
            .build();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &caps)
            .build()
            .map_err(|error| format!("capsfilter: {error}"))?;
        let sink = gst::ElementFactory::make("appsink")
            .name("encoded_sink")
            .property("sync", false)
            .property("max-buffers", 2u32)
            .property("drop", false)
            .build()
            .map_err(|error| format!("appsink: {error}"))?;
        pipeline
            .add_many([
                &appsrc,
                &queue,
                &convert,
                &encoder_element,
                &parser,
                &capsfilter,
                &sink,
            ])
            .map_err(|error| format!("pipeline add: {error}"))?;
        for (left, right) in [
            (&appsrc, &queue),
            (&queue, &convert),
            (&convert, &encoder_element),
            (&encoder_element, &parser),
            (&parser, &capsfilter),
            (&capsfilter, &sink),
        ] {
            gst::Element::link(left, right).map_err(|error| format!("pipeline link: {error}"))?;
        }
        let appsrc = appsrc
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "appsrc is not an AppSrc")?;
        let sink = sink
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "appsink is not an AppSink")?;
        appsrc.set_is_live(true);
        appsrc.set_format(gst::Format::Time);
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("cannot start encoder: {error}"))?;
        Ok(Self {
            pipeline,
            appsrc,
            sink,
            width: 0,
            height: 0,
        })
    }

    fn geometry(&self) -> Option<(u32, u32)> {
        if self.width == 0 {
            None
        } else {
            Some((self.width, self.height))
        }
    }

    fn push(&mut self, frame: RgbFrame, pts_ns: u64) -> Result<(), String> {
        if (self.width, self.height) != (frame.width, frame.height) {
            let caps = gst::Caps::builder("video/x-raw")
                .field("format", "RGB")
                .field("width", frame.width as i32)
                .field("height", frame.height as i32)
                .field("framerate", gst::Fraction::new(30, 1))
                .build();
            self.appsrc.set_caps(Some(&caps));
            self.width = frame.width;
            self.height = frame.height;
        }
        let mut buffer = gst::Buffer::from_mut_slice(frame.bytes);
        if let Some(writable) = buffer.get_mut() {
            writable.set_pts(gst::ClockTime::from_nseconds(pts_ns));
            writable.set_duration(gst::ClockTime::from_nseconds(1_000_000_000 / 30));
        }
        self.appsrc
            .push_buffer(buffer)
            .map_err(|error| format!("encoder rejected frame: {error}"))?;
        Ok(())
    }

    fn pull(&mut self) -> Result<Option<(MediaRecord, bool)>, String> {
        let Some(sample) = self.sink.try_pull_sample(gst::ClockTime::ZERO) else {
            return Ok(None);
        };
        let caps = sample.caps().ok_or("encoded sample missing caps")?;
        if caps.structure(0).map(|s| s.name().as_str()) != Some("video/x-h264") {
            return Err("encoded sample is not H.264".into());
        }
        let buffer = sample.buffer().ok_or("encoded sample missing buffer")?;
        let data = buffer
            .map_readable()
            .map_err(|_| "cannot map encoded buffer")?;
        let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        let pts = buffer.pts().map(|pts| pts.nseconds()).unwrap_or(0);
        let record = MediaRecord::new(pts, keyframe, false, data.as_slice())
            .map_err(|error| format!("media record: {error}"))?;
        Ok(Some((record, keyframe)))
    }

    fn stop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Drop for EncoderPipeline {
    fn drop(&mut self) {
        self.stop();
    }
}

fn apply_settings(element: &gst::Element, encoder: H264Encoder) {
    let set_str = |name: &str, value: &str| {
        if element.has_property(name) {
            element.set_property_from_str(name, value);
        }
    };
    match encoder {
        H264Encoder::Nvidia => {
            set_str("preset", "p1");
            set_str("rc-mode", "cbr");
            if element.has_property("zerolatency") {
                element.set_property("zerolatency", true);
            }
            if element.has_property("bframes") {
                element.set_property("bframes", 0u32);
            }
            if element.has_property("bitrate") {
                element.set_property("bitrate", 4000u32);
            }
        }
        H264Encoder::X264 => {
            set_str("tune", "zerolatency");
            set_str("speed-preset", "ultrafast");
            if element.has_property("bitrate") {
                element.set_property("bitrate", 4000u32);
            }
            if element.has_property("bframes") {
                element.set_property("bframes", 0u32);
            }
        }
        H264Encoder::OpenH264 => {
            if element.has_property("bitrate") {
                element.set_property("bitrate", 4_000_000u32);
            }
        }
    }
}

/// Read one media record from a blocking TCP stream. Returns `None` when the
/// peer has cleanly disconnected.
pub fn read_media_record(
    stream: &mut std::net::TcpStream,
    cipher: &mut crate::transport::RecordCipher,
) -> Result<Option<MediaRecord>, String> {
    match crate::media_transport::read_record(stream, cipher) {
        Ok(record) => Ok(Some(record)),
        Err(crate::media_transport::MediaError::Truncated) => Ok(None),
        Err(error) => Err(format!("media read failed: {error}")),
    }
}

/// Write one record to a blocking TCP stream, flushing after each access unit.
pub fn write_media_record(
    stream: &mut std::net::TcpStream,
    cipher: &mut crate::transport::RecordCipher,
    record: &MediaRecord,
) -> Result<usize, String> {
    let bytes = crate::media_transport::write_record(stream, cipher, record)
        .map_err(|error| format!("media write failed: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("media flush failed: {error}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_binary_ppm_with_comment_and_trims_extra() {
        let mut bytes = b"P6\n# grim\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 99]);
        let frame = parse_ppm(&bytes).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(frame.bytes, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn rejects_bad_or_truncated_ppm() {
        assert!(parse_ppm(b"P5\n1 1\n255\nx").is_err());
        assert!(parse_ppm(b"P6\n1 1\n255\n\x00\x00").is_err());
        assert!(parse_ppm(b"P6\n0 1\n255\n").is_err());
        assert!(parse_ppm(b"P6\n1 1\n254\n\x00\x00\x00").is_err());
    }

    #[test]
    fn shared_queue_drops_oldest_and_marks_discontinuity() {
        let shared = SharedMedia::new();
        let geometry = StreamGeometry {
            width: 2,
            height: 2,
            epoch: 1,
        };
        for value in 0..(QUEUE_DEPTH + 2) {
            shared.push(
                MediaRecord::new(value as u64, true, false, vec![0, 0, 0, 1, value as u8]).unwrap(),
                geometry,
                "test",
            );
        }
        let (first, _, _) = shared.next().unwrap();
        assert!(first.discontinuity);
        assert_eq!(first.pts_ns, 2);
        assert!(shared.stats().2 > 0);
    }

    #[test]
    fn stopping_a_producer_keeps_the_media_channel_open_for_the_next_target() {
        let shared = Arc::new(SharedMedia::new());
        let mut producer = Producer {
            stop: Arc::new(AtomicBool::new(false)),
            join: None,
            target: StreamTarget::Desktop,
            shared: shared.clone(),
        };
        producer.stop();
        assert!(!shared.is_closed());

        let epoch = shared.begin();
        shared.push(
            MediaRecord::new(1, true, false, vec![0, 0, 0, 1, 0x65]).unwrap(),
            StreamGeometry {
                width: 2,
                height: 2,
                epoch,
            },
            "test",
        );
        let (record, geometry, _) = shared.next().unwrap();
        assert!(record.discontinuity);
        assert_eq!(geometry.epoch, epoch);
    }

    #[test]
    fn target_identifiers_are_stable() {
        assert_eq!(StreamTarget::Desktop.identifier(), "desktop");
        assert_eq!(StreamTarget::Workspace { id: 4 }.identifier(), "ws4");
        assert_eq!(StreamTarget::Window { id: 9 }.identifier(), "w9");
    }
}
