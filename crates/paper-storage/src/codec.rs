//! Binary encoding of a chunk.
//!
//! Layout (all integers big-endian):
//!
//! ```text
//! "MGCH" | version u8 | chunk x i32 | chunk z i32 | min_y i32 | height u32
//! | section count u16
//! | per section: palette len u16 | palette ids u32 * n | bits u8
//!                | word count u32 | words u64 * n
//! | crc32 u32 (over everything before it)
//! ```
//!
//! Decoding checks the checksum first, then validates every length against fixed
//! limits before allocating, so damaged or hostile files cannot cause large
//! allocations.

use paper_world::block::BlockStateId;
use paper_world::chunk::{Chunk, WorldHeight};
use paper_world::palette::{PalettedContainer, SECTION_VOLUME};
use paper_world::pos::ChunkPos;

use crate::crc::crc32;
use crate::error::StorageError;

/// File signature.
pub const MAGIC: [u8; 4] = *b"MGCH";

/// Current format version.
pub const VERSION: u8 = 1;

const MAX_PALETTE: usize = SECTION_VOLUME;
const MAX_WORDS: usize = 4096;

/// Serializes a chunk, including the trailing checksum.
pub fn encode_chunk(chunk: &Chunk) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&chunk.pos().x.to_be_bytes());
    out.extend_from_slice(&chunk.pos().z.to_be_bytes());
    out.extend_from_slice(&chunk.height().min_y().to_be_bytes());
    out.extend_from_slice(&chunk.height().height().to_be_bytes());
    let Ok(count) = u16::try_from(chunk.sections().len()) else {
        return Err(StorageError::Unencodable("too many sections"));
    };
    out.extend_from_slice(&count.to_be_bytes());
    for section in chunk.sections() {
        let (palette, bits, words) = section.parts();
        let Ok(palette_len) = u16::try_from(palette.len()) else {
            return Err(StorageError::Unencodable("palette too large"));
        };
        let Ok(word_count) = u32::try_from(words.len()) else {
            return Err(StorageError::Unencodable("too many packed words"));
        };
        out.extend_from_slice(&palette_len.to_be_bytes());
        for id in &palette {
            out.extend_from_slice(&id.0.to_be_bytes());
        }
        out.push(bits);
        out.extend_from_slice(&word_count.to_be_bytes());
        for word in &words {
            out.extend_from_slice(&word.to_be_bytes());
        }
    }
    let checksum = crc32(&out);
    out.extend_from_slice(&checksum.to_be_bytes());
    Ok(out)
}

struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], StorageError> {
        if self.buf.len() < len {
            return Err(StorageError::Corrupt("unexpected end of data"));
        }
        let (head, tail) = self.buf.split_at(len);
        self.buf = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StorageError> {
        let bytes = self.take(N)?;
        let Ok(array) = <[u8; N]>::try_from(bytes) else {
            return Err(StorageError::Corrupt("unexpected end of data"));
        };
        Ok(array)
    }

    fn u8(&mut self) -> Result<u8, StorageError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, StorageError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, StorageError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, StorageError> {
        Ok(i32::from_be_bytes(self.array()?))
    }
}

fn read_section(cursor: &mut Cursor<'_>) -> Result<PalettedContainer, StorageError> {
    let palette_len = usize::from(cursor.u16()?);
    if palette_len == 0 || palette_len > MAX_PALETTE {
        return Err(StorageError::Corrupt("bad palette size"));
    }
    let raw = cursor.take(palette_len * 4)?;
    let (entries, _) = raw.as_chunks::<4>();
    let palette = entries
        .iter()
        .map(|bytes| BlockStateId(u32::from_be_bytes(*bytes)))
        .collect::<Vec<_>>();
    let bits = cursor.u8()?;
    let word_count = cursor.u32()? as usize;
    if word_count > MAX_WORDS {
        return Err(StorageError::Corrupt("too many packed words"));
    }
    let raw = cursor.take(word_count * 8)?;
    let (entries, _) = raw.as_chunks::<8>();
    let words = entries
        .iter()
        .map(|bytes| u64::from_be_bytes(*bytes))
        .collect::<Vec<_>>();
    PalettedContainer::from_parts(palette, bits, words)
        .map_err(|_| StorageError::Corrupt("invalid section data"))
}

/// Parses a chunk, verifying the checksum and that it is the chunk at `expected`.
pub fn decode_chunk(bytes: &[u8], expected: ChunkPos) -> Result<Chunk, StorageError> {
    if bytes.len() < MAGIC.len() + 1 + 4 {
        return Err(StorageError::Corrupt("file too short"));
    }
    let (body, trailer) = bytes.split_at(bytes.len() - 4);
    let stored = u32::from_be_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if crc32(body) != stored {
        return Err(StorageError::Corrupt("checksum mismatch"));
    }
    let mut cursor = Cursor { buf: body };
    if cursor.array::<4>()? != MAGIC {
        return Err(StorageError::Corrupt("bad magic"));
    }
    let version = cursor.u8()?;
    if version != VERSION {
        return Err(StorageError::Unsupported(version));
    }
    let pos = ChunkPos::new(cursor.i32()?, cursor.i32()?);
    if pos != expected {
        return Err(StorageError::PosMismatch);
    }
    let min_y = cursor.i32()?;
    let height = cursor.u32()?;
    let Ok(height) = WorldHeight::new(min_y, height) else {
        return Err(StorageError::Corrupt("bad height"));
    };
    let count = usize::from(cursor.u16()?);
    if count != height.sections() {
        return Err(StorageError::Corrupt("section count does not match height"));
    }
    let mut sections = Vec::with_capacity(count);
    for _ in 0..count {
        sections.push(read_section(&mut cursor)?);
    }
    if !cursor.buf.is_empty() {
        return Err(StorageError::Corrupt("trailing data"));
    }
    Chunk::from_sections(pos, height, sections).map_err(|_| StorageError::Corrupt("bad sections"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chunk(pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::new(pos, WorldHeight::default());
        chunk.set_block(1, -64, 2, BlockStateId(7));
        chunk.set_block(15, 100, 15, BlockStateId(1));
        for value in 1..=20_u32 {
            let x = (value % 16) as u8;
            let z = (value / 16) as u8;
            chunk.set_block(x, 0, z, BlockStateId(100 + value));
        }
        chunk
    }

    fn reseal(mut body: Vec<u8>) -> Vec<u8> {
        body.truncate(body.len() - 4);
        let checksum = crc32(&body);
        body.extend_from_slice(&checksum.to_be_bytes());
        body
    }

    #[test]
    fn round_trips_exactly() {
        let pos = ChunkPos::new(-3, 12);
        let chunk = sample_chunk(pos);
        let bytes = encode_chunk(&chunk).unwrap();
        assert_eq!(decode_chunk(&bytes, pos).unwrap(), chunk);
    }

    #[test]
    fn every_single_byte_flip_is_detected() {
        let pos = ChunkPos::new(1, 2);
        let bytes = encode_chunk(&sample_chunk(pos)).unwrap();
        for index in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[index] ^= 0x01;
            assert!(decode_chunk(&damaged, pos).is_err(), "byte {index}");
        }
    }

    #[test]
    fn every_truncation_is_detected() {
        let pos = ChunkPos::new(1, 2);
        let bytes = encode_chunk(&sample_chunk(pos)).unwrap();
        for len in 0..bytes.len() {
            assert!(decode_chunk(&bytes[..len], pos).is_err(), "length {len}");
        }
    }

    #[test]
    fn rejects_a_chunk_stored_under_another_position() {
        let bytes = encode_chunk(&sample_chunk(ChunkPos::new(1, 2))).unwrap();
        let result = decode_chunk(&bytes, ChunkPos::new(3, 4));
        assert!(matches!(result, Err(StorageError::PosMismatch)));
    }

    #[test]
    fn rejects_valid_checksums_over_bad_content() {
        let pos = ChunkPos::new(1, 2);
        let bytes = encode_chunk(&sample_chunk(pos)).unwrap();

        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        let result = decode_chunk(&reseal(bad_magic), pos);
        assert!(matches!(result, Err(StorageError::Corrupt("bad magic"))));

        let mut bad_version = bytes.clone();
        bad_version[4] = 2;
        let result = decode_chunk(&reseal(bad_version), pos);
        assert!(matches!(result, Err(StorageError::Unsupported(2))));

        let mut trailing = bytes;
        let checksum_at = trailing.len() - 4;
        trailing.insert(checksum_at, 0);
        let result = decode_chunk(&reseal(trailing), pos);
        assert!(matches!(
            result,
            Err(StorageError::Corrupt("trailing data"))
        ));
    }
}
