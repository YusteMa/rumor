//! Sleep timer (SPEC §4.2): counts down while the noise plays and fades it
//! out at the end. Measured against `Instant`, never by adding up ticks
//! (CLAUDE.md rule 11). Pure: the caller passes `now`.

use std::time::{Duration, Instant};

/// Long fade when the timer runs out (SPEC §4.4, "unos 10 s").
pub const END_FADE_MS: f32 = 10_000.0;
/// Longest timer accepted from the panel.
pub const MAX_MINUTES: u32 = 24 * 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timer {
    minutes: u32,
    deadline: Option<Instant>,
}

impl Timer {
    /// Chosen length in minutes; 0 means no timer.
    pub fn minutes(&self) -> u32 {
        self.minutes
    }

    /// Chooses a length (0 = none). If the noise is playing the countdown
    /// restarts now; otherwise it starts with the next play.
    pub fn set(&mut self, minutes: u32, playing: bool, now: Instant) {
        self.minutes = minutes.min(MAX_MINUTES);
        self.on_playing(playing, now);
    }

    /// Playback started or stopped: the countdown starts with playback and is
    /// cancelled when it stops.
    pub fn on_playing(&mut self, playing: bool, now: Instant) {
        self.deadline = (playing && self.minutes > 0)
            .then(|| now + Duration::from_secs(u64::from(self.minutes) * 60));
    }

    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    /// Whole seconds left, rounded up so the display never shows 0:00 early.
    pub fn remaining_secs(&self, now: Instant) -> Option<u64> {
        self.remaining(now)
            .map(|r| r.as_secs() + u64::from(r.subsec_nanos() > 0))
    }

    /// True once when the countdown runs out; the timer then disarms (it keeps
    /// its length for the next play).
    pub fn take_expired(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
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
    fn counts_down_only_while_playing() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, false, t0);
        assert_eq!(t.remaining(t0), None, "armed, waiting for play");
        t.on_playing(true, t0 + mins(1));
        assert_eq!(t.remaining(t0 + mins(11)), Some(mins(15)));
        t.on_playing(false, t0 + mins(12));
        assert_eq!(t.remaining(t0 + mins(12)), None);
        assert_eq!(t.minutes(), 25, "keeps its length");
    }

    #[test]
    fn choosing_while_playing_restarts_and_zero_cancels() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(50, true, t0);
        assert_eq!(t.remaining(t0 + mins(10)), Some(mins(40)));
        t.set(25, true, t0 + mins(10));
        assert_eq!(t.remaining(t0 + mins(10)), Some(mins(25)));
        t.set(0, true, t0 + mins(11));
        assert_eq!(t.remaining(t0 + mins(11)), None);
    }

    #[test]
    fn expires_exactly_once() {
        let t0 = Instant::now();
        let mut t = Timer::default();
        t.set(25, true, t0);
        assert!(!t.take_expired(t0 + mins(25) - Duration::from_millis(1)));
        assert!(t.take_expired(t0 + mins(25)));
        assert!(!t.take_expired(t0 + mins(26)));
        assert_eq!(t.remaining(t0 + mins(26)), None);
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
