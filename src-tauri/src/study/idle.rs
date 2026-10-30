//! Keyboard and mouse inactivity (SPEC §4.3b): Windows says how long ago the
//! last input was (`GetLastInputInfo`); `IdleWatch` turns that into "away
//! from X to Y" once the user is back.

use std::time::{Duration, Instant};

/// Time since the last keyboard or mouse input, for the whole session.
#[cfg(windows)]
pub fn since_last_input() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid, correctly sized LASTINPUTINFO.
    let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
    // Both are 32-bit millisecond tick counts; the difference survives the
    // wrap every 49.7 days.
    let now = unsafe { GetTickCount() };
    ok.then(|| Duration::from_millis(u64::from(now.wrapping_sub(info.dwTime))))
}

#[cfg(not(windows))]
pub fn since_last_input() -> Option<Duration> {
    None
}

/// A stretch without input, reported when the user comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Away {
    pub from: Instant,
    pub to: Instant,
}

impl Away {
    pub fn length(&self) -> Duration {
        self.to.saturating_duration_since(self.from)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IdleWatch {
    away_since: Option<Instant>,
}

impl IdleWatch {
    /// Feeds the current idle time. Returns the stretch away once there is
    /// input again after at least `threshold` without any.
    pub fn update(&mut self, now: Instant, idle: Duration, threshold: Duration) -> Option<Away> {
        let last_input = now.checked_sub(idle).unwrap_or(now);
        match self.away_since {
            None if idle >= threshold => {
                self.away_since = Some(last_input);
                None
            }
            Some(from) if idle < threshold => {
                self.away_since = None;
                Some(Away {
                    from,
                    to: last_input.max(from),
                })
            }
            _ => None,
        }
    }

    /// Forget a stretch in progress (the session paused or ended).
    pub fn reset(&mut self) {
        self.away_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn reports_the_stretch_once_the_user_is_back() {
        let t0 = Instant::now();
        let mut w = IdleWatch::default();
        let threshold = 10 * MIN;
        assert_eq!(w.update(t0 + 5 * MIN, 5 * MIN, threshold), None);
        // Last input at t0; 10 min later it counts as away.
        assert_eq!(w.update(t0 + 10 * MIN, 10 * MIN, threshold), None);
        assert_eq!(w.update(t0 + 14 * MIN, 14 * MIN, threshold), None);
        // Input again at minute 14, seen a second later.
        let back = w.update(
            t0 + 14 * MIN + Duration::from_secs(1),
            Duration::from_secs(1),
            threshold,
        );
        assert_eq!(
            back,
            Some(Away {
                from: t0,
                to: t0 + 14 * MIN
            })
        );
        assert_eq!(back.map(|a| a.length()), Some(14 * MIN));
        assert_eq!(w.update(t0 + 15 * MIN, Duration::ZERO, threshold), None);
    }

    #[test]
    fn short_breaks_are_not_reported() {
        let t0 = Instant::now();
        let mut w = IdleWatch::default();
        assert_eq!(w.update(t0 + 9 * MIN, 9 * MIN, 10 * MIN), None);
        assert_eq!(w.update(t0 + 9 * MIN, Duration::ZERO, 10 * MIN), None);
    }
}
