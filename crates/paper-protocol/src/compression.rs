//! Packet compression (zlib), as negotiated by Set Compression.
//!
//! Once compression is enabled, each frame payload starts with a `VarInt` data
//! length: `0` means the rest is uncompressed, otherwise it is the packet size
//! after decompression. Decompression is capped so a small input cannot expand
//! into an unbounded buffer.

use std::error::Error;
use std::fmt;
use std::io::{Read, Write};

use crate::frame::MAX_FRAME_LEN;
use crate::varint;

/// Errors produced while compressing or decompressing a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionError {
    /// The data length prefix was malformed.
    BadDataLength(varint::VarIntError),
    /// The data length prefix was negative.
    NegativeDataLength,
    /// The declared or actual size exceeds the protocol maximum.
    TooLarge(usize),
    /// A compressed packet was smaller than the threshold: `(size, threshold)`.
    BelowThreshold(usize, usize),
    /// The inflated size did not match the declared size.
    SizeMismatch,
    /// The compressed data was not valid zlib.
    Inflate,
    /// Compression failed.
    Deflate,
}

impl fmt::Display for CompressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadDataLength(err) => write!(f, "invalid data length: {err}"),
            Self::NegativeDataLength => f.write_str("data length is negative"),
            Self::TooLarge(len) => write!(f, "packet of {len} bytes is too large"),
            Self::BelowThreshold(len, min) => write!(f, "{len} bytes is below {min}"),
            Self::SizeMismatch => f.write_str("inflated size differs from declared size"),
            Self::Inflate => f.write_str("invalid compressed data"),
            Self::Deflate => f.write_str("compression failed"),
        }
    }
}

impl Error for CompressionError {}

fn deflate_err<E>(_: E) -> CompressionError {
    CompressionError::Deflate
}

fn inflate_err<E>(_: E) -> CompressionError {
    CompressionError::Inflate
}

/// Builds the compressed-mode frame payload for `packet` (packet id and body).
///
/// Packets smaller than `threshold` are stored uncompressed with a data length
/// of zero.
pub fn compress(packet: &[u8], threshold: usize) -> Result<Vec<u8>, CompressionError> {
    let len = packet.len();
    if len > MAX_FRAME_LEN {
        return Err(CompressionError::TooLarge(len));
    }
    let mut out = Vec::new();
    if len < threshold {
        varint::write_varint(0, &mut out);
        out.extend_from_slice(packet);
        return Ok(out);
    }
    let Ok(declared) = i32::try_from(len) else {
        return Err(CompressionError::TooLarge(len));
    };
    varint::write_varint(declared, &mut out);
    let mut encoder = flate2::write::ZlibEncoder::new(out, flate2::Compression::default());
    encoder.write_all(packet).map_err(deflate_err)?;
    encoder.finish().map_err(deflate_err)
}

/// Recovers the packet (id and body) from a compressed-mode frame payload.
pub fn decompress(payload: &[u8], threshold: usize) -> Result<Vec<u8>, CompressionError> {
    let (declared, used) = match varint::read_varint(payload) {
        Ok(parsed) => parsed,
        Err(err) => return Err(CompressionError::BadDataLength(err)),
    };
    let rest = &payload[used..];
    if declared == 0 {
        return Ok(rest.to_vec());
    }
    let Ok(expected) = usize::try_from(declared) else {
        return Err(CompressionError::NegativeDataLength);
    };
    if expected > MAX_FRAME_LEN {
        return Err(CompressionError::TooLarge(expected));
    }
    if expected < threshold {
        return Err(CompressionError::BelowThreshold(expected, threshold));
    }
    let limit = expected as u64 + 1;
    let mut inflater = flate2::read::ZlibDecoder::new(rest).take(limit);
    let mut out = Vec::with_capacity(expected);
    inflater.read_to_end(&mut out).map_err(inflate_err)?;
    if out.len() != expected {
        return Err(CompressionError::SizeMismatch);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_packets_are_stored_uncompressed() {
        let packed = compress(&[1, 2, 3], 256).unwrap();
        assert_eq!(packed, [0, 1, 2, 3]);
        assert_eq!(decompress(&packed, 256), Ok(vec![1, 2, 3]));
    }

    #[test]
    fn large_packets_round_trip_compressed() {
        let data = vec![7_u8; 1000];
        let packed = compress(&data, 256).unwrap();
        assert!(packed.len() < data.len());
        assert_eq!(decompress(&packed, 256), Ok(data));
    }

    #[test]
    fn rejects_bad_data_lengths() {
        let mut payload = Vec::new();
        varint::write_varint(2_097_152, &mut payload);
        assert_eq!(
            decompress(&payload, 256),
            Err(CompressionError::TooLarge(2_097_152))
        );

        let mut payload = Vec::new();
        varint::write_varint(-1, &mut payload);
        assert_eq!(
            decompress(&payload, 256),
            Err(CompressionError::NegativeDataLength)
        );

        let mut payload = Vec::new();
        varint::write_varint(10, &mut payload);
        assert_eq!(
            decompress(&payload, 256),
            Err(CompressionError::BelowThreshold(10, 256))
        );

        let result = decompress(&[0x80; 5], 256);
        let expected = CompressionError::BadDataLength(varint::VarIntError::TooLong);
        assert_eq!(result, Err(expected));
    }

    #[test]
    fn rejects_size_mismatch() {
        let packed = compress(&[1_u8; 300], 256).unwrap();
        let mut tampered = Vec::new();
        varint::write_varint(301, &mut tampered);
        tampered.extend_from_slice(&packed[2..]);
        assert_eq!(
            decompress(&tampered, 256),
            Err(CompressionError::SizeMismatch)
        );
    }

    #[test]
    fn rejects_invalid_zlib_data() {
        let mut payload = Vec::new();
        varint::write_varint(300, &mut payload);
        payload.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(decompress(&payload, 256), Err(CompressionError::Inflate));
    }
}
