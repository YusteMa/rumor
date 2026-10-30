//! Study module (SPEC §4.3b): stopwatch/Pomodoro, idle detection and session
//! history. It knows nothing about the sound: the coordinator in `state.rs`
//! links the two (CLAUDE.md rule 12).

pub mod calendar;
pub mod db;
pub mod idle;
pub mod machine;

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use db::{Db, DbError, Tag};
use idle::{Away, IdleWatch};
use machine::{Machine, Mode, Phase, PhaseChange, Plan, Run};

/// A running session is written to disk this often.
const SAVE_EVERY: Duration = Duration::from_secs(60);

/// Something the coordinator has to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Phase(PhaseChange),
    /// The user is back after a stretch without input; the question waits.
    Idle(Duration),
}

/// What the panel and the tray show (`state-changed`, `study-tick`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudyView {
    pub run: Run,
    pub mode: Mode,
    pub phase: Phase,
    pub cycle: u32,
    pub cycles: u32,
    /// Work counted in this session.
    pub worked_secs: u64,
    /// Pomodoro only: time left in the phase and its length.
    pub phase_left_secs: Option<u64>,
    pub phase_secs: Option<u64>,
    pub today_secs: u64,
    pub week_secs: u64,
    pub tag: Option<i64>,
    pub tags: Vec<Tag>,
    /// Minutes away, while "¿Seguías estudiando?" waits for an answer.
    pub idle_minutes: Option<u64>,
}

/// Whole seconds, rounded up for countdowns so they never show 0:00 early.
fn secs_up(d: Duration) -> u64 {
    d.as_secs() + u64::from(d.subsec_nanos() > 0)
}

/// Stored work for today and this week, kept until something is saved or
/// the day changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Totals {
    key: (SystemTime, SystemTime, Option<i64>),
    today: Duration,
    week: Duration,
}

pub struct Study {
    machine: Machine,
    db: Option<Db>,
    /// Row of the session in progress.
    session: Option<i64>,
    tag: Option<i64>,
    idle: IdleWatch,
    /// Stretches away not yet answered.
    away: Vec<Away>,
    saved_at: Option<Instant>,
    totals: Option<Totals>,
    tags: Vec<Tag>,
}

impl Study {
    /// Opens the history in `dir`. Without it (or if it can't be opened) the
    /// study module still works; it just doesn't remember.
    pub fn open(dir: Option<&Path>, mode: Mode, plan: Plan, tag: Option<i64>) -> Self {
        let db = dir.and_then(|dir| {
            std::fs::create_dir_all(dir)
                .map_err(DbError::from)
                .and_then(|()| Db::open(&dir.join("study.sqlite3")))
                .map_err(|e| log::error!("study history unavailable: {e}"))
                .ok()
        });
        let tags = db
            .as_ref()
            .and_then(|db| db.tags().map_err(|e| log::warn!("tags: {e}")).ok())
            .unwrap_or_default();
        let tag = tag.filter(|id| tags.iter().any(|t| t.id == *id));
        Self {
            machine: Machine::new(mode, plan),
            db,
            session: None,
            tag,
            idle: IdleWatch::default(),
            away: Vec::new(),
            saved_at: None,
            totals: None,
            tags,
        }
    }

    pub fn run(&self) -> Run {
        self.machine.run()
    }

    pub fn phase(&self) -> Phase {
        self.machine.phase()
    }

    pub fn tag(&self) -> Option<i64> {
        self.tag
    }

    pub fn worked(&self, now: Instant) -> Duration {
        self.machine.worked(now)
    }

    /// Pomodoro only: seconds left in the phase.
    pub fn phase_left_secs(&self, now: Instant) -> Option<u64> {
        self.machine.phase_left(now).map(|(left, _)| secs_up(left))
    }

    pub fn idle_pending(&self) -> bool {
        !self.away.is_empty()
    }

    /// Starts a session, or continues a paused one.
    pub fn start(&mut self, now: Instant, wall: SystemTime) {
        if self.machine.run() == Run::Stopped {
            self.away.clear();
            self.idle.reset();
            self.session = self.db.as_mut().and_then(|db| {
                db.begin(wall, self.machine.mode(), self.tag)
                    .map_err(|e| log::warn!("session not stored: {e}"))
                    .ok()
            });
            self.saved_at = Some(now);
            self.totals = None;
            log::info!("study session started ({:?})", self.machine.mode());
        }
        self.machine.start(now, wall);
    }

    pub fn pause(&mut self, now: Instant) {
        self.machine.pause(now);
        self.idle.reset();
        self.save(now, false);
    }

    /// Ends the session and stores it. An unanswered idle question counts.
    pub fn finish(&mut self, now: Instant) {
        if !self.machine.has_session() {
            return;
        }
        self.away.clear();
        self.idle.reset();
        let record = self.machine.finish(now);
        if let (Some(db), Some(id)) = (self.db.as_mut(), self.session.take()) {
            let result = match &record {
                Some(record) => db.save(id, record, true),
                None => db.discard(id),
            };
            if let Err(e) = result {
                log::warn!("session not stored: {e}");
            }
        }
        self.totals = None;
        if let Some(r) = record {
            log::info!("study session finished: {} s of work", r.net.as_secs());
        }
    }

    /// Writes the session in progress (every minute, on pause, on quit).
    fn save(&mut self, now: Instant, done: bool) {
        self.saved_at = Some(now);
        let (Some(db), Some(id)) = (self.db.as_mut(), self.session) else {
            return;
        };
        if let Some(record) = self.machine.record(now) {
            if let Err(e) = db.save(id, &record, done) {
                log::warn!("session not saved: {e}");
            }
        }
    }

    /// Phase changes and idle detection; `idle_after` is `None` when idle
    /// detection is off.
    pub fn tick(
        &mut self,
        now: Instant,
        wall: SystemTime,
        idle_after: Option<Duration>,
    ) -> Vec<Event> {
        let mut events: Vec<Event> = self
            .machine
            .tick(now, wall)
            .into_iter()
            .map(Event::Phase)
            .collect();
        match idle_after {
            Some(threshold) if self.machine.is_counting() => {
                let idle = idle::since_last_input().unwrap_or_default();
                if let Some(away) = self.idle.update(now, idle, threshold) {
                    self.away.push(away);
                    let total = self.away.iter().map(Away::length).sum();
                    events.push(Event::Idle(total));
                }
            }
            _ => self.idle.reset(),
        }
        if self.machine.has_session()
            && self
                .saved_at
                .is_none_or(|t| now.saturating_duration_since(t) >= SAVE_EVERY)
        {
            self.save(now, false);
        }
        events
    }

    /// The answer to "¿Seguías estudiando?": keep the time, or take it out.
    pub fn resolve_idle(&mut self, keep: bool, now: Instant) {
        for away in std::mem::take(&mut self.away) {
            if !keep {
                self.machine.discount(away.from, away.to);
            }
        }
        if !keep {
            log::info!("idle time discounted");
            self.save(now, false);
        }
    }

    pub fn set_mode(&mut self, mode: Mode) -> bool {
        self.machine.set_mode(mode)
    }

    pub fn set_plan(&mut self, plan: Plan) {
        self.machine.set_plan(plan);
    }

    /// Tags the session in progress (if any) and the next ones.
    pub fn set_tag(&mut self, tag: Option<i64>) {
        let tag = tag.filter(|id| self.tags.iter().any(|t| t.id == *id));
        self.tag = tag;
        if let (Some(db), Some(id)) = (self.db.as_mut(), self.session) {
            if let Err(e) = db.set_tag(id, tag) {
                log::warn!("tag not stored: {e}");
            }
        }
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    fn reload_tags(&mut self) {
        if let Some(db) = &self.db {
            match db.tags() {
                Ok(tags) => self.tags = tags,
                Err(e) => log::warn!("tags: {e}"),
            }
        }
        if self
            .tag
            .is_some_and(|id| !self.tags.iter().any(|t| t.id == id))
        {
            self.tag = None;
        }
    }

    pub fn upsert_tag(&mut self, id: Option<i64>, name: &str, color: &str) -> Result<Tag, DbError> {
        let db = self.db.as_mut().ok_or(DbError::Unavailable)?;
        let tag = db.upsert_tag(id, name, color)?;
        self.reload_tags();
        Ok(tag)
    }

    pub fn delete_tag(&mut self, id: i64) -> Result<(), DbError> {
        let db = self.db.as_mut().ok_or(DbError::Unavailable)?;
        db.delete_tag(id)?;
        self.reload_tags();
        Ok(())
    }

    fn stored(&mut self, today: SystemTime, week: SystemTime, now_wall: SystemTime) -> Totals {
        let key = (today, week, self.session);
        if let Some(t) = self.totals.filter(|t| t.key == key) {
            return t;
        }
        let sum = |db: &Db, from| {
            db.worked_between(from, now_wall + Duration::from_secs(86_400), self.session)
                .map_err(|e| log::warn!("totals: {e}"))
                .unwrap_or_default()
        };
        let (today_d, week_d) = self
            .db
            .as_ref()
            .map_or((Duration::ZERO, Duration::ZERO), |db| {
                (sum(db, today), sum(db, week))
            });
        let totals = Totals {
            key,
            today: today_d,
            week: week_d,
        };
        self.totals = Some(totals);
        totals
    }

    pub fn view(&mut self, now: Instant, wall: SystemTime) -> StudyView {
        let (today, week) = calendar::bounds(&calendar::now_local(), calendar::week_start());
        let stored = self.stored(today, week, wall);
        let far = wall + Duration::from_secs(86_400);
        let live_today = self.machine.worked_between(now, today, far);
        let live_week = self.machine.worked_between(now, week, far);
        let phase = self.machine.phase_left(now);
        let away: Duration = self.away.iter().map(Away::length).sum();
        StudyView {
            run: self.machine.run(),
            mode: self.machine.mode(),
            phase: self.machine.phase(),
            cycle: self.machine.cycle(),
            cycles: self.machine.plan().cycles,
            worked_secs: self.machine.worked(now).as_secs(),
            phase_left_secs: phase.map(|(left, _)| secs_up(left)),
            phase_secs: phase.map(|(_, total)| total.as_secs()),
            today_secs: (stored.today + live_today).as_secs(),
            week_secs: (stored.week + live_week).as_secs(),
            tag: self.tag,
            tags: self.tags.clone(),
            idle_minutes: (!self.away.is_empty()).then(|| away.as_secs().div_ceil(60)),
        }
    }
}
