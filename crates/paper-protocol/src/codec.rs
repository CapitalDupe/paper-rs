//! Bounds-checked reading and bounded writing of protocol primitives.

use crate::error::{DecodeError, EncodeError};
use crate::varint;

/// A bounds-checked cursor over a packet payload.
///
/// Every read validates the remaining length first, and variable-length data
/// (strings, byte arrays) takes an explicit limit so untrusted input cannot
/// trigger large allocations.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Creates a reader over `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// Number of unread bytes.
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    /// Fails if any bytes are left unread.
    pub fn finish(self) -> Result<(), DecodeError> {
        match self.buf.len() {
            0 => Ok(()),
            count => Err(DecodeError::TrailingBytes(count)),
        }
    }

    /// Reads exactly `len` bytes.
    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.buf.len() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        let (head, tail) = self.buf.split_at(len);
        self.buf = tail;
        Ok(head)
    }

    /// Reads one byte.
    pub fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let (&byte, rest) = self.buf.split_first().ok_or(DecodeError::UnexpectedEof)?;
        self.buf = rest;
        Ok(byte)
    }

    /// Reads a boolean, rejecting any byte other than 0 or 1.
    pub fn read_bool(&mut self) -> Result<bool, DecodeError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(DecodeError::InvalidValue("bool", i64::from(other))),
        }
    }

    /// Reads a big-endian `u16`.
    pub fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a big-endian `i64`.
    pub fn read_i64(&mut self) -> Result<i64, DecodeError> {
        let bytes = self.read_bytes(8)?;
        let array: [u8; 8] = bytes.try_into().map_err(|_| DecodeError::UnexpectedEof)?;
        Ok(i64::from_be_bytes(array))
    }

    /// Reads a `VarInt`.
    pub fn read_varint(&mut self) -> Result<i32, DecodeError> {
        let (value, used) = varint::read_varint(self.buf)?;
        self.buf = &self.buf[used..];
        Ok(value)
    }

    /// Reads a `VarInt` that must be a non-negative length.
    pub fn read_length(&mut self) -> Result<usize, DecodeError> {
        let raw = self.read_varint()?;
        let Ok(len) = usize::try_from(raw) else {
            return Err(DecodeError::InvalidLength(raw));
        };
        Ok(len)
    }

    /// Reads a length-prefixed UTF-8 string of at most `max_chars` characters.
    ///
    /// Characters are counted as Unicode scalar values. The byte length is
    /// checked first (at most three bytes per character is accepted), so an
    /// oversized prefix is rejected before any data is read.
    pub fn read_string(&mut self, max_chars: usize) -> Result<String, DecodeError> {
        let len = self.read_length()?;
        if len > max_chars.saturating_mul(3) {
            return Err(DecodeError::StringTooLong(max_chars));
        }
        let bytes = self.read_bytes(len)?;
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err(DecodeError::InvalidUtf8);
        };
        if text.chars().count() > max_chars {
            return Err(DecodeError::StringTooLong(max_chars));
        }
        Ok(text.to_owned())
    }

    /// Reads a length-prefixed byte array of at most `max_len` bytes.
    pub fn read_byte_array(&mut self, max_len: usize) -> Result<Vec<u8>, DecodeError> {
        let len = self.read_length()?;
        if len > max_len {
            return Err(DecodeError::ByteArrayTooLong(max_len));
        }
        Ok(self.read_bytes(len)?.to_vec())
    }
}

/// Appends a length as a `VarInt`.
pub fn write_length(out: &mut Vec<u8>, len: usize) -> Result<(), EncodeError> {
    let Ok(value) = i32::try_from(len) else {
        return Err(EncodeError::LengthOverflow);
    };
    varint::write_varint(value, out);
    Ok(())
}

/// Appends a length-prefixed UTF-8 string, enforcing a character limit.
pub fn write_string(out: &mut Vec<u8>, text: &str, max_chars: usize) -> Result<(), EncodeError> {
    if text.chars().count() > max_chars {
        return Err(EncodeError::StringTooLong(max_chars));
    }
    write_length(out, text.len())?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Appends a length-prefixed byte array, enforcing a size limit.
pub fn write_byte_array(out: &mut Vec<u8>, data: &[u8], max_len: usize) -> Result<(), EncodeError> {
    if data.len() > max_len {
        return Err(EncodeError::ByteArrayTooLong(max_len));
    }
    write_length(out, data.len())?;
    out.extend_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_primitives_in_order() {
        let bytes = [&[0x01, 0x63, 0xdd][..], &7_i64.to_be_bytes()[..]].concat();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_bool(), Ok(true));
        assert_eq!(reader.read_u16(), Ok(25565));
        assert_eq!(reader.read_i64(), Ok(7));
        assert_eq!(reader.remaining(), 0);
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn rejects_bad_bool_and_truncated_input() {
        let mut reader = Reader::new(&[2]);
        assert_eq!(
            reader.read_bool(),
            Err(DecodeError::InvalidValue("bool", 2))
        );
        let mut reader = Reader::new(&[1, 2, 3]);
        assert_eq!(reader.read_i64(), Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn reports_trailing_bytes() {
        assert_eq!(
            Reader::new(&[1, 2]).finish(),
            Err(DecodeError::TrailingBytes(2))
        );
    }

    #[test]
    fn string_round_trip_and_limits() {
        let mut out = Vec::new();
        write_string(&mut out, "héllo", 5).unwrap();
        assert_eq!(Reader::new(&out).read_string(5), Ok("héllo".to_owned()));
        assert_eq!(Reader::new(&out).read_string(4), Err(DecodeError::StringTooLong(4)));
        let mut sink = Vec::new();
        assert_eq!(write_string(&mut sink, "héllo", 4), Err(EncodeError::StringTooLong(4)));
        assert!(sink.is_empty());
    }

    #[test]
    fn string_rejects_bad_input() {
        let mut reader = Reader::new(&[2, 0xff, 0xfe]);
        assert_eq!(reader.read_string(8), Err(DecodeError::InvalidUtf8));
        let negative = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert_eq!(
            Reader::new(&negative).read_string(8),
            Err(DecodeError::InvalidLength(-1))
        );
        let mut reader = Reader::new(&[5, b'a', b'b']);
        assert_eq!(reader.read_string(8), Err(DecodeError::UnexpectedEof));
    }

    #[test]
    fn byte_array_limits() {
        let mut out = Vec::new();
        write_byte_array(&mut out, &[1, 2, 3], 4).unwrap();
        assert_eq!(Reader::new(&out).read_byte_array(4), Ok(vec![1, 2, 3]));
        let mut oversized = Vec::new();
        varint::write_varint(300, &mut oversized);
        let result = Reader::new(&oversized).read_byte_array(256);
        assert_eq!(result, Err(DecodeError::ByteArrayTooLong(256)));
        let mut sink = Vec::new();
        let result = write_byte_array(&mut sink, &[0; 5], 4);
        assert_eq!(result, Err(EncodeError::ByteArrayTooLong(4)));
    }
}
