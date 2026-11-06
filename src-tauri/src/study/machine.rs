//! Stopwatch and Pomodoro state machine (SPEC §4.3b). Pure: the caller passes
//! `now` (an `Instant`, which measures every duration, CLAUDE.md rule 11) and
//! the wall clock at that moment (only to date what gets stored).
//!
//! Only work counts. A session is a list of work segments; pauses, Pomodoro
//! breaks and discounted idle time fall between them, so totals per day come
//! out exact even for a session that crosses midnight.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

pub use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Counts up; no phases (SPEC §9 decision 8: the default).
    #[default]
    Stopwatch,
    Pomodoro,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn is_break(self) -> bool {
        self != Self::Work
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub work: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
    /// Work blocks before a long break.
    pub cycles: u32,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            work: Duration::from_secs(25 * 60),
            short_break: Duration::from_secs(5 * 60),
            long_break: Duration::from_secs(15 * 60),
            cycles: 4,
        }
    }
}

impl Plan {
    fn length(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Work => self.work,
            Phase::ShortBreak => self.short_break,
            Phase::LongBreak => self.long_break,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Run {
    #[default]
    Stopped,
    Running,
    Paused,
}

/// A stretch of work. `wall` is the wall-clock time at `start`; the end's
/// wall time is derived from the `Instant` length, so changing the clock
/// never changes a duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    start: Instant,
    wall: SystemTime,
    /// `None` while it is still running.
    end: Option<Instant>,
}

impl Segment {
    fn len(&self, now: Instant) -> Duration {
        self.end
            .unwrap_or(now)
            .saturating_duration_since(self.start)
    }
}

/// A Pomodoro phase change, reported by `tick`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseChange {
    pub to: Phase,
    /// Work block number (1-based) of the phase entered.
    pub cycle: u32,
}

/// A finished session, ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub started: SystemTime,
    pub ended: SystemTime,
    pub net: Duration,
    pub mode: Mode,
    /// Closed work segments as wall-clock intervals.
    pub segments: Vec<(SystemTime, SystemTime)>,
}

#[derive(Clone, Debug, Default)]
pub struct Machine {
    mode: Mode,
    plan: Plan,
    run: Run,
    started: Option<(Instant, SystemTime)>,
    segments: Vec<Segment>,
    phase: Phase,
    cycle: u32,
    /// Phase time already spent before the current run of it.
    phase_spent: Duration,
    /// When the current phase started running (while running).
    phase_since: Option<Instant>,
}

impl Machine {
    pub fn new(mode: Mode, plan: Plan) -> Self {
        Self {
            mode,
            plan,
            cycle: 1,
            ..Self::default()
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn run(&self) -> Run {
        self.run
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn cycle(&self) -> u32 {
        self.cycle
    }

    pub fn plan(&self) -> Plan {
        self.plan
    }

    /// Wall-clock start of the session, while there is one.
    pub fn started(&self) -> Option<SystemTime> {
        self.started.map(|(_, wall)| wall)
    }

    /// Debug builds: ends the current Pomodoro phase now, for screenshots.
    #[cfg(any(debug_assertions, test))]
    pub fn end_phase(&mut self, now: Instant) {
        if self.mode == Mode::Pomodoro && self.run == Run::Running {
            self.phase_spent = self.plan.length(self.phase);
            self.phase_since = Some(now);
        }
    }

    pub fn has_session(&self) -> bool {
        self.run != Run::Stopped
    }

    /// The mode only changes between sessions. Returns whether it changed.
    pub fn set_mode(&mut self, mode: Mode) -> bool {
        if self.has_session() || self.mode == mode {
            return false;
        }
        self.mode = mode;
        true
    }

    /// New Pomodoro lengths apply at once; a phase already past its new
    /// length ends on the next tick.
    pub fn set_plan(&mut self, plan: Plan) {
        self.plan = Plan {
            cycles: plan.cycles.max(1),
            ..plan
        };
        self.cycle = self.cycle.min(self.plan.cycles);
    }

    fn counts(&self) -> bool {
        self.run == Run::Running && !self.phase.is_break()
    }

    fn open_segment(&mut self, now: Instant, wall: SystemTime) {
        if self.segments.last().is_none_or(|s| s.end.is_some()) {
            self.segments.push(Segment {
                start: now,
                wall,
                end: None,
            });
        }
    }

    fn close_segment(&mut self, at: Instant) {
        if let Some(s) = self.segments.last_mut().filter(|s| s.end.is_none()) {
            s.end = Some(at.max(s.start));
        }
    }

    /// Starts a session (from stopped) or continues a paused one.
    pub fn start(&mut self, now: Instant, wall: SystemTime) {
        match self.run {
            Run::Running => return,
            Run::Stopped => {
                self.started = Some((now, wall));
                self.segments.clear();
                self.phase = Phase::Work;
                self.cycle = 1;
                self.phase_spent = Duration::ZERO;
            }
            Run::Paused => {}
        }
        self.run = Run::Running;
        self.phase_since = Some(now);
        if !self.phase.is_break() {
            self.open_segment(now, wall);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if self.run != Run::Running {
            return;
        }
        self.close_segment(now);
        if let Some(since) = self.phase_since.take() {
            self.phase_spent += now.saturating_duration_since(since);
        }
        self.run = Run::Paused;
    }

    /// Ends the session; `None` if there was none or no work was counted.
    pub fn finish(&mut self, now: Instant) -> Option<Record> {
        if !self.has_session() {
            return None;
        }
        self.close_segment(now);
        let record = self.record(now);
        *self = Self::new(self.mode, self.plan);
        record.filter(|r| !r.net.is_zero())
    }

    /// The session so far, as it would be stored (open segment cut at `now`).
    pub fn record(&self, now: Instant) -> Option<Record> {
        let (started_at, started) = self.started?;
        let segments: Vec<_> = self
            .segments
            .iter()
            .map(|s| (s.wall, s.wall + s.len(now)))
            .filter(|(a, b)| b > a)
            .collect();
        let end = self
            .segments
            .last()
            .map_or(started_at, |s| s.end.unwrap_or(now));
        let ended = started + end.saturating_duration_since(started_at);
        Some(Record {
            started,
            ended,
            net: self.worked(now),
            mode: self.mode,
            segments,
        })
    }

    /// Work counted in this session.
    pub fn worked(&self, now: Instant) -> Duration {
        self.segments.iter().map(|s| s.len(now)).sum()
    }

    fn phase_elapsed(&self, now: Instant) -> Duration {
        self.phase_spent
            + self
                .phase_since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    /// Time left in the current Pomodoro phase, and its full length.
    pub fn phase_left(&self, now: Instant) -> Option<(Duration, Duration)> {
        if self.mode != Mode::Pomodoro || !self.has_session() {
            return None;
        }
        let total = self.plan.length(self.phase);
        Some((total.saturating_sub(self.phase_elapsed(now)), total))
    }

    fn next_phase(&self) -> (Phase, u32) {
        match self.phase {
            Phase::Work if self.cycle >= self.plan.cycles => (Phase::LongBreak, self.cycle),
            Phase::Work => (Phase::ShortBreak, self.cycle),
            Phase::ShortBreak => (Phase::Work, self.cycle + 1),
            Phase::LongBreak => (Phase::Work, 1),
        }
    }

    /// Moves the Pomodoro on by itself. Each change happens at the exact
    /// instant the phase ran out, whatever the polling rate.
    pub fn tick(&mut self, now: Instant, wall: SystemTime) -> Vec<PhaseChange> {
        let mut changes = Vec::new();
        if self.mode != Mode::Pomodoro || self.run != Run::Running {
            return changes;
        }
        while let Some(since) = self.phase_since {
            let length = self.plan.length(self.phase);
            let boundary = since + length.saturating_sub(self.phase_spent);
            if now < boundary {
                break;
            }
            if !self.phase.is_break() {
                self.close_segment(boundary);
            }
            let (phase, cycle) = self.next_phase();
            self.phase = phase;
            self.cycle = cycle;
            self.phase_spent = Duration::ZERO;
            self.phase_since = Some(boundary);
            if !phase.is_break() {
                let ago = now.saturating_duration_since(boundary);
                self.open_segment(boundary, wall.checked_sub(ago).unwrap_or(wall));
            }
            changes.push(PhaseChange { to: phase, cycle });
        }
        changes
    }

    /// Takes `from..to` out of the counted work ("No, descontar" after being
    /// idle). The Pomodoro clock is not affected.
    pub fn discount(&mut self, from: Instant, to: Instant) {
        if to <= from {
            return;
        }
        let mut kept = Vec::with_capacity(self.segments.len() + 1);
        for s in self.segments.drain(..) {
            let end = s.end;
            let stops = end.is_some_and(|e| e <= from);
            if s.start >= to || stops {
                kept.push(s);
                continue;
            }
            if s.start < from {
                kept.push(Segment {
                    end: Some(from),
                    ..s
                });
            }
            if end.is_none_or(|e| e > to) {
                kept.push(Segment {
                    start: to,
                    wall: s.wall + to.saturating_duration_since(s.start),
                    end,
                });
            }
        }
        self.segments = kept;
        // A running work phase always has an open segment, even if the cut
        // reached past `now`.
        if self.counts() && self.segments.last().is_none_or(|s| s.end.is_some()) {
            let at = to;
            let wall = self
                .started
                .map(|(i, w)| w + at.saturating_duration_since(i))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            self.segments.push(Segment {
                start: at,
                wall,
                end: None,
            });
        }
    }

    /// Whether work is being counted right now (for idle detection).
    pub fn is_counting(&self) -> bool {
        self.counts()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn m(mins: u64) -> Duration {
        s(mins * 60)
    }

    struct Clock {
        t0: Instant,
        w0: SystemTime,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                t0: Instant::now(),
                w0: SystemTime::UNIX_EPOCH + s(1_800_000_000),
            }
        }
        fn at(&self, d: Duration) -> (Instant, SystemTime) {
            (self.t0 + d, self.w0 + d)
        }
    }

    #[test]
    fn the_session_knows_when_it_started_and_a_phase_can_be_cut_short() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Pomodoro, Plan::default());
        assert_eq!(st.started(), None);
        let (now, wall) = c.at(s(0));
        st.start(now, wall);
        assert_eq!(st.started(), Some(c.w0));
        // Ten minutes in, the phase is ended now: the next tick is the break,
        // and the ten minutes worked stay.
        let (now, wall) = c.at(m(10));
        st.end_phase(now);
        let changes = st.tick(now, wall);
        assert_eq!(changes.len(), 1);
        assert_eq!(st.phase(), Phase::ShortBreak);
        assert_eq!(st.worked(now), m(10));
        assert_eq!(st.started(), Some(c.w0), "same session");
        st.finish(now);
        assert_eq!(st.started(), None);
    }

    #[test]
    fn stopwatch_counts_only_while_running() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        st.pause(c.at(m(30)).0);
        // An hour of pause counts nothing.
        let (t, w) = c.at(m(90));
        assert_eq!(st.worked(t), m(30));
        st.start(t, w);
        let r = st.finish(c.at(m(100)).0).unwrap();
        assert_eq!(r.net, m(40));
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.started, c.w0);
        assert_eq!(r.ended, c.w0 + m(100));
        assert_eq!(st.run(), Run::Stopped);
    }

    #[test]
    fn two_hours_with_pauses_match_to_the_second() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let mut expected = Duration::ZERO;
        let mut at = Duration::ZERO;
        // 24 rounds of 4 min 59 s working and 1 min 7 s paused.
        for _ in 0..24 {
            let (t, w) = c.at(at);
            st.start(t, w);
            at += s(299);
            expected += s(299);
            st.pause(c.at(at).0);
            at += s(67);
        }
        assert_eq!(st.worked(c.at(at).0), expected);
        assert_eq!(st.finish(c.at(at).0).unwrap().net, expected);
    }

    #[test]
    fn pomodoro_changes_phase_on_its_own_and_counts_only_work() {
        let c = Clock::new();
        let plan = Plan {
            work: m(25),
            short_break: m(5),
            long_break: m(15),
            cycles: 2,
        };
        let mut st = Machine::new(Mode::Pomodoro, plan);
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        assert!(st.tick(c.at(m(24)).0, c.at(m(24)).1).is_empty());
        let (t, w) = c.at(m(25) + s(1));
        let changes = st.tick(t, w);
        assert_eq!(
            changes,
            vec![PhaseChange {
                to: Phase::ShortBreak,
                cycle: 1
            }]
        );
        // The segment ends at the boundary, not at the late tick.
        assert_eq!(st.worked(t), m(25));
        assert_eq!(st.phase_left(t), Some((m(5) - s(1), m(5))));
        // Work 2, then the long break.
        let (t, w) = c.at(m(30));
        assert_eq!(st.tick(t, w)[0].to, Phase::Work);
        assert_eq!(st.cycle(), 2);
        let (t, w) = c.at(m(55));
        assert_eq!(st.tick(t, w)[0].to, Phase::LongBreak);
        let (t, w) = c.at(m(70));
        let back = st.tick(t, w);
        assert_eq!(
            back[0],
            PhaseChange {
                to: Phase::Work,
                cycle: 1
            }
        );
        assert_eq!(st.finish(c.at(m(80)).0).unwrap().net, m(60));
    }

    #[test]
    fn a_late_tick_catches_up_every_phase() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Pomodoro, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        let (t, w) = c.at(m(31));
        let changes = st.tick(t, w);
        assert_eq!(changes.len(), 2);
        assert_eq!(st.phase(), Phase::Work);
        assert_eq!(st.worked(t), m(26));
    }

    #[test]
    fn pausing_freezes_the_pomodoro_phase() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Pomodoro, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        st.pause(c.at(m(10)).0);
        let (t, w) = c.at(m(60));
        assert!(st.tick(t, w).is_empty());
        assert_eq!(st.phase_left(t), Some((m(15), m(25))));
        st.start(t, w);
        let (t, w) = c.at(m(75));
        assert_eq!(st.tick(t, w)[0].to, Phase::ShortBreak);
        assert_eq!(st.worked(t), m(25));
    }

    #[test]
    fn mode_only_changes_between_sessions() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        assert!(!st.set_mode(Mode::Pomodoro));
        st.finish(c.at(m(1)).0);
        assert!(st.set_mode(Mode::Pomodoro));
        assert_eq!(st.mode(), Mode::Pomodoro);
    }

    #[test]
    fn discounting_idle_time_splits_the_work() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        // Away from minute 20 to 34, answered "no" at minute 40.
        st.discount(c.at(m(20)).0, c.at(m(34)).0);
        let now = c.at(m(40)).0;
        assert_eq!(st.worked(now), m(26));
        let r = st.finish(now).unwrap();
        assert_eq!(
            r.segments,
            vec![(c.w0, c.w0 + m(20)), (c.w0 + m(34), c.w0 + m(40))]
        );
    }

    #[test]
    fn discount_across_a_pause_and_past_segments() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        st.pause(c.at(m(10)).0);
        let (t, w) = c.at(m(15));
        st.start(t, w);
        st.discount(c.at(m(5)).0, c.at(m(20)).0);
        // 0–5 and 20–30 remain.
        assert_eq!(st.worked(c.at(m(30)).0), m(15));
    }

    #[test]
    fn nothing_is_stored_for_an_empty_session() {
        let c = Clock::new();
        let mut st = Machine::new(Mode::Stopwatch, Plan::default());
        let (t, w) = c.at(Duration::ZERO);
        st.start(t, w);
        assert_eq!(st.finish(t), None);
        assert_eq!(st.finish(t), None);
    }
}
