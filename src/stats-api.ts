// The Statistics window's link to Rust (SPEC §4.3d). Rust aggregates (one
// function feeds the panel, this window and the CSV); this side only draws.
// Outside Tauri a mock with ten weeks of plausible sessions stands in, for
// design work in a browser:
//   ?empty=1   no sessions at all       ?quiet=1  history, but not this week
//   ?notags=1  sessions without tags    ?running=1 a session in progress
//   ?tab=month|sessions                 ?lang=es|en   ?color=brown|white

import { isTauri, type Color, type Lang, type Tag } from "./api";

export type Tab = "week" | "month" | "sessions";
/** `null` is "Sin etiqueta". */
export type TagId = number | null;

export interface TagTime {
  tag: TagId;
  secs: number;
}

export interface DayBar {
  /** Local date, `YYYY-MM-DD`. */
  date: string;
  secs: number;
  /** In legend order; only tags with time. */
  byTag: TagTime[];
}

export interface Dated {
  secs: number;
  /** Local date, `YYYY-MM-DD` (the first day, for a week). */
  date: string;
}

export interface Records {
  longestSession: Dated | null;
  bestDay: Dated | null;
  bestWeek: Dated | null;
  /** `date` is the first day with study. */
  total: Dated | null;
}

/** Against the previous week or month, up to the same point in the current
 *  one. Below 30 min before, a percentage means nothing: `pct` is `null` and
 *  the difference goes in minutes. */
export interface Comparison {
  previousSecs: number;
  /** This period minus the previous one. */
  deltaSecs: number;
  pct: number | null;
  /** Local `YYYY-MM-DDTHH:MM` the current period is compared up to. */
  until: string | null;
}

/** A sentence for "Así ha ido": an i18n key, its values, and its trend. */
export interface Insight {
  trend: "up" | "neutral" | "down";
  key: InsightKey;
  args: Record<string, string | number>;
}

export type InsightKey =
  | "insightTagDown"
  | "insightTagUp"
  | "insightAboveAverage"
  | "insightBelowAverage"
  | "insightAboveAverageSoFar"
  | "insightBelowAverageSoFar"
  | "insightLongerSessions"
  | "insightShorterSessions"
  | "insightMornings"
  | "insightAfternoons"
  | "insightEvenings"
  | "insightWeekday";

/** "Cuándo estudias más", over the last weeks up to the one shown. */
export interface Heat {
  /** Rows: days from the week's first; columns: 6 h … 22 h, then "noche"
   *  (23 h – 6 h). Seconds. */
  rows: number[][];
  weeks: number;
  /** Start hour of the best two-hour slot between 6 and 23 h. */
  bestSlot: number | null;
}

/** The weekly goal (Settings → Study) and how it is going. */
export interface Goal {
  /** Seconds a week. */
  secs: number;
  /** Weeks in a row that met it, counted back from the current one. */
  streak: number;
}

export const HEAT_FIRST_HOUR = 6;
export const HEAT_HOURS = 17;

export interface WeekView {
  /** Steps back from the current week (0 = this one). */
  offset: number;
  start: string;
  end: string;
  /** Index of today in `days`, in the current week. */
  today: number | null;
  secs: number;
  comparison: Comparison | null;
  dailyAverage: number;
  sessionAverage: number;
  /** Finished sessions: the one in progress counts as time (`secs`, the
   *  bars) but not as a session, a record or a sentence until it ends. */
  sessions: number;
  activeDays: number;
  /** Days gone by: 7, or up to today in the current week. */
  elapsedDays: number;
  days: DayBar[];
  /** Legend order: most time first, "Sin etiqueta" last. */
  byTag: TagTime[];
  /** The previous week per tag, up to the same point in the current one;
   *  empty with nothing before to compare with. */
  tagPrevious: TagTime[];
  /** The last 8 weeks, oldest first (`date`: the week's first day); the
   *  last one is this week. */
  trend: Dated[];
  /** The mean of the earlier weeks of `trend` that had study. */
  typicalSecs: number | null;
  /** `null` without a weekly goal. */
  goal: Goal | null;
  heat: Heat | null;
  records: Records;
  insights: Insight[];
  /** Anything ever recorded. */
  hasHistory: boolean;
  /** The closest earlier week with study, when this one has none. */
  lastWithData: number | null;
}

export interface MonthView {
  offset: number;
  /** First day of the month. */
  start: string;
  /** 0 = Monday … 6 = Sunday, from Windows' regional settings. */
  weekStart: number;
  today: string | null;
  days: Dated[];
  secs: number;
  comparison: Comparison | null;
  activeDays: number;
  elapsedDays: number;
  /** Mean over the days with study. */
  activeAverage: number;
  bestDay: Dated | null;
  byTag: TagTime[];
  hasHistory: boolean;
}

export interface Session {
  id: number;
  /** Milliseconds since the epoch. */
  start: number;
  end: number;
  secs: number;
  tag: TagId;
  /** The noise playing when it started; `null` without noise or before it
   *  was recorded. */
  noise: Color | null;
  mode: "stopwatch" | "pomodoro";
  /** 0 = as recorded, 1 = edited, 2 = added by hand. */
  edited: 0 | 1 | 2;
  running: boolean;
}

export interface SessionDay {
  date: string;
  secs: number;
  sessions: Session[];
}

/** Whole days, newest first; `next` asks for the page before. */
export interface SessionPage {
  days: SessionDay[];
  next: number | null;
}

/** Why an edit or an addition was refused; an overlap names the session it
 *  runs into by its start (ms). */
export type SessionError =
  | { kind: "overlap"; start: number }
  | { kind: "future" | "tooShort" | "running" | "notFound" | "unavailable" };

/** What the window needs to look like the rest of Rumor. */
export interface StatsContext {
  lang: Lang;
  /** The accent follows the noise, as in the panel. */
  color: Color;
  systemAccent: string | null;
  mica: boolean;
}

export interface StatsBackend {
  context(): Promise<StatsContext>;
  tags(): Promise<Tag[]>;
  week(offset: number): Promise<WeekView>;
  month(offset: number): Promise<MonthView>;
  /** At least `days` days with sessions, started before `before` (ms). */
  sessions(before: number | null, days: number): Promise<SessionPage>;
  /** Rejects with a `SessionError`. */
  updateSession(id: number, secs: number, tag: TagId): Promise<void>;
  deleteSession(id: number): Promise<void>;
  /** Puts a deleted session back ("Deshacer"). */
  restoreSession(id: number): Promise<void>;
  /** Rejects with a `SessionError`. */
  addSession(start: number, secs: number, tag: TagId): Promise<void>;
  /** `false` if the user cancelled the save dialog. */
  exportCsv(): Promise<boolean>;
  ready(): Promise<void>;
  /** A session ended, changed, appeared or went away. */
  onChanged(cb: () => void): void;
  /** Language, accent or Mica changed. */
  onContext(cb: (cx: StatsContext) => void): void;
}

async function tauriBackend(): Promise<StatsBackend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    context: () => invoke("stats_context"),
    tags: () => invoke("tags_list"),
    week: (offset) => invoke("stats_week", { offset }),
    month: (offset) => invoke("stats_month", { offset }),
    sessions: (before, days) => invoke("stats_sessions", { before, days }),
    updateSession: (id, secs, tag) => invoke("session_update", { id, secs, tag }),
    deleteSession: (id) => invoke("session_delete", { id }),
    restoreSession: (id) => invoke("session_restore", { id }),
    addSession: (start, secs, tag) => invoke("session_add", { start, secs, tag }),
    exportCsv: () => invoke("stats_export_csv"),
    ready: () => invoke("stats_ready"),
    onChanged: (cb) => void listen("stats-changed", () => cb()),
    onContext: (cb) => void listen<StatsContext>("stats-context", (e) => cb(e.payload)),
  };
}

// ---- Mock ------------------------------------------------------------------------
// A rough copy of what `study/stats.rs` computes, enough to design with.

const DAY = 86_400_000;
const HOUR = 3_600_000;
/** Below this the previous period gets minutes, not a percentage. */
const PCT_FLOOR = 30 * 60;

/** Local `YYYY-MM-DD`. */
export function isoDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function isoMinute(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${isoDate(d)}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Local midnight of a `YYYY-MM-DD`, in ms. */
function day0(iso: string): number {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d).getTime();
}

function midnight(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n, d.getHours(), d.getMinutes());
}

/** Monday of the week containing `d`. */
function mondayOf(d: Date): Date {
  return addDays(midnight(d), -((d.getDay() + 6) % 7));
}

/** Deterministic, so screenshots don't change between runs. */
function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

function pick<T>(rand: () => number, weighted: [T, number][]): T {
  let r = rand() * weighted.reduce((a, [, w]) => a + w, 0);
  for (const [v, w] of weighted) {
    if ((r -= w) < 0) return v;
  }
  return weighted[weighted.length - 1][0];
}

function generate(now: Date, notags: boolean, quiet: boolean): Session[] {
  const rand = random(7);
  const sessions: Session[] = [];
  const first = addDays(mondayOf(now), -9 * 7);
  let id = 1;
  for (let day = first; day <= now; day = addDays(day, 1)) {
    const weeksAgo = Math.round((mondayOf(now).getTime() - mondayOf(day).getTime()) / (7 * DAY));
    if (quiet && weeksAgo === 0) continue;
    const weekend = day.getDay() === 0 || day.getDay() === 6;
    if (rand() > (weekend ? 0.45 : 0.85)) continue;
    // A little more each week, a slow week two weeks ago, and "Redes"
    // fading over the last two.
    const effort = (1 + (9 - weeksAgo) * 0.04) * (weeksAgo === 2 ? 0.55 : 1);
    const slots = [9, 11, 16, 18, 21].filter(() => rand() < (weeksAgo === 2 ? 0.35 : 0.55)).slice(0, weekend ? 1 : 3);
    for (const hour of slots.length ? slots : [10]) {
      const startMs = midnight(day).getTime() + hour * HOUR + Math.floor(rand() * 30) * 60_000;
      const mins = Math.round((30 + rand() * 75) * effort * (weekend ? 0.7 : 1));
      const endMs = startMs + mins * 60_000;
      if (endMs > now.getTime()) break;
      const redes = weeksAgo >= 2 ? 0.3 : weeksAgo === 1 ? 0.12 : 0.03;
      let tag = notags ? null : pick<TagId>(rand, [[1, 0.42], [2, 0.42], [null, 0.16]]);
      if (!notags && rand() < redes) tag = 3;
      sessions.push({
        id: id++,
        start: startMs,
        end: endMs,
        secs: mins * 60,
        tag,
        noise: pick<Color>(rand, [["brown", 0.5], ["pink", 0.35], ["white", 0.15]]),
        mode: rand() < 0.75 ? "stopwatch" : "pomodoro",
        edited: 0,
        running: false,
      });
    }
  }
  return sessions;
}

const MOCK_TAGS: Tag[] = [
  { id: 1, name: "Cálculo", color: "rose" },
  { id: 2, name: "EDA", color: "amber" },
  { id: 3, name: "Redes", color: "sky" },
];

/** Sessions belong to the day they start on (SPEC §4.3d). */
const startedIn = (s: Session, from: number, to: number) => s.start >= from && s.start < to;

/** Work by `until`: whole sessions before it, the part done of one across it. */
function workBy(s: Session, until: number): number {
  return Math.max(0, Math.min(s.end, until) - s.start) / 1000;
}

function sum(sessions: Session[], from: number, to: number, until = Infinity): number {
  return sessions.filter((s) => startedIn(s, from, Math.min(to, until))).reduce((a, s) => a + Math.min(s.secs, workBy(s, until)), 0);
}

function byTagOf(sessions: Session[], from: number, to: number, until = Infinity): TagTime[] {
  const map = new Map<TagId, number>();
  for (const s of sessions) {
    if (startedIn(s, from, Math.min(to, until))) map.set(s.tag, (map.get(s.tag) ?? 0) + Math.min(s.secs, workBy(s, until)));
  }
  return [...map]
    .map(([tag, secs]) => ({ tag, secs }))
    .sort((a, b) => (a.tag === null ? 1 : b.tag === null ? -1 : b.secs - a.secs));
}

function compare(secs: number, previousSecs: number, until: Date | null): Comparison | null {
  if (secs === 0 && previousSecs === 0) return null;
  return {
    previousSecs,
    deltaSecs: secs - previousSecs,
    pct: previousSecs >= PCT_FLOOR ? Math.round(((secs - previousSecs) / previousSecs) * 100) : null,
    until: until ? isoMinute(until) : null,
  };
}

function records(sessions: Session[]): Records {
  if (sessions.length === 0) return { longestSession: null, bestDay: null, bestWeek: null, total: null };
  const longest = sessions.reduce((a, b) => (b.secs > a.secs ? b : a));
  const days = new Map<string, number>();
  const weeks = new Map<string, number>();
  for (const s of sessions) {
    const d = new Date(s.start);
    days.set(isoDate(d), (days.get(isoDate(d)) ?? 0) + s.secs);
    const w = isoDate(mondayOf(d));
    weeks.set(w, (weeks.get(w) ?? 0) + s.secs);
  }
  const best = (m: Map<string, number>): Dated => {
    const [date, secs] = [...m].reduce((a, b) => (b[1] > a[1] ? b : a));
    return { date, secs };
  };
  const first = sessions.reduce((a, b) => (b.start < a.start ? b : a));
  return {
    longestSession: { secs: longest.secs, date: isoDate(new Date(longest.start)) },
    bestDay: best(days),
    bestWeek: best(weeks),
    total: { secs: sessions.reduce((a, s) => a + s.secs, 0), date: isoDate(new Date(first.start)) },
  };
}

function heatOf(sessions: Session[], weekStart: Date, weeks: number): Heat | null {
  const from = addDays(weekStart, -7 * (weeks - 1)).getTime();
  const to = addDays(weekStart, 7).getTime();
  const inside = sessions.filter((s) => startedIn(s, from, to));
  if (inside.length < 5 || inside.reduce((a, s) => a + s.secs, 0) < 5 * 3600) return null;
  const rows = Array.from({ length: 7 }, () => new Array<number>(HEAT_HOURS + 1).fill(0));
  for (const s of inside) {
    const row = (new Date(s.start).getDay() + 6) % 7;
    for (let t = s.start; t < s.end; t += 60_000) {
      const h = new Date(t).getHours();
      const col = h >= HEAT_FIRST_HOUR && h < HEAT_FIRST_HOUR + HEAT_HOURS ? h - HEAT_FIRST_HOUR : HEAT_HOURS;
      rows[row][col] += 60;
    }
  }
  let bestSlot: number | null = null;
  let best = 0;
  for (let c = 0; c < HEAT_HOURS - 1; c++) {
    const v = rows.reduce((a, r) => a + r[c] + r[c + 1], 0);
    if (v > best) [best, bestSlot] = [v, HEAT_FIRST_HOUR + c];
  }
  return { rows, weeks, bestSlot };
}

/** The sentences that stray most from the user's own habits, leaving out
 *  what the page already shows (SPEC §4.3d). */
function insightsOf(sessions: Session[], start: Date, until: number, tags: Tag[]): Insight[] {
  const weekMs = 7 * DAY;
  const from = start.getTime();
  const current = until < from + weekMs;
  const inWeek = sessions.filter((s) => startedIn(s, from, from + weekMs) && s.start < until);
  const total = inWeek.reduce((a, s) => a + s.secs, 0);
  if (inWeek.length < 2 || total < 3600) return [];
  const elapsed = until - from;
  const past = Array.from({ length: 8 }, (_, k) => {
    const a = from - weekMs * (k + 1);
    return sessions.filter((s) => startedIn(s, a, a + weekMs));
  }).filter((w) => w.length > 0);
  const scored: [number, Insight][] = [];

  // A subject going down (or up) two weeks in a row, over closed weeks only.
  const closed = current ? 1 : 0;
  for (const tag of tags) {
    const at = (k: number) => sum(sessions.filter((s) => s.tag === tag.id), from - weekMs * k, from - weekMs * k + weekMs);
    const [w2, w1, w0] = [at(closed + 2), at(closed + 1), at(closed)];
    if (w2 < 3600) continue;
    if (w1 <= w2 * 0.75 && w0 <= w1 * 0.75) scored.push([1 - w0 / w2, { trend: "down", key: "insightTagDown", args: { tag: tag.name } }]);
    else if (w1 >= w2 * 1.25 && w0 >= w1 * 1.25) scored.push([w0 / w2 - 1, { trend: "up", key: "insightTagUp", args: { tag: tag.name } }]);
  }
  // This week against the usual week, up to the same point.
  if (past.length >= 4) {
    const usual = past.reduce((a, w) => a + w.filter((s) => s.start - mondayOf(new Date(s.start)).getTime() < elapsed).reduce((b, s) => b + s.secs, 0), 0) / past.length;
    const dev = usual > 0 ? total / usual - 1 : 0;
    if (Math.abs(dev) >= 0.2) {
      const args = { pct: Math.round(Math.abs(dev) * 100), weeks: past.length };
      scored.push([
        Math.abs(dev),
        dev > 0
          ? { trend: "up", key: current ? "insightAboveAverageSoFar" : "insightAboveAverage", args }
          : { trend: "down", key: current ? "insightBelowAverageSoFar" : "insightBelowAverage", args },
      ]);
    }
  }
  // Session length against the usual.
  const before = past.flat();
  if (before.length >= 10 && inWeek.length >= 3) {
    const usual = before.reduce((a, s) => a + s.secs, 0) / before.length;
    const now = total / inWeek.length;
    const dev = now / usual - 1;
    const mins = Math.round(Math.abs(now - usual) / 60);
    if (Math.abs(dev) >= 0.2 && mins >= 5) {
      scored.push([Math.abs(dev), dev > 0 ? { trend: "up", key: "insightLongerSessions", args: { mins } } : { trend: "down", key: "insightShorterSessions", args: { mins } }]);
    }
  }
  // Time of day, against the usual.
  if (before.length >= 10) {
    const part = (s: Session) => { const h = new Date(s.start).getHours(); return h < 14 ? 0 : h < 20 ? 1 : 2; };
    const share = (list: Session[]) => {
      const t = [0, 0, 0];
      for (const s of list) t[part(s)] += s.secs;
      const all = t[0] + t[1] + t[2] || 1;
      return t.map((v) => v / all);
    };
    const [a, b] = [share(inWeek), share(before)];
    const i = [0, 1, 2].reduce((m, k) => (a[k] - b[k] > a[m] - b[m] ? k : m), 0);
    if (a[i] - b[i] >= 0.25) {
      scored.push([a[i] - b[i], { trend: "neutral", key: (["insightMornings", "insightAfternoons", "insightEvenings"] as const)[i], args: {} }]);
    }
  }
  // The strongest weekday, once there's a month of history.
  if (past.length >= 4) {
    const byDay = new Array(7).fill(0);
    for (const s of before) byDay[(new Date(s.start).getDay() + 6) % 7] += s.secs;
    const mean = byDay.reduce((x, y) => x + y, 0) / 7;
    const top = byDay.indexOf(Math.max(...byDay));
    if (mean > 0 && byDay[top] / mean - 1 >= 0.3) {
      scored.push([(byDay[top] / mean - 1) * 0.5, { trend: "neutral", key: "insightWeekday", args: { day: top } }]);
    }
  }
  // One "down" at most.
  let downs = 0;
  return scored
    .sort((x, y) => y[0] - x[0])
    .map(([, i]) => i)
    .filter((i) => i.trend !== "down" || ++downs === 1)
    .slice(0, 3);
}

function mockBackend(): StatsBackend {
  const params = new URLSearchParams(location.search);
  const now = new Date();
  let sessions = params.get("empty") ? [] : generate(now, !!params.get("notags"), !!params.get("quiet"));
  const deleted = new Map<number, Session>();
  let nextId = sessions.length + 1;
  const listeners: (() => void)[] = [];
  const changed = () => listeners.forEach((cb) => cb());
  const tags = params.get("notags") ? [] : MOCK_TAGS;
  const running: Session | null = params.get("running")
    ? {
        id: 0,
        start: now.getTime() - 41 * 60_000,
        end: now.getTime(),
        secs: 41 * 60,
        tag: tags.length ? 1 : null,
        noise: "brown",
        mode: "stopwatch",
        edited: 0,
        running: true,
      }
    : null;
  const all = () => (running ? [...sessions, running] : sessions);

  const week = (offset: number): WeekView => {
    const list = all();
    const start = addDays(mondayOf(now), -7 * offset);
    const from = start.getTime();
    const to = addDays(start, 7).getTime();
    const current = offset === 0;
    const days: DayBar[] = Array.from({ length: 7 }, (_, i) => {
      const a = addDays(start, i).getTime();
      const b = addDays(start, i + 1).getTime();
      return { date: isoDate(addDays(start, i)), secs: sum(list, a, b), byTag: byTagOf(list, a, b) };
    });
    const secs = sum(list, from, to);
    const prevFrom = addDays(start, -7).getTime();
    const until = current ? addDays(now, -7) : null;
    const previousSecs = sum(list, prevFrom, from, until ? until.getTime() : Infinity);
    const hasPrev = list.some((s) => s.start < from);
    const done = sessions;
    const inWeek = done.filter((s) => startedIn(s, from, to));
    const elapsed = current ? ((now.getDay() + 6) % 7) + 1 : 7;
    // ?goal=15 sets a weekly goal of 15 h.
    const goalSecs = Number(params.get("goal") ?? 0) * 3600;
    const weekSecs = (back: number) => {
      const a = addDays(mondayOf(now), -7 * back);
      return sum(list, a.getTime(), addDays(a, 7).getTime());
    };
    const trend: Dated[] = Array.from({ length: 8 }, (_, i) => {
      const back = offset + 7 - i;
      return { date: isoDate(addDays(mondayOf(now), -7 * back)), secs: weekSecs(back) };
    });
    const earlier = trend.slice(0, -1).map((w) => w.secs).filter((s) => s > 0);
    let streak = goalSecs > 0 && weekSecs(0) >= goalSecs ? 1 : 0;
    for (let back = 1; goalSecs > 0 && weekSecs(back) >= goalSecs; back++) streak++;
    let lastWithData: number | null = null;
    if (secs === 0) {
      for (let k = offset + 1; k < offset + 60; k++) {
        const a = addDays(mondayOf(now), -7 * k).getTime();
        if (sum(list, a, a + 7 * DAY) > 0) {
          lastWithData = k;
          break;
        }
      }
    }
    return {
      offset,
      start: isoDate(start),
      end: isoDate(addDays(start, 6)),
      today: current ? (now.getDay() + 6) % 7 : null,
      secs,
      comparison: hasPrev ? compare(secs, previousSecs, until) : null,
      dailyAverage: secs / elapsed,
      sessionAverage: inWeek.length ? sum(done, from, to) / inWeek.length : 0,
      sessions: inWeek.length,
      activeDays: days.filter((d) => d.secs > 0).length,
      elapsedDays: elapsed,
      days,
      byTag: byTagOf(list, from, to),
      tagPrevious: hasPrev ? byTagOf(list, prevFrom, from, until ? until.getTime() : Infinity) : [],
      trend,
      typicalSecs: earlier.length >= 3 ? earlier.reduce((x, y) => x + y, 0) / earlier.length : null,
      goal: goalSecs > 0 ? { secs: goalSecs, streak } : null,
      heat: heatOf(done, start, 8),
      records: records(done),
      insights: insightsOf(done, start, current ? now.getTime() : to, tags),
      hasHistory: list.length > 0,
      lastWithData,
    };
  };

  const month = (offset: number): MonthView => {
    const list = all();
    const start = new Date(now.getFullYear(), now.getMonth() - offset, 1);
    const next = new Date(start.getFullYear(), start.getMonth() + 1, 1);
    const days: Dated[] = [];
    for (let d = start; d < next; d = addDays(d, 1)) {
      days.push({ date: isoDate(d), secs: sum(list, d.getTime(), addDays(d, 1).getTime()) });
    }
    const secs = days.reduce((a, d) => a + d.secs, 0);
    const prevStart = new Date(start.getFullYear(), start.getMonth() - 1, 1);
    const until = offset === 0 ? new Date(now.getFullYear(), now.getMonth() - 1, Math.min(now.getDate(), new Date(now.getFullYear(), now.getMonth(), 0).getDate()), now.getHours(), now.getMinutes()) : null;
    const active = days.filter((d) => d.secs > 0);
    return {
      offset,
      start: isoDate(start),
      weekStart: 0,
      today: offset === 0 ? isoDate(now) : null,
      days,
      secs,
      comparison: list.some((s) => s.start < start.getTime()) ? compare(secs, sum(list, prevStart.getTime(), start.getTime(), until ? until.getTime() : Infinity), until) : null,
      activeDays: active.length,
      elapsedDays: offset === 0 ? now.getDate() : days.length,
      activeAverage: active.length ? secs / active.length : 0,
      bestDay: (() => {
        const finished = days.map((d) => ({ date: d.date, secs: sum(sessions, day0(d.date), day0(d.date) + DAY) })).filter((d) => d.secs > 0);
        return finished.length ? finished.reduce((a, b) => (b.secs > a.secs ? b : a)) : null;
      })(),
      byTag: byTagOf(list, start.getTime(), next.getTime()),
      hasHistory: list.length > 0,
    };
  };

  const check = (start: number, secs: number, ignore: number | null) => {
    if (secs < 60) throw { kind: "tooShort" } satisfies SessionError;
    if (start + secs * 1000 > Date.now()) throw { kind: "future" } satisfies SessionError;
    const clash = all()
      .filter((s) => s.id !== ignore && s.start < start + secs * 1000 && start < s.end)
      .sort((a, b) => a.start - b.start)[0];
    if (clash) throw { kind: "overlap", start: clash.start } satisfies SessionError;
  };

  return {
    context: async () => ({
      lang: (params.get("lang") as Lang) ?? (navigator.language.startsWith("es") ? "es" : "en"),
      color: (params.get("color") as Color) ?? "pink",
      systemAccent: null,
      mica: false,
    }),
    tags: async () => tags,
    week: async (offset) => week(offset),
    month: async (offset) => month(offset),
    sessions: async (before, count) => {
      const sorted = [...all()].sort((a, b) => b.start - a.start).filter((s) => before === null || s.start < before);
      const days: SessionDay[] = [];
      for (const s of sorted) {
        const date = isoDate(new Date(s.start));
        if (days[days.length - 1]?.date !== date) {
          if (days.length === count) break;
          days.push({ date, secs: 0, sessions: [] });
        }
        const d = days[days.length - 1];
        d.sessions.push(s);
        d.secs += s.secs;
      }
      const oldest = days[days.length - 1];
      const more = oldest && sorted.some((s) => s.start < midnight(new Date(oldest.sessions[oldest.sessions.length - 1].start)).getTime());
      return { days, next: more ? midnight(new Date(oldest.sessions[oldest.sessions.length - 1].start)).getTime() : null };
    },
    updateSession: async (id, secs, tag) => {
      const s = sessions.find((x) => x.id === id);
      if (!s) throw { kind: "notFound" } satisfies SessionError;
      check(s.start, secs, id);
      Object.assign(s, { secs, end: s.start + secs * 1000, tag, edited: s.edited === 2 ? 2 : 1 });
      changed();
    },
    deleteSession: async (id) => {
      const s = sessions.find((x) => x.id === id);
      if (!s) throw { kind: "notFound" } satisfies SessionError;
      deleted.set(id, s);
      sessions = sessions.filter((x) => x.id !== id);
      changed();
    },
    restoreSession: async (id) => {
      const s = deleted.get(id);
      if (!s) return;
      deleted.delete(id);
      sessions.push(s);
      changed();
    },
    addSession: async (start, secs, tag) => {
      check(start, secs, null);
      sessions.push({ id: nextId++, start, end: start + secs * 1000, secs, tag, noise: null, mode: "stopwatch", edited: 2, running: false });
      changed();
    },
    exportCsv: async () => true,
    ready: async () => {},
    onChanged: (cb) => listeners.push(cb),
    onContext: () => {},
  };
}

export const statsBackend: Promise<StatsBackend> = isTauri ? tauriBackend() : Promise.resolve(mockBackend());
