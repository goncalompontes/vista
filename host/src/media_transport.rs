//! Bounded media-record transport for encoded H.264 access units.
//!
//! One record carries exactly one access unit with its presentation timestamp,
//! keyframe flag and an explicit decoder-discontinuity flag. Records are sealed
//! with the session-derived ChaCha20-Poly1305 key from the authenticated control
//! handshake and framed with a big-endian length prefix.
//!
//! This is the reusable record boundary, not a production WebRTC/QUIC stack. It
//! runs over a reliable ordered byte stream; loss inside the transport is not
//! possible, but a slow consumer or a reconnect creates an explicit
//! discontinuity so the decoder waits for a fresh SPS/PPS IDR.

use crate::transport::{Direction, RecordCipher, TransportError};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fmt,
    io::{self, Read, Write},
};

pub const MAX_MEDIA_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MEDIA_HEADER_BYTES: usize = 9;
pub const AEAD_TAG_BYTES: usize = 16;
pub const MAX_MEDIA_RECORD_BYTES: usize =
    MEDIA_HEADER_BYTES + MAX_MEDIA_FRAME_BYTES + AEAD_TAG_BYTES;
pub const DEFAULT_QUEUE_DEPTH: usize = 2;

const FLAG_KEYFRAME: u8 = 0b0000_0001;
const FLAG_DISCONTINUITY: u8 = 0b0000_0010;
const KNOWN_FLAGS: u8 = FLAG_KEYFRAME | FLAG_DISCONTINUITY;

#[derive(Debug)]
pub enum MediaError {
    Io(io::Error),
    Transport(TransportError),
    FrameTooLarge,
    EmptyFrame,
    Truncated,
    InvalidFlags(u8),
    InvalidLength(usize),
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "media I/O error: {error}"),
            Self::Transport(error) => write!(formatter, "media record error: {error}"),
            Self::FrameTooLarge => {
                formatter.write_str("media access unit exceeds the 16 MiB bound")
            }
            Self::EmptyFrame => formatter.write_str("media access unit is empty"),
            Self::Truncated => formatter.write_str("media record is truncated"),
            Self::InvalidFlags(flags) => {
                write!(formatter, "unknown media record flags {flags:#04x}")
            }
            Self::InvalidLength(length) => {
                write!(formatter, "invalid media record length {length}")
            }
        }
    }
}

impl std::error::Error for MediaError {}

impl From<io::Error> for MediaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<TransportError> for MediaError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaRecord {
    pub pts_ns: u64,
    pub keyframe: bool,
    pub discontinuity: bool,
    pub bytes: Vec<u8>,
}

impl MediaRecord {
    pub fn new(
        pts_ns: u64,
        keyframe: bool,
        discontinuity: bool,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, MediaError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(MediaError::EmptyFrame);
        }
        if bytes.len() > MAX_MEDIA_FRAME_BYTES {
            return Err(MediaError::FrameTooLarge);
        }
        Ok(Self {
            pts_ns,
            keyframe,
            discontinuity,
            bytes,
        })
    }

    fn encode(&self) -> Vec<u8> {
        let mut plaintext = Vec::with_capacity(MEDIA_HEADER_BYTES + self.bytes.len());
        plaintext.extend_from_slice(&self.pts_ns.to_be_bytes());
        let mut flags = 0u8;
        if self.keyframe {
            flags |= FLAG_KEYFRAME;
        }
        if self.discontinuity {
            flags |= FLAG_DISCONTINUITY;
        }
        plaintext.push(flags);
        plaintext.extend_from_slice(&self.bytes);
        plaintext
    }

    fn decode(plaintext: &[u8]) -> Result<Self, MediaError> {
        if plaintext.len() < MEDIA_HEADER_BYTES {
            return Err(MediaError::Truncated);
        }
        let pts_ns = u64::from_be_bytes(
            plaintext[..8]
                .try_into()
                .expect("eight-byte presentation timestamp"),
        );
        let flags = plaintext[8];
        if flags & !KNOWN_FLAGS != 0 {
            return Err(MediaError::InvalidFlags(flags));
        }
        let bytes = &plaintext[MEDIA_HEADER_BYTES..];
        if bytes.is_empty() {
            return Err(MediaError::EmptyFrame);
        }
        if bytes.len() > MAX_MEDIA_FRAME_BYTES {
            return Err(MediaError::FrameTooLarge);
        }
        Ok(Self {
            pts_ns,
            keyframe: flags & FLAG_KEYFRAME != 0,
            discontinuity: flags & FLAG_DISCONTINUITY != 0,
            bytes: bytes.to_vec(),
        })
    }
}

/// Media cipher with one-access-unit plaintext headroom.
pub fn media_cipher(
    key: &[u8; 32],
    session_id: &str,
    direction: Direction,
) -> Result<RecordCipher, MediaError> {
    RecordCipher::with_limit(
        key,
        session_id,
        direction,
        MEDIA_HEADER_BYTES + MAX_MEDIA_FRAME_BYTES,
    )
    .map_err(MediaError::from)
}

/// Write one framed, sealed media record.
pub fn write_record<W: Write>(
    writer: &mut W,
    cipher: &mut RecordCipher,
    record: &MediaRecord,
) -> Result<usize, MediaError> {
    let sealed = cipher.seal(&record.encode())?;
    let length = u32::try_from(sealed.len()).map_err(|_| MediaError::FrameTooLarge)?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&sealed)?;
    writer.flush()?;
    Ok(4 + sealed.len())
}

/// Read one framed, sealed media record. A clean end of stream before a length
/// prefix is reported as `Truncated`; callers treat it as a disconnect.
pub fn read_record<R: Read>(
    reader: &mut R,
    cipher: &mut RecordCipher,
) -> Result<MediaRecord, MediaError> {
    let mut prefix = [0u8; 4];
    reader.read_exact(&mut prefix).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            MediaError::Truncated
        } else {
            MediaError::Io(error)
        }
    })?;
    let length = u32::from_be_bytes(prefix) as usize;
    if !(AEAD_TAG_BYTES..=MAX_MEDIA_RECORD_BYTES).contains(&length) {
        return Err(MediaError::InvalidLength(length));
    }
    let mut ciphertext = vec![0u8; length];
    reader.read_exact(&mut ciphertext).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            MediaError::Truncated
        } else {
            MediaError::Io(error)
        }
    })?;
    let plaintext = cipher.open(&ciphertext)?;
    MediaRecord::decode(&plaintext)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MediaQueueStats {
    pub queued: usize,
    pub sent: u64,
    pub dropped: u64,
    pub discontinuities: u64,
}

/// Bounded producer queue. On pressure the oldest access unit is dropped and the
/// next delivered unit carries an explicit discontinuity, so the consumer does
/// not silently decode deltas against lost reference frames.
pub struct MediaQueue {
    queue: VecDeque<MediaRecord>,
    capacity: usize,
    discontinuity_pending: bool,
    stats: MediaQueueStats,
}

impl MediaQueue {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            queue: VecDeque::with_capacity(capacity),
            capacity,
            discontinuity_pending: false,
            stats: MediaQueueStats::default(),
        }
    }

    pub fn push(&mut self, record: MediaRecord) -> Result<(), MediaError> {
        if record.bytes.is_empty() {
            return Err(MediaError::EmptyFrame);
        }
        if record.bytes.len() > MAX_MEDIA_FRAME_BYTES {
            return Err(MediaError::FrameTooLarge);
        }
        if self.queue.len() == self.capacity {
            self.queue.pop_front();
            self.stats.dropped = self.stats.dropped.saturating_add(1);
            self.discontinuity_pending = true;
        }
        self.queue.push_back(record);
        self.stats.queued = self.queue.len();
        Ok(())
    }

    /// Pop the next unit. If any unit was dropped since the previous pop, the
    /// returned unit is marked discontinuous and the counter advances once.
    pub fn pop(&mut self) -> Option<MediaRecord> {
        let mut record = self.queue.pop_front()?;
        if self.discontinuity_pending {
            record.discontinuity = true;
            self.discontinuity_pending = false;
            self.stats.discontinuities = self.stats.discontinuities.saturating_add(1);
        }
        self.stats.sent = self.stats.sent.saturating_add(1);
        self.stats.queued = self.queue.len();
        Some(record)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn stats(&self) -> MediaQueueStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SESSION: &str = "s1";
    const H2C: [u8; 32] = [0x11; 32];
    const C2H: [u8; 32] = [0x22; 32];

    fn ciphers() -> (RecordCipher, RecordCipher) {
        (
            media_cipher(&H2C, SESSION, Direction::HostToClient).unwrap(),
            media_cipher(&H2C, SESSION, Direction::HostToClient).unwrap(),
        )
    }

    fn record() -> MediaRecord {
        MediaRecord::new(42, true, false, vec![0, 0, 0, 1, 0x65]).unwrap()
    }

    #[test]
    fn record_round_trips_with_metadata() {
        let (mut writer, mut reader) = ciphers();
        let mut buffer = Cursor::new(Vec::new());
        let written = write_record(&mut buffer, &mut writer, &record()).unwrap();
        assert!(written > MEDIA_HEADER_BYTES);
        buffer.set_position(0);
        let decoded = read_record(&mut buffer, &mut reader).unwrap();
        assert_eq!(decoded, record());
    }

    #[test]
    fn discontinuity_flag_round_trips() {
        let (mut writer, mut reader) = ciphers();
        let mut buffer = Cursor::new(Vec::new());
        let discontinuous = MediaRecord::new(7, false, true, vec![0, 0, 0, 1, 0x41]).unwrap();
        write_record(&mut buffer, &mut writer, &discontinuous).unwrap();
        buffer.set_position(0);
        assert!(read_record(&mut buffer, &mut reader).unwrap().discontinuity);
    }

    #[test]
    fn tampered_or_wrong_direction_records_are_rejected() {
        let (mut writer, _) = ciphers();
        let mut buffer = Cursor::new(Vec::new());
        write_record(&mut buffer, &mut writer, &record()).unwrap();
        let mut bytes = buffer.into_inner();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let (_, mut reader) = ciphers();
        let error = read_record(&mut Cursor::new(bytes), &mut reader).unwrap_err();
        assert!(matches!(
            error,
            MediaError::Transport(TransportError::Crypto)
        ));

        let (mut writer, _) = ciphers();
        let mut buffer = Cursor::new(Vec::new());
        write_record(&mut buffer, &mut writer, &record()).unwrap();
        buffer.set_position(0);
        let mut wrong = media_cipher(&C2H, SESSION, Direction::HostToClient).unwrap();
        assert!(matches!(
            read_record(&mut buffer, &mut wrong),
            Err(MediaError::Transport(TransportError::Crypto))
        ));
    }

    #[test]
    fn truncated_and_invalid_records_are_rejected() {
        let (mut writer, mut reader) = ciphers();
        let mut buffer = Cursor::new(Vec::new());
        write_record(&mut buffer, &mut writer, &record()).unwrap();
        let bytes = buffer.into_inner();
        assert!(matches!(
            read_record(&mut Cursor::new(bytes[..2].to_vec()), &mut reader),
            Err(MediaError::Truncated)
        ));
        let (_, mut reader) = ciphers();
        assert!(matches!(
            read_record(&mut Cursor::new(vec![0, 0, 0, 1, 0]), &mut reader),
            Err(MediaError::InvalidLength(1))
        ));
    }

    #[test]
    fn frame_bounds_are_enforced() {
        assert!(matches!(
            MediaRecord::new(0, false, false, Vec::new()),
            Err(MediaError::EmptyFrame)
        ));
        assert!(matches!(
            MediaRecord::new(0, false, false, vec![0; MAX_MEDIA_FRAME_BYTES + 1]),
            Err(MediaError::FrameTooLarge)
        ));
        let mut queue = MediaQueue::new(1);
        assert!(queue.push(record()).is_ok());
    }

    #[test]
    fn media_cipher_accepts_larger_than_control_frames() {
        let mut writer = media_cipher(&H2C, SESSION, Direction::HostToClient).unwrap();
        let large = MediaRecord::new(1, true, false, vec![0x5a; 128 * 1024]).unwrap();
        let mut buffer = Cursor::new(Vec::new());
        write_record(&mut buffer, &mut writer, &large).unwrap();
        assert!(buffer.get_ref().len() > 128 * 1024);

        let mut control = RecordCipher::new(&H2C, SESSION, Direction::HostToClient).unwrap();
        assert!(matches!(
            control.seal(&vec![0u8; 128 * 1024]),
            Err(TransportError::RecordTooLarge)
        ));
    }

    #[test]
    fn queue_drops_oldest_and_marks_the_next_unit_discontinuous() {
        let mut queue = MediaQueue::new(2);
        queue
            .push(MediaRecord::new(1, true, false, vec![1]).unwrap())
            .unwrap();
        queue
            .push(MediaRecord::new(2, false, false, vec![2]).unwrap())
            .unwrap();
        queue
            .push(MediaRecord::new(3, false, false, vec![3]).unwrap())
            .unwrap();

        let stats = queue.stats();
        assert_eq!(stats.dropped, 1);
        assert_eq!(stats.queued, 2);

        let first = queue.pop().unwrap();
        assert_eq!(first.pts_ns, 2);
        assert!(first.discontinuity);
        let second = queue.pop().unwrap();
        assert_eq!(second.pts_ns, 3);
        assert!(!second.discontinuity);
        assert_eq!(queue.stats().discontinuities, 1);
        assert_eq!(queue.stats().sent, 2);
    }

    #[test]
    fn queue_preserves_order_without_pressure() {
        let mut queue = MediaQueue::new(DEFAULT_QUEUE_DEPTH);
        for pts in 1..=2 {
            queue
                .push(MediaRecord::new(pts, pts == 1, false, vec![pts as u8]).unwrap())
                .unwrap();
        }
        assert_eq!(queue.pop().unwrap().pts_ns, 1);
        assert_eq!(queue.pop().unwrap().pts_ns, 2);
        assert!(queue.pop().is_none());
        assert_eq!(queue.stats().dropped, 0);
        assert_eq!(queue.stats().discontinuities, 0);
    }
}
