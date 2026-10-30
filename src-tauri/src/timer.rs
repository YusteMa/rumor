//! Sleep timer (SPEC §4.2): counts down while the noise plays; the caller
//! fades the noise out at the end (Settings → Timer sets how long).
//! Measured against `Instant`, never by adding up ticks (CLAUDE.md rule 11).
//! Pure: the caller passes `now`.
//!
//! Only choosing a length (re)starts the countdown. Pausing freezes it and
//! playing continues it; nothing else (colour, volume…) touches it.

use std::time::{Duration, Instant};

/// Longest timer accepted from the panel.
pub const MAX_MINUTES: u32 = 24 * 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timer {
    minutes: u32,
    /// Set while counting down (playing).
    deadline: Option<Instant>,
    /// What was left when playback paused mid-countdown.
    frozen: Option<Duration>,
}

impl Timer {
    fn full(&self) -> Duration {
        Duration::from_secs(u64::from(self.minutes) * 60)
    }

    /// Chosen length in minutes; 0 means no timer.
    pub fn minutes(&self) -> u32 {
        self.minutes
    }

    /// Chooses a length (0 = none) and starts it afresh: now if the noise is
    /// playing, otherwise with the next play.
    pub fn set(&mut self, minutes: u32, playing: bool, now: Instant) {
        self.minutes = minutes.min(MAX_MINUTES);
        self.frozen = None;
        self.deadline = (playing && self.minutes > 0).then(|| now + self.full());
    }

    /// Playback started or stopped: stopping freezes the countdown, playing
    /// continues it from where it was (or starts it, if it hadn't started).
    pub fn on_playing(&mut self, playing: bool, now: Instant) {
        if playing {
            if self.minutes > 0 && self.deadline.is_none() {
                let left = self.frozen.take().unwrap_or_else(|| self.full());
                self.deadline = Some(now + left);
            }
        } else if let Some(deadline) = self.deadline.take() {
            self.frozen = Some(deadline.saturating_duration_since(now));
        }
    }

    /// Time left: counting down while playing, frozen while paused, `None`
    /// when no countdown has started.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(now))
            .or(self.frozen)
    }

    /// Whole seconds left, rounded up so the display never shows 0:00 early.
    pub fn remaining_secs(&self, now: Instant) -> Option<u64> {
        self.remaining(now)
            .map(|r| r.as_secs() + u64::from(r.subsec_nanos() > 0))
    }

    /// True once when the countdown runs out; the timer then disarms (it keeps
    /// its length for the next play, which starts it afresh).
    pub fn take_expired(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            self.frozen = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    #[test]
    fn starts_with_the_first_play() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, false, t0);
        assert_eq!(t.remaining(t0), None, "armed, waiting for play");
        t.on_playing(true, t0 + mins(1));
        assert_eq!(t.remaining(t0 + mins(11)), Some(mins(15)));
        assert_eq!(t.minutes(), 25, "keeps its length");
    }

    #[test]
    fn pausing_freezes_and_playing_continues() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, true, t0);
        t.on_playing(false, t0 + mins(10));
        // Paused for an hour: nothing passes.
        assert_eq!(t.remaining(t0 + mins(70)), Some(mins(15)));
        t.on_playing(true, t0 + mins(70));
        assert_eq!(t.remaining(t0 + mins(75)), Some(mins(10)));
    }

    #[test]
    fn repeated_play_or_pause_changes_nothing() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, true, t0);
        // Already playing (e.g. a colour change refreshes the state).
        t.on_playing(true, t0 + mins(5));
        assert_eq!(t.remaining(t0 + mins(5)), Some(mins(20)));
        t.on_playing(false, t0 + mins(6));
        t.on_playing(false, t0 + mins(9));
        assert_eq!(t.remaining(t0 + mins(9)), Some(mins(19)));
    }

    #[test]
    fn choosing_restarts_and_zero_cancels() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(50, true, t0);
        assert_eq!(t.remaining(t0 + mins(10)), Some(mins(40)));
        t.set(25, true, t0 + mins(10));
        assert_eq!(t.remaining(t0 + mins(10)), Some(mins(25)));
        // Choosing while paused discards the frozen time.
        t.on_playing(false, t0 + mins(12));
        t.set(50, false, t0 + mins(12));
        assert_eq!(t.remaining(t0 + mins(12)), None);
        t.set(0, true, t0 + mins(13));
        assert_eq!(t.remaining(t0 + mins(13)), None);
    }

    #[test]
    fn expires_exactly_once_and_restarts_full_next_time() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, true, t0);
        assert!(!t.take_expired(t0 + mins(25) - Duration::from_millis(1)));
        assert!(t.take_expired(t0 + mins(25)));
        assert!(!t.take_expired(t0 + mins(26)));
        assert_eq!(t.remaining(t0 + mins(26)), None);
        t.on_playing(false, t0 + mins(26));
        t.on_playing(true, t0 + mins(30));
        assert_eq!(t.remaining(t0 + mins(30)), Some(mins(25)));
    }

    #[test]
    fn remaining_seconds_round_up() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, true, t0);
        assert_eq!(
            t.remaining_secs(t0 + Duration::from_millis(500)),
            Some(25 * 60)
        );
        assert_eq!(
            t.remaining_secs(t0 + mins(25) - Duration::from_millis(1)),
            Some(1)
        );
    }

    #[test]
    fn clamps_absurd_lengths() {
        let mut t = Timer::default();
        t.set(u32::MAX, false, Instant::now());
        assert_eq!(t.minutes(), MAX_MINUTES);
    }
}
