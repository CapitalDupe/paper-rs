//! The production zlib codec, backed by the `flate2` crate (feature `zlib`).

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::compression::{Zlib, ZlibError};

/// zlib compression through `flate2`.
#[derive(Debug, Clone, Copy)]
pub struct FlateZlib {
    level: Compression,
}

impl FlateZlib {
    /// Creates a codec using flate2's default compression level.
    #[must_use]
    pub fn new() -> Self {
        Self {
            level: Compression::default(),
        }
    }

    /// Creates a codec with an explicit level from 0 (none) to 9 (best).
    #[must_use]
    pub fn with_level(level: u32) -> Self {
        Self {
            level: Compression::new(level.min(9)),
        }
    }
}

impl Default for FlateZlib {
    fn default() -> Self {
        Self::new()
    }
}

impl Zlib for FlateZlib {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>, ZlibError> {
        let mut encoder = ZlibEncoder::new(Vec::new(), self.level);
        encoder
            .write_all(input)
            .map_err(|err| ZlibError(err.to_string()))?;
        encoder.finish().map_err(|err| ZlibError(err.to_string()))
    }

    fn decompress(&self, input: &[u8], expected_len: usize) -> Result<Vec<u8>, ZlibError> {
        // Read at most one byte past the expected size: enough to notice a stream that
        // is larger than declared, without ever buffering an unbounded expansion.
        let limit = (expected_len as u64).saturating_add(1);
        let mut decoder = ZlibDecoder::new(input).take(limit);
        let mut out = Vec::with_capacity(expected_len.min(64 * 1024));
        decoder
            .read_to_end(&mut out)
            .map_err(|err| ZlibError(err.to_string()))?;
        if out.len() > expected_len {
            return Err(ZlibError(
                "stream is larger than its declared size".to_owned(),
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::{CompressionError, compress_body, decompress_body};

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn round_trip() {
        let zlib = FlateZlib::new();
        let data: Vec<u8> = (0..5000_u32).map(|n| (n % 251) as u8).collect();
        let compressed = zlib.compress(&data).unwrap();
        assert_eq!(zlib.decompress(&compressed, data.len()).unwrap(), data);
    }

    /// Output of Python's `zlib.compress(b"hello hello hello hello hello hello", 6)`,
    /// proving interoperability with an independent zlib implementation.
    #[test]
    fn decodes_a_stream_made_by_another_zlib() {
        let stream = hex("789ccb48cdc9c957c8c0470200eb550d19");
        let out = FlateZlib::new().decompress(&stream, 35).unwrap();
        assert_eq!(out, b"hello hello hello hello hello hello");
    }

    #[test]
    fn a_stream_larger_than_declared_is_rejected() {
        let zlib = FlateZlib::new();
        let compressed = zlib.compress(&vec![0_u8; 1_000_000]).unwrap();
        assert!(compressed.len() < 2000);
        assert!(zlib.decompress(&compressed, 10).is_err());
    }

    #[test]
    fn garbage_and_truncated_streams_are_errors() {
        let zlib = FlateZlib::new();
        assert!(zlib.decompress(&[1, 2, 3, 4, 5], 100).is_err());
        let compressed = zlib
            .compress(b"some reasonably long input some reasonably long input")
            .unwrap();
        assert!(
            zlib.decompress(&compressed[..compressed.len() - 4], 100)
                .is_err()
        );
    }

    #[test]
    fn works_through_the_framing_rules() {
        let zlib = FlateZlib::new();
        let payload: Vec<u8> = (0..2000_u32).map(|n| (n % 13) as u8).collect();

        let mut body = Vec::new();
        compress_body(&zlib, 256, &payload, &mut body).unwrap();
        assert!(body.len() < payload.len());
        assert_eq!(decompress_body(&zlib, 256, &body).unwrap(), payload);

        let small = [1_u8, 2, 3];
        let mut raw = Vec::new();
        compress_body(&zlib, 256, &small, &mut raw).unwrap();
        assert_eq!(raw, [0, 1, 2, 3]);
        assert_eq!(decompress_body(&zlib, 256, &raw).unwrap(), small);
    }

    #[test]
    fn a_lying_size_header_is_caught_with_real_zlib() {
        let zlib = FlateZlib::new();
        let mut body = Vec::new();
        compress_body(&zlib, 0, &[9_u8; 100], &mut body).unwrap();
        // Rewrite the declared size from 100 to 90.
        body[0] = 90;
        assert!(matches!(
            decompress_body(&zlib, 0, &body),
            Err(CompressionError::Zlib(_))
        ));
    }
}
