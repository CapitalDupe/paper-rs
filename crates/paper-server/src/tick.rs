//! Fixed-step tick scheduling and timing metrics.
//!
//! [`TickClock`] is pure: given the elapsed time it says how many ticks are due,
//! so the scheduling rules are unit-testable without a real clock. When the
//! server falls far behind it runs a bounded number of catch-up ticks and then
//! drops the rest of the backlog instead of spiraling. [`run`] drives a callback
//! at that rate until shutdown, recording timings in [`TickMetrics`].
//!
//! The callback runs on the async runtime, so it must stay short. Moving world
//! ticking to a dedicated thread is a later change.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

/// The standard 20 ticks per second.
pub const TICK_DURATION: Duration = Duration::from_millis(50);

/// Most ticks run back-to-back to catch up before the backlog is dropped.
pub const MAX_CATCH_UP_TICKS: u32 = 10;

/// How much work is due at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    /// Ticks to run now.
    pub ticks: u32,
    /// Ticks dropped because the server fell too far behind.
    pub skipped: u64,
}

/// Decides how many ticks are due, given elapsed time since the start.
#[derive(Debug, Clone)]
pub struct TickClock {
    step: Duration,
    next_due: Duration,
    max_catch_up: u32,
}

impl TickClock {
    /// Creates a clock whose first tick is due immediately.
    pub fn new(step: Duration, max_catch_up: u32) -> Self {
        Self {
            step,
            next_due: Duration::ZERO,
            max_catch_up,
        }
    }

    /// When the next tick is due, as time since the start.
    pub fn next_due(&self) -> Duration {
        self.next_due
    }

    /// Returns the ticks due at `now`, advancing the schedule.
    pub fn due(&mut self, now: Duration) -> Due {
        let mut ticks = 0;
        while self.next_due <= now && ticks < self.max_catch_up {
            ticks += 1;
            self.next_due += self.step;
        }
        let mut skipped = 0;
        if self.next_due <= now {
            let behind = now - self.next_due;
            skipped = (behind.as_nanos() / self.step.as_nanos() + 1) as u64;
            self.next_due = now + self.step;
        }
        Due { ticks, skipped }
    }
}

/// Lock-free tick timing counters, safe to read from other tasks.
#[derive(Debug, Default)]
pub struct TickMetrics {
    ticks: AtomicU64,
    total_micros: AtomicU64,
    max_micros: AtomicU64,
    slow_ticks: AtomicU64,
    skipped_ticks: AtomicU64,
}

/// A point-in-time copy of [`TickMetrics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickSnapshot {
    /// Ticks run.
    pub ticks: u64,
    /// Mean tick duration.
    pub average: Duration,
    /// Longest tick duration.
    pub max: Duration,
    /// Ticks that took longer than the tick step.
    pub slow_ticks: u64,
    /// Ticks dropped while overloaded.
    pub skipped_ticks: u64,
}

impl TickMetrics {
    /// Records one tick that took `duration` against a budget of `step`.
    pub fn record(&self, duration: Duration, step: Duration) {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        self.ticks.fetch_add(1, Ordering::Relaxed);
        self.total_micros.fetch_add(micros, Ordering::Relaxed);
        self.max_micros.fetch_max(micros, Ordering::Relaxed);
        if duration > step {
            self.slow_ticks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records ticks dropped while overloaded.
    pub fn record_skipped(&self, count: u64) {
        self.skipped_ticks.fetch_add(count, Ordering::Relaxed);
    }

    /// Copies the current counters.
    pub fn snapshot(&self) -> TickSnapshot {
        let ticks = self.ticks.load(Ordering::Relaxed);
        let total = self.total_micros.load(Ordering::Relaxed);
        let average = total.checked_div(ticks).unwrap_or(0);
        TickSnapshot {
            ticks,
            average: Duration::from_micros(average),
            max: Duration::from_micros(self.max_micros.load(Ordering::Relaxed)),
            slow_ticks: self.slow_ticks.load(Ordering::Relaxed),
            skipped_ticks: self.skipped_ticks.load(Ordering::Relaxed),
        }
    }
}

/// Runs `on_tick` at the fixed rate until `shutdown` becomes true.
///
/// `on_tick` receives the 1-based tick number.
pub async fn run<F>(metrics: Arc<TickMetrics>, mut shutdown: watch::Receiver<bool>, mut on_tick: F)
where
    F: FnMut(u64),
{
    let start = Instant::now();
    let mut clock = TickClock::new(TICK_DURATION, MAX_CATCH_UP_TICKS);
    let mut tick_number: u64 = 0;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let due = clock.due(start.elapsed());
        if due.skipped > 0 {
            tracing::warn!("server overloaded: skipped {} ticks", due.skipped);
            metrics.record_skipped(due.skipped);
        }
        for _ in 0..due.ticks {
            let began = Instant::now();
            tick_number += 1;
            on_tick(tick_number);
            metrics.record(began.elapsed(), TICK_DURATION);
        }
        tokio::select! {
            _ = tokio::time::sleep_until(start + clock.next_due()) => {}
            _ = shutdown.changed() => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_millis(50);

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn first_tick_is_due_immediately_then_every_step() {
        let mut clock = TickClock::new(STEP, 10);
        assert_eq!(
            clock.due(ms(0)),
            Due {
                ticks: 1,
                skipped: 0
            }
        );
        assert_eq!(
            clock.due(ms(49)),
            Due {
                ticks: 0,
                skipped: 0
            }
        );
        assert_eq!(
            clock.due(ms(50)),
            Due {
                ticks: 1,
                skipped: 0
            }
        );
        assert_eq!(clock.next_due(), ms(100));
    }

    #[test]
    fn runs_catch_up_ticks_when_slightly_behind() {
        let mut clock = TickClock::new(STEP, 10);
        clock.due(ms(0));
        assert_eq!(
            clock.due(ms(200)),
            Due {
                ticks: 4,
                skipped: 0
            }
        );
    }

    #[test]
    fn drops_the_backlog_when_far_behind() {
        let mut clock = TickClock::new(STEP, 10);
        clock.due(ms(0));
        let due = clock.due(ms(10_000));
        assert_eq!(
            due,
            Due {
                ticks: 10,
                skipped: 190
            }
        );
        assert_eq!(clock.next_due(), ms(10_050));
    }

    #[test]
    fn metrics_track_average_max_and_slow_ticks() {
        let metrics = TickMetrics::default();
        metrics.record(ms(10), STEP);
        metrics.record(ms(30), STEP);
        metrics.record(ms(80), STEP);
        metrics.record_skipped(3);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.ticks, 3);
        assert_eq!(snapshot.average, ms(40));
        assert_eq!(snapshot.max, ms(80));
        assert_eq!(snapshot.slow_ticks, 1);
        assert_eq!(snapshot.skipped_ticks, 3);
    }

    #[test]
    fn empty_metrics_have_zero_average() {
        assert_eq!(TickMetrics::default().snapshot().average, Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn loop_ticks_at_the_fixed_rate_and_stops_on_shutdown() {
        let metrics = Arc::new(TickMetrics::default());
        let (tx, rx) = watch::channel(false);
        let last = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&last);
        let handle = tokio::spawn(run(Arc::clone(&metrics), rx, move |number| {
            seen.store(number, Ordering::SeqCst);
        }));
        tokio::time::sleep(ms(525)).await;
        tx.send(true).unwrap();
        handle.await.unwrap();
        let ticks = metrics.snapshot().ticks;
        assert!((10..=12).contains(&ticks), "ticks: {ticks}");
        assert_eq!(last.load(Ordering::SeqCst), ticks);
    }
}
