//! `VarInt` and `VarLong` codecs used throughout the Minecraft Java protocol.
//!
//! Both are little-endian base-128 encodings: seven payload bits per byte, with
//! the high bit set on every byte except the last. Decoding is bounded (5 bytes
//! for a `VarInt`, 10 for a `VarLong`) so malformed input cannot cause unbounded
//! work.

use std::error::Error;
use std::fmt;

/// Maximum encoded size of a `VarInt`, in bytes.
pub const VARINT_MAX_BYTES: usize = 5;

/// Maximum encoded size of a `VarLong`, in bytes.
pub const VARLONG_MAX_BYTES: usize = 10;

/// Errors produced while decoding a `VarInt` or `VarLong`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarIntError {
    /// The input ended before the final byte of the value.
    UnexpectedEof,
    /// The value used more bytes than the type allows.
    TooLong,
}

impl fmt::Display for VarIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("input ended inside a variable-length integer"),
            Self::TooLong => f.write_str("variable-length integer is too long"),
        }
    }
}

impl Error for VarIntError {}

/// Appends `value` to `out` as a `VarInt`.
pub fn write_varint(value: i32, out: &mut Vec<u8>) {
    let mut rest = value as u32;
    loop {
        let low = (rest & 0x7F) as u8;
        rest >>= 7;
        if rest == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Decodes a `VarInt` from the front of `buf`.
///
/// Returns the value and the number of bytes consumed.
pub fn read_varint(buf: &[u8]) -> Result<(i32, usize), VarIntError> {
    let mut result: u32 = 0;
    for index in 0..VARINT_MAX_BYTES {
        let byte = *buf.get(index).ok_or(VarIntError::UnexpectedEof)?;
        result |= u32::from(byte & 0x7F) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((result as i32, index + 1));
        }
    }
    Err(VarIntError::TooLong)
}

/// Appends `value` to `out` as a `VarLong`.
pub fn write_varlong(value: i64, out: &mut Vec<u8>) {
    let mut rest = value as u64;
    loop {
        let low = (rest & 0x7F) as u8;
        rest >>= 7;
        if rest == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Decodes a `VarLong` from the front of `buf`.
///
/// Returns the value and the number of bytes consumed.
pub fn read_varlong(buf: &[u8]) -> Result<(i64, usize), VarIntError> {
    let mut result: u64 = 0;
    for index in 0..VARLONG_MAX_BYTES {
        let byte = *buf.get(index).ok_or(VarIntError::UnexpectedEof)?;
        result |= u64::from(byte & 0x7F) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((result as i64, index + 1));
        }
    }
    Err(VarIntError::TooLong)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(value, &mut out);
        out
    }

    fn encode_long(value: i64) -> Vec<u8> {
        let mut out = Vec::new();
        write_varlong(value, &mut out);
        out
    }

    #[test]
    fn varint_known_encodings() {
        let cases: [(i32, &[u8]); 9] = [
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (25565, &[0xdd, 0xc7, 0x01]),
            (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
            (i32::MIN, &[0x80, 0x80, 0x80, 0x80, 0x08]),
        ];
        for (value, bytes) in cases {
            assert_eq!(encode_int(value), bytes);
            assert_eq!(read_varint(bytes), Ok((value, bytes.len())));
        }
    }

    #[test]
    fn varint_round_trips() {
        for value in [0, 1, -1, 127, 128, 16_384, i32::MAX, i32::MIN] {
            let bytes = encode_int(value);
            assert_eq!(read_varint(&bytes), Ok((value, bytes.len())));
        }
    }

    #[test]
    fn varint_ignores_trailing_bytes() {
        assert_eq!(read_varint(&[0x01, 0xff]), Ok((1, 1)));
    }

    #[test]
    fn varint_rejects_truncated_and_overlong_input() {
        assert_eq!(read_varint(&[]), Err(VarIntError::UnexpectedEof));
        assert_eq!(read_varint(&[0x80]), Err(VarIntError::UnexpectedEof));
        assert_eq!(read_varint(&[0x80; 5]), Err(VarIntError::TooLong));
        assert_eq!(read_varint(&[0xff; 6]), Err(VarIntError::TooLong));
    }

    #[test]
    fn varlong_known_encodings() {
        let max = [&[0xff_u8; 8][..], &[0x7f_u8][..]].concat();
        let minus_one = [&[0xff_u8; 9][..], &[0x01_u8][..]].concat();
        let min = [&[0x80_u8; 9][..], &[0x01_u8][..]].concat();
        assert_eq!(encode_long(i64::MAX), max);
        assert_eq!(encode_long(-1), minus_one);
        assert_eq!(encode_long(i64::MIN), min);
        assert_eq!(encode_long(128), [0x80, 0x01]);
        assert_eq!(read_varlong(&max), Ok((i64::MAX, 9)));
        assert_eq!(read_varlong(&minus_one), Ok((-1, 10)));
        assert_eq!(read_varlong(&min), Ok((i64::MIN, 10)));
    }

    #[test]
    fn varlong_rejects_truncated_and_overlong_input() {
        assert_eq!(read_varlong(&[]), Err(VarIntError::UnexpectedEof));
        assert_eq!(read_varlong(&[0x80; 9]), Err(VarIntError::UnexpectedEof));
        assert_eq!(read_varlong(&[0x80; 10]), Err(VarIntError::TooLong));
    }
}
