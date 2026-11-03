//! Error types for packet decoding and encoding.

use std::error::Error;
use std::fmt;

use crate::varint::VarIntError;

/// Errors produced while decoding a packet body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The packet ended before all fields were read.
    UnexpectedEof,
    /// A `VarInt` field was malformed.
    VarInt(VarIntError),
    /// A length prefix was negative or otherwise invalid.
    InvalidLength(i32),
    /// A string exceeded its character limit.
    StringTooLong(usize),
    /// A byte array exceeded its size limit.
    ByteArrayTooLong(usize),
    /// A list had more items than allowed.
    TooManyItems(usize),
    /// A string was not valid UTF-8.
    InvalidUtf8,
    /// A field held a value outside its allowed set: `(field, value)`.
    InvalidValue(&'static str, i64),
    /// Bytes were left over after the last field.
    TrailingBytes(usize),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("unexpected end of packet"),
            Self::VarInt(err) => write!(f, "bad variable-length integer: {err}"),
            Self::InvalidLength(len) => write!(f, "invalid length prefix: {len}"),
            Self::StringTooLong(max) => write!(f, "string exceeds {max} chars"),
            Self::ByteArrayTooLong(max) => write!(f, "byte array exceeds {max} bytes"),
            Self::TooManyItems(max) => write!(f, "list exceeds {max} items"),
            Self::InvalidUtf8 => f.write_str("string is not valid UTF-8"),
            Self::InvalidValue(field, value) => write!(f, "invalid {field}: {value}"),
            Self::TrailingBytes(count) => write!(f, "{count} unread bytes after packet"),
        }
    }
}

impl Error for DecodeError {}

impl From<VarIntError> for DecodeError {
    fn from(err: VarIntError) -> Self {
        Self::VarInt(err)
    }
}

/// Errors produced while encoding a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A string exceeded its character limit.
    StringTooLong(usize),
    /// A byte array exceeded its size limit.
    ByteArrayTooLong(usize),
    /// A list had more items than allowed.
    TooManyItems(usize),
    /// A length did not fit in a `VarInt`.
    LengthOverflow,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StringTooLong(max) => write!(f, "string exceeds {max} chars"),
            Self::ByteArrayTooLong(max) => write!(f, "byte array exceeds {max} bytes"),
            Self::TooManyItems(max) => write!(f, "list exceeds {max} items"),
            Self::LengthOverflow => f.write_str("length does not fit in a VarInt"),
        }
    }
}

impl Error for EncodeError {}
