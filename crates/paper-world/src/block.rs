//! Block state identifiers.

/// An opaque block state id. `0` is air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockStateId(pub u32);

impl BlockStateId {
    /// The air block state.
    pub const AIR: Self = Self(0);
}
