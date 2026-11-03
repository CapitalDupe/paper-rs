//! Packet compression framing.
//!
//! Once the server sends `SetCompression`, every frame body changes shape. After the
//! frame length prefix comes a `VarInt` *data length*:
//!
//! - `0` means the rest of the body is the packet (id and data), uncompressed. Packets
//!   smaller than the threshold are sent this way.
//! - Any other value is the uncompressed size of the packet, and the rest of the body
//!   is that packet compressed with zlib.
//!
//! This module implements those rules and their limits. The zlib codec itself is
//! supplied through the [`Zlib`] trait so the framing logic can be tested without it;
//! `FlateZlib` (feature `zlib`) is the production implementation.

use std::error::Error;
use std::fmt;

use crate::codec::{CodecError, Reader};
use crate::varint::write_varint;

/// Largest uncompressed packet accepted or produced, in bytes (8 MiB).
pub const MAX_UNCOMPRESSED_LEN: usize = 8 * 1024 * 1024;

/// A failure inside the zlib codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZlibError(pub String);

impl fmt::Display for ZlibError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "zlib error: {}", self.0)
    }
}

impl Error for ZlibError {}

/// A zlib compressor and bounded decompressor.
pub trait Zlib {
    /// Compresses `input` into a zlib stream.
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, ZlibError>;

    /// Decompresses a zlib stream that is expected to hold exactly `expected_len` bytes.
    ///
    /// Implementations must stop and return an error rather than produce more than
    /// `expected_len` bytes, so a small hostile input cannot expand without limit.
    fn decompress(&self, input: &[u8], expected_len: usize) -> Result<Vec<u8>, ZlibError>;
}

/// Errors produced while compressing or decompressing a frame body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressionError {
    /// The packet was empty; every packet carries at least an id.
    Empty,
    /// The sender compressed a packet that was below the compression threshold.
    BelowThreshold {
        /// The declared uncompressed size.
        declared: usize,
        /// The threshold in force.
        threshold: usize,
    },
    /// The uncompressed size exceeds [`MAX_UNCOMPRESSED_LEN`].
    TooLarge {
        /// The declared (or actual) uncompressed size.
        len: usize,
        /// The maximum allowed.
        max: usize,
    },
    /// The data decompressed to a different size than the sender declared.
    LengthMismatch {
        /// The declared uncompressed size.
        declared: usize,
        /// The size actually produced.
        actual: usize,
    },
    /// The data length prefix was malformed.
    Codec(CodecError),
    /// The zlib codec failed.
    Zlib(ZlibError),
}

impl fmt::Display for CompressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("compressed frame holds an empty packet"),
            Self::BelowThreshold {
                declared,
                threshold,
            } => write!(
                f,
                "packet of {declared} bytes was compressed but is below the threshold of {threshold}"
            ),
            Self::TooLarge { len, max } => {
                write!(
                    f,
                    "uncompressed packet of {len} bytes exceeds the maximum of {max}"
                )
            }
            Self::LengthMismatch { declared, actual } => write!(
                f,
                "packet declared {declared} uncompressed bytes but held {actual}"
            ),
            Self::Codec(err) => write!(f, "malformed compression header: {err}"),
            Self::Zlib(err) => err.fmt(f),
        }
    }
}

impl Error for CompressionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Codec(err) => Some(err),
            Self::Zlib(err) => Some(err),
            _ => None,
        }
    }
}

impl From<CodecError> for CompressionError {
    fn from(err: CodecError) -> Self {
        Self::Codec(err)
    }
}

impl From<ZlibError> for CompressionError {
    fn from(err: ZlibError) -> Self {
        Self::Zlib(err)
    }
}

/// Builds a frame body from a packet payload (id and data) when compression is on.
///
/// Payloads shorter than `threshold` are sent raw behind a data length of `0`; larger
/// ones are compressed. The returned bytes are what goes inside the frame, after its
/// length prefix.
pub fn compress_body<Z: Zlib>(
    zlib: &Z,
    threshold: usize,
    payload: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), CompressionError> {
    if payload.is_empty() {
        return Err(CompressionError::Empty);
    }
    if payload.len() > MAX_UNCOMPRESSED_LEN {
        return Err(CompressionError::TooLarge {
            len: payload.len(),
            max: MAX_UNCOMPRESSED_LEN,
        });
    }
    if payload.len() < threshold {
        write_varint(0, out);
        out.extend_from_slice(payload);
    } else {
        let compressed = zlib.compress(payload)?;
        // Cannot truncate: `payload.len() <= MAX_UNCOMPRESSED_LEN`.
        write_varint(payload.len() as i32, out);
        out.extend_from_slice(&compressed);
    }
    Ok(())
}

/// Recovers the packet payload (id and data) from a frame body when compression is on.
///
/// Enforces the protocol's rules: a compressed packet must be at least `threshold` bytes
/// when uncompressed, must not exceed [`MAX_UNCOMPRESSED_LEN`], and must decompress to
/// exactly the size it declared.
pub fn decompress_body<Z: Zlib>(
    zlib: &Z,
    threshold: usize,
    body: &[u8],
) -> Result<Vec<u8>, CompressionError> {
    let mut reader = Reader::new(body);
    let declared = reader.read_varint()?;
    let declared = usize::try_from(declared).map_err(|_| CodecError::NegativeLength)?;

    if declared == 0 {
        let rest = reader.read_rest();
        if rest.is_empty() {
            return Err(CompressionError::Empty);
        }
        return Ok(rest.to_vec());
    }
    if declared < threshold {
        return Err(CompressionError::BelowThreshold {
            declared,
            threshold,
        });
    }
    if declared > MAX_UNCOMPRESSED_LEN {
        return Err(CompressionError::TooLarge {
            len: declared,
            max: MAX_UNCOMPRESSED_LEN,
        });
    }

    let payload = zlib.decompress(reader.read_rest(), declared)?;
    if payload.len() != declared {
        return Err(CompressionError::LengthMismatch {
            declared,
            actual: payload.len(),
        });
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{MAX_PACKET_LEN, decode_frame, write_frame};

    /// Not real zlib: prefixes a marker byte. It lets the framing rules be tested
    /// without a compression dependency; real zlib is tested in `zlib.rs`.
    struct Marker;

    impl Zlib for Marker {
        fn compress(&self, input: &[u8]) -> Result<Vec<u8>, ZlibError> {
            let mut out = vec![0xAA];
            out.extend_from_slice(input);
            Ok(out)
        }

        fn decompress(&self, input: &[u8], expected_len: usize) -> Result<Vec<u8>, ZlibError> {
            match input.split_first() {
                Some((0xAA, rest)) if rest.len() <= expected_len => Ok(rest.to_vec()),
                Some((0xAA, _)) => Err(ZlibError("larger than declared".to_owned())),
                _ => Err(ZlibError("bad marker".to_owned())),
            }
        }
    }

    fn body(threshold: usize, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        compress_body(&Marker, threshold, payload, &mut out).unwrap();
        out
    }

    #[test]
    fn small_packets_are_sent_raw_behind_a_zero() {
        let out = body(10, &[1, 2, 3]);
        assert_eq!(out, [0, 1, 2, 3]);
        assert_eq!(decompress_body(&Marker, 10, &out).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn packets_at_the_threshold_are_compressed() {
        let payload = [7_u8; 10];
        let out = body(10, &payload);
        assert_eq!(out[0], 10);
        assert_eq!(out[1], 0xAA);
        assert_eq!(decompress_body(&Marker, 10, &out).unwrap(), payload);
    }

    #[test]
    fn threshold_zero_compresses_everything() {
        let out = body(0, &[5]);
        assert_eq!(out, [1, 0xAA, 5]);
        assert_eq!(decompress_body(&Marker, 0, &out).unwrap(), [5]);
    }

    #[test]
    fn empty_payloads_are_refused() {
        assert_eq!(
            compress_body(&Marker, 10, &[], &mut Vec::new()),
            Err(CompressionError::Empty)
        );
        assert_eq!(
            decompress_body(&Marker, 10, &[0]),
            Err(CompressionError::Empty)
        );
    }

    #[test]
    fn compressing_a_packet_below_the_threshold_is_rejected() {
        let mut bad = vec![5, 0xAA];
        bad.extend_from_slice(&[1; 5]);
        assert_eq!(
            decompress_body(&Marker, 10, &bad),
            Err(CompressionError::BelowThreshold {
                declared: 5,
                threshold: 10
            })
        );
    }

    #[test]
    fn declared_size_above_the_maximum_is_rejected_without_decompressing() {
        let mut bad = Vec::new();
        write_varint((MAX_UNCOMPRESSED_LEN + 1) as i32, &mut bad);
        bad.push(0xAA);
        assert_eq!(
            decompress_body(&Marker, 0, &bad),
            Err(CompressionError::TooLarge {
                len: MAX_UNCOMPRESSED_LEN + 1,
                max: MAX_UNCOMPRESSED_LEN
            })
        );
    }

    #[test]
    fn oversized_payloads_are_refused_on_the_way_out() {
        let payload = vec![0_u8; MAX_UNCOMPRESSED_LEN + 1];
        assert!(matches!(
            compress_body(&Marker, 0, &payload, &mut Vec::new()),
            Err(CompressionError::TooLarge { .. })
        ));
    }

    #[test]
    fn size_mismatch_is_rejected() {
        // Declares 5 bytes but only 3 come out.
        let bad = [5, 0xAA, 1, 2, 3];
        assert_eq!(
            decompress_body(&Marker, 1, &bad),
            Err(CompressionError::LengthMismatch {
                declared: 5,
                actual: 3
            })
        );
    }

    #[test]
    fn output_larger_than_declared_is_rejected_by_the_codec() {
        let bad = [2, 0xAA, 1, 2, 3, 4];
        assert!(matches!(
            decompress_body(&Marker, 1, &bad),
            Err(CompressionError::Zlib(_))
        ));
    }

    #[test]
    fn negative_and_truncated_headers_are_rejected() {
        let mut negative = Vec::new();
        write_varint(-5, &mut negative);
        assert_eq!(
            decompress_body(&Marker, 0, &negative),
            Err(CompressionError::Codec(CodecError::NegativeLength))
        );
        assert!(matches!(
            decompress_body(&Marker, 0, &[]),
            Err(CompressionError::Codec(_))
        ));
        assert!(matches!(
            decompress_body(&Marker, 0, &[0x80]),
            Err(CompressionError::Codec(_))
        ));
    }

    #[test]
    fn works_inside_a_frame() {
        let payload: Vec<u8> = (0..100).collect();
        let mut wire = Vec::new();
        write_frame(&body(64, &payload), MAX_PACKET_LEN, &mut wire).unwrap();
        let frame = decode_frame(&wire, MAX_PACKET_LEN).unwrap().unwrap();
        assert_eq!(
            decompress_body(&Marker, 64, frame.payload).unwrap(),
            payload
        );
    }
}
