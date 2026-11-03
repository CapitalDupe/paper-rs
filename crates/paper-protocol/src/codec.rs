//! Bounded primitive codecs for packet bodies.
//!
//! Every length read from the wire is checked against an explicit maximum
//! before anything is allocated, so malformed input cannot cause unbounded work
//! or memory use. Multi-byte integers are big-endian, as in the protocol.

use std::error::Error;
use std::fmt;

use crate::varint::{VarIntError, read_varint, write_varint};

/// Errors produced while decoding or encoding packet bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The input ended before the value was complete.
    UnexpectedEof,
    /// A `VarInt` was malformed.
    VarInt(VarIntError),
    /// A length prefix was negative.
    NegativeLength,
    /// A length prefix exceeded the allowed maximum.
    LengthTooLarge {
        /// The length found on the wire (or being written).
        len: usize,
        /// The maximum allowed for this field.
        max: usize,
    },
    /// A string had more UTF-16 code units than the field allows.
    StringTooLong {
        /// The number of UTF-16 code units.
        chars: usize,
        /// The maximum allowed for this field.
        max: usize,
    },
    /// A string was not valid UTF-8.
    InvalidUtf8,
    /// A value was outside the set the protocol allows.
    InvalidValue(&'static str),
    /// Bytes were left over after the packet body was fully decoded.
    TrailingBytes(usize),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("input ended unexpectedly"),
            Self::VarInt(err) => write!(f, "invalid variable-length integer: {err}"),
            Self::NegativeLength => f.write_str("negative length prefix"),
            Self::LengthTooLarge { len, max } => {
                write!(f, "length {len} exceeds the maximum of {max}")
            }
            Self::StringTooLong { chars, max } => {
                write!(
                    f,
                    "string of {chars} characters exceeds the maximum of {max}"
                )
            }
            Self::InvalidUtf8 => f.write_str("string is not valid UTF-8"),
            Self::InvalidValue(what) => write!(f, "invalid value for {what}"),
            Self::TrailingBytes(count) => write!(f, "{count} unexpected trailing bytes"),
        }
    }
}

impl Error for CodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::VarInt(err) => Some(err),
            _ => None,
        }
    }
}

impl From<VarIntError> for CodecError {
    fn from(err: VarIntError) -> Self {
        Self::VarInt(err)
    }
}

/// A cursor over a packet body that decodes values with explicit bounds.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader positioned at the start of `buf`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Number of bytes not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CodecError> {
        if count > self.remaining() {
            return Err(CodecError::UnexpectedEof);
        }
        let slice = &self.buf[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }

    /// Reads a fixed-size array of bytes.
    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let slice = self.take(N)?;
        let mut array = [0_u8; N];
        array.copy_from_slice(slice);
        Ok(array)
    }

    /// Reads one byte.
    pub fn read_u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.read_array::<1>()?[0])
    }

    /// Reads a boolean; only `0` and `1` are accepted.
    pub fn read_bool(&mut self) -> Result<bool, CodecError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::InvalidValue("boolean")),
        }
    }

    /// Reads a big-endian unsigned 16-bit integer.
    pub fn read_u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_be_bytes(self.read_array()?))
    }

    /// Reads a big-endian signed 64-bit integer.
    pub fn read_i64(&mut self) -> Result<i64, CodecError> {
        Ok(i64::from_be_bytes(self.read_array()?))
    }

    /// Reads a `VarInt`.
    pub fn read_varint(&mut self) -> Result<i32, CodecError> {
        let (value, used) = read_varint(&self.buf[self.pos..])?;
        self.pos += used;
        Ok(value)
    }

    /// Reads a `VarInt` length prefix, rejecting negative values and values above `max`.
    pub fn read_len(&mut self, max: usize) -> Result<usize, CodecError> {
        let value = self.read_varint()?;
        let len = usize::try_from(value).map_err(|_| CodecError::NegativeLength)?;
        if len > max {
            return Err(CodecError::LengthTooLarge { len, max });
        }
        Ok(len)
    }

    /// Reads a length-prefixed byte array of at most `max` bytes.
    pub fn read_bytes(&mut self, max: usize) -> Result<&'a [u8], CodecError> {
        let len = self.read_len(max)?;
        self.take(len)
    }

    /// Reads a length-prefixed UTF-8 string of at most `max_chars` UTF-16 code units.
    ///
    /// The protocol limits strings by UTF-16 code units, while the length prefix counts
    /// UTF-8 bytes. A code unit takes at most three UTF-8 bytes, so the byte length is
    /// bounded by `3 * max_chars` before the data is touched.
    pub fn read_string(&mut self, max_chars: usize) -> Result<String, CodecError> {
        let bytes = self.read_bytes(max_chars.saturating_mul(3))?;
        let text = std::str::from_utf8(bytes).map_err(|_| CodecError::InvalidUtf8)?;
        let chars = text.encode_utf16().count();
        if chars > max_chars {
            return Err(CodecError::StringTooLong {
                chars,
                max: max_chars,
            });
        }
        Ok(text.to_owned())
    }

    /// Consumes and returns everything that is left.
    pub fn read_rest(&mut self) -> &'a [u8] {
        let slice = &self.buf[self.pos..];
        self.pos = self.buf.len();
        slice
    }

    /// Succeeds only if every byte has been consumed.
    pub fn finish(self) -> Result<(), CodecError> {
        match self.remaining() {
            0 => Ok(()),
            count => Err(CodecError::TrailingBytes(count)),
        }
    }
}

/// Appends a boolean as one byte.
pub fn write_bool(value: bool, out: &mut Vec<u8>) {
    out.push(u8::from(value));
}

/// Appends a big-endian unsigned 16-bit integer.
pub fn write_u16(value: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian signed 64-bit integer.
pub fn write_i64(value: i64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_len(len: usize, max: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    if len > max {
        return Err(CodecError::LengthTooLarge { len, max });
    }
    let value = i32::try_from(len).map_err(|_| CodecError::LengthTooLarge { len, max })?;
    write_varint(value, out);
    Ok(())
}

/// Appends a length-prefixed byte array, refusing anything longer than `max` bytes.
pub fn write_bytes(bytes: &[u8], max: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    write_len(bytes.len(), max, out)?;
    out.extend_from_slice(bytes);
    Ok(())
}

/// Appends a length-prefixed string, refusing anything over `max_chars` UTF-16 code units.
pub fn write_string(text: &str, max_chars: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    let chars = text.encode_utf16().count();
    if chars > max_chars {
        return Err(CodecError::StringTooLong {
            chars,
            max: max_chars,
        });
    }
    write_bytes(text.as_bytes(), max_chars.saturating_mul(3), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_big_endian() {
        let mut out = Vec::new();
        write_u16(25565, &mut out);
        write_i64(0x0102_0304_0506_0708, &mut out);
        assert_eq!(out, [0x63, 0xDD, 1, 2, 3, 4, 5, 6, 7, 8]);

        let mut reader = Reader::new(&out);
        assert_eq!(reader.read_u16(), Ok(25565));
        assert_eq!(reader.read_i64(), Ok(0x0102_0304_0506_0708));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn booleans_accept_only_zero_and_one() {
        assert_eq!(Reader::new(&[0]).read_bool(), Ok(false));
        assert_eq!(Reader::new(&[1]).read_bool(), Ok(true));
        assert_eq!(
            Reader::new(&[2]).read_bool(),
            Err(CodecError::InvalidValue("boolean"))
        );
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        assert_eq!(Reader::new(&[]).read_u8(), Err(CodecError::UnexpectedEof));
        assert_eq!(Reader::new(&[1]).read_u16(), Err(CodecError::UnexpectedEof));
        assert_eq!(
            Reader::new(&[1, 2, 3]).read_i64(),
            Err(CodecError::UnexpectedEof)
        );
    }

    #[test]
    fn string_round_trip() {
        let mut out = Vec::new();
        write_string("localhost", 255, &mut out).unwrap();
        assert_eq!(out[0], 9);
        assert_eq!(&out[1..], b"localhost");
        let mut reader = Reader::new(&out);
        assert_eq!(reader.read_string(255).unwrap(), "localhost");
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn string_limits_count_utf16_code_units_not_bytes() {
        // "é" is 2 bytes but 1 code unit; the emoji is 4 bytes and 2 code units.
        let mut out = Vec::new();
        write_string("é", 1, &mut out).unwrap();
        assert_eq!(Reader::new(&out).read_string(1).unwrap(), "é");

        let mut emoji = Vec::new();
        assert_eq!(
            write_string("😀", 1, &mut emoji),
            Err(CodecError::StringTooLong { chars: 2, max: 1 })
        );
        let mut ok = Vec::new();
        write_string("😀", 2, &mut ok).unwrap();
        assert_eq!(Reader::new(&ok).read_string(2).unwrap(), "😀");
        // Four bytes exceed the 3-bytes-per-code-unit bound for a limit of 1, so the
        // reader rejects it on the byte length before looking at the data.
        assert_eq!(
            Reader::new(&ok).read_string(1),
            Err(CodecError::LengthTooLarge { len: 4, max: 3 })
        );

        // Within the byte bound but over the character limit: caught by the code-unit count.
        let mut abc = Vec::new();
        write_string("abc", 3, &mut abc).unwrap();
        assert_eq!(
            Reader::new(&abc).read_string(2),
            Err(CodecError::StringTooLong { chars: 3, max: 2 })
        );
    }

    #[test]
    fn oversized_length_prefix_is_rejected_before_reading_data() {
        // Declares 1,000,000 bytes but supplies none; must fail on the length, not on EOF.
        let mut out = Vec::new();
        write_varint(1_000_000, &mut out);
        assert_eq!(
            Reader::new(&out).read_string(16),
            Err(CodecError::LengthTooLarge {
                len: 1_000_000,
                max: 48
            })
        );
        assert_eq!(
            Reader::new(&out).read_bytes(100),
            Err(CodecError::LengthTooLarge {
                len: 1_000_000,
                max: 100
            })
        );
    }

    #[test]
    fn negative_length_prefix_is_rejected() {
        let mut out = Vec::new();
        write_varint(-1, &mut out);
        assert_eq!(
            Reader::new(&out).read_bytes(100),
            Err(CodecError::NegativeLength)
        );
    }

    #[test]
    fn declared_length_longer_than_input_is_eof() {
        assert_eq!(
            Reader::new(&[5, b'a', b'b']).read_string(16),
            Err(CodecError::UnexpectedEof)
        );
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        assert_eq!(
            Reader::new(&[2, 0xC3, 0x28]).read_string(16),
            Err(CodecError::InvalidUtf8)
        );
    }

    #[test]
    fn writers_enforce_their_limits() {
        let mut out = Vec::new();
        assert_eq!(
            write_bytes(&[0; 5], 4, &mut out),
            Err(CodecError::LengthTooLarge { len: 5, max: 4 })
        );
        assert!(out.is_empty());
    }

    #[test]
    fn finish_reports_trailing_bytes() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(reader.read_u8(), Ok(1));
        assert_eq!(reader.finish(), Err(CodecError::TrailingBytes(2)));
    }

    #[test]
    fn varint_errors_are_wrapped() {
        assert_eq!(
            Reader::new(&[0x80]).read_varint(),
            Err(CodecError::VarInt(VarIntError::UnexpectedEof))
        );
    }
}
