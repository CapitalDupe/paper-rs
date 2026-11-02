//! Players and server-side movement validation.
//!
//! The client reports where it thinks it is; the server must not trust that. A
//! move is rejected if it contains non-finite numbers, leaves the world, or covers
//! more distance in one update than the configured limit. A rejected move leaves
//! the player where it was, so the caller can send a correction.

use std::error::Error;
use std::fmt;

use crate::entity::{EntityId, Rotation, Uuid, Vec3};

/// A player's game mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    /// Normal play.
    Survival,
    /// Unlimited resources and flight.
    Creative,
    /// Limited interaction.
    Adventure,
    /// Free camera, no interaction.
    Spectator,
}

/// Bounds applied to client-reported movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementLimits {
    /// Largest distance, in blocks, a single move may cover.
    pub max_step: f64,
    /// Largest absolute coordinate allowed on any axis.
    pub max_coordinate: f64,
}

impl Default for MovementLimits {
    fn default() -> Self {
        Self {
            max_step: 100.0,
            max_coordinate: 30_000_000.0,
        }
    }
}

/// Why a move or spawn was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// A coordinate or angle was NaN or infinite.
    NotFinite,
    /// A coordinate is beyond the world limit.
    OutOfWorld,
    /// The move covers more distance than allowed.
    TooFar,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite => f.write_str("position or rotation is not finite"),
            Self::OutOfWorld => f.write_str("position is outside the world"),
            Self::TooFar => f.write_str("moved too far in one update"),
        }
    }
}

impl Error for MoveError {}

/// Validates a target position and rotation, returning the normalized rotation.
pub fn check_position(
    target: Vec3,
    rotation: Rotation,
    limits: &MovementLimits,
) -> Result<Rotation, MoveError> {
    let Some(rotation) = rotation.normalized() else {
        return Err(MoveError::NotFinite);
    };
    if !target.is_finite() {
        return Err(MoveError::NotFinite);
    }
    if target.max_abs() > limits.max_coordinate {
        return Err(MoveError::OutOfWorld);
    }
    Ok(rotation)
}

/// A connected player.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    id: EntityId,
    uuid: Uuid,
    name: String,
    game_mode: GameMode,
    position: Vec3,
    rotation: Rotation,
    on_ground: bool,
}

impl Player {
    /// Creates a player at `spawn`, validating the position.
    pub fn new(
        id: EntityId,
        uuid: Uuid,
        name: String,
        spawn: Vec3,
        limits: &MovementLimits,
    ) -> Result<Self, MoveError> {
        let rotation = check_position(spawn, Rotation::default(), limits)?;
        Ok(Self {
            id,
            uuid,
            name,
            game_mode: GameMode::Survival,
            position: spawn,
            rotation,
            on_ground: false,
        })
    }

    /// The player's entity id.
    pub fn id(&self) -> EntityId {
        self.id
    }

    /// The player's UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// The player's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The player's game mode.
    pub fn game_mode(&self) -> GameMode {
        self.game_mode
    }

    /// Changes the game mode.
    pub fn set_game_mode(&mut self, mode: GameMode) {
        self.game_mode = mode;
    }

    /// Current position.
    pub fn position(&self) -> Vec3 {
        self.position
    }

    /// Current rotation.
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Whether the client reports standing on the ground.
    pub fn on_ground(&self) -> bool {
        self.on_ground
    }

    /// Applies a client-reported move, or rejects it and changes nothing.
    pub fn move_to(
        &mut self,
        target: Vec3,
        rotation: Rotation,
        on_ground: bool,
        limits: &MovementLimits,
    ) -> Result<(), MoveError> {
        let rotation = check_position(target, rotation, limits)?;
        if self.position.distance_squared(target) > limits.max_step * limits.max_step {
            return Err(MoveError::TooFar);
        }
        self.position = target;
        self.rotation = rotation;
        self.on_ground = on_ground;
        Ok(())
    }

    /// Moves the player anywhere in the world (a server-initiated teleport).
    pub fn teleport(
        &mut self,
        target: Vec3,
        rotation: Rotation,
        limits: &MovementLimits,
    ) -> Result<(), MoveError> {
        let rotation = check_position(target, rotation, limits)?;
        self.position = target;
        self.rotation = rotation;
        self.on_ground = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Player {
        let limits = MovementLimits::default();
        let spawn = Vec3::new(0.0, 64.0, 0.0);
        Player::new(EntityId(1), Uuid([1; 16]), "Steve".to_owned(), spawn, &limits).unwrap()
    }

    #[test]
    fn spawns_in_survival_at_the_given_position() {
        let player = player();
        assert_eq!(player.game_mode(), GameMode::Survival);
        assert_eq!(player.position(), Vec3::new(0.0, 64.0, 0.0));
        assert_eq!(player.name(), "Steve");
        assert!(!player.on_ground());
    }

    #[test]
    fn accepts_a_normal_move_and_normalizes_rotation() {
        let mut player = player();
        let limits = MovementLimits::default();
        let target = Vec3::new(1.5, 64.0, -2.0);
        player
            .move_to(target, Rotation::new(540.0, 200.0), true, &limits)
            .unwrap();
        assert_eq!(player.position(), target);
        assert_eq!(player.rotation(), Rotation::new(-180.0, 90.0));
        assert!(player.on_ground());
    }

    #[test]
    fn rejects_moves_that_are_too_far_without_changing_state() {
        let mut player = player();
        let limits = MovementLimits::default();
        let before = player.clone();
        let result = player.move_to(Vec3::new(500.0, 64.0, 0.0), Rotation::default(), false, &limits);
        assert_eq!(result, Err(MoveError::TooFar));
        assert_eq!(player, before);
    }

    #[test]
    fn rejects_non_finite_and_out_of_world_positions() {
        let mut player = player();
        let limits = MovementLimits::default();
        let nan = Vec3::new(f64::NAN, 0.0, 0.0);
        assert_eq!(
            player.move_to(nan, Rotation::default(), false, &limits),
            Err(MoveError::NotFinite)
        );
        let bad_rotation = Rotation::new(f32::NAN, 0.0);
        assert_eq!(
            player.move_to(Vec3::default(), bad_rotation, false, &limits),
            Err(MoveError::NotFinite)
        );
        let far = Vec3::new(0.0, 0.0, 40_000_000.0);
        assert_eq!(
            player.teleport(far, Rotation::default(), &limits),
            Err(MoveError::OutOfWorld)
        );
    }

    #[test]
    fn teleport_ignores_the_step_limit() {
        let mut player = player();
        let limits = MovementLimits::default();
        let target = Vec3::new(10_000.0, 80.0, 10_000.0);
        player.teleport(target, Rotation::new(90.0, 0.0), &limits).unwrap();
        assert_eq!(player.position(), target);
        assert!(!player.on_ground());
    }

    #[test]
    fn spawn_is_validated() {
        let limits = MovementLimits::default();
        let result = Player::new(
            EntityId(2),
            Uuid([2; 16]),
            "Alex".to_owned(),
            Vec3::new(f64::INFINITY, 0.0, 0.0),
            &limits,
        );
        assert_eq!(result, Err(MoveError::NotFinite));
    }
}
