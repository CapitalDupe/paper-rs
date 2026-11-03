//! Which chunks a player has been sent, and which still need sending.
//!
//! The tracker is pure bookkeeping: it never touches chunk data or the network.
//! Moving to a new chunk reports which sent chunks fell out of view and
//! re-queues the missing ones nearest-first; [`ChunkTracker::next_batch`] then
//! hands out the queue a few chunks at a time so a join or a long teleport cannot
//! flood the connection in one tick.

use std::collections::{HashSet, VecDeque};

use crate::pos::ChunkPos;

/// Largest supported view distance, in chunks.
pub const MAX_VIEW_DISTANCE: u8 = 32;

/// Smallest supported view distance, in chunks.
pub const MIN_VIEW_DISTANCE: u8 = 2;

fn distance_squared(a: ChunkPos, b: ChunkPos) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dz = i64::from(a.z) - i64::from(b.z);
    dx * dx + dz * dz
}

/// Per-player chunk visibility state.
#[derive(Debug, Clone)]
pub struct ChunkTracker {
    radius: u8,
    center: Option<ChunkPos>,
    sent: HashSet<ChunkPos>,
    pending: VecDeque<ChunkPos>,
}

impl ChunkTracker {
    /// Creates a tracker; `radius` is clamped to the supported range.
    pub fn new(radius: u8) -> Self {
        Self {
            radius: radius.clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE),
            center: None,
            sent: HashSet::new(),
            pending: VecDeque::new(),
        }
    }

    /// The view distance in chunks.
    pub fn radius(&self) -> u8 {
        self.radius
    }

    /// The chunk the player is currently in.
    pub fn center(&self) -> Option<ChunkPos> {
        self.center
    }

    /// Number of chunks already handed out.
    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    /// Number of chunks still waiting to be handed out.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Whether `pos` has been handed out and not unloaded since.
    pub fn is_sent(&self, pos: ChunkPos) -> bool {
        self.sent.contains(&pos)
    }

    /// Moves the view center.
    ///
    /// Returns the chunks that are no longer visible and must be unloaded on the
    /// client (sorted by position). Newly visible chunks are queued for
    /// [`ChunkTracker::next_batch`]. Calling with the current center does nothing.
    pub fn update(&mut self, center: ChunkPos) -> Vec<ChunkPos> {
        if self.center == Some(center) {
            return Vec::new();
        }
        self.center = Some(center);
        let radius = i32::from(self.radius);
        let mut desired = HashSet::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if let (Some(x), Some(z)) = (center.x.checked_add(dx), center.z.checked_add(dz)) {
                    desired.insert(ChunkPos::new(x, z));
                }
            }
        }
        let mut unload: Vec<ChunkPos> = self
            .sent
            .iter()
            .filter(|pos| !desired.contains(pos))
            .copied()
            .collect();
        unload.sort_by_key(|pos| (pos.x, pos.z));
        for pos in &unload {
            self.sent.remove(pos);
        }
        let mut wanted: Vec<ChunkPos> = desired
            .iter()
            .filter(|pos| !self.sent.contains(pos))
            .copied()
            .collect();
        wanted.sort_by_key(|pos| (distance_squared(center, *pos), pos.x, pos.z));
        self.pending = wanted.into();
        unload
    }

    /// Takes up to `max` queued chunks, nearest first, and marks them sent.
    pub fn next_batch(&mut self, max: usize) -> Vec<ChunkPos> {
        let mut batch = Vec::new();
        while batch.len() < max {
            let Some(pos) = self.pending.pop_front() else {
                break;
            };
            self.sent.insert(pos);
            batch.push(pos);
        }
        batch
    }

    /// Forgets everything, returning the chunks that had been sent (sorted).
    pub fn clear(&mut self) -> Vec<ChunkPos> {
        let mut sent: Vec<ChunkPos> = self.sent.drain().collect();
        sent.sort_by_key(|pos| (pos.x, pos.z));
        self.pending.clear();
        self.center = None;
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(radius: u8) -> usize {
        usize::from(radius) * 2 + 1
    }

    #[test]
    fn radius_is_clamped() {
        assert_eq!(ChunkTracker::new(0).radius(), MIN_VIEW_DISTANCE);
        assert_eq!(ChunkTracker::new(200).radius(), MAX_VIEW_DISTANCE);
        assert_eq!(ChunkTracker::new(8).radius(), 8);
    }

    #[test]
    fn first_update_queues_the_whole_square_nearest_first() {
        let mut tracker = ChunkTracker::new(3);
        assert!(tracker.update(ChunkPos::new(10, -4)).is_empty());
        assert_eq!(tracker.pending_count(), side(3) * side(3));
        let batch = tracker.next_batch(5);
        assert_eq!(batch[0], ChunkPos::new(10, -4));
        let distances: Vec<i64> = batch
            .iter()
            .map(|pos| distance_squared(ChunkPos::new(10, -4), *pos))
            .collect();
        assert!(distances.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn batches_are_bounded_and_drain_the_queue() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        let total = side(2) * side(2);
        assert_eq!(tracker.next_batch(10).len(), 10);
        assert_eq!(tracker.sent_count(), 10);
        assert_eq!(tracker.next_batch(100).len(), total - 10);
        assert_eq!(tracker.pending_count(), 0);
        assert!(tracker.next_batch(10).is_empty());
    }

    #[test]
    fn moving_one_chunk_unloads_the_trailing_edge_only() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        tracker.next_batch(1000);
        let unload = tracker.update(ChunkPos::new(1, 0));
        assert_eq!(unload.len(), side(2));
        assert!(unload.iter().all(|pos| pos.x == -2));
        assert_eq!(tracker.pending_count(), side(2));
        let fresh = tracker.next_batch(1000);
        assert!(fresh.iter().all(|pos| pos.x == 3));
        assert_eq!(tracker.sent_count(), side(2) * side(2));
    }

    #[test]
    fn unchanged_center_is_a_no_op() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        tracker.next_batch(3);
        let before = (tracker.sent_count(), tracker.pending_count());
        assert!(tracker.update(ChunkPos::new(0, 0)).is_empty());
        assert_eq!(before, (tracker.sent_count(), tracker.pending_count()));
    }

    #[test]
    fn a_long_jump_unloads_everything_sent_and_requeues_the_new_area() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        tracker.next_batch(1000);
        let unload = tracker.update(ChunkPos::new(1000, 1000));
        assert_eq!(unload.len(), side(2) * side(2));
        assert_eq!(tracker.sent_count(), 0);
        assert_eq!(tracker.pending_count(), side(2) * side(2));
    }

    #[test]
    fn unsent_chunks_that_leave_view_are_dropped_from_the_queue() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        tracker.next_batch(5);
        let unload = tracker.update(ChunkPos::new(500, 0));
        assert_eq!(unload.len(), 5);
        let queued = tracker.next_batch(1000);
        assert!(queued.iter().all(|pos| pos.x >= 498));
    }

    #[test]
    fn clear_reports_what_was_sent() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(0, 0));
        tracker.next_batch(4);
        assert_eq!(tracker.clear().len(), 4);
        assert_eq!(tracker.center(), None);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn coordinates_near_the_integer_limits_do_not_overflow() {
        let mut tracker = ChunkTracker::new(2);
        tracker.update(ChunkPos::new(i32::MAX, i32::MIN));
        assert_eq!(tracker.pending_count(), 9);
    }
}
