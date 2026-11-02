//! Tracks connected players and routes their movement.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::entity::{EntityId, IdAllocator, Rotation, Uuid, Vec3};
use crate::player::{self, MoveError, MovementLimits, Player};

/// Why a player could not join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    /// A player with this UUID is already connected.
    DuplicateUuid,
    /// Entity ids are exhausted.
    OutOfIds,
    /// The spawn position was invalid.
    BadSpawn(MoveError),
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateUuid => f.write_str("a player with this UUID is already online"),
            Self::OutOfIds => f.write_str("entity ids are exhausted"),
            Self::BadSpawn(err) => write!(f, "invalid spawn: {err}"),
        }
    }
}

impl Error for JoinError {}

/// Why a move could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerMoveError {
    /// No player has this entity id.
    NoSuchPlayer,
    /// The move itself was rejected.
    Move(MoveError),
}

impl fmt::Display for PlayerMoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchPlayer => f.write_str("no such player"),
            Self::Move(err) => write!(f, "move rejected: {err}"),
        }
    }
}

impl Error for PlayerMoveError {}

/// The set of connected players.
#[derive(Debug, Default)]
pub struct PlayerManager {
    ids: IdAllocator,
    players: HashMap<EntityId, Player>,
    by_uuid: HashMap<Uuid, EntityId>,
}

impl PlayerManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a player at `spawn` and returns its entity id.
    pub fn join(
        &mut self,
        uuid: Uuid,
        name: String,
        spawn: Vec3,
        limits: &MovementLimits,
    ) -> Result<EntityId, JoinError> {
        if self.by_uuid.contains_key(&uuid) {
            return Err(JoinError::DuplicateUuid);
        }
        player::check_position(spawn, Rotation::default(), limits).map_err(JoinError::BadSpawn)?;
        let id = self.ids.allocate().ok_or(JoinError::OutOfIds)?;
        let player = Player::new(id, uuid, name, spawn, limits).map_err(JoinError::BadSpawn)?;
        self.by_uuid.insert(uuid, id);
        self.players.insert(id, player);
        Ok(id)
    }

    /// Removes a player, returning it if it was connected.
    pub fn leave(&mut self, id: EntityId) -> Option<Player> {
        let player = self.players.remove(&id)?;
        self.by_uuid.remove(&player.uuid());
        Some(player)
    }

    /// Looks up a player by entity id.
    pub fn get(&self, id: EntityId) -> Option<&Player> {
        self.players.get(&id)
    }

    /// Looks up a player by entity id for modification.
    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Player> {
        self.players.get_mut(&id)
    }

    /// Looks up a player by UUID.
    pub fn find_by_uuid(&self, uuid: &Uuid) -> Option<&Player> {
        self.players.get(self.by_uuid.get(uuid)?)
    }

    /// Number of connected players.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// Whether no players are connected.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// Iterates over connected players in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &Player> {
        self.players.values()
    }

    /// Applies a client-reported move to a player.
    pub fn move_player(
        &mut self,
        id: EntityId,
        target: Vec3,
        rotation: Rotation,
        on_ground: bool,
        limits: &MovementLimits,
    ) -> Result<(), PlayerMoveError> {
        let player = self.players.get_mut(&id).ok_or(PlayerMoveError::NoSuchPlayer)?;
        player
            .move_to(target, rotation, on_ground, limits)
            .map_err(PlayerMoveError::Move)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn() -> Vec3 {
        Vec3::new(0.0, 64.0, 0.0)
    }

    #[test]
    fn join_assigns_unique_ids_and_tracks_players() {
        let limits = MovementLimits::default();
        let mut players = PlayerManager::new();
        let a = players.join(Uuid([1; 16]), "A".to_owned(), spawn(), &limits).unwrap();
        let b = players.join(Uuid([2; 16]), "B".to_owned(), spawn(), &limits).unwrap();
        assert_ne!(a, b);
        assert_eq!(players.len(), 2);
        assert_eq!(players.get(a).unwrap().name(), "A");
        assert_eq!(players.find_by_uuid(&Uuid([2; 16])).unwrap().id(), b);
        assert_eq!(players.iter().count(), 2);
    }

    #[test]
    fn duplicate_uuids_are_rejected() {
        let limits = MovementLimits::default();
        let mut players = PlayerManager::new();
        players.join(Uuid([1; 16]), "A".to_owned(), spawn(), &limits).unwrap();
        let result = players.join(Uuid([1; 16]), "A2".to_owned(), spawn(), &limits);
        assert_eq!(result, Err(JoinError::DuplicateUuid));
        assert_eq!(players.len(), 1);
    }

    #[test]
    fn leaving_frees_the_uuid_but_never_reuses_the_id() {
        let limits = MovementLimits::default();
        let mut players = PlayerManager::new();
        let first = players.join(Uuid([1; 16]), "A".to_owned(), spawn(), &limits).unwrap();
        assert_eq!(players.leave(first).unwrap().name(), "A");
        assert!(players.is_empty());
        assert_eq!(players.leave(first), None);
        let second = players.join(Uuid([1; 16]), "A".to_owned(), spawn(), &limits).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn bad_spawns_do_not_register_the_player() {
        let limits = MovementLimits::default();
        let mut players = PlayerManager::new();
        let bad = Vec3::new(f64::NAN, 0.0, 0.0);
        let result = players.join(Uuid([1; 16]), "A".to_owned(), bad, &limits);
        assert_eq!(result, Err(JoinError::BadSpawn(MoveError::NotFinite)));
        assert!(players.is_empty());
        assert!(players.find_by_uuid(&Uuid([1; 16])).is_none());
    }

    #[test]
    fn move_player_routes_and_reports_errors() {
        let limits = MovementLimits::default();
        let mut players = PlayerManager::new();
        let id = players.join(Uuid([1; 16]), "A".to_owned(), spawn(), &limits).unwrap();
        let target = Vec3::new(2.0, 64.0, 2.0);
        players.move_player(id, target, Rotation::default(), true, &limits).unwrap();
        assert_eq!(players.get(id).unwrap().position(), target);

        let far = Vec3::new(9_000.0, 64.0, 0.0);
        assert_eq!(
            players.move_player(id, far, Rotation::default(), true, &limits),
            Err(PlayerMoveError::Move(MoveError::TooFar))
        );
        assert_eq!(
            players.move_player(EntityId(99), target, Rotation::default(), true, &limits),
            Err(PlayerMoveError::NoSuchPlayer)
        );
    }
}
