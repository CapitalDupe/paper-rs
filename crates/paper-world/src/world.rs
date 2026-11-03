//! A world: generated chunks plus the players inside it.
//!
//! [`World`] owns chunk data and player state and decides *what* each player
//! should be told, expressed as [`Outbound`] values. It does not encode packets:
//! turning an `Outbound` into bytes depends on the target Minecraft version
//! (ADR 0003) and belongs to the network layer.
//!
//! Chunks are generated on demand and cached. There is no unloading or saving
//! yet; both arrive when the world is wired to `paper-storage`.

use std::collections::HashMap;

use crate::chunk::Chunk;
use crate::entity::{EntityId, Rotation, Uuid, Vec3};
use crate::generator::FlatGenerator;
use crate::manager::{JoinError, PlayerManager, PlayerMoveError};
use crate::player::{MovementLimits, Player};
use crate::pos::{BlockPos, ChunkPos};
use crate::tracker::ChunkTracker;

/// Something the network layer should tell a player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outbound {
    /// Send the data of this chunk to the player.
    ChunkData(EntityId, ChunkPos),
    /// Tell the player to forget this chunk.
    ChunkUnload(EntityId, ChunkPos),
    /// Tell the player where the server says it is (after a rejected move or a
    /// teleport).
    PositionCorrection(EntityId, Vec3, Rotation),
}

/// World settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldConfig {
    /// Chunk radius sent to each player.
    pub view_distance: u8,
    /// Most chunks sent to one player per call to [`World::tick_chunks`].
    pub chunks_per_tick: usize,
    /// Where new players appear.
    pub spawn: Vec3,
    /// Bounds on client-reported movement.
    pub limits: MovementLimits,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            view_distance: 10,
            chunks_per_tick: 4,
            spawn: Vec3::new(0.5, 64.0, 0.5),
            limits: MovementLimits::default(),
        }
    }
}

/// The chunk containing a world-space position.
pub fn chunk_of(position: Vec3) -> ChunkPos {
    let x = position.x.floor() as i32;
    let z = position.z.floor() as i32;
    BlockPos::new(x, 0, z).chunk_pos()
}

/// A world with its players.
#[derive(Debug)]
pub struct World {
    generator: FlatGenerator,
    chunks: HashMap<ChunkPos, Chunk>,
    players: PlayerManager,
    trackers: HashMap<EntityId, ChunkTracker>,
    config: WorldConfig,
}

impl World {
    /// Creates an empty world.
    pub fn new(generator: FlatGenerator, config: WorldConfig) -> Self {
        Self {
            generator,
            chunks: HashMap::new(),
            players: PlayerManager::new(),
            trackers: HashMap::new(),
            config,
        }
    }

    /// The connected players.
    pub fn players(&self) -> &PlayerManager {
        &self.players
    }

    /// A chunk, if it has been generated.
    pub fn chunk(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&pos)
    }

    /// Number of chunks held in memory.
    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Adds a player at the spawn point and queues the chunks around it.
    pub fn join(&mut self, uuid: Uuid, name: String) -> Result<EntityId, JoinError> {
        let id = self
            .players
            .join(uuid, name, self.config.spawn, &self.config.limits)?;
        let mut tracker = ChunkTracker::new(self.config.view_distance);
        tracker.update(chunk_of(self.config.spawn));
        self.trackers.insert(id, tracker);
        Ok(id)
    }

    /// Removes a player.
    pub fn leave(&mut self, id: EntityId) -> Option<Player> {
        self.trackers.remove(&id);
        self.players.leave(id)
    }

    /// Applies a client-reported move.
    ///
    /// A rejected move changes nothing and yields a [`Outbound::PositionCorrection`]
    /// so the client snaps back. A valid move that enters a new chunk yields
    /// unload messages for chunks that fell out of view; newly visible chunks are
    /// sent by [`World::tick_chunks`]. Returns `None` if the player is unknown.
    pub fn move_player(
        &mut self,
        id: EntityId,
        target: Vec3,
        rotation: Rotation,
        on_ground: bool,
    ) -> Option<Vec<Outbound>> {
        let result = self
            .players
            .move_player(id, target, rotation, on_ground, &self.config.limits);
        match result {
            Err(PlayerMoveError::NoSuchPlayer) => None,
            Err(PlayerMoveError::Move(_)) => {
                let player = self.players.get(id)?;
                Some(vec![Outbound::PositionCorrection(
                    id,
                    player.position(),
                    player.rotation(),
                )])
            }
            Ok(()) => Some(self.track(id, target)),
        }
    }

    /// Moves a player anywhere in the world and tells its client.
    pub fn teleport(
        &mut self,
        id: EntityId,
        target: Vec3,
        rotation: Rotation,
    ) -> Result<Vec<Outbound>, PlayerMoveError> {
        let player = self
            .players
            .get_mut(id)
            .ok_or(PlayerMoveError::NoSuchPlayer)?;
        player
            .teleport(target, rotation, &self.config.limits)
            .map_err(PlayerMoveError::Move)?;
        let (position, rotation) = (player.position(), player.rotation());
        let mut out = self.track(id, position);
        out.push(Outbound::PositionCorrection(id, position, rotation));
        Ok(out)
    }

    fn track(&mut self, id: EntityId, position: Vec3) -> Vec<Outbound> {
        let Some(tracker) = self.trackers.get_mut(&id) else {
            return Vec::new();
        };
        tracker
            .update(chunk_of(position))
            .into_iter()
            .map(|pos| Outbound::ChunkUnload(id, pos))
            .collect()
    }

    /// Hands each player its next few queued chunks, generating them if needed.
    ///
    /// Players are served in entity-id order so results are deterministic.
    pub fn tick_chunks(&mut self) -> Vec<Outbound> {
        let mut ids: Vec<EntityId> = self.trackers.keys().copied().collect();
        ids.sort();
        let mut out = Vec::new();
        for id in ids {
            let Some(tracker) = self.trackers.get_mut(&id) else {
                continue;
            };
            for pos in tracker.next_batch(self.config.chunks_per_tick) {
                self.chunks
                    .entry(pos)
                    .or_insert_with(|| self.generator.generate(pos));
                out.push(Outbound::ChunkData(id, pos));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockStateId;
    use crate::chunk::WorldHeight;
    use crate::generator::Layer;

    fn world(view_distance: u8, chunks_per_tick: usize) -> World {
        let generator = FlatGenerator::new(
            WorldHeight::default(),
            vec![Layer {
                block: BlockStateId(1),
                thickness: 4,
            }],
        );
        let config = WorldConfig {
            view_distance,
            chunks_per_tick,
            ..WorldConfig::default()
        };
        World::new(generator, config)
    }

    fn chunk_messages(out: &[Outbound]) -> Vec<ChunkPos> {
        out.iter()
            .filter_map(|message| match message {
                Outbound::ChunkData(_, pos) => Some(*pos),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn chunk_of_floors_negative_coordinates() {
        assert_eq!(chunk_of(Vec3::new(0.5, 0.0, 0.5)), ChunkPos::new(0, 0));
        assert_eq!(chunk_of(Vec3::new(-0.5, 0.0, 15.9)), ChunkPos::new(-1, 0));
        assert_eq!(chunk_of(Vec3::new(16.0, 0.0, -16.5)), ChunkPos::new(1, -2));
    }

    #[test]
    fn joining_streams_chunks_nearest_first_at_a_bounded_rate() {
        let mut world = world(2, 4);
        let id = world.join(Uuid([1; 16]), "Steve".to_owned()).unwrap();
        let first = world.tick_chunks();
        assert_eq!(first.len(), 4);
        assert_eq!(first[0], Outbound::ChunkData(id, ChunkPos::new(0, 0)));

        let mut total = first.len();
        loop {
            let batch = world.tick_chunks();
            if batch.is_empty() {
                break;
            }
            assert!(batch.len() <= 4);
            total += batch.len();
        }
        assert_eq!(total, 25);
        assert_eq!(world.loaded_chunks(), 25);
    }

    #[test]
    fn generated_chunks_hold_the_generated_terrain() {
        let mut world = world(2, 100);
        world.join(Uuid([1; 16]), "Steve".to_owned()).unwrap();
        world.tick_chunks();
        let chunk = world.chunk(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(chunk.get_block(3, -64, 3), Some(BlockStateId(1)));
        assert_eq!(chunk.get_block(3, -60, 3), Some(BlockStateId::AIR));
    }

    #[test]
    fn players_share_cached_chunks() {
        let mut world = world(2, 100);
        world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        world.join(Uuid([2; 16]), "B".to_owned()).unwrap();
        let out = world.tick_chunks();
        assert_eq!(chunk_messages(&out).len(), 50);
        assert_eq!(world.loaded_chunks(), 25);
    }

    #[test]
    fn moving_inside_a_chunk_sends_nothing() {
        let mut world = world(2, 100);
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        world.tick_chunks();
        let out = world
            .move_player(id, Vec3::new(5.0, 64.0, 5.0), Rotation::default(), true)
            .unwrap();
        assert!(out.is_empty());
        assert!(world.tick_chunks().is_empty());
    }

    #[test]
    fn crossing_a_chunk_border_unloads_behind_and_streams_ahead() {
        let mut world = world(2, 100);
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        world.tick_chunks();
        let out = world
            .move_player(id, Vec3::new(17.0, 64.0, 0.5), Rotation::default(), true)
            .unwrap();
        assert_eq!(out.len(), 5);
        assert!(
            out.iter()
                .all(|m| matches!(m, Outbound::ChunkUnload(_, pos) if pos.x == -2))
        );
        let ahead = chunk_messages(&world.tick_chunks());
        assert_eq!(ahead.len(), 5);
        assert!(ahead.iter().all(|pos| pos.x == 3));
    }

    #[test]
    fn rejected_moves_snap_the_client_back() {
        let mut world = world(2, 100);
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        let before = world.players().get(id).unwrap().position();
        let out = world
            .move_player(id, Vec3::new(5_000.0, 64.0, 0.0), Rotation::default(), true)
            .unwrap();
        assert_eq!(
            out,
            vec![Outbound::PositionCorrection(
                id,
                before,
                Rotation::default()
            )]
        );
        assert_eq!(world.players().get(id).unwrap().position(), before);
    }

    #[test]
    fn teleport_moves_the_view_and_corrects_the_client() {
        let mut world = world(2, 100);
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        world.tick_chunks();
        let target = Vec3::new(1600.5, 70.0, 0.5);
        let out = world
            .teleport(id, target, Rotation::new(90.0, 0.0))
            .unwrap();
        let unloads = out
            .iter()
            .filter(|m| matches!(m, Outbound::ChunkUnload(..)))
            .count();
        assert_eq!(unloads, 25);
        assert_eq!(
            out.last(),
            Some(&Outbound::PositionCorrection(
                id,
                target,
                Rotation::new(90.0, 0.0)
            ))
        );
        let sent = chunk_messages(&world.tick_chunks());
        assert_eq!(sent[0], ChunkPos::new(100, 0));
    }

    #[test]
    fn unknown_players_and_invalid_teleports_are_reported() {
        let mut world = world(2, 100);
        let ghost = EntityId(99);
        assert_eq!(
            world.move_player(ghost, Vec3::default(), Rotation::default(), true),
            None
        );
        assert_eq!(
            world.teleport(ghost, Vec3::default(), Rotation::default()),
            Err(PlayerMoveError::NoSuchPlayer)
        );
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        let bad = Vec3::new(f64::NAN, 0.0, 0.0);
        assert!(matches!(
            world.teleport(id, bad, Rotation::default()),
            Err(PlayerMoveError::Move(_))
        ));
    }

    #[test]
    fn leaving_removes_the_player_and_duplicate_joins_fail() {
        let mut world = world(2, 100);
        let id = world.join(Uuid([1; 16]), "A".to_owned()).unwrap();
        assert_eq!(
            world.join(Uuid([1; 16]), "A".to_owned()),
            Err(JoinError::DuplicateUuid)
        );
        assert_eq!(world.leave(id).unwrap().name(), "A");
        assert!(world.players().is_empty());
        assert!(world.tick_chunks().is_empty());
    }
}
