//! Length-prefixed packet framing.
//!
//! Each packet on the wire is a `VarInt` length followed by that many bytes. The
//! protocol caps the length prefix at three bytes, so a frame can never exceed
//! [`MAX_PACKET_LEN`]. Decoding never waits for more data than the declared limit
//! allows, and rejects oversized or malformed prefixes as soon as they are seen.

use std::error::Error;
use std::fmt;

use crate::varint::write_varint;

/// Largest packet length the protocol can express: a three-byte `VarInt`.
pub const MAX_PACKET_LEN: usize = 2_097_151;

/// The length prefix of a frame is at most this many bytes.
pub const MAX_LENGTH_PREFIX_BYTES: usize = 3;

/// Errors produced while framing packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The length prefix used more than three bytes.
    LengthPrefixTooLong,
    /// The frame declared a length of zero; every packet carries at least an id.
    Empty,
    /// The frame is longer than the permitted maximum.
    TooLarge {
        /// The declared (or attempted) payload length.
        len: usize,
        /// The maximum in force.
        max: usize,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthPrefixTooLong => f.write_str("packet length prefix is longer than 3 bytes"),
            Self::Empty => f.write_str("packet has zero length"),
            Self::TooLarge { len, max } => {
                write!(f, "packet length {len} exceeds the maximum of {max}")
            }
        }
    }
}

impl Error for FrameError {}

/// One complete frame borrowed from an input buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// The frame contents (packet id followed by the body), without the length prefix.
    pub payload: &'a [u8],
    /// Total bytes consumed from the buffer, including the length prefix.
    pub consumed: usize,
}

/// Tries to read one frame from the front of `buf`.
///
/// Returns `Ok(None)` when more bytes are needed, `Ok(Some(frame))` when a whole frame
/// is available, and an error as soon as the data is provably invalid. `max_len` is
/// clamped to [`MAX_PACKET_LEN`]. An oversized declared length is rejected immediately,
/// without waiting for the payload to arrive.
pub fn decode_frame(buf: &[u8], max_len: usize) -> Result<Option<Frame<'_>>, FrameError> {
    let max = max_len.min(MAX_PACKET_LEN);

    let mut len: usize = 0;
    let mut prefix_len = 0;
    let mut complete = false;
    for index in 0..MAX_LENGTH_PREFIX_BYTES {
        let Some(&byte) = buf.get(index) else {
            return Ok(None);
        };
        len |= usize::from(byte & 0x7F) << (7 * index);
        prefix_len = index + 1;
        if byte & 0x80 == 0 {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(FrameError::LengthPrefixTooLong);
    }
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }

    let total = prefix_len + len;
    match buf.get(prefix_len..total) {
        Some(payload) => Ok(Some(Frame {
            payload,
            consumed: total,
        })),
        None => Ok(None),
    }
}

/// Appends `payload` to `out` as a length-prefixed frame.
pub fn write_frame(payload: &[u8], max_len: usize, out: &mut Vec<u8>) -> Result<(), FrameError> {
    let max = max_len.min(MAX_PACKET_LEN);
    if payload.is_empty() {
        return Err(FrameError::Empty);
    }
    if payload.len() > max {
        return Err(FrameError::TooLarge {
            len: payload.len(),
            max,
        });
    }
    // Cannot fail: `payload.len() <= MAX_PACKET_LEN`, which fits in an i32.
    write_varint(payload.len() as i32, out);
    out.extend_from_slice(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(payload, MAX_PACKET_LEN, &mut out).unwrap();
        out
    }

    #[test]
    fn decodes_a_complete_frame() {
        let buf = framed(&[0x00, 0xAA, 0xBB]);
        assert_eq!(buf, [3, 0x00, 0xAA, 0xBB]);
        let frame = decode_frame(&buf, MAX_PACKET_LEN).unwrap().unwrap();
        assert_eq!(frame.payload, [0x00, 0xAA, 0xBB]);
        assert_eq!(frame.consumed, 4);
    }

    #[test]
    fn incomplete_input_asks_for_more() {
        assert_eq!(decode_frame(&[], MAX_PACKET_LEN), Ok(None));
        assert_eq!(decode_frame(&[0x80], MAX_PACKET_LEN), Ok(None));
        assert_eq!(decode_frame(&[0x80, 0x80], MAX_PACKET_LEN), Ok(None));
        assert_eq!(decode_frame(&[3, 0, 1], MAX_PACKET_LEN), Ok(None));
    }

    #[test]
    fn consumes_only_one_frame_at_a_time() {
        let mut buf = framed(&[1, 2]);
        buf.extend(framed(&[3, 4, 5]));
        let first = decode_frame(&buf, MAX_PACKET_LEN).unwrap().unwrap();
        assert_eq!(first.payload, [1, 2]);
        let second = decode_frame(&buf[first.consumed..], MAX_PACKET_LEN)
            .unwrap()
            .unwrap();
        assert_eq!(second.payload, [3, 4, 5]);
        assert_eq!(first.consumed + second.consumed, buf.len());
    }

    #[test]
    fn oversized_length_is_rejected_without_waiting_for_data() {
        let mut buf = Vec::new();
        write_varint(1000, &mut buf);
        assert_eq!(
            decode_frame(&buf, 100),
            Err(FrameError::TooLarge {
                len: 1000,
                max: 100
            })
        );
    }

    #[test]
    fn max_len_is_clamped_to_the_protocol_limit() {
        // A 3-byte prefix cannot express more than MAX_PACKET_LEN, so a larger
        // caller-supplied maximum behaves like MAX_PACKET_LEN.
        let buf = [0xFF, 0xFF, 0x7F];
        assert_eq!(buf.len(), MAX_LENGTH_PREFIX_BYTES);
        assert_eq!(decode_frame(&buf, usize::MAX), Ok(None));
    }

    #[test]
    fn four_byte_length_prefix_is_rejected() {
        assert_eq!(
            decode_frame(&[0x80, 0x80, 0x80, 0x01, 0], MAX_PACKET_LEN),
            Err(FrameError::LengthPrefixTooLong)
        );
        // Even before the fourth byte arrives, three continuation bytes are invalid.
        assert_eq!(
            decode_frame(&[0x80, 0x80, 0x80], MAX_PACKET_LEN),
            Err(FrameError::LengthPrefixTooLong)
        );
    }

    #[test]
    fn zero_length_frame_is_rejected() {
        assert_eq!(decode_frame(&[0], MAX_PACKET_LEN), Err(FrameError::Empty));
        assert_eq!(
            write_frame(&[], MAX_PACKET_LEN, &mut Vec::new()),
            Err(FrameError::Empty)
        );
    }

    #[test]
    fn writer_enforces_the_limit() {
        let mut out = Vec::new();
        assert_eq!(
            write_frame(&[0; 11], 10, &mut out),
            Err(FrameError::TooLarge { len: 11, max: 10 })
        );
        assert!(out.is_empty());
    }

    #[test]
    fn maximum_size_frame_round_trips() {
        let payload = vec![7_u8; MAX_PACKET_LEN];
        let buf = framed(&payload);
        assert_eq!(buf.len(), MAX_PACKET_LEN + MAX_LENGTH_PREFIX_BYTES);
        let frame = decode_frame(&buf, MAX_PACKET_LEN).unwrap().unwrap();
        assert_eq!(frame.payload.len(), MAX_PACKET_LEN);
        assert_eq!(frame.consumed, buf.len());
    }

    #[test]
    fn every_truncation_of_a_valid_frame_needs_more_data() {
        let buf = framed(&[9, 8, 7, 6, 5]);
        for cut in 0..buf.len() {
            assert_eq!(
                decode_frame(&buf[..cut], MAX_PACKET_LEN),
                Ok(None),
                "cut {cut}"
            );
        }
        assert!(decode_frame(&buf, MAX_PACKET_LEN).unwrap().is_some());
    }

    #[test]
    fn arbitrary_bytes_never_panic_or_over_consume() {
        // Deterministic xorshift so failures reproduce without a fuzzing dependency.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = (next() % 24) as usize;
            let buf: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
            if let Ok(Some(frame)) = decode_frame(&buf, 64) {
                assert!(frame.consumed <= buf.len());
                assert!(!frame.payload.is_empty() && frame.payload.len() <= 64);
            }
        }
    }
}
