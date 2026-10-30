//! Ear break reminder (SPEC §4.3b): after X hours of listening on headphones,
//! one reminder. Only time playing on a headphones output counts; a pause of
//! 15 minutes or more starts the count again. Pure: the caller passes `now`.

use std::time::{Duration, Instant};

/// A rest this long counts as having rested the ears.
pub const REST: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Default)]
pub struct EarBreak {
    /// Listening counted before the current stretch.
    listened: Duration,
    /// Start of the current listening stretch.
    since: Option<Instant>,
    /// Start of the current quiet stretch.
    quiet_since: Option<Instant>,
    reminded: bool,
}

impl EarBreak {
    fn total(&self, now: Instant) -> Duration {
        self.listened
            + self
                .since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    /// Feeds whether the noise is playing on headphones now. True once per
    /// stretch, when `after` of listening has been reached.
    pub fn update(&mut self, now: Instant, listening: bool, after: Duration) -> bool {
        if listening {
            if let Some(quiet) = self.quiet_since.take() {
                if now.saturating_duration_since(quiet) >= REST {
                    *self = Self::default();
                }
            }
            self.since.get_or_insert(now);
            if !self.reminded && self.total(now) >= after {
                self.reminded = true;
                return true;
            }
        } else {
            if let Some(since) = self.since.take() {
                self.listened += now.saturating_duration_since(since);
            }
            self.quiet_since.get_or_insert(now);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);
    const H3: Duration = Duration::from_secs(3 * 3600);

    #[test]
    fn reminds_once_after_three_hours_of_listening() {
        let t0 = Instant::now();
        let mut e = EarBreak::default();
        assert!(!e.update(t0, true, H3));
        assert!(!e.update(t0 + 179 * MIN, true, H3));
        assert!(e.update(t0 + 180 * MIN, true, H3));
        assert!(!e.update(t0 + 240 * MIN, true, H3), "only once");
    }

    #[test]
    fn short_pauses_stop_the_count_but_keep_it() {
        let t0 = Instant::now();
        let mut e = EarBreak::default();
        e.update(t0, true, H3);
        e.update(t0 + 100 * MIN, false, H3);
        // Ten minutes off: not a rest, and not counted either.
        e.update(t0 + 110 * MIN, true, H3);
        assert!(!e.update(t0 + 189 * MIN, true, H3));
        assert!(e.update(t0 + 190 * MIN, true, H3));
    }

    #[test]
    fn a_long_rest_starts_again() {
        let t0 = Instant::now();
        let mut e = EarBreak::default();
        e.update(t0, true, H3);
        e.update(t0 + 170 * MIN, false, H3);
        e.update(t0 + 185 * MIN, true, H3);
        assert!(!e.update(t0 + 300 * MIN, true, H3));
        assert!(e.update(t0 + 365 * MIN, true, H3));
    }

    #[test]
    fn after_a_reminder_a_rest_allows_another() {
        let t0 = Instant::now();
        let mut e = EarBreak::default();
        e.update(t0, true, H3);
        assert!(e.update(t0 + 180 * MIN, true, H3));
        e.update(t0 + 181 * MIN, false, H3);
        e.update(t0 + 200 * MIN, true, H3);
        assert!(e.update(t0 + 380 * MIN, true, H3));
    }
}
