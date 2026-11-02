//! Entity identifiers and basic spatial types.

/// A server-unique entity id. Ids are handed out once and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u32);

/// A 128-bit UUID as 16 big-endian bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid(pub [u8; 16]);

/// A point or vector in world space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// East-west coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
    /// North-south coordinate.
    pub z: f64,
}

impl Vec3 {
    /// Creates a vector.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Whether every component is finite (not NaN or infinite).
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Squared distance to `other`.
    pub fn distance_squared(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }

    /// The largest absolute component.
    pub fn max_abs(self) -> f64 {
        self.x.abs().max(self.y.abs()).max(self.z.abs())
    }
}

/// Where an entity is facing, in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rotation {
    /// Horizontal angle.
    pub yaw: f32,
    /// Vertical angle (positive looks down).
    pub pitch: f32,
}

impl Rotation {
    /// Creates a rotation.
    pub const fn new(yaw: f32, pitch: f32) -> Self {
        Self { yaw, pitch }
    }

    /// Wraps yaw into `[-180, 180)` and clamps pitch to `[-90, 90]`.
    ///
    /// Returns `None` if either angle is not finite.
    pub fn normalized(self) -> Option<Self> {
        if !self.yaw.is_finite() || !self.pitch.is_finite() {
            return None;
        }
        let mut yaw = self.yaw % 360.0;
        if yaw >= 180.0 {
            yaw -= 360.0;
        } else if yaw < -180.0 {
            yaw += 360.0;
        }
        Some(Self::new(yaw, self.pitch.clamp(-90.0, 90.0)))
    }
}

/// Hands out unique [`EntityId`]s, starting at 1.
#[derive(Debug)]
pub struct IdAllocator {
    next: u32,
}

impl IdAllocator {
    /// Creates an allocator.
    pub fn new() -> Self {
        Self { next: 1 }
    }

    /// The next id, or `None` once the id space is exhausted.
    pub fn allocate(&mut self) -> Option<EntityId> {
        let id = self.next;
        self.next = id.checked_add(1)?;
        Some(EntityId(id))
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_wraps_into_range() {
        let wrap = |yaw| Rotation::new(yaw, 0.0).normalized().unwrap().yaw;
        assert_eq!(wrap(0.0), 0.0);
        assert_eq!(wrap(179.0), 179.0);
        assert_eq!(wrap(180.0), -180.0);
        assert_eq!(wrap(540.0), -180.0);
        assert_eq!(wrap(-180.0), -180.0);
        assert_eq!(wrap(-190.0), 170.0);
        assert_eq!(wrap(725.0), 5.0);
    }

    #[test]
    fn pitch_is_clamped_and_non_finite_angles_are_rejected() {
        assert_eq!(Rotation::new(0.0, 120.0).normalized().unwrap().pitch, 90.0);
        assert_eq!(
            Rotation::new(0.0, -120.0).normalized().unwrap().pitch,
            -90.0
        );
        assert_eq!(Rotation::new(f32::NAN, 0.0).normalized(), None);
        assert_eq!(Rotation::new(0.0, f32::INFINITY).normalized(), None);
    }

    #[test]
    fn vector_helpers() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(3.0, 4.0, 0.0);
        assert_eq!(a.distance_squared(b), 25.0);
        assert_eq!(Vec3::new(-7.0, 2.0, 3.0).max_abs(), 7.0);
        assert!(a.is_finite());
        assert!(!Vec3::new(f64::NAN, 0.0, 0.0).is_finite());
        assert!(!Vec3::new(0.0, f64::INFINITY, 0.0).is_finite());
    }

    #[test]
    fn ids_are_unique_and_start_at_one() {
        let mut ids = IdAllocator::new();
        assert_eq!(ids.allocate(), Some(EntityId(1)));
        assert_eq!(ids.allocate(), Some(EntityId(2)));
    }

    #[test]
    fn allocator_reports_exhaustion() {
        let mut ids = IdAllocator { next: u32::MAX };
        assert_eq!(ids.allocate(), None);
    }
}
