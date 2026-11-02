//! Chunk columns made of palette-compressed sections.

use std::error::Error;
use std::fmt;

use crate::block::BlockStateId;
use crate::palette::PalettedContainer;
use crate::pos::ChunkPos;

/// Default lowest block y (modern worlds).
pub const DEFAULT_MIN_Y: i32 = -64;

/// Default world height in blocks (modern worlds).
pub const DEFAULT_HEIGHT: u32 = 384;

/// Errors produced by world data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldError {
    /// The height or minimum y is not a positive multiple of 16 within limits.
    BadHeight,
    /// The section count does not match the world height: `(expected, actual)`.
    WrongSectionCount(usize, usize),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHeight => f.write_str("height and min_y must be multiples of 16"),
            Self::WrongSectionCount(want, got) => {
                write!(f, "expected {want} sections, found {got}")
            }
        }
    }
}

impl Error for WorldError {}

/// The vertical extent of a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldHeight {
    min_y: i32,
    height: u32,
}

impl WorldHeight {
    /// Creates a height; `min_y` and `height` must be multiples of 16 and
    /// `height` must be between 16 and 4096.
    pub fn new(min_y: i32, height: u32) -> Result<Self, WorldError> {
        let aligned = min_y & 15 == 0 && height & 15 == 0;
        if !aligned || !(16..=4096).contains(&height) {
            return Err(WorldError::BadHeight);
        }
        if min_y.checked_add(height as i32).is_none() {
            return Err(WorldError::BadHeight);
        }
        Ok(Self { min_y, height })
    }

    /// Lowest block y.
    pub fn min_y(self) -> i32 {
        self.min_y
    }

    /// Height in blocks.
    pub fn height(self) -> u32 {
        self.height
    }

    /// Number of 16-block sections.
    pub fn sections(self) -> usize {
        (self.height / 16) as usize
    }
}

impl Default for WorldHeight {
    fn default() -> Self {
        Self {
            min_y: DEFAULT_MIN_Y,
            height: DEFAULT_HEIGHT,
        }
    }
}

/// A 16x16 column of blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pos: ChunkPos,
    height: WorldHeight,
    sections: Vec<PalettedContainer>,
}

impl Chunk {
    /// An all-air chunk.
    pub fn new(pos: ChunkPos, height: WorldHeight) -> Self {
        let air = PalettedContainer::uniform(BlockStateId::AIR);
        Self {
            pos,
            height,
            sections: vec![air; height.sections()],
        }
    }

    /// Builds a chunk from existing sections, checking the count.
    pub fn from_sections(
        pos: ChunkPos,
        height: WorldHeight,
        sections: Vec<PalettedContainer>,
    ) -> Result<Self, WorldError> {
        if sections.len() != height.sections() {
            return Err(WorldError::WrongSectionCount(
                height.sections(),
                sections.len(),
            ));
        }
        Ok(Self {
            pos,
            height,
            sections,
        })
    }

    /// The chunk's position.
    pub fn pos(&self) -> ChunkPos {
        self.pos
    }

    /// The chunk's vertical extent.
    pub fn height(&self) -> WorldHeight {
        self.height
    }

    /// The sections, lowest first.
    pub fn sections(&self) -> &[PalettedContainer] {
        &self.sections
    }

    fn locate(&self, x: u8, y: i32, z: u8) -> Option<(usize, usize)> {
        if x >= 16 || z >= 16 {
            return None;
        }
        let relative = u32::try_from(y.checked_sub(self.height.min_y())?).ok()?;
        if relative >= self.height.height() {
            return None;
        }
        let section = (relative / 16) as usize;
        let local_y = (relative % 16) as usize;
        Some((section, (local_y * 16 + usize::from(z)) * 16 + usize::from(x)))
    }

    /// Reads a block by chunk-local `x`/`z` (0..16) and world `y`.
    pub fn get_block(&self, x: u8, y: i32, z: u8) -> Option<BlockStateId> {
        let (section, index) = self.locate(x, y, z)?;
        self.sections.get(section)?.get(index)
    }

    /// Writes a block and returns the previous state, or `None` if out of bounds.
    pub fn set_block(&mut self, x: u8, y: i32, z: u8, id: BlockStateId) -> Option<BlockStateId> {
        let (section, index) = self.locate(x, y, z)?;
        self.sections.get_mut(section)?.set(index, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockStateId = BlockStateId(1);

    fn chunk() -> Chunk {
        Chunk::new(ChunkPos::new(0, 0), WorldHeight::default())
    }

    #[test]
    fn default_height_has_24_sections() {
        let height = WorldHeight::default();
        assert_eq!(height.sections(), 24);
        assert_eq!(chunk().sections().len(), 24);
    }

    #[test]
    fn new_chunks_are_air() {
        let chunk = chunk();
        assert_eq!(chunk.get_block(3, 70, 9), Some(BlockStateId::AIR));
        assert!(chunk.sections().iter().all(|s| s.is_uniform()));
    }

    #[test]
    fn reads_and_writes_at_the_vertical_limits() {
        let mut chunk = chunk();
        assert_eq!(chunk.set_block(0, -64, 0, STONE), Some(BlockStateId::AIR));
        assert_eq!(chunk.set_block(15, 319, 15, STONE), Some(BlockStateId::AIR));
        assert_eq!(chunk.get_block(0, -64, 0), Some(STONE));
        assert_eq!(chunk.get_block(15, 319, 15), Some(STONE));
        assert_eq!(chunk.get_block(0, 320, 0), None);
        assert_eq!(chunk.get_block(0, -65, 0), None);
        assert_eq!(chunk.set_block(0, 320, 0, STONE), None);
    }

    #[test]
    fn section_boundaries_are_independent() {
        let mut chunk = chunk();
        chunk.set_block(4, -49, 4, STONE);
        assert_eq!(chunk.get_block(4, -49, 4), Some(STONE));
        assert_eq!(chunk.get_block(4, -48, 4), Some(BlockStateId::AIR));
        assert!(!chunk.sections()[0].is_uniform());
        assert!(chunk.sections()[1].is_uniform());
    }

    #[test]
    fn rejects_out_of_range_columns() {
        let mut chunk = chunk();
        assert_eq!(chunk.get_block(16, 0, 0), None);
        assert_eq!(chunk.set_block(0, 0, 16, STONE), None);
    }

    #[test]
    fn distinct_columns_do_not_alias() {
        let mut chunk = chunk();
        chunk.set_block(0, 0, 0, STONE);
        chunk.set_block(15, 0, 15, BlockStateId(2));
        assert_eq!(chunk.get_block(0, 0, 0), Some(STONE));
        assert_eq!(chunk.get_block(15, 0, 15), Some(BlockStateId(2)));
        assert_eq!(chunk.get_block(0, 0, 15), Some(BlockStateId::AIR));
        assert_eq!(chunk.get_block(15, 0, 0), Some(BlockStateId::AIR));
    }

    #[test]
    fn world_height_validation() {
        assert_eq!(WorldHeight::new(0, 256).unwrap().sections(), 16);
        assert_eq!(WorldHeight::new(0, 250), Err(WorldError::BadHeight));
        assert_eq!(WorldHeight::new(5, 256), Err(WorldError::BadHeight));
        assert_eq!(WorldHeight::new(0, 0), Err(WorldError::BadHeight));
        assert_eq!(WorldHeight::new(0, 8192), Err(WorldError::BadHeight));
    }

    #[test]
    fn from_sections_checks_the_count() {
        let result = Chunk::from_sections(ChunkPos::new(0, 0), WorldHeight::default(), Vec::new());
        assert_eq!(result, Err(WorldError::WrongSectionCount(24, 0)));
    }
}
