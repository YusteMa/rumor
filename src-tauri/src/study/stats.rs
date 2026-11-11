//! Study statistics (SPEC §4.3d): one aggregation behind the panel's
//! "Hoy" and "Semana", the Statistics window and the CSV export, so the three
//! always agree.
//!
//! A session belongs, whole, to the local day it started on, even when it
//! runs past midnight (decided 2026-11-05), so a day's total is always the
//! sum of the sessions listed under it. Times are UTC milliseconds; days are
//! local to the time zone passed in (`Local` in the app, Europe/Madrid in the
//! tests), so a day with a clock change simply has 23 or 25 hours.
//!
//! The session in progress (decided 2026-11-05, third round): its work so
//! far counts in every sum of time (the totals, the bars, the subjects, the
//! daily average, the comparison, the month and the panel's "Hoy"), so the
//! window, the panel and the CSV agree to the second. Everything about
//! sessions as such waits until it ends: the number of sessions, the average
//! session, the records, "Así ha ido" and "Cuándo estudias más".
//!
//! Pure: no SQLite, no Tauri. `db.rs` loads the sessions, `mod.rs` adds the
//! one in progress.

use std::collections::BTreeMap;

use chrono::{
    Datelike, Days, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Timelike,
    Weekday,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::db::Tag;

/// `None` is "Sin etiqueta".
pub type TagId = Option<i64>;

/// Below this, the previous period is too short for a percentage to mean
/// anything: the difference is given in minutes instead.
pub const PCT_FLOOR: u64 = 30 * 60;
/// "Cuándo estudias más" looks this far back, the shown week included.
pub const HEAT_WEEKS: u32 = 8;
/// Weeks in the trend beside the week's total, the one shown included.
pub const TREND_WEEKS: u32 = 8;
/// Earlier weeks with study that "your usual week" needs to mean something.
const TYPICAL_MIN_WEEKS: usize = 3;
/// Hours of the day in the heat map, one column each; outside them, the
/// night column.
pub const HEAT_FIRST_HOUR: u32 = 6;
pub const HEAT_HOURS: usize = 17;

/// A stored session, or the one in progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    /// UTC milliseconds.
    pub start: i64,
    pub end: i64,
    /// Work only, without pauses.
    pub secs: u64,
    pub tag: TagId,
    /// The noise playing when it started (`None`: silence, or older than the
    /// column).
    pub noise: Option<String>,
    pub mode: String,
    /// 0 as recorded, 1 edited, 2 added by hand.
    pub edited: u8,
    pub running: bool,
    /// Work stretches, when loaded; only "up to the same point" needs them.
    #[serde(skip)]
    pub segments: Vec<(i64, i64)>,
}

impl Session {
    /// Work done by `until`: all of it if it ended by then, otherwise its
    /// segments up to that instant.
    fn secs_by(&self, until: i64) -> u64 {
        if self.end <= until {
            return self.secs;
        }
        if self.segments.is_empty() {
            return (u64::try_from(until - self.start).unwrap_or(0) / 1000).min(self.secs);
        }
        let ms: i64 = self
            .segments
            .iter()
            .map(|&(a, b)| (b.min(until) - a).max(0))
            .sum();
        (u64::try_from(ms).unwrap_or(0) / 1000).min(self.secs)
    }
}

// ---- Time --------------------------------------------------------------------------

/// Local wall time of an instant.
pub fn local<Tz: TimeZone>(tz: &Tz, ms: i64) -> NaiveDateTime {
    tz.timestamp_millis_opt(ms)
        .single()
        .map(|d| d.naive_local())
        .or_else(|| chrono::DateTime::from_timestamp_millis(ms).map(|d| d.naive_utc()))
        .unwrap_or_default()
}

/// The instant of a local wall time; in a spring-forward gap, the first
/// instant after it.
pub fn instant<Tz: TimeZone>(tz: &Tz, at: NaiveDateTime) -> i64 {
    (0..=3)
        .find_map(|h| {
            tz.from_local_datetime(&(at + TimeDelta::hours(h)))
                .earliest()
        })
        .map_or(0, |d| d.timestamp_millis())
}

pub fn midnight<Tz: TimeZone>(tz: &Tz, day: NaiveDate) -> i64 {
    instant(tz, day.and_time(NaiveTime::MIN))
}

pub fn day_of<Tz: TimeZone>(tz: &Tz, ms: i64) -> NaiveDate {
    local(tz, ms).date()
}

/// First day of the week holding `day`.
pub fn week_of(day: NaiveDate, first: Weekday) -> NaiveDate {
    day - Days::new(u64::from(day.weekday().days_since(first)))
}

fn add_days(day: NaiveDate, n: i64) -> NaiveDate {
    if n >= 0 {
        day + Days::new(n.unsigned_abs())
    } else {
        day - Days::new(n.unsigned_abs())
    }
}

/// Seconds as an `i64`, for differences.
fn signed(secs: u64) -> i64 {
    i64::try_from(secs).unwrap_or(i64::MAX)
}

// ---- The aggregation ------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DayTotal {
    /// Work, the session in progress included.
    pub secs: u64,
    pub by_tag: BTreeMap<TagId, u64>,
    /// Finished sessions, and their work.
    pub sessions: u32,
    pub finished_secs: u64,
}

/// Every day with study and what it holds: the one sum everything else is
/// read from.
pub fn by_day<Tz: TimeZone>(tz: &Tz, sessions: &[Session]) -> BTreeMap<NaiveDate, DayTotal> {
    let mut days: BTreeMap<NaiveDate, DayTotal> = BTreeMap::new();
    for s in sessions {
        let d = days.entry(day_of(tz, s.start)).or_default();
        d.secs += s.secs;
        *d.by_tag.entry(s.tag).or_default() += s.secs;
        if !s.running {
            d.sessions += 1;
            d.finished_secs += s.secs;
        }
    }
    days
}

/// The sessions that have ended: records, insights and the heat map only
/// look at these.
fn finished(sessions: &[Session]) -> Vec<Session> {
    sessions.iter().filter(|s| !s.running).cloned().collect()
}

fn sum_days(days: &BTreeMap<NaiveDate, DayTotal>, from: NaiveDate, to: NaiveDate) -> u64 {
    days.range(from..to).map(|(_, d)| d.secs).sum()
}

/// The panel's "Hoy" and "Semana".
pub fn totals<Tz: TimeZone>(
    tz: &Tz,
    sessions: &[Session],
    today: NaiveDate,
    first: Weekday,
) -> (u64, u64) {
    let days = by_day(tz, sessions);
    let week = week_of(today, first);
    (
        days.get(&today).map_or(0, |d| d.secs),
        sum_days(&days, week, add_days(week, 7)),
    )
}

/// Tags most studied first, "Sin etiqueta" last.
fn legend(by_tag: &BTreeMap<TagId, u64>) -> Vec<TagTime> {
    let mut rows: Vec<TagTime> = by_tag
        .iter()
        .filter(|(_, secs)| **secs > 0)
        .map(|(&tag, &secs)| TagTime { tag, secs })
        .collect();
    rows.sort_by(|a, b| match (a.tag, b.tag) {
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        _ => b.secs.cmp(&a.secs),
    });
    rows
}

fn merge_tags<'a>(days: impl Iterator<Item = &'a DayTotal>) -> BTreeMap<TagId, u64> {
    let mut all = BTreeMap::new();
    for d in days {
        for (&tag, &secs) in &d.by_tag {
            *all.entry(tag).or_default() += secs;
        }
    }
    all
}

// ---- Views --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagTime {
    pub tag: TagId,
    pub secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayBar {
    pub date: String,
    pub secs: u64,
    pub by_tag: Vec<TagTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dated {
    pub secs: u64,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Records {
    pub longest_session: Option<Dated>,
    pub best_day: Option<Dated>,
    pub best_week: Option<Dated>,
    pub total: Option<Dated>,
}

/// Against the previous period, up to the same point in the current one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub previous_secs: u64,
    pub delta_secs: i64,
    /// `None` below `PCT_FLOOR`: the UI shows minutes.
    pub pct: Option<i64>,
    /// Local `YYYY-MM-DDTHH:MM` it compares up to, in a current period.
    pub until: Option<String>,
}

fn compare(secs: u64, previous: u64, until: Option<NaiveDateTime>) -> Option<Comparison> {
    if secs == 0 && previous == 0 {
        return None;
    }
    let delta = signed(secs) - signed(previous);
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let pct =
        (previous >= PCT_FLOOR).then(|| (delta as f64 / previous as f64 * 100.0).round() as i64);
    Some(Comparison {
        previous_secs: previous,
        delta_secs: delta,
        pct,
        until: until.map(|t| t.format("%Y-%m-%dT%H:%M").to_string()),
    })
}

/// The weekly goal (Settings → Study) and how it is going.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    /// The goal itself, in seconds a week.
    pub secs: u64,
    /// Weeks in a row that met it, counted back from the current one.
    pub streak: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heat {
    /// Rows from the week's first day; `HEAT_HOURS` columns from
    /// `HEAT_FIRST_HOUR`, then the night (23 h – 6 h). Seconds.
    pub rows: Vec<Vec<u64>>,
    pub weeks: u32,
    pub best_slot: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub trend: &'static str,
    pub key: &'static str,
    pub args: BTreeMap<&'static str, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekView {
    pub offset: u32,
    pub start: String,
    pub end: String,
    pub today: Option<u32>,
    pub secs: u64,
    pub comparison: Option<Comparison>,
    pub daily_average: u64,
    pub session_average: u64,
    pub sessions: u32,
    pub active_days: u32,
    pub elapsed_days: u32,
    pub days: Vec<DayBar>,
    pub by_tag: Vec<TagTime>,
    /// The previous week per tag, up to the same point in the current one;
    /// empty when there is nothing before to compare with.
    pub tag_previous: Vec<TagTime>,
    /// The last `TREND_WEEKS` weeks, oldest first; the last is this one.
    pub trend: Vec<Dated>,
    /// The mean of the earlier weeks of `trend` that had study.
    pub typical_secs: Option<u64>,
    /// `None` without a weekly goal.
    pub goal: Option<Goal>,
    pub heat: Option<Heat>,
    pub records: Records,
    pub insights: Vec<Insight>,
    pub has_history: bool,
    pub last_with_data: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthView {
    pub offset: u32,
    pub start: String,
    /// 0 = Monday … 6 = Sunday.
    pub week_start: u32,
    pub today: Option<String>,
    pub days: Vec<Dated>,
    pub secs: u64,
    pub comparison: Option<Comparison>,
    pub active_days: u32,
    pub elapsed_days: u32,
    pub active_average: u64,
    pub best_day: Option<Dated>,
    pub by_tag: Vec<TagTime>,
    pub has_history: bool,
}

/// What the views need besides the sessions.
pub struct Context<'a, Tz: TimeZone> {
    pub tz: &'a Tz,
    /// Local wall time now.
    pub now: NaiveDateTime,
    pub first: Weekday,
    pub tags: &'a [Tag],
    /// The weekly goal in seconds, if Settings has one.
    pub goal_secs: Option<u64>,
}

impl<Tz: TimeZone> Context<'_, Tz> {
    fn today(&self) -> NaiveDate {
        self.now.date()
    }

    /// `work`, split by tag.
    fn work_by_tag(
        &self,
        sessions: &[Session],
        from: NaiveDate,
        to: NaiveDate,
        until: Option<i64>,
    ) -> BTreeMap<TagId, u64> {
        let (a, b) = (midnight(self.tz, from), midnight(self.tz, to));
        let b = until.map_or(b, |u| b.min(u));
        let mut by_tag = BTreeMap::new();
        for s in sessions.iter().filter(|s| s.start >= a && s.start < b) {
            *by_tag.entry(s.tag).or_default() += until.map_or(s.secs, |u| s.secs_by(u));
        }
        by_tag
    }

    /// Work in sessions started in `[from, to)` (days), up to `until` if given.
    fn work(
        &self,
        sessions: &[Session],
        from: NaiveDate,
        to: NaiveDate,
        until: Option<i64>,
    ) -> u64 {
        let (a, b) = (midnight(self.tz, from), midnight(self.tz, to));
        let b = until.map_or(b, |u| b.min(u));
        sessions
            .iter()
            .filter(|s| s.start >= a && s.start < b)
            .map(|s| until.map_or(s.secs, |u| s.secs_by(u)))
            .sum()
    }
}

/// Work per week, by the week's first day.
fn by_week(days: &BTreeMap<NaiveDate, DayTotal>, first: Weekday) -> BTreeMap<NaiveDate, u64> {
    let mut weeks: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for (day, d) in days {
        *weeks.entry(week_of(*day, first)).or_default() += d.secs;
    }
    weeks
}

/// Weeks in a row that met the goal, counted back from `this_week`. The week
/// in progress adds as soon as it gets there and never breaks the streak
/// while it lasts: resting a day costs nothing (SPEC §4.3d).
pub fn streak(weeks: &BTreeMap<NaiveDate, u64>, this_week: NaiveDate, goal_secs: u64) -> u32 {
    if goal_secs == 0 {
        return 0;
    }
    let met = |week: NaiveDate| weeks.get(&week).is_some_and(|secs| *secs >= goal_secs);
    let mut count = u32::from(met(this_week));
    let mut week = add_days(this_week, -7);
    while met(week) {
        count += 1;
        week = add_days(week, -7);
    }
    count
}

/// All-time bests over `sessions` (finished ones: see the module docs).
pub fn records<Tz: TimeZone>(tz: &Tz, sessions: &[Session], first: Weekday) -> Records {
    let Some(first_session) = sessions.iter().min_by_key(|s| s.start) else {
        return Records::default();
    };
    let days = by_day(tz, sessions);
    let longest = sessions
        .iter()
        .max_by(|a, b| a.secs.cmp(&b.secs).then(b.start.cmp(&a.start)));
    let best_day = days
        .iter()
        .max_by(|a, b| a.1.secs.cmp(&b.1.secs).then(b.0.cmp(a.0)));
    let mut weeks: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for (day, d) in &days {
        *weeks.entry(week_of(*day, first)).or_default() += d.secs;
    }
    let best_week = weeks.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)));
    Records {
        longest_session: longest.map(|s| Dated {
            secs: s.secs,
            date: day_of(tz, s.start).to_string(),
        }),
        best_day: best_day.map(|(day, d)| Dated {
            secs: d.secs,
            date: day.to_string(),
        }),
        best_week: best_week.map(|(day, secs)| Dated {
            secs: *secs,
            date: day.to_string(),
        }),
        total: Some(Dated {
            secs: days.values().map(|d| d.secs).sum(),
            date: day_of(tz, first_session.start).to_string(),
        }),
    }
}

/// Seconds per weekday row and hour column over the `HEAT_WEEKS` up to the
/// week starting `week`. Each session stays on the row of its start day;
/// its work falls in the hours it really happened. `None` with too little to
/// show (fewer than 5 sessions or 5 h).
pub fn heat<Tz: TimeZone>(
    cx: &Context<'_, Tz>,
    sessions: &[Session],
    week: NaiveDate,
) -> Option<Heat> {
    let from = midnight(cx.tz, add_days(week, -7 * i64::from(HEAT_WEEKS - 1)));
    let to = midnight(cx.tz, add_days(week, 7));
    let inside: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.start >= from && s.start < to)
        .collect();
    if inside.len() < 5 || inside.iter().map(|s| s.secs).sum::<u64>() < 5 * 3600 {
        return None;
    }
    let mut rows = vec![vec![0_u64; HEAT_HOURS + 1]; 7];
    for s in inside {
        let row = day_of(cx.tz, s.start).weekday().days_since(cx.first) as usize;
        let stretches = if s.segments.is_empty() {
            vec![(s.start, s.start + signed(s.secs) * 1000)]
        } else {
            s.segments.clone()
        };
        for (a, b) in stretches {
            let mut t = a;
            while t < b {
                let at = local(cx.tz, t);
                // To the next local hour (clock changes happen on the hour).
                let into_hour = i64::from(at.minute() * 60 + at.second()) * 1000
                    + i64::from(at.and_utc().timestamp_subsec_millis());
                let next = (t + 3_600_000 - into_hour).min(b);
                let h = at.hour();
                let col = if (HEAT_FIRST_HOUR..HEAT_FIRST_HOUR + HEAT_HOURS as u32).contains(&h) {
                    (h - HEAT_FIRST_HOUR) as usize
                } else {
                    HEAT_HOURS
                };
                rows[row][col] += u64::try_from(next - t).unwrap_or(0) / 1000;
                t = next;
            }
        }
    }
    // The best two hours of the day; the night isn't a slot.
    let best_slot = (0..HEAT_HOURS - 1)
        .map(|c| (c, rows.iter().map(|r| r[c] + r[c + 1]).sum::<u64>()))
        .filter(|&(_, v)| v > 0)
        .fold(None, |best: Option<(usize, u64)>, (c, v)| match best {
            Some((_, bv)) if bv >= v => best,
            _ => Some((c, v)),
        })
        .map(|(c, _)| HEAT_FIRST_HOUR + c as u32);
    Some(Heat {
        rows,
        weeks: HEAT_WEEKS,
        best_slot,
    })
}

fn insight(trend: &'static str, key: &'static str, args: &[(&'static str, Value)]) -> Insight {
    Insight {
        trend,
        key,
        args: args.iter().cloned().collect(),
    }
}

/// "Así ha ido": the two or three things that stray most from the user's own
/// habits. Left out on purpose, because the page already shows them: the
/// change against last week (the pill), the longest session (records) and
/// the days studied (figures). Nothing with too little data, and at most one
/// "down": a second one gives way to any neutral or positive sentence.
#[allow(clippy::cast_precision_loss)]
pub fn insights<Tz: TimeZone>(
    cx: &Context<'_, Tz>,
    sessions: &[Session],
    week: NaiveDate,
) -> Vec<Insight> {
    let current = week_of(cx.today(), cx.first) == week;
    // In the current week every comparison stops at the same point.
    let elapsed = current.then(|| cx.now - week.and_time(NaiveTime::MIN));
    let cut = |k: i64| {
        elapsed.map(|e| instant(cx.tz, add_days(week, -7 * k).and_time(NaiveTime::MIN) + e))
    };
    let sum = |list: &[Session], k: i64| {
        let from = add_days(week, -7 * k);
        cx.work(list, from, add_days(from, 7), cut(k))
    };
    let started_in = |s: &Session, k: i64| {
        let from = add_days(week, -7 * k);
        let day = day_of(cx.tz, s.start);
        day >= from && day < add_days(from, 7)
    };
    let this: Vec<&Session> = sessions
        .iter()
        .filter(|s| started_in(s, 0) && cut(0).is_none_or(|c| s.start < c))
        .collect();
    let total = sum(sessions, 0);
    if this.len() < 2 || total < 3600 {
        return Vec::new();
    }
    let past: Vec<i64> = (1..=8)
        .filter(|&k| sessions.iter().any(|s| started_in(s, k)))
        .collect();
    let before: Vec<&Session> = sessions
        .iter()
        .filter(|s| (1..=8).any(|k| started_in(s, k)))
        .collect();
    let mut scored: Vec<(f64, Insight)> = Vec::new();

    // A subject going down (or up) two weeks in a row, over closed weeks
    // only: half a week always looks like a drop.
    let closed = i64::from(current);
    let whole = |list: &[Session], k: i64| {
        let from = add_days(week, -7 * k);
        cx.work(list, from, add_days(from, 7), None)
    };
    for tag in cx.tags {
        let mine: Vec<Session> = sessions
            .iter()
            .filter(|s| s.tag == Some(tag.id))
            .cloned()
            .collect();
        let (w2, w1, w0) = (
            whole(&mine, closed + 2) as f64,
            whole(&mine, closed + 1) as f64,
            whole(&mine, closed) as f64,
        );
        if w2 < 3600.0 {
            continue;
        }
        let name = || [("tag", json!(tag.name))];
        if w1 <= w2 * 0.75 && w0 <= w1 * 0.75 {
            scored.push((1.0 - w0 / w2, insight("down", "insightTagDown", &name())));
        } else if w1 >= w2 * 1.25 && w0 >= w1 * 1.25 {
            scored.push((w0 / w2 - 1.0, insight("up", "insightTagUp", &name())));
        }
    }

    // This week against the usual one, up to the same point. The pill
    // compares with last week, so the sentence names its own base.
    if past.len() >= 4 {
        let usual = past.iter().map(|&k| sum(sessions, k) as f64).sum::<f64>() / past.len() as f64;
        if usual > 0.0 {
            let dev = total as f64 / usual - 1.0;
            #[allow(clippy::cast_possible_truncation)]
            let pct = (dev.abs() * 100.0).round() as i64;
            let args = [("pct", json!(pct)), ("weeks", json!(past.len()))];
            let (above, below) = if current {
                ("insightAboveAverageSoFar", "insightBelowAverageSoFar")
            } else {
                ("insightAboveAverage", "insightBelowAverage")
            };
            if dev >= 0.2 {
                scored.push((dev, insight("up", above, &args)));
            } else if dev <= -0.2 {
                scored.push((-dev, insight("down", below, &args)));
            }
        }
    }

    // How long sessions last, against the usual.
    if before.len() >= 10 && this.len() >= 3 {
        let usual = before.iter().map(|s| s.secs as f64).sum::<f64>() / before.len() as f64;
        let now = this.iter().map(|s| s.secs as f64).sum::<f64>() / this.len() as f64;
        let dev = now / usual - 1.0;
        #[allow(clippy::cast_possible_truncation)]
        let mins = ((now - usual).abs() / 60.0).round() as i64;
        if dev.abs() >= 0.2 && mins >= 5 {
            let (trend, key) = if dev > 0.0 {
                ("up", "insightLongerSessions")
            } else {
                ("down", "insightShorterSessions")
            };
            scored.push((dev.abs(), insight(trend, key, &[("mins", json!(mins))])));
        }
    }

    // Time of day, against the usual: morning (< 14 h), afternoon (< 20 h),
    // evening.
    if before.len() >= 10 {
        let share = |list: &[&Session]| {
            let mut parts = [0.0_f64; 3];
            for s in list {
                let h = local(cx.tz, s.start).hour();
                parts[usize::from(h >= 14) + usize::from(h >= 20)] += s.secs as f64;
            }
            let all = parts.iter().sum::<f64>().max(1.0);
            parts.map(|p| p / all)
        };
        let (a, b) = (share(&this), share(&before));
        let i = (0..3)
            .max_by(|&x, &y| (a[x] - b[x]).total_cmp(&(a[y] - b[y])))
            .unwrap_or(0);
        if a[i] - b[i] >= 0.25 {
            let key = ["insightMornings", "insightAfternoons", "insightEvenings"][i];
            scored.push((a[i] - b[i], insight("neutral", key, &[])));
        }
    }

    // The strongest weekday, with a month of history.
    if past.len() >= 4 {
        let mut by_day = [0.0_f64; 7];
        for s in &before {
            by_day[day_of(cx.tz, s.start).weekday().num_days_from_monday() as usize] +=
                s.secs as f64;
        }
        let mean = by_day.iter().sum::<f64>() / 7.0;
        let top = (0..7)
            .max_by(|&x, &y| by_day[x].total_cmp(&by_day[y]))
            .unwrap_or(0);
        if mean > 0.0 && by_day[top] / mean - 1.0 >= 0.3 {
            scored.push((
                (by_day[top] / mean - 1.0) * 0.5,
                insight("neutral", "insightWeekday", &[("day", json!(top))]),
            ));
        }
    }

    pick(scored)
}

/// The three that stray most, with one "down" at most.
fn pick(mut scored: Vec<(f64, Insight)>) -> Vec<Insight> {
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut downs = 0;
    scored
        .into_iter()
        .map(|(_, i)| i)
        .filter(|i| {
            downs += usize::from(i.trend == "down");
            i.trend != "down" || downs == 1
        })
        .take(3)
        .collect()
}

pub fn week_view<Tz: TimeZone>(
    cx: &Context<'_, Tz>,
    sessions: &[Session],
    offset: u32,
) -> WeekView {
    let this_week = week_of(cx.today(), cx.first);
    let week = add_days(this_week, -7 * i64::from(offset));
    let end = add_days(week, 7);
    let current = offset == 0;
    let days = by_day(cx.tz, sessions);
    let done = finished(sessions);
    let bars: Vec<DayBar> = (0..7)
        .map(|i| {
            let day = add_days(week, i);
            let d = days.get(&day);
            DayBar {
                date: day.to_string(),
                secs: d.map_or(0, |d| d.secs),
                by_tag: d.map_or_else(Vec::new, |d| legend(&d.by_tag)),
            }
        })
        .collect();
    let secs = sum_days(&days, week, end);
    let count: u32 = days.range(week..end).map(|(_, d)| d.sessions).sum();
    let finished_secs: u64 = days.range(week..end).map(|(_, d)| d.finished_secs).sum();
    let elapsed = if current {
        u32::try_from((cx.today() - week).num_days() + 1).unwrap_or(7)
    } else {
        7
    };
    let had_before = days.range(..week).next().is_some();
    let until = current.then(|| cx.now - TimeDelta::days(7));
    let previous = cx.work(
        sessions,
        add_days(week, -7),
        week,
        until.map(|u| instant(cx.tz, u)),
    );
    let until_ms = until.map(|u| instant(cx.tz, u));
    let tag_previous = if had_before {
        legend(&cx.work_by_tag(sessions, add_days(week, -7), week, until_ms))
    } else {
        Vec::new()
    };
    let weeks = by_week(&days, cx.first);
    let trend: Vec<Dated> = (0..TREND_WEEKS)
        .rev()
        .map(|back| {
            let start = add_days(week, -7 * i64::from(back));
            Dated {
                secs: weeks.get(&start).copied().unwrap_or(0),
                date: start.to_string(),
            }
        })
        .collect();
    let earlier: Vec<u64> = trend
        .iter()
        .rev()
        .skip(1)
        .map(|w| w.secs)
        .filter(|secs| *secs > 0)
        .collect();
    let typical_secs = (earlier.len() >= TYPICAL_MIN_WEEKS)
        .then(|| earlier.iter().sum::<u64>() / u64::try_from(earlier.len()).unwrap_or(1));
    let goal = cx.goal_secs.filter(|g| *g > 0).map(|secs| Goal {
        secs,
        streak: streak(&weeks, this_week, secs),
    });
    let last_with_data = if secs == 0 {
        days.range(..week).next_back().map(|(day, _)| {
            let weeks = (week - week_of(*day, cx.first)).num_days() / 7;
            offset + u32::try_from(weeks).unwrap_or(0)
        })
    } else {
        None
    };
    WeekView {
        offset,
        start: week.to_string(),
        end: add_days(week, 6).to_string(),
        today: current.then(|| cx.today().weekday().days_since(cx.first)),
        secs,
        comparison: if had_before {
            compare(secs, previous, until)
        } else {
            None
        },
        daily_average: secs / u64::from(elapsed),
        session_average: if count > 0 {
            finished_secs / u64::from(count)
        } else {
            0
        },
        sessions: count,
        active_days: u32::try_from(bars.iter().filter(|b| b.secs > 0).count()).unwrap_or(0),
        elapsed_days: elapsed,
        days: bars,
        by_tag: legend(&merge_tags(days.range(week..end).map(|(_, d)| d))),
        tag_previous,
        trend,
        typical_secs,
        goal,
        heat: heat(cx, &done, week),
        records: records(cx.tz, &done, cx.first),
        insights: insights(cx, &done, week),
        has_history: !sessions.is_empty(),
        last_with_data,
    }
}

pub fn month_view<Tz: TimeZone>(
    cx: &Context<'_, Tz>,
    sessions: &[Session],
    offset: u32,
) -> MonthView {
    let this_month = cx.today().with_day(1).unwrap_or(cx.today());
    let start = this_month - Months::new(offset);
    let next = start + Months::new(1);
    let current = offset == 0;
    let days = by_day(cx.tz, sessions);
    let list: Vec<Dated> = start
        .iter_days()
        .take_while(|d| *d < next)
        .map(|d| Dated {
            secs: days.get(&d).map_or(0, |t| t.secs),
            date: d.to_string(),
        })
        .collect();
    let secs: u64 = list.iter().map(|d| d.secs).sum();
    let active: Vec<&Dated> = list.iter().filter(|d| d.secs > 0).collect();
    // A record: finished sessions only.
    let best_day = days
        .range(start..next)
        .filter(|(_, d)| d.finished_secs > 0)
        .max_by(|a, b| a.1.finished_secs.cmp(&b.1.finished_secs).then(b.0.cmp(a.0)))
        .map(|(day, d)| Dated {
            secs: d.finished_secs,
            date: day.to_string(),
        });
    let prev_start = start - Months::new(1);
    // The same day of last month (or its last day), at the same time.
    let until = current.then(|| {
        let day = cx
            .today()
            .day()
            .min(u32::try_from((start - prev_start).num_days()).unwrap_or(28));
        prev_start
            .with_day(day)
            .unwrap_or(prev_start)
            .and_time(cx.now.time())
    });
    let previous = cx.work(
        sessions,
        prev_start,
        start,
        until.map(|u| instant(cx.tz, u)),
    );
    let count = u32::try_from(active.len()).unwrap_or(0);
    MonthView {
        offset,
        start: start.to_string(),
        week_start: cx.first.num_days_from_monday(),
        today: current.then(|| cx.today().to_string()),
        secs,
        comparison: if days.range(..start).next().is_some() {
            compare(secs, previous, until)
        } else {
            None
        },
        active_days: count,
        elapsed_days: if current {
            cx.today().day()
        } else {
            u32::try_from(list.len()).unwrap_or(31)
        },
        active_average: if count > 0 {
            secs / u64::from(count)
        } else {
            0
        },
        best_day,
        by_tag: legend(&merge_tags(days.range(start..next).map(|(_, d)| d))),
        days: list,
        has_history: !sessions.is_empty(),
    }
}

// ---- Sessions list ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDay {
    pub date: String,
    pub secs: u64,
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub days: Vec<SessionDay>,
    /// Ask for sessions before this instant (a local midnight) for more.
    pub next: Option<i64>,
}

/// Groups sessions, newest first, into at most `want` whole days. `more`
/// says whether older sessions exist beyond `newest_first`.
pub fn page<Tz: TimeZone>(
    tz: &Tz,
    newest_first: Vec<Session>,
    want: usize,
    more: bool,
) -> SessionPage {
    let mut days: Vec<SessionDay> = Vec::new();
    let mut cut = false;
    for s in newest_first {
        let date = day_of(tz, s.start).to_string();
        if days.last().is_none_or(|d| d.date != date) {
            if days.len() == want {
                cut = true;
                break;
            }
            days.push(SessionDay {
                date,
                secs: 0,
                sessions: Vec::new(),
            });
        }
        if let Some(d) = days.last_mut() {
            d.secs += s.secs;
            d.sessions.push(s);
        }
    }
    let next = (cut || more)
        .then(|| days.last())
        .flatten()
        .and_then(|d| d.date.parse::<NaiveDate>().ok())
        .map(|d| midnight(tz, d));
    SessionPage { days, next }
}

// ---- CSV -----------------------------------------------------------------------------

/// The CSV's words, in the user's language.
pub struct CsvLabels {
    pub header: [String; 10],
    pub no_tag: String,
    pub noise: Box<dyn Fn(&str) -> String>,
    pub mode: Box<dyn Fn(&str) -> String>,
    pub edited: [String; 3],
    pub running: String,
}

fn field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn clock(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Every session, oldest first: one row each, with the in-progress one
/// marked. UTF-8 with a BOM and CRLF, which Excel opens as-is.
pub fn csv<Tz: TimeZone>(
    tz: &Tz,
    sessions: &[Session],
    tags: &[Tag],
    labels: &CsvLabels,
) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(
        &labels
            .header
            .iter()
            .map(|h| field(h))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("\r\n");
    let mut sorted: Vec<&Session> = sessions.iter().collect();
    sorted.sort_by_key(|s| s.start);
    for s in sorted {
        let (a, b) = (local(tz, s.start), local(tz, s.end));
        let tag = s
            .tag
            .and_then(|id| tags.iter().find(|t| t.id == id))
            .map_or(labels.no_tag.as_str(), |t| t.name.as_str());
        let row = [
            a.date().to_string(),
            a.format("%H:%M").to_string(),
            if s.running {
                String::new()
            } else {
                b.format("%H:%M").to_string()
            },
            clock(s.secs),
            s.secs.to_string(),
            tag.to_owned(),
            s.noise
                .as_deref()
                .map_or_else(String::new, |n| (labels.noise)(n)),
            (labels.mode)(&s.mode),
            labels.edited[usize::from(s.edited.min(2))].clone(),
            if s.running {
                labels.running.clone()
            } else {
                String::new()
            },
        ];
        out.push_str(&row.iter().map(|f| field(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Madrid;

    const H: i64 = 3_600_000;
    const M: i64 = 60_000;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        instant(
            &Madrid,
            NaiveDate::from_ymd_opt(y, mo, d)
                .unwrap()
                .and_hms_opt(h, mi, 0)
                .unwrap(),
        )
    }

    fn day(y: i32, mo: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap()
    }

    fn session(id: i64, start: i64, mins: i64, tag: TagId) -> Session {
        Session {
            id,
            start,
            end: start + mins * M,
            secs: u64::try_from(mins * 60).unwrap(),
            tag,
            mode: "stopwatch".into(),
            segments: vec![(start, start + mins * M)],
            ..Session::default()
        }
    }

    fn cx(now: NaiveDateTime) -> Context<'static, chrono_tz::Tz> {
        Context {
            tz: &Madrid,
            now,
            first: Weekday::Mon,
            tags: &[],
            goal_secs: None,
        }
    }

    #[test]
    fn weeks_start_where_windows_says() {
        // Sunday 1 Nov 2026.
        assert_eq!(week_of(day(2026, 11, 1), Weekday::Sun), day(2026, 11, 1));
        assert_eq!(week_of(day(2026, 11, 1), Weekday::Mon), day(2026, 10, 26));
        let sessions = [
            session(1, at(2026, 11, 1, 10, 0), 60, None),
            session(2, at(2026, 10, 31, 10, 0), 30, None),
        ];
        assert_eq!(
            totals(&Madrid, &sessions, day(2026, 11, 1), Weekday::Sun),
            (3600, 3600)
        );
        assert_eq!(
            totals(&Madrid, &sessions, day(2026, 11, 1), Weekday::Mon),
            (3600, 5400)
        );
    }

    #[test]
    fn a_session_across_midnight_counts_on_the_day_it_started() {
        // 23:30 → 00:45, with a pause.
        let start = at(2026, 11, 3, 23, 30);
        let s = Session {
            segments: vec![(start, start + 20 * M), (start + 40 * M, start + 75 * M)],
            end: start + 75 * M,
            ..session(1, start, 55, None)
        };
        let days = by_day(&Madrid, &[s]);
        assert_eq!(days[&day(2026, 11, 3)].secs, 55 * 60);
        assert!(!days.contains_key(&day(2026, 11, 4)));
    }

    #[test]
    fn clock_changes_keep_every_session_on_its_local_day() {
        // Spring forward: 29 March 2026, 02:00 → 03:00 (a 23-hour day).
        // Fall back: 25 October 2026, 03:00 → 02:00 (a 25-hour day).
        let sessions = [
            session(1, at(2026, 3, 28, 23, 30), 60, None),
            session(2, at(2026, 3, 29, 1, 30), 90, None),
            session(3, at(2026, 3, 29, 23, 50), 30, None),
            session(4, at(2026, 10, 25, 0, 30), 60, None),
            // 02:30 happens twice; `instant` takes the first.
            session(5, at(2026, 10, 25, 2, 30), 60, None),
            session(6, at(2026, 10, 25, 23, 30), 45, None),
        ];
        let days = by_day(&Madrid, &sessions);
        assert_eq!(days[&day(2026, 3, 28)].secs, 3600);
        assert_eq!(days[&day(2026, 3, 29)].secs, 120 * 60);
        assert_eq!(days[&day(2026, 10, 25)].secs, 165 * 60);
        // The day after a change starts at its own local midnight.
        assert_eq!(
            midnight(&Madrid, day(2026, 3, 30)) - midnight(&Madrid, day(2026, 3, 29)),
            23 * H
        );
        assert_eq!(
            midnight(&Madrid, day(2026, 10, 26)) - midnight(&Madrid, day(2026, 10, 25)),
            25 * H
        );
        // Weeks spanning a change still hold all their sessions.
        let (_, week) = totals(&Madrid, &sessions, day(2026, 3, 29), Weekday::Mon);
        assert_eq!(week, 180 * 60);
        let (today, week) = totals(&Madrid, &sessions, day(2026, 10, 25), Weekday::Mon);
        assert_eq!((today, week), (165 * 60, 165 * 60));
    }

    #[test]
    fn the_heat_map_follows_the_wall_clock_through_a_clock_change() {
        let mut sessions: Vec<Session> = (0..5)
            .map(|i| session(i, at(2026, 10, 19 + i as u32, 10, 0), 70, None))
            .collect();
        // 01:30 → 03:30 local on the fall-back night lasts three real hours.
        let start = at(2026, 10, 25, 1, 30);
        sessions.push(Session {
            end: start + 3 * H,
            secs: 3 * 3600,
            segments: vec![(start, start + 3 * H)],
            ..session(9, start, 0, None)
        });
        let heat = heat(
            &cx(day(2026, 10, 26).and_hms_opt(12, 0, 0).unwrap()),
            &sessions,
            day(2026, 10, 19),
        )
        .unwrap();
        let sunday = &heat.rows[6];
        assert_eq!(sunday[HEAT_HOURS], 3 * 3600, "all of it at night");
        // 10:00 → 11:10 on weekdays: 60 min at 10 h, 10 at 11 h.
        assert_eq!(heat.rows[0][4], 3600);
        assert_eq!(heat.rows[0][5], 600);
        assert_eq!(heat.best_slot, Some(10));
    }

    #[test]
    fn the_current_week_compares_up_to_the_same_point() {
        // Thursday 5 Nov 2026, 20:44.
        let now = day(2026, 11, 5).and_hms_opt(20, 44, 0).unwrap();
        let sessions = [
            // Last week: Monday 1 h, Thursday 19:00–21:00 (so 1 h 44 by
            // 20:44), Friday 3 h (after the point).
            session(1, at(2026, 10, 26, 10, 0), 60, None),
            session(2, at(2026, 10, 29, 19, 0), 120, None),
            session(3, at(2026, 10, 30, 10, 0), 180, None),
            // This week: 4 h.
            session(4, at(2026, 11, 2, 10, 0), 240, None),
        ];
        let w = week_view(&cx(now), &sessions, 0);
        let c = w.comparison.unwrap();
        assert_eq!(c.previous_secs, (60 + 104) * 60);
        assert_eq!(c.delta_secs, 240 * 60 - 164 * 60);
        assert_eq!(c.pct, Some(46));
        assert_eq!(c.until.as_deref(), Some("2026-10-29T20:44"));
        // A past week compares with the whole week before it.
        let past = week_view(&cx(now), &sessions, 1);
        assert_eq!(past.comparison, None, "nothing before it");
        assert_eq!(past.secs, 360 * 60);
    }

    #[test]
    fn a_thin_previous_week_gets_minutes_not_a_percentage() {
        // Monday morning: last Monday by now there were only 20 min.
        let now = day(2026, 11, 2).and_hms_opt(11, 0, 0).unwrap();
        let sessions = [
            session(1, at(2026, 10, 26, 9, 0), 20, None),
            session(2, at(2026, 10, 27, 9, 0), 300, None),
            session(3, at(2026, 11, 2, 9, 0), 45, None),
        ];
        let c = week_view(&cx(now), &sessions, 0).comparison.unwrap();
        assert_eq!(
            (c.previous_secs, c.delta_secs, c.pct),
            (20 * 60, 25 * 60, None)
        );
        // At 30 min the percentage comes back.
        let sessions = [
            session(1, at(2026, 10, 26, 9, 0), 30, None),
            session(3, at(2026, 11, 2, 9, 0), 45, None),
        ];
        assert_eq!(
            week_view(&cx(now), &sessions, 0).comparison.unwrap().pct,
            Some(50)
        );
    }

    #[test]
    fn the_streak_counts_weeks_not_days() {
        // Goal: 2 h a week. Three weeks met, then the one in progress.
        let goal = 2 * 3600;
        let mut sessions = vec![
            session(1, at(2026, 10, 5, 10, 0), 30, None), // short week: breaks it
            session(2, at(2026, 10, 12, 10, 0), 120, None),
            session(3, at(2026, 10, 21, 10, 0), 150, None),
            session(4, at(2026, 10, 26, 10, 0), 60, None),
            session(5, at(2026, 11, 1, 10, 0), 60, None), // Sunday: same week
        ];
        let weeks = |s: &[Session]| by_week(&by_day(&Madrid, s), Weekday::Mon);
        // Tuesday 3 Nov, nothing yet this week: the streak holds at three.
        assert_eq!(streak(&weeks(&sessions), day(2026, 11, 2), goal), 3);
        // The week in progress adds as soon as it gets there.
        sessions.push(session(6, at(2026, 11, 3, 9, 0), 120, None));
        assert_eq!(streak(&weeks(&sessions), day(2026, 11, 2), goal), 4);
        // A missed week in between ends it, whatever came before.
        sessions.retain(|s| s.id != 3);
        assert_eq!(streak(&weeks(&sessions), day(2026, 11, 2), goal), 2);
        // No goal, no streak.
        assert_eq!(streak(&weeks(&sessions), day(2026, 11, 2), 0), 0);
    }

    #[test]
    fn the_week_carries_its_goal_trend_and_tags_before() {
        // Thursday 5 Nov 2026, 12:00.
        let now = day(2026, 11, 5).and_hms_opt(12, 0, 0).unwrap();
        let sessions = vec![
            session(1, at(2026, 10, 12, 10, 0), 120, Some(1)),
            session(2, at(2026, 10, 19, 10, 0), 180, Some(1)),
            // Last week: Monday, and Friday (after "the same point").
            session(3, at(2026, 10, 26, 10, 0), 60, Some(1)),
            session(4, at(2026, 10, 30, 10, 0), 240, Some(2)),
            session(5, at(2026, 11, 3, 10, 0), 150, Some(1)),
        ];
        let with_goal = Context {
            goal_secs: Some(2 * 3600),
            ..cx(now)
        };
        let w = week_view(&with_goal, &sessions, 0);
        assert_eq!(w.trend.len(), TREND_WEEKS as usize);
        let last: Vec<u64> = w.trend.iter().rev().take(4).map(|t| t.secs / 60).collect();
        assert_eq!(last, [150, 300, 180, 120], "this week first, whole weeks");
        assert_eq!(w.trend.last().map(|t| t.date.as_str()), Some("2026-11-02"));
        // Three earlier weeks with study: (300 + 180 + 120) / 3.
        assert_eq!(w.typical_secs, Some(200 * 60));
        // Per tag, last week only up to Thursday 12:00: Friday's isn't there.
        assert_eq!(
            w.tag_previous,
            [TagTime {
                tag: Some(1),
                secs: 3600
            }]
        );
        // 150 min this week meets 2 h, and so did the three before.
        assert_eq!(
            w.goal,
            Some(Goal {
                secs: 7200,
                streak: 4
            })
        );
        // Without a goal there is nothing to show.
        assert_eq!(week_view(&cx(now), &sessions, 0).goal, None);
    }

    #[test]
    fn empty_weeks_say_whether_there_is_history() {
        let now = day(2026, 11, 5).and_hms_opt(12, 0, 0).unwrap();
        let none = week_view(&cx(now), &[], 0);
        assert!(!none.has_history);
        assert_eq!(
            (none.secs, none.comparison.clone(), none.last_with_data),
            (0, None, None)
        );
        assert!(none.insights.is_empty() && none.heat.is_none());
        // Studied three weeks ago, not since.
        let sessions = [session(1, at(2026, 10, 14, 9, 0), 90, None)];
        let quiet = week_view(&cx(now), &sessions, 0);
        assert!(quiet.has_history);
        assert_eq!(quiet.secs, 0);
        assert_eq!(quiet.last_with_data, Some(3));
        assert_eq!(quiet.records.total.unwrap().secs, 90 * 60);
        assert_eq!(quiet.comparison, None, "0 against 0 says nothing");
    }

    #[test]
    fn a_subject_dropping_two_weeks_running_makes_the_list() {
        let now = day(2026, 11, 8).and_hms_opt(22, 0, 0).unwrap();
        let tags = [Tag {
            id: 3,
            name: "Redes".into(),
            color: "sky".into(),
        }];
        // Three closed weeks going down, and nothing yet this week.
        let sessions = weeks_of_redes(&[
            (day(2026, 10, 12), 240),
            (day(2026, 10, 19), 150),
            (day(2026, 10, 26), 60),
            (day(2026, 11, 2), 0),
        ]);
        let cx = Context {
            tags: &tags,
            ..cx(now)
        };
        let found = insights(&cx, &sessions, day(2026, 11, 2));
        assert!(found.iter().any(|i| i.key == "insightTagDown"
            && i.args["tag"] == json!("Redes")
            && i.trend == "down"));
        // Never what the page already shows.
        assert!(
            found
                .iter()
                .all(|i| !["insightMore", "insightRecord", "insightDays"].contains(&i.key))
        );
        assert!(found.len() <= 3);
    }

    /// Five 90-minute sessions of tag 1 on weekdays and one of "Redes" (tag
    /// 3) on Monday afternoon, per week; 0 minutes leaves Redes out.
    fn weeks_of_redes(weeks: &[(NaiveDate, i64)]) -> Vec<Session> {
        let mut sessions = Vec::new();
        let mut id = 0;
        let start = |date: NaiveDate, h| at(date.year(), date.month(), date.day(), h, 0);
        for &(week_start, redes) in weeks {
            for d in 0..5 {
                id += 1;
                sessions.push(session(
                    id,
                    start(week_start + Days::new(d), 9),
                    90,
                    Some(1),
                ));
            }
            if redes > 0 {
                id += 1;
                sessions.push(session(id, start(week_start, 16), redes, Some(3)));
            }
        }
        sessions
    }

    #[test]
    fn a_half_week_is_not_a_trend() {
        // Monday evening: Redes is at 0 only because the week has just
        // begun. Up to the same point it would read 160 → 110 → 0.
        let now = day(2026, 11, 2).and_hms_opt(20, 0, 0).unwrap();
        let tags = [Tag {
            id: 3,
            name: "Redes".into(),
            color: "sky".into(),
        }];
        let mut sessions = weeks_of_redes(&[
            (day(2026, 10, 12), 100),
            (day(2026, 10, 19), 160),
            (day(2026, 10, 26), 110),
        ]);
        sessions.push(session(90, at(2026, 11, 2, 9, 0), 60, Some(1)));
        sessions.push(session(91, at(2026, 11, 2, 11, 0), 60, Some(1)));
        let cx = Context {
            tags: &tags,
            ..cx(now)
        };
        let found = insights(&cx, &sessions, day(2026, 11, 2));
        assert!(found.iter().all(|i| i.key != "insightTagDown"), "{found:?}");
    }

    #[test]
    fn one_down_at_most() {
        let i = |trend, key| insight(trend, key, &[]);
        let picked = pick(vec![
            (0.9, i("down", "insightTagDown")),
            (0.8, i("down", "insightBelowAverage")),
            (0.3, i("neutral", "insightMornings")),
            (0.2, i("down", "insightShorterSessions")),
        ]);
        let keys: Vec<&str> = picked.iter().map(|i| i.key).collect();
        assert_eq!(keys, ["insightTagDown", "insightMornings"]);
        // The base of the comparison is in the sentence.
        let now = day(2026, 11, 5).and_hms_opt(20, 0, 0).unwrap();
        let mut sessions = weeks_of_redes(&[
            (day(2026, 9, 28), 0),
            (day(2026, 10, 5), 0),
            (day(2026, 10, 12), 0),
            (day(2026, 10, 19), 0),
            (day(2026, 10, 26), 0),
        ]);
        sessions.push(session(90, at(2026, 11, 2, 9, 0), 60, None));
        sessions.push(session(91, at(2026, 11, 3, 9, 0), 60, None));
        let found = insights(&cx(now), &sessions, day(2026, 11, 2));
        let below = found
            .iter()
            .find(|i| i.key == "insightBelowAverageSoFar")
            .expect("well below the usual week");
        assert_eq!(below.args["weeks"], json!(5));
    }

    #[test]
    fn a_session_in_progress_is_time_not_yet_a_session() {
        let now = day(2026, 11, 5).and_hms_opt(20, 0, 0).unwrap();
        let mut sessions = vec![
            session(1, at(2026, 11, 2, 9, 0), 60, None),
            session(2, at(2026, 11, 3, 9, 0), 90, None),
            session(3, at(2026, 10, 28, 9, 0), 120, None),
        ];
        // Longer than any finished one.
        sessions.push(Session {
            running: true,
            ..session(4, at(2026, 11, 5, 17, 0), 180, None)
        });
        let w = week_view(&cx(now), &sessions, 0);
        assert_eq!(w.secs, (60 + 90 + 180) * 60, "its time counts");
        assert_eq!(w.days[3].secs, 180 * 60);
        assert_eq!(w.sessions, 2, "it isn't a session yet");
        assert_eq!(w.session_average, 75 * 60);
        assert_eq!(w.active_days, 3);
        let r = &w.records;
        assert_eq!(r.longest_session.as_ref().unwrap().secs, 120 * 60);
        assert_eq!(r.best_day.as_ref().unwrap().secs, 120 * 60);
        assert_eq!(r.total.as_ref().unwrap().secs, 270 * 60);
        let m = month_view(&cx(now), &sessions, 0);
        assert_eq!(m.secs, (60 + 90 + 180) * 60);
        assert_eq!(m.best_day.unwrap().secs, 90 * 60);
    }

    #[test]
    fn too_little_data_says_nothing() {
        let now = day(2026, 11, 5).and_hms_opt(20, 0, 0).unwrap();
        let sessions = [session(1, at(2026, 11, 2, 9, 0), 50, None)];
        assert!(insights(&cx(now), &sessions, day(2026, 11, 2)).is_empty());
    }

    #[test]
    fn pages_hold_whole_days_and_point_at_the_next() {
        let sessions: Vec<Session> = [
            at(2026, 11, 5, 18, 0),
            at(2026, 11, 5, 9, 0),
            at(2026, 11, 3, 9, 0),
            at(2026, 11, 1, 9, 0),
        ]
        .iter()
        .enumerate()
        .map(|(i, &s)| session(i as i64, s, 30, None))
        .collect();
        let p = page(&Madrid, sessions.clone(), 2, false);
        assert_eq!(p.days.len(), 2);
        assert_eq!(p.days[0].sessions.len(), 2);
        assert_eq!(p.days[0].secs, 3600);
        assert_eq!(p.next, Some(midnight(&Madrid, day(2026, 11, 3))));
        let rest = page(&Madrid, sessions[3..].to_vec(), 2, false);
        assert_eq!((rest.days.len(), rest.next), (1, None));
    }

    #[test]
    fn panel_window_and_csv_agree() {
        let tags = [Tag {
            id: 1,
            name: "Cálculo, I".into(),
            color: "rose".into(),
        }];
        let now = day(2026, 11, 5).and_hms_opt(20, 0, 0).unwrap();
        let mut sessions = vec![
            session(1, at(2026, 11, 2, 9, 0), 61, Some(1)),
            session(2, at(2026, 11, 3, 23, 40), 50, None),
            session(3, at(2026, 11, 5, 8, 15), 33, Some(1)),
            session(4, at(2026, 10, 30, 9, 0), 120, None),
        ];
        // The session in progress counts in every total, and the CSV.
        sessions.push(Session {
            running: true,
            noise: Some("brown".into()),
            ..session(5, at(2026, 11, 5, 19, 20), 40, Some(1))
        });
        let (today, week) = totals(&Madrid, &sessions, now.date(), Weekday::Mon);
        let w = week_view(&cx(now), &sessions, 0);
        assert_eq!(w.secs, week);
        assert_eq!(w.days[3].secs, today);
        assert_eq!(w.days.iter().map(|d| d.secs).sum::<u64>(), week);
        // Sessions and their average: the finished ones only.
        assert_eq!(w.sessions, 3);
        assert_eq!(w.session_average, (61 + 50 + 33) * 60 / 3);
        let page = page(
            &Madrid,
            {
                let mut s = sessions.clone();
                s.sort_by_key(|s| -s.start);
                s
            },
            30,
            false,
        );
        assert_eq!(page.days[0].secs, today);
        let labels = CsvLabels {
            header: [
                "date", "start", "end", "duration", "seconds", "tag", "noise", "mode", "edited",
                "running",
            ]
            .map(String::from),
            no_tag: "Sin etiqueta".into(),
            noise: Box::new(|c| c.to_owned()),
            mode: Box::new(|m| m.to_owned()),
            edited: [String::new(), "edited".into(), "added".into()],
            running: "yes".into(),
        };
        let text = csv(&Madrid, &sessions, &tags, &labels);
        assert!(text.starts_with('\u{feff}'));
        let rows: Vec<Vec<&str>> = text
            .trim_start_matches('\u{feff}')
            .split("\r\n")
            .skip(1)
            .filter(|l| !l.is_empty())
            .map(|l| l.split(',').collect())
            .collect();
        // Quoted "Cálculo, I" adds a comma: count seconds from the 5th field.
        let secs_in = |from: NaiveDate, to: NaiveDate| -> u64 {
            rows.iter()
                .filter(|r| {
                    let d: NaiveDate = r[0].parse().unwrap();
                    d >= from && d < to
                })
                .map(|r| r[4].parse::<u64>().unwrap())
                .sum()
        };
        assert_eq!(secs_in(day(2026, 11, 2), day(2026, 11, 9)), week);
        assert_eq!(secs_in(now.date(), day(2026, 11, 6)), today);
        assert!(text.contains("\"Cálculo, I\""));
        assert!(
            text.contains(",brown,stopwatch,,yes\r\n"),
            "the running one is marked"
        );
    }
}
