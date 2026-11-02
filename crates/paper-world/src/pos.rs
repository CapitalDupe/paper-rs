//! Block and chunk coordinates.

/// Blocks along each horizontal side of a chunk.
pub const CHUNK_SIZE: i32 = 16;

/// A chunk column position (in chunks, not blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    /// Chunk x.
    pub x: i32,
    /// Chunk z.
    pub z: i32,
}

impl ChunkPos {
    /// Creates a chunk position.
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// A block position in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos {
    /// Block x.
    pub x: i32,
    /// Block y.
    pub y: i32,
    /// Block z.
    pub z: i32,
}

impl BlockPos {
    /// Creates a block position.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The chunk containing this position (rounds toward negative infinity).
    pub fn chunk_pos(self) -> ChunkPos {
        ChunkPos::new(
            self.x.div_euclid(CHUNK_SIZE),
            self.z.div_euclid(CHUNK_SIZE),
        )
    }

    /// The x and z offset inside the containing chunk, each in `0..16`.
    pub fn local_xz(self) -> (u8, u8) {
        let x = self.x.rem_euclid(CHUNK_SIZE) as u8;
        let z = self.z.rem_euclid(CHUNK_SIZE) as u8;
        (x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_coordinates() {
        let pos = BlockPos::new(17, 70, 31);
        assert_eq!(pos.chunk_pos(), ChunkPos::new(1, 1));
        assert_eq!(pos.local_xz(), (1, 15));
    }

    #[test]
    fn negative_coordinates_round_toward_negative_infinity() {
        let pos = BlockPos::new(-1, 0, -16);
        assert_eq!(pos.chunk_pos(), ChunkPos::new(-1, -1));
        assert_eq!(pos.local_xz(), (15, 0));
        let pos = BlockPos::new(-17, 0, 0);
        assert_eq!(pos.chunk_pos(), ChunkPos::new(-2, 0));
        assert_eq!(pos.local_xz(), (15, 0));
    }
}
