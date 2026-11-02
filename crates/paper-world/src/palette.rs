//! Palette-compressed storage for one 16x16x16 chunk section.
//!
//! A section is either *uniform* (every block is the same, stored as one id) or
//! *mixed* (a palette of distinct ids plus a packed array of palette indices).
//! The palette grows as new block states appear and the index width grows with
//! it, never below 4 bits.

use std::error::Error;
use std::fmt;

use crate::block::BlockStateId;
use crate::packed::PackedArray;

/// Blocks in one section.
pub const SECTION_VOLUME: usize = 16 * 16 * 16;

/// Narrowest index width used for a mixed section.
pub const MIN_BITS: u8 = 4;

fn bits_for(palette_len: usize) -> u8 {
    let needed = usize::BITS - palette_len.saturating_sub(1).leading_zeros();
    (needed as u8).max(MIN_BITS)
}

fn repack(old: &PackedArray, bits: u8) -> PackedArray {
    let mut new = PackedArray::new(bits, old.len());
    for index in 0..old.len() {
        if let Some(value) = old.get(index) {
            let _ = new.set(index, value);
        }
    }
    new
}

/// Errors produced when rebuilding a container from stored parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerError {
    /// The palette is empty or too large.
    BadPalette,
    /// The packed indices are malformed or point outside the palette.
    BadIndices,
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPalette => f.write_str("invalid palette"),
            Self::BadIndices => f.write_str("invalid palette indices"),
        }
    }
}

impl Error for ContainerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Storage {
    Uniform(BlockStateId),
    Mixed {
        palette: Vec<BlockStateId>,
        indices: PackedArray,
    },
}

/// Block states for one section, indexed `(y * 16 + z) * 16 + x`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalettedContainer {
    storage: Storage,
}

impl PalettedContainer {
    /// A section filled with one block state.
    pub fn uniform(id: BlockStateId) -> Self {
        Self {
            storage: Storage::Uniform(id),
        }
    }

    /// Whether every block is the same state.
    pub fn is_uniform(&self) -> bool {
        matches!(self.storage, Storage::Uniform(_))
    }

    /// Number of palette entries (1 for a uniform section).
    pub fn palette_len(&self) -> usize {
        match &self.storage {
            Storage::Uniform(_) => 1,
            Storage::Mixed { palette, .. } => palette.len(),
        }
    }

    /// Index width in bits (0 for a uniform section).
    pub fn bits(&self) -> u8 {
        match &self.storage {
            Storage::Uniform(_) => 0,
            Storage::Mixed { indices, .. } => indices.bits(),
        }
    }

    /// Reads the block at `index`, or `None` if out of range.
    pub fn get(&self, index: usize) -> Option<BlockStateId> {
        if index >= SECTION_VOLUME {
            return None;
        }
        match &self.storage {
            Storage::Uniform(id) => Some(*id),
            Storage::Mixed { palette, indices } => {
                let slot = usize::try_from(indices.get(index)?).ok()?;
                palette.get(slot).copied()
            }
        }
    }

    /// Writes the block at `index` and returns the previous state, or `None` if
    /// `index` is out of range.
    pub fn set(&mut self, index: usize, id: BlockStateId) -> Option<BlockStateId> {
        if index >= SECTION_VOLUME {
            return None;
        }
        if let Storage::Uniform(current) = self.storage {
            if current == id {
                return Some(current);
            }
            self.storage = Storage::Mixed {
                palette: vec![current],
                indices: PackedArray::new(MIN_BITS, SECTION_VOLUME),
            };
        }
        let Storage::Mixed { palette, indices } = &mut self.storage else {
            return None;
        };
        let slot = match palette.iter().position(|entry| *entry == id) {
            Some(position) => position,
            None => {
                palette.push(id);
                if palette.len() > (1_usize << indices.bits()) {
                    *indices = repack(indices, bits_for(palette.len()));
                }
                palette.len() - 1
            }
        };
        let old_slot = indices.set(index, u32::try_from(slot).ok()?).ok()?;
        palette.get(usize::try_from(old_slot).ok()?).copied()
    }

    /// Drops unused palette entries and shrinks the index width, collapsing to a
    /// uniform section when only one state remains.
    pub fn compact(&mut self) {
        let Storage::Mixed { palette, indices } = &self.storage else {
            return;
        };
        let mut used = vec![false; palette.len()];
        for index in 0..SECTION_VOLUME {
            let slot = indices.get(index).unwrap_or(0) as usize;
            if let Some(flag) = used.get_mut(slot) {
                *flag = true;
            }
        }
        let mut remap = vec![0_u32; palette.len()];
        let mut kept = Vec::new();
        for (old, id) in palette.iter().enumerate() {
            if used[old] {
                remap[old] = kept.len() as u32;
                kept.push(*id);
            }
        }
        if kept.len() == 1 {
            self.storage = Storage::Uniform(kept[0]);
            return;
        }
        let mut packed = PackedArray::new(bits_for(kept.len()), SECTION_VOLUME);
        for index in 0..SECTION_VOLUME {
            let slot = indices.get(index).unwrap_or(0) as usize;
            let _ = packed.set(index, remap.get(slot).copied().unwrap_or(0));
        }
        self.storage = Storage::Mixed {
            palette: kept,
            indices: packed,
        };
    }

    /// Splits into `(palette, bits, longs)` for storage. A uniform section has
    /// `bits == 0` and no longs.
    pub fn parts(&self) -> (Vec<BlockStateId>, u8, Vec<u64>) {
        match &self.storage {
            Storage::Uniform(id) => (vec![*id], 0, Vec::new()),
            Storage::Mixed { palette, indices } => {
                (palette.clone(), indices.bits(), indices.as_longs().to_vec())
            }
        }
    }

    /// Rebuilds a container from [`PalettedContainer::parts`], validating that
    /// every index points inside the palette.
    pub fn from_parts(
        palette: Vec<BlockStateId>,
        bits: u8,
        longs: Vec<u64>,
    ) -> Result<Self, ContainerError> {
        if palette.is_empty() || palette.len() > SECTION_VOLUME {
            return Err(ContainerError::BadPalette);
        }
        if bits == 0 {
            if palette.len() != 1 || !longs.is_empty() {
                return Err(ContainerError::BadIndices);
            }
            return Ok(Self::uniform(palette[0]));
        }
        let Ok(indices) = PackedArray::from_raw(bits, SECTION_VOLUME, longs) else {
            return Err(ContainerError::BadIndices);
        };
        for index in 0..SECTION_VOLUME {
            match indices.get(index) {
                Some(slot) if (slot as usize) < palette.len() => {}
                _ => return Err(ContainerError::BadIndices),
            }
        }
        Ok(Self {
            storage: Storage::Mixed { palette, indices },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockStateId = BlockStateId(1);

    #[test]
    fn uniform_sections_report_their_block() {
        let section = PalettedContainer::uniform(BlockStateId::AIR);
        assert!(section.is_uniform());
        assert_eq!(section.get(0), Some(BlockStateId::AIR));
        assert_eq!(section.get(SECTION_VOLUME - 1), Some(BlockStateId::AIR));
        assert_eq!(section.get(SECTION_VOLUME), None);
        assert_eq!((section.palette_len(), section.bits()), (1, 0));
    }

    #[test]
    fn first_distinct_write_makes_the_section_mixed() {
        let mut section = PalettedContainer::uniform(BlockStateId::AIR);
        assert_eq!(section.set(5, BlockStateId::AIR), Some(BlockStateId::AIR));
        assert!(section.is_uniform());
        assert_eq!(section.set(5, STONE), Some(BlockStateId::AIR));
        assert!(!section.is_uniform());
        assert_eq!(section.get(5), Some(STONE));
        assert_eq!(section.get(6), Some(BlockStateId::AIR));
        assert_eq!(section.bits(), MIN_BITS);
        assert_eq!(section.set(SECTION_VOLUME, STONE), None);
    }

    #[test]
    fn index_width_grows_with_the_palette() {
        let mut section = PalettedContainer::uniform(BlockStateId::AIR);
        for value in 1..=20_u32 {
            section.set(value as usize, BlockStateId(value));
        }
        assert_eq!(section.palette_len(), 21);
        assert_eq!(section.bits(), 5);
        for value in 1..=20_u32 {
            assert_eq!(section.get(value as usize), Some(BlockStateId(value)));
        }
        assert_eq!(section.get(0), Some(BlockStateId::AIR));
    }

    #[test]
    fn compact_collapses_unused_entries() {
        let mut section = PalettedContainer::uniform(BlockStateId::AIR);
        for value in 1..=20_u32 {
            section.set(value as usize, BlockStateId(value));
        }
        for value in 3..=20_u32 {
            section.set(value as usize, BlockStateId::AIR);
        }
        section.compact();
        assert_eq!(section.palette_len(), 3);
        assert_eq!(section.bits(), MIN_BITS);
        assert_eq!(section.get(1), Some(BlockStateId(1)));
        assert_eq!(section.get(2), Some(BlockStateId(2)));
        assert_eq!(section.get(10), Some(BlockStateId::AIR));

        section.set(1, BlockStateId::AIR);
        section.set(2, BlockStateId::AIR);
        section.compact();
        assert!(section.is_uniform());
        assert_eq!(section.get(1), Some(BlockStateId::AIR));
    }

    #[test]
    fn parts_round_trip() {
        let mut section = PalettedContainer::uniform(BlockStateId::AIR);
        section.set(100, STONE);
        section.set(200, BlockStateId(9));
        let (palette, bits, longs) = section.parts();
        assert_eq!(
            PalettedContainer::from_parts(palette, bits, longs),
            Ok(section)
        );

        let uniform = PalettedContainer::uniform(STONE);
        let (palette, bits, longs) = uniform.parts();
        assert_eq!(
            PalettedContainer::from_parts(palette, bits, longs),
            Ok(uniform)
        );
    }

    #[test]
    fn from_parts_rejects_invalid_data() {
        let two = vec![BlockStateId::AIR, STONE];
        let mut longs = vec![0_u64; 256];
        longs[0] = 5;
        assert_eq!(
            PalettedContainer::from_parts(two.clone(), 4, longs),
            Err(ContainerError::BadIndices)
        );
        assert_eq!(
            PalettedContainer::from_parts(Vec::new(), 4, vec![0; 256]),
            Err(ContainerError::BadPalette)
        );
        assert_eq!(
            PalettedContainer::from_parts(two.clone(), 0, Vec::new()),
            Err(ContainerError::BadIndices)
        );
        assert_eq!(
            PalettedContainer::from_parts(two, 4, vec![0; 10]),
            Err(ContainerError::BadIndices)
        );
    }
}
