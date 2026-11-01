//! Length-prefixed packet framing.
//!
//! Every packet on the wire is a `VarInt` length followed by that many payload
//! bytes. The decoder validates the length before any payload is buffered, so a
//! peer cannot force a large allocation by announcing a huge packet.

use std::error::Error;
use std::fmt;

use crate::varint;

/// Largest frame payload the protocol allows (the maximum of a three-byte `VarInt`).
pub const MAX_FRAME_LEN: usize = 2_097_151;

/// Errors produced while framing or unframing packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The length prefix was not a valid `VarInt`.
    BadLength(varint::VarIntError),
    /// The length prefix was negative.
    NegativeLength,
    /// The frame has no payload, but every packet needs at least a packet id.
    Empty,
    /// The frame length exceeds the limit: `(length, limit)`.
    TooLarge(usize, usize),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength(err) => write!(f, "invalid frame length: {err}"),
            Self::NegativeLength => f.write_str("frame length is negative"),
            Self::Empty => f.write_str("frame has no payload"),
            Self::TooLarge(len, max) => write!(f, "frame of {len} bytes exceeds {max}"),
        }
    }
}

impl Error for FrameError {}

/// Attempts to split one frame from the front of `buf`.
///
/// Returns `Ok(None)` when more bytes are needed, or `Ok(Some((payload, used)))`
/// where `used` counts the length prefix as well. The effective limit is the
/// smaller of `max_len` and [`MAX_FRAME_LEN`]. An oversized length is rejected as
/// soon as the prefix is read, before the payload arrives.
pub fn decode_frame(buf: &[u8], max_len: usize) -> Result<Option<(&[u8], usize)>, FrameError> {
    let max = max_len.min(MAX_FRAME_LEN);
    let (raw, prefix_len) = match varint::read_varint(buf) {
        Ok(parsed) => parsed,
        Err(varint::VarIntError::UnexpectedEof) => return Ok(None),
        Err(err) => return Err(FrameError::BadLength(err)),
    };
    let Ok(len) = usize::try_from(raw) else {
        return Err(FrameError::NegativeLength);
    };
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge(len, max));
    }
    let end = prefix_len + len;
    Ok(buf.get(prefix_len..end).map(|payload| (payload, end)))
}

/// Appends `payload` to `out` with its length prefix.
pub fn encode_frame(payload: &[u8], max_len: usize, out: &mut Vec<u8>) -> Result<(), FrameError> {
    let max = max_len.min(MAX_FRAME_LEN);
    if payload.is_empty() {
        return Err(FrameError::Empty);
    }
    if payload.len() > max {
        return Err(FrameError::TooLarge(payload.len(), max));
    }
    let Ok(len) = i32::try_from(payload.len()) else {
        return Err(FrameError::TooLarge(payload.len(), max));
    };
    varint::write_varint(len, out);
    out.extend_from_slice(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_frame() {
        let mut wire = Vec::new();
        encode_frame(&[0x00, 1, 2, 3], 1024, &mut wire).unwrap();
        assert_eq!(wire, [4, 0x00, 1, 2, 3]);
        let (payload, used) = decode_frame(&wire, 1024).unwrap().unwrap();
        assert_eq!(payload, [0x00, 1, 2, 3]);
        assert_eq!(used, 5);
    }

    #[test]
    fn decodes_back_to_back_frames() {
        let wire = [2, 0xaa, 0xbb, 1, 0xcc];
        let (first, used) = decode_frame(&wire, 1024).unwrap().unwrap();
        assert_eq!(first, [0xaa, 0xbb]);
        let (second, rest) = decode_frame(&wire[used..], 1024).unwrap().unwrap();
        assert_eq!(second, [0xcc]);
        assert_eq!(used + rest, wire.len());
    }

    #[test]
    fn waits_for_more_bytes() {
        assert_eq!(decode_frame(&[], 1024), Ok(None));
        assert_eq!(decode_frame(&[0x80], 1024), Ok(None));
        assert_eq!(decode_frame(&[5, 1, 2], 1024), Ok(None));
    }

    #[test]
    fn rejects_oversized_length_before_payload_arrives() {
        let result = decode_frame(&[0x80, 0x80, 0x01], 1024);
        assert_eq!(result, Err(FrameError::TooLarge(16_384, 1024)));
    }

    #[test]
    fn enforces_the_global_cap() {
        let mut wire = Vec::new();
        varint::write_varint(2_097_152, &mut wire);
        let result = decode_frame(&wire, usize::MAX);
        assert_eq!(result, Err(FrameError::TooLarge(2_097_152, MAX_FRAME_LEN)));
    }

    #[test]
    fn rejects_malformed_lengths() {
        let negative = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert_eq!(decode_frame(&negative, 1024), Err(FrameError::NegativeLength));
        assert_eq!(decode_frame(&[0x00], 1024), Err(FrameError::Empty));
        let overlong = [0x80; 5];
        let expected = FrameError::BadLength(varint::VarIntError::TooLong);
        assert_eq!(decode_frame(&overlong, 1024), Err(expected));
    }

    #[test]
    fn encode_rejects_empty_and_oversized_payloads() {
        let mut wire = Vec::new();
        assert_eq!(encode_frame(&[], 16, &mut wire), Err(FrameError::Empty));
        let payload = [0_u8; 17];
        let result = encode_frame(&payload, 16, &mut wire);
        assert_eq!(result, Err(FrameError::TooLarge(17, 16)));
        assert!(wire.is_empty());
    }
}
