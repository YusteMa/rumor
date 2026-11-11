//! Study module (SPEC §4.3b): stopwatch/Pomodoro, idle detection and session
//! history. It knows nothing about the sound: the coordinator in `state.rs`
//! links the two (CLAUDE.md rule 12).

pub mod calendar;
pub mod db;
pub mod idle;
pub mod machine;
pub mod stats;

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use serde::Serialize;

use db::{Db, DbError, SessionError, Tag, millis};
use idle::{Away, IdleWatch};
use machine::{Machine, Mode, Phase, PhaseChange, Plan, Run};
use stats::{CsvLabels, MonthView, Session, SessionPage, WeekView};

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
    /// A Pomodoro work block, as planned (shown before starting).
    pub work_secs: u64,
    /// When the session started (UTC ms), while there is one.
    pub started_at: Option<i64>,
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

/// This week's stored sessions (without the one in progress), kept for the
/// panel's once-a-second totals until something is saved or the week
/// changes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Week {
    start: chrono::NaiveDate,
    session: Option<i64>,
    sessions: Vec<Session>,
}

/// Sessions a page of the list asks the database for at a time.
const PAGE_ROWS: usize = 64;

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
    /// The noise playing when the session in progress started.
    noise: Option<String>,
    week: Option<Week>,
    /// Something was written since `take_saved` last looked.
    saved: bool,
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
            noise: None,
            week: None,
            saved: false,
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

    /// Starts a session, or continues a paused one. `noise` is the noise
    /// playing right now, kept with a new session (SPEC §4.3d).
    pub fn start(&mut self, now: Instant, wall: SystemTime, noise: Option<&str>) {
        if self.machine.run() == Run::Stopped {
            self.away.clear();
            self.idle.reset();
            self.noise = noise.map(str::to_owned);
            self.session = self.db.as_mut().and_then(|db| {
                db.begin(wall, self.machine.mode(), self.tag, noise)
                    .map_err(|e| log::warn!("session not stored: {e}"))
                    .ok()
            });
            self.saved_at = Some(now);
            self.week = None;
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
        self.week = None;
        self.saved = true;
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
            match db.save(id, &record, done) {
                Ok(()) => self.saved = true,
                Err(e) => log::warn!("session not saved: {e}"),
            }
        }
    }

    /// Whether anything was written since the last call (Statistics then
    /// refreshes).
    pub fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved)
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

    /// Debug builds: the Pomodoro phase ends now (`--dev-study-skip`).
    #[cfg(debug_assertions)]
    pub fn end_phase(&mut self, now: Instant) {
        self.machine.end_phase(now);
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

    /// The session in progress as the statistics see it: what it has worked
    /// so far, up to `now`.
    fn live(&self, now: Instant) -> Option<Session> {
        let record = self.machine.record(now)?;
        Some(Session {
            id: self.session.unwrap_or(-1),
            start: millis(record.started),
            end: millis(record.ended),
            secs: record.net.as_secs(),
            tag: self.tag,
            noise: self.noise.clone(),
            mode: db::mode_name(record.mode).to_owned(),
            edited: 0,
            running: true,
            segments: record
                .segments
                .iter()
                .map(|(a, b)| (millis(*a), millis(*b)))
                .collect(),
        })
    }

    /// Stored sessions with the one in progress in place of its stored row.
    fn with_live(&self, mut sessions: Vec<Session>, now: Instant) -> Vec<Session> {
        if let Some(live) = self.live(now) {
            sessions.retain(|s| s.id != live.id);
            sessions.push(live);
        }
        sessions
    }

    fn db(&self) -> Result<&Db, DbError> {
        self.db.as_ref().ok_or(DbError::Unavailable)
    }

    /// The whole history, the one in progress included; segments only
    /// between `from` and `to` (local days), where a view needs them.
    fn history(
        &self,
        now: Instant,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<Vec<Session>, DbError> {
        let db = self.db()?;
        let mut all = db.sessions(i64::MIN, i64::MAX)?;
        db.load_segments(
            &mut all,
            stats::midnight(&Local, from),
            stats::midnight(&Local, to),
        )?;
        Ok(self.with_live(all, now))
    }

    fn context(&self) -> (chrono::NaiveDateTime, chrono::Weekday) {
        (calendar::now_local().naive_local(), calendar::week_start())
    }

    /// `goal_hours`: the weekly goal from Settings, 0 for none.
    pub fn week_view(
        &self,
        offset: u32,
        goal_hours: u32,
        now: Instant,
    ) -> Result<WeekView, DbError> {
        let (local_now, first) = self.context();
        let week =
            stats::week_of(local_now.date(), first) - chrono::Days::new(7 * u64::from(offset));
        // Heat map, insights and "the same point" reach 8 weeks back.
        let sessions = self.history(
            now,
            week - chrono::Days::new(7 * u64::from(stats::HEAT_WEEKS)),
            week + chrono::Days::new(7),
        )?;
        let cx = stats::Context {
            tz: &Local,
            now: local_now,
            first,
            tags: &self.tags,
            goal_secs: (goal_hours > 0).then(|| u64::from(goal_hours) * 3600),
        };
        Ok(stats::week_view(&cx, &sessions, offset))
    }

    pub fn month_view(&self, offset: u32, now: Instant) -> Result<MonthView, DbError> {
        let (local_now, first) = self.context();
        let month = chrono::Datelike::with_day(&local_now.date(), 1).unwrap_or(local_now.date());
        let sessions = self.history(
            now,
            month - chrono::Months::new(offset + 1),
            month - chrono::Months::new(offset) + chrono::Months::new(1),
        )?;
        let cx = stats::Context {
            tz: &Local,
            now: local_now,
            first,
            tags: &self.tags,
            goal_secs: None,
        };
        Ok(stats::month_view(&cx, &sessions, offset))
    }

    /// At least `days` whole days of sessions started before `before`,
    /// newest first, reading the database a page at a time.
    pub fn sessions_page(
        &self,
        before: Option<i64>,
        days: usize,
        now: Instant,
    ) -> Result<SessionPage, DbError> {
        let db = self.db()?;
        let before = before.unwrap_or(i64::MAX);
        let live = self.live(now).filter(|s| s.start < before);
        let mut limit = PAGE_ROWS.max(days * 8);
        loop {
            let mut rows = db.sessions_before(before, limit)?;
            let exhausted = rows.len() < limit;
            if let Some(live) = &live {
                rows.retain(|s| s.id != live.id);
                rows.insert(0, live.clone());
            }
            let distinct = {
                let mut seen: Vec<chrono::NaiveDate> = rows
                    .iter()
                    .map(|s| stats::day_of(&Local, s.start))
                    .collect();
                seen.dedup();
                seen.len()
            };
            // One day more than asked for proves the last one is whole.
            if exhausted || distinct > days {
                return Ok(stats::page(&Local, rows, days, false));
            }
            limit *= 2;
        }
    }

    fn not_running(&self, id: i64) -> Result<(), SessionError> {
        if self.session == Some(id) {
            return Err(SessionError::Running);
        }
        Ok(())
    }

    fn changed(&mut self) {
        self.week = None;
        self.saved = true;
    }

    pub fn edit_session(
        &mut self,
        id: i64,
        secs: u64,
        tag: Option<i64>,
    ) -> Result<(), SessionError> {
        self.not_running(id)?;
        let tag = tag.filter(|id| self.tags.iter().any(|t| t.id == *id));
        let db = self.db.as_mut().ok_or(SessionError::Unavailable)?;
        db.edit(id, secs, tag, millis(SystemTime::now()))?;
        self.changed();
        log::info!("study session {id} edited");
        Ok(())
    }

    pub fn add_session(
        &mut self,
        start: i64,
        secs: u64,
        tag: Option<i64>,
    ) -> Result<(), SessionError> {
        let tag = tag.filter(|id| self.tags.iter().any(|t| t.id == *id));
        let db = self.db.as_mut().ok_or(SessionError::Unavailable)?;
        let id = db.add(start, secs, tag, millis(SystemTime::now()))?;
        self.changed();
        log::info!("study session {id} added by hand");
        Ok(())
    }

    pub fn delete_session(&mut self, id: i64) -> Result<(), SessionError> {
        self.not_running(id)?;
        let db = self.db.as_mut().ok_or(SessionError::Unavailable)?;
        db.delete(id, millis(SystemTime::now()))?;
        self.changed();
        Ok(())
    }

    pub fn restore_session(&mut self, id: i64) -> Result<(), SessionError> {
        let db = self.db.as_mut().ok_or(SessionError::Unavailable)?;
        db.restore(id)?;
        self.changed();
        Ok(())
    }

    /// The whole history as CSV, the session in progress marked.
    pub fn csv(&self, labels: &CsvLabels, now: Instant) -> Result<String, DbError> {
        let all = self.db()?.sessions(i64::MIN, i64::MAX)?;
        Ok(stats::csv(
            &Local,
            &self.with_live(all, now),
            &self.tags,
            labels,
        ))
    }

    /// Today and this week for the panel: the same sum as Statistics.
    fn totals(&mut self, now: Instant) -> (u64, u64) {
        let (local_now, first) = self.context();
        let start = stats::week_of(local_now.date(), first);
        let fresh = self
            .week
            .as_ref()
            .is_some_and(|w| w.start == start && w.session == self.session);
        if !fresh {
            let sessions = self
                .db
                .as_ref()
                .map(|db| {
                    db.sessions(
                        stats::midnight(&Local, start),
                        stats::midnight(&Local, start + chrono::Days::new(7)),
                    )
                    .map_err(|e| log::warn!("totals: {e}"))
                    .unwrap_or_default()
                })
                .unwrap_or_default();
            self.week = Some(Week {
                start,
                session: self.session,
                sessions,
            });
        }
        let stored = self
            .week
            .as_ref()
            .map(|w| w.sessions.clone())
            .unwrap_or_default();
        stats::totals(
            &Local,
            &self.with_live(stored, now),
            local_now.date(),
            first,
        )
    }

    pub fn view(&mut self, now: Instant) -> StudyView {
        let (today_secs, week_secs) = self.totals(now);
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
            work_secs: self.machine.plan().work.as_secs(),
            started_at: self.machine.started().map(millis),
            today_secs,
            week_secs,
            tag: self.tag,
            tags: self.tags.clone(),
            idle_minutes: (!self.away.is_empty()).then(|| away.as_secs().div_ceil(60)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    /// A study module with its history in a fresh folder, removed on drop.
    struct Scratch {
        dir: std::path::PathBuf,
        study: Option<Study>,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rumor-{name}-{}-{}",
                std::process::id(),
                millis(SystemTime::now())
            ));
            let study = Study::open(Some(&dir), Mode::Stopwatch, Plan::default(), None);
            Self {
                dir,
                study: Some(study),
            }
        }

        fn study(&mut self) -> &mut Study {
            self.study.as_mut().unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            self.study = None;
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Records, week, month and panel, as each sees the history right now.
    fn snapshot(study: &mut Study) -> (u64, u64, u64, WeekView) {
        let now = Instant::now();
        let week = study.week_view(0, 0, now).unwrap();
        let month = study.month_view(0, now).unwrap();
        let panel = study.view(now);
        // The panel and the window always agree.
        assert_eq!(panel.week_secs, week.secs);
        if let Some(today) = week.today {
            assert_eq!(panel.today_secs, week.days[today as usize].secs);
        }
        let longest = week.records.longest_session.as_ref().unwrap().secs;
        let total = week.records.total.as_ref().unwrap().secs;
        (longest, total, month.secs, week)
    }

    #[test]
    fn every_view_follows_an_edit_a_delete_and_an_undo() {
        let mut scratch = Scratch::new("coherence");
        let study = scratch.study();
        let now = millis(SystemTime::now());
        study
            .add_session(now - 50 * 60 * MIN, 90 * 60, None)
            .unwrap();
        study
            .add_session(now - 26 * 60 * MIN, 60 * 60, None)
            .unwrap();
        study
            .add_session(now - 5 * 60 * MIN, 30 * 60, None)
            .unwrap();
        let (longest, total, ..) = snapshot(study);
        assert_eq!((longest, total), (90 * 60, 180 * 60));

        // Lengthened past the record: the record follows at once.
        let short = study
            .sessions_page(None, 30, Instant::now())
            .unwrap()
            .days
            .iter()
            .flat_map(|d| &d.sessions)
            .find(|s| s.secs == 30 * 60)
            .map(|s| s.id)
            .unwrap();
        study.edit_session(short, 200 * 60, None).unwrap();
        assert!(study.take_saved(), "Statistics is told");
        let (longest, total, month, week) = snapshot(study);
        assert_eq!((longest, total), (200 * 60, 350 * 60));
        assert!(week.records.best_day.unwrap().secs >= 200 * 60);
        assert!(month >= 200 * 60);

        // Deleted: back to the old record; undone: the new one again.
        study.delete_session(short).unwrap();
        let (longest, total, ..) = snapshot(study);
        assert_eq!((longest, total), (90 * 60, 150 * 60));
        study.restore_session(short).unwrap();
        let (longest, total, ..) = snapshot(study);
        assert_eq!((longest, total), (200 * 60, 350 * 60));
    }

    #[test]
    fn the_session_in_progress_is_never_edited() {
        let mut scratch = Scratch::new("running");
        let study = scratch.study();
        let now = Instant::now();
        study.start(now, SystemTime::now(), Some("brown"));
        let running = study
            .sessions_page(None, 1, now)
            .unwrap()
            .days
            .first()
            .and_then(|d| d.sessions.first().cloned())
            .unwrap();
        assert!(running.running);
        assert_eq!(
            study.edit_session(running.id, 600, None),
            Err(SessionError::Running)
        );
        assert_eq!(study.delete_session(running.id), Err(SessionError::Running));
        // Until it ends it isn't a session in the figures.
        let week = study.week_view(0, 0, now).unwrap();
        assert_eq!(week.sessions, 0);
    }
}
