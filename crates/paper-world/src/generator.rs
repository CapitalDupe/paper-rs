//! Flat world generation.

use crate::block::BlockStateId;
use crate::chunk::{Chunk, WorldHeight};
use crate::pos::ChunkPos;

/// A horizontal slab of one block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// The block state filling the slab.
    pub block: BlockStateId,
    /// How many blocks thick the slab is.
    pub thickness: u32,
}

/// Generates identical flat chunks from a stack of layers, bottom first.
///
/// Layers that would extend above the world height are cut off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatGenerator {
    height: WorldHeight,
    layers: Vec<Layer>,
}

impl FlatGenerator {
    /// Creates a generator.
    pub fn new(height: WorldHeight, layers: Vec<Layer>) -> Self {
        Self { height, layers }
    }

    /// The y of the first block above the top layer.
    pub fn surface_y(&self) -> i32 {
        let total: u32 = self.layers.iter().map(|layer| layer.thickness).sum();
        let used = total.min(self.height.height());
        self.height.min_y() + used as i32
    }

    /// Builds the chunk at `pos`.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::new(pos, self.height);
        let mut y = self.height.min_y();
        let mut remaining = self.height.height();
        for layer in &self.layers {
            let thickness = layer.thickness.min(remaining);
            remaining -= thickness;
            for _ in 0..thickness {
                if layer.block != BlockStateId::AIR {
                    for x in 0..16 {
                        for z in 0..16 {
                            chunk.set_block(x, y, z, layer.block);
                        }
                    }
                }
                y += 1;
            }
        }
        chunk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: u32, thickness: u32) -> Layer {
        Layer {
            block: BlockStateId(id),
            thickness,
        }
    }

    #[test]
    fn builds_layers_bottom_up() {
        let generator = FlatGenerator::new(
            WorldHeight::default(),
            vec![layer(1, 1), layer(2, 2), layer(3, 1)],
        );
        let chunk = generator.generate(ChunkPos::new(4, -7));
        assert_eq!(chunk.pos(), ChunkPos::new(4, -7));
        let column = |y| chunk.get_block(5, y, 9);
        assert_eq!(column(-64), Some(BlockStateId(1)));
        assert_eq!(column(-63), Some(BlockStateId(2)));
        assert_eq!(column(-62), Some(BlockStateId(2)));
        assert_eq!(column(-61), Some(BlockStateId(3)));
        assert_eq!(column(-60), Some(BlockStateId::AIR));
        assert_eq!(generator.surface_y(), -60);
    }

    #[test]
    fn every_column_is_identical() {
        let generator = FlatGenerator::new(WorldHeight::default(), vec![layer(1, 3)]);
        let chunk = generator.generate(ChunkPos::new(0, 0));
        for (x, z) in [(0, 0), (15, 15), (0, 15), (7, 3)] {
            assert_eq!(chunk.get_block(x, -62, z), Some(BlockStateId(1)));
        }
    }

    #[test]
    fn air_layers_leave_gaps_and_oversized_layers_are_cut_off() {
        let height = WorldHeight::new(0, 16).unwrap();
        let generator = FlatGenerator::new(height, vec![layer(0, 2), layer(5, 1000)]);
        let chunk = generator.generate(ChunkPos::new(0, 0));
        assert_eq!(chunk.get_block(0, 1, 0), Some(BlockStateId::AIR));
        assert_eq!(chunk.get_block(0, 2, 0), Some(BlockStateId(5)));
        assert_eq!(chunk.get_block(0, 15, 0), Some(BlockStateId(5)));
        assert_eq!(generator.surface_y(), 16);
    }

    #[test]
    fn generation_is_deterministic() {
        let generator = FlatGenerator::new(WorldHeight::default(), vec![layer(1, 2)]);
        assert_eq!(
            generator.generate(ChunkPos::new(1, 1)),
            generator.generate(ChunkPos::new(1, 1))
        );
    }
}
