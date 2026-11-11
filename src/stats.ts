// The Statistics window (SPEC §4.3d): Week, Month and Sessions. Rust does
// the sums; this page draws them and sends the user's edits back.

import type { Color, Tag } from "./api";
import { applyStatic, setLang, t } from "./i18n";
import {
  HEAT_FIRST_HOUR,
  HEAT_HOURS,
  isoDate,
  statsBackend,
  type Comparison,
  type DayBar,
  type Insight,
  type MonthView,
  type Session,
  type SessionDay,
  type SessionError,
  type StatsContext,
  type Tab,
  type TagId,
  type TagTime,
  type WeekView,
} from "./stats-api";
import { applyAccent } from "./ui/accent";
import { dayBars, goalRing, heatmap, level, px, tagColor, weekTrend } from "./ui/charts";
import { dropdown } from "./ui/dropdown";

const api = await statsBackend;
const body = document.getElementById("body") as HTMLElement;
const period = document.getElementById("period") as HTMLElement;
const prev = document.getElementById("prev") as HTMLButtonElement;
const next = document.getElementById("next") as HTMLButtonElement;
const tabs = document.getElementById("tabs") as HTMLElement;
const toast = document.getElementById("toast") as HTMLElement;

type Key = Parameters<typeof t>[0];

let lang = "es";
let tags: Tag[] = [];
let tab: Tab = remembered();
const offsets: Record<Tab, number> = { week: 0, month: 0, sessions: 0 };
/** Redraws the charts at the new width. */
let onResize: (() => void) | null = null;
/** Arriving at a view (a tab, another week): its cards come in and its
 *  bars rise. A refresh of the view already on screen changes in place. */
let entering = true;
const calm = window.matchMedia("(prefers-reduced-motion: reduce)");

// ---- Helpers ---------------------------------------------------------------------

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

function remembered(): Tab {
  const param = new URLSearchParams(location.search).get("tab");
  if (param === "week" || param === "month" || param === "sessions") return param;
  try {
    const saved = localStorage.getItem("stats-tab");
    if (saved === "month" || saved === "sessions") return saved;
  } catch {
    // Storage can be unavailable; Week is the default anyway.
  }
  return "week";
}

/** "16 h 48 min", "3 h" or "52 min". */
function dur(secs: number): string {
  const total = Math.round(secs / 60);
  const h = Math.floor(total / 60);
  const m = total % 60;
  if (h === 0) return t("durMinutes", { m });
  return m === 0 ? t("durHoursOnly", { h }) : t("durHours", { h, m });
}

/** Counts a duration up to its value, on arriving at a view. */
function countUp(node: HTMLElement, secs: number): void {
  const ms = px("--count-up");
  if (!entering || calm.matches || ms <= 0 || secs < 60) return;
  const start = performance.now();
  const step = (now: number) => {
    const k = Math.min(1, (now - start) / ms);
    // Ease out, like every other movement.
    node.textContent = dur(secs * (1 - (1 - k) ** 3));
    if (k < 1 && node.isConnected) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

/** A `YYYY-MM-DD` as a local date. */
function day(iso: string): Date {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d);
}

function fmt(date: Date, options: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(lang, options).format(date).replace(/\.$/, "");
}

const short = (iso: string) => fmt(day(iso), { day: "numeric", month: "short" });
const withWeekday = (iso: string) => fmt(day(iso), { weekday: "short", day: "numeric", month: "short" });
const longDay = (iso: string) => fmt(day(iso), { weekday: "long", day: "numeric", month: "long" });
const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const clockTime = (ms: number) => fmt(new Date(ms), { hour: "2-digit", minute: "2-digit" });
const letter = (iso: string) => t("dayLetters").split(",")[day(iso).getDay()];

const tagOf = (id: number) => tags.find((g) => g.id === id);
const tagName = (tag: TagId) => (tag === null ? t("tagNone") : (tagOf(tag)?.name ?? t("tagNone")));
const fill = (tag: TagId) => (tags.length === 0 ? "var(--accent)" : tagColor(tag, (id) => tagOf(id)?.color));

function swatch(tag: TagId): HTMLElement {
  const dot = el("span", { className: "swatch" });
  dot.style.background = fill(tag);
  return dot;
}

function card(className: string, ...children: Node[]): HTMLElement {
  return el("section", { className: `card ${className}` }, ...children);
}

function cardTitle(key: Key): HTMLElement {
  return el("h2", { className: "card-title", textContent: t(key) });
}

function figure(label: string, value: string, note?: string): HTMLElement {
  return el(
    "div",
    { className: "figure" },
    el("span", { className: "label", textContent: label }),
    el("span", { className: "value", textContent: value }),
    ...(note ? [el("span", { className: "label", textContent: note })] : []),
  );
}

/** "+18 % vs semana pasada" (or "+25 min" when the previous period was too
 *  short for a percentage), green when it went up and neutral otherwise,
 *  with the point it compares up to in the current period. */
function comparison(c: Comparison | null, vs: Key, until: (d: Date) => string): HTMLElement[] {
  if (!c) return [];
  const up = c.deltaSecs > 0;
  const sign = up ? "+" : "−";
  const pill = el("span", { className: `pill${up ? " is-up" : ""}` });
  const same = c.pct === 0 || (c.pct === null && Math.abs(c.deltaSecs) < 60);
  const delta = c.pct !== null ? `${sign}${Math.abs(c.pct)} %` : `${sign}${dur(Math.abs(c.deltaSecs))}`;
  pill.textContent = same ? t("compareSame") : t(vs, { delta });
  if (!c.until) return [pill];
  const [date, time] = c.until.split("T");
  const when = new Date(`${date}T${time}`);
  return [pill, el("span", { className: "label", textContent: t("compareUntil", { when: until(when) }) })];
}

/** Text with `*bold*` parts. */
function rich(text: string): Node[] {
  return text.split(/\*([^*]+)\*/).map((part, i) => (i % 2 ? el("b", { textContent: part }) : document.createTextNode(part)));
}

function insight(i: Insight): HTMLElement {
  const args = Object.fromEntries(
    Object.entries(i.args).map(([k, v]) => [k, k === "day" ? t("weekdaysPlural").split(",")[Number(v)] : v]),
  );
  const icon = { up: "▲", neutral: "●", down: "▼" }[i.trend];
  return el(
    "li",
    { className: `insight is-${i.trend}` },
    el("span", { className: "insight-icon", textContent: icon, ariaHidden: "true" }),
    el("span", {}, ...rich(t(i.key, args))),
  );
}

/** Below this, a subject did the same as the week before. */
const DELTA_FLOOR = 5 * 60;

/** "▲ 40 min" against the week before: green up, quiet down, never red. */
function delta(secs: number, before: number, tip: string): HTMLElement[] {
  const d = secs - before;
  if (Math.abs(d) < DELTA_FLOOR) return [];
  const chip = el("span", { className: `delta${d > 0 ? " is-up" : ""}`, textContent: `${d > 0 ? "▲" : "▼"} ${dur(Math.abs(d))}` });
  chip.title = tip;
  return [chip];
}

/** Thin horizontal bars per tag (SPEC §4.3d "Por asignatura"); with
 *  `before`, each one says how it moved against the previous week. */
function tagBars(byTag: TagTime[], before?: { rows: TagTime[]; tip: string }): HTMLElement {
  const sum = byTag.reduce((a, b) => a + b.secs, 0) || 1;
  return el(
    "ul",
    { className: "tag-bars" },
    ...byTag.map((row) => {
      const pct = Math.round((row.secs / sum) * 100);
      const bar = el("span", { className: "tag-bar-fill" });
      bar.style.width = `${(row.secs / sum) * 100}%`;
      bar.style.background = fill(row.tag);
      return el(
        "li",
        {},
        el(
          "div",
          { className: "tag-bar-head" },
          el("span", { className: "tag-name" }, swatch(row.tag), tagName(row.tag)),
          el(
            "span",
            { className: "tag-side" },
            ...(before ? delta(row.secs, before.rows.find((p) => p.tag === row.tag)?.secs ?? 0, before.tip) : []),
            el("span", { className: "tag-time", textContent: `${dur(row.secs)} · ${pct} %` }),
          ),
        ),
        el("span", { className: "tag-bar" }, bar),
      );
    }),
  );
}

function emptyCard(title: Key, hint: Key, action?: HTMLElement): HTMLElement {
  return el(
    "div",
    { className: "empty" },
    el("p", { className: "empty-title", textContent: t(title) }),
    el("p", { className: "empty-hint", textContent: t(hint) }),
    ...(action ? [action] : []),
  );
}

function button(label: string, onClick: () => void, className = "btn"): HTMLButtonElement {
  const b = el("button", { className, type: "button", textContent: label });
  b.addEventListener("click", onClick);
  return b;
}

// ---- Week ------------------------------------------------------------------------

/** "2 – 8 nov", or "26 oct – 1 nov" across two months. */
function range(start: string, end: string): string {
  const sameMonth = start.slice(0, 7) === end.slice(0, 7);
  return `${sameMonth ? String(day(start).getDate()) : short(start)} – ${short(end)}`;
}

function renderWeek(w: WeekView): void {
  period.textContent = range(w.start, w.end);
  prev.disabled = !w.hasHistory;
  const empty = w.secs === 0;
  const order = w.byTag.map((b) => b.tag);

  // Bars per day, or their ghost.
  const chart = el("div", { className: "chart-host" });
  const legend = el(
    "div",
    { className: "legend" },
    ...(tags.length && !empty
      ? w.byTag.map((b) => el("span", { className: "legend-item" }, swatch(b.tag), tagName(b.tag)))
      : []),
  );
  const barsCard = card(
    "bars-card",
    el("div", { className: "card-head" }, cardTitle("statsPerDay"), legend),
    chart,
  );
  const tip = (d: DayBar) =>
    el(
      "div",
      {},
      el("div", { className: "tip-head" }, el("span", { textContent: capital(fmt(day(d.date), { weekday: "long", day: "numeric", month: "short" })) }), el("b", { textContent: dur(d.secs) })),
      ...(tags.length
        ? d.byTag.map((p) =>
            el("div", { className: "tip-row" }, el("span", { className: "tag-name" }, swatch(p.tag), tagName(p.tag)), el("span", { textContent: dur(p.secs) })),
          )
        : []),
    );
  const goal = w.goal;
  let rise = entering && !calm.matches;
  const draw = () => {
    dayBars(chart, {
      goal: goal ? { secs: goal.secs / 7, label: t("goalDaily", { d: dur(goal.secs / 7) }) } : null,
      rise,
      days: w.days,
      order,
      fill,
      labels: w.days.map((d) => letter(d.date)),
      today: w.today,
      ghost: empty,
      hourLabel: (h) => (h === 0 ? "0" : t("hourShort", { h })),
      tip,
      describe: (d) => `${capital(longDay(d.date))}: ${dur(d.secs)}`,
    });
    // Only the first draw rises; a resize just redraws.
    rise = false;
  };

  if (!w.hasHistory) {
    barsCard.append(emptyCard("emptyFirstTitle", "emptyFirstHint"));
    body.replaceChildren(barsCard);
    onResize = draw;
    draw();
    return;
  }

  const content: Node[] = [];
  const draws = [draw];
  if (!empty) {
    const big = el("span", { className: "big", textContent: dur(w.secs) });
    countUp(big, w.secs);
    const pct = goal ? Math.round((w.secs / goal.secs) * 100) : 0;
    const ring = goal
      ? [
          el(
            "div",
            { className: "goal" },
            goalRing(w.secs / goal.secs, `${pct} %`, t("goalRingLabel", { pct, goal: dur(goal.secs) })),
            el("span", { className: "label", textContent: t("goalOf", { goal: dur(goal.secs) }) }),
          ),
        ]
      : [];
    // The last weeks, once there are at least two to look at.
    const trend: HTMLElement[] = [];
    if (w.trend.filter((x) => x.secs > 0).length >= 2) {
      const host = el("div", { className: "chart-host trend-host" });
      const weekRange = (start: string) => {
        const first = day(start);
        return range(start, isoDate(new Date(first.getFullYear(), first.getMonth(), first.getDate() + 6)));
      };
      draws.push(() =>
        weekTrend(host, {
          weeks: w.trend,
          typical: w.typicalSecs,
          tip: (i) => el("div", { className: "tip-head" }, el("span", { textContent: weekRange(w.trend[i].date) }), el("b", { textContent: dur(w.trend[i].secs) })),
          describe: (i) => t("trendWeek", { range: weekRange(w.trend[i].date), d: dur(w.trend[i].secs) }),
          onPick: (i) => {
            offsets.week = w.offset + (w.trend.length - 1 - i);
            void show();
          },
        }),
      );
      trend.push(
        el(
          "div",
          { className: "trend" },
          el(
            "div",
            { className: "trend-head" },
            el("span", { className: "label", textContent: t("statsTrend", { n: w.trend.length }) }),
            ...(w.typicalSecs !== null ? [el("span", { className: "label trend-key", textContent: t("trendTypical", { d: dur(w.typicalSecs) }) })] : []),
          ),
          host,
        ),
      );
    }
    const pill = comparison(w.comparison, w.offset === 0 ? "compareLastWeek" : "comparePrevWeek", (d) =>
      `${fmt(d, { weekday: "long" })} ${clockTime(d.getTime())}`,
    );
    content.push(
      el(
        "div",
        { className: "row" },
        card(
          "total",
          el(
            "div",
            { className: "total-main" },
            el(
              "div",
              { className: "total-text" },
              el("span", { className: "label", textContent: t(w.offset === 0 ? "statsThisWeek" : "statsTotal") }),
              big,
              ...(pill.length ? [el("div", { className: "total-foot" }, ...pill)] : []),
            ),
            ...ring,
          ),
          ...trend,
        ),
        card(
          "figures",
          figure(t("statsDailyAverage"), dur(w.dailyAverage)),
          figure(t("statsSessionAverage"), dur(w.sessionAverage)),
          figure(t("statsSessions"), String(w.sessions)),
          // With a goal, the streak of weeks that met it takes this place.
          goal
            ? figure(t("statsStreak"), goal.streak === 1 ? t("streakOne") : t("streakMany", { n: goal.streak }), goal.streak > 0 ? t("streakNote") : undefined)
            : figure(t("statsActiveDays"), t("countOf", { n: w.activeDays, of: w.elapsedDays })),
        ),
      ),
    );
  }
  if (empty) {
    const jump =
      w.lastWithData !== null
        ? button(t("emptyJump"), () => {
            offsets.week = w.lastWithData ?? 0;
            void show();
          }, "btn subtle")
        : undefined;
    barsCard.append(emptyCard(w.offset === 0 ? "emptyWeekTitle" : "emptyPastWeekTitle", "emptyWeekHint", jump));
  }
  content.push(barsCard);

  if (!empty) {
    const second: Node[] = [];
    if (tags.length) {
      const before = w.comparison ? { rows: w.tagPrevious, tip: t(w.offset === 0 ? "tagDeltaNow" : "tagDelta") } : undefined;
      second.push(card("by-tag", cardTitle("statsByTag"), tagBars(w.byTag, before)));
    }
    const h = w.heat;
    if (h) {
      const heat = el("div", { className: "chart-host" });
      const weekdays = t("weekdaysPlural").split(",");
      draws.push(() =>
        heatmap(heat, {
          rows: h.rows,
          rowLabels: w.days.map((d) => letter(d.date)),
          firstHour: HEAT_FIRST_HOUR,
          hourLabel: (hour) => t("hourShort", { h: hour }),
          nightLabel: t("heatNight"),
          describe: (r, col, secs) =>
            `${capital(weekdays[(day(w.days[r].date).getDay() + 6) % 7])}, ${
              col < HEAT_HOURS ? t("slot", { from: HEAT_FIRST_HOUR + col, to: HEAT_FIRST_HOUR + col + 1 }) : t("heatNightRange")
            }: ${dur(secs)}`,
        }),
      );
      second.push(
        card(
          "heat-card",
          el("div", { className: "card-head" }, cardTitle("statsWhen"), el("span", { className: "label", textContent: t("heatWeeks", { n: h.weeks }) })),
          heat,
          ...(h.bestSlot !== null
            ? [el("p", { className: "label best" }, `${t("statsBestSlot")} `, el("b", { textContent: t("slot", { from: h.bestSlot, to: h.bestSlot + 2 }) }))]
            : []),
        ),
      );
    }
    if (second.length) content.push(el("div", { className: "row" }, ...second));
    onResize = () => draws.forEach((f) => f());
  } else {
    onResize = draw;
  }

  const r = w.records;
  const third: Node[] = [];
  if (w.insights.length && !empty) {
    third.push(card("insights", cardTitle("statsHowItWent"), el("ul", { className: "insight-list" }, ...w.insights.map(insight))));
  }
  if (r.total) {
    third.push(
      card(
        "records",
        cardTitle("statsRecords"),
        el(
          "div",
          { className: "figures" },
        ...(r.longestSession ? [figure(t("recLongest"), dur(r.longestSession.secs), capital(withWeekday(r.longestSession.date)))] : []),
        ...(r.bestDay ? [figure(t("recBestDay"), dur(r.bestDay.secs), capital(withWeekday(r.bestDay.date)))] : []),
        ...(r.bestWeek ? [figure(t("recBestWeek"), dur(r.bestWeek.secs), r.bestWeek.date === w.start && w.offset === 0 ? t("recThisWeek") : t("recWeekOf", { d: short(r.bestWeek.date) }))] : []),
          figure(t("recTotal"), dur(r.total.secs), t("recSince", { d: short(r.total.date) })),
        ),
      ),
    );
  }
  if (third.length) content.push(el("div", { className: "row" }, ...third));
  body.replaceChildren(...content);
  onResize?.();
}

// ---- Month -----------------------------------------------------------------------

function renderMonth(m: MonthView): void {
  prev.disabled = !m.hasHistory;
  period.textContent = capital(fmt(day(m.start), { month: "long", year: "numeric" }).replace(" de ", " "));
  const max = Math.max(...m.days.map((d) => d.secs), 0);
  const letters = t("dayLetters").split(",");
  // Column of the first day, counted from the week's first day.
  const first = (day(m.start).getDay() + 6 - m.weekStart) % 7;
  const head = Array.from({ length: 7 }, (_, i) => el("span", { className: "cal-head", textContent: letters[(i + 1 + m.weekStart) % 7] }));
  const cells = [
    ...Array.from({ length: first }, () => el("span", { className: "cal-blank" })),
    ...m.days.map((d) => {
      const n = day(d.date).getDate();
      const future = m.today !== null && d.date > m.today;
      const state = future ? " is-future" : d.date === m.today ? " is-today" : "";
      const cell = el("span", { className: `cal-day heat-${future ? 0 : level(d.secs, max)}${state}`, textContent: String(n) });
      cell.title = `${capital(longDay(d.date))} · ${d.secs > 0 ? dur(d.secs) : t("calNothing")}`;
      cell.tabIndex = 0;
      cell.setAttribute("aria-label", cell.title);
      return cell;
    }),
  ];
  const calendar = card(
    "calendar-card",
    el("div", { className: "calendar", role: "grid" }, ...head, ...cells),
    el(
      "div",
      { className: "scale", ariaHidden: "true" },
      el("span", { className: "label", textContent: t("scaleLess") }),
      ...[0, 1, 2, 3, 4].map((l) => el("span", { className: `scale-step heat-${l}` })),
      el("span", { className: "label", textContent: t("scaleMore") }),
    ),
  );
  onResize = null;

  if (!m.hasHistory) {
    calendar.append(emptyCard("emptyFirstTitle", "emptyFirstHint"));
    body.replaceChildren(calendar);
    return;
  }
  const pill = comparison(m.comparison, m.offset === 0 ? "compareLastMonth" : "comparePrevMonth", (d) =>
    `${short(isoDate(d))} ${clockTime(d.getTime())}`,
  );
  const side = card(
    "total month-total",
    el("span", { className: "label", textContent: t(m.offset === 0 ? "statsThisMonth" : "statsTotal") }),
    el("span", { className: "big", textContent: dur(m.secs) }),
    ...(pill.length ? [el("div", { className: "total-foot" }, ...pill)] : []),
    el(
      "div",
      { className: "figures inline" },
      figure(t("statsActiveDays"), t("countOf", { n: m.activeDays, of: m.elapsedDays })),
      figure(t("statsActiveAverage"), dur(m.activeAverage)),
      ...(m.bestDay ? [figure(t("recBestDay"), dur(m.bestDay.secs), capital(withWeekday(m.bestDay.date)))] : []),
    ),
  );
  const rows: Node[] = [el("div", { className: "row month" }, calendar, side)];
  if (m.secs === 0) calendar.append(emptyCard(m.offset === 0 ? "emptyMonthTitle" : "emptyPastMonthTitle", "emptyWeekHint"));
  if (tags.length && m.secs > 0) rows.push(card("by-tag", cardTitle("statsByTag"), tagBars(m.byTag)));
  body.replaceChildren(...rows);
}

// ---- Sessions --------------------------------------------------------------------

/** Days of sessions per page; older ones load on demand. */
const PAGE_DAYS = 14;
let shown: SessionDay[] = [];

/** "1:25" (h:mm) or "85" (minutes) to seconds; `null` if it doesn't read. */
function parseDuration(text: string): number | null {
  const t = text.trim();
  const minutes = t.match(/^(\d{1,3})$/);
  if (minutes) return Number(minutes[1]) * 60;
  const hm = t.match(/^(\d{1,2}):([0-5]\d)$/);
  return hm ? (Number(hm[1]) * 60 + Number(hm[2])) * 60 : null;
}

function hm(secs: number): string {
  const m = Math.round(secs / 60);
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}`;
}

const ERRORS: Record<SessionError["kind"], Key> = {
  overlap: "errOverlap",
  future: "errFuture",
  tooShort: "errTooShort",
  running: "errRunning",
  notFound: "errNotFound",
  unavailable: "errUnavailable",
};

/** A tag dropdown; "" is no tag, ids travel as strings. */
function tagPicker(initial: TagId) {
  let value = initial === null ? "" : String(initial);
  const picker = dropdown<string>(
    [["", t("tagNone")], ...tags.map((g): [string, string] => [String(g.id), g.name])],
    (v) => (value = v),
  );
  picker.setValue(value);
  picker.node.setAttribute("aria-label", t("sessionTag"));
  return { node: picker.node, value: (): TagId => (value ? Number(value) : null) };
}

function sessionForm(opts: { session?: Session; onDone: () => void }): HTMLElement {
  const s = opts.session;
  const error = el("p", { className: "form-error", hidden: true });
  error.setAttribute("role", "alert");
  const durInput = el("input", { className: "input narrow", value: s ? hm(s.secs) : "1:00", placeholder: t("durationFormat") });
  durInput.setAttribute("aria-label", `${t("sessionDuration")} (${t("durationHint")})`);
  const picker = tags.length ? tagPicker(s?.tag ?? null) : null;
  const now = new Date();
  const dateInput = el("input", { className: "input", type: "date", value: isoDate(now), max: isoDate(now) });
  dateInput.setAttribute("aria-label", t("sessionDate"));
  const timeInput = el("input", { className: "input", type: "time", value: "09:00" });
  timeInput.setAttribute("aria-label", t("sessionStart"));

  const save = async () => {
    const secs = parseDuration(durInput.value);
    if (secs === null) {
      error.textContent = t("errDuration");
      error.hidden = false;
      return;
    }
    const tag = picker ? picker.value() : null;
    try {
      if (s) {
        await api.updateSession(s.id, secs, tag);
      } else {
        const start = new Date(`${dateInput.value}T${timeInput.value}`).getTime();
        await api.addSession(start, secs, tag);
      }
      opts.onDone();
    } catch (e) {
      error.textContent = errorText(e as SessionError, s ? s.start : new Date(`${dateInput.value}T${timeInput.value}`).getTime());
      error.hidden = false;
    }
  };

  const fields = el(
    "div",
    { className: "form-fields" },
    ...(s
      ? [field(t("sessionWhen"), el("span", { className: "field-static", textContent: `${clockTime(s.start)} – ${clockTime(s.end)}` }))]
      : [field(t("sessionDate"), dateInput), field(t("sessionStart"), timeInput)]),
    field(t("sessionDuration"), durInput, t("durationFormat")),
    ...(picker ? [field(t("sessionTag"), picker.node)] : []),
  );
  const form = el(
    "form",
    { className: `session-form${s ? "" : " is-new"}` },
    fields,
    el("div", { className: "form-actions" }, button(t("btnCancel"), opts.onDone), button(t("btnSave"), () => void save(), "btn primary")),
    error,
  );
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    void save();
  });
  return form;
}

/** A label over its control; `format` says how to write it ("h:mm"). */
function field(label: string, control: HTMLElement, format?: string): HTMLElement {
  const head = el("span", { className: "label", textContent: label });
  if (format) head.append(" ", el("span", { className: "field-format", textContent: format }));
  return el("label", { className: "field" }, head, control);
}

/** The message for a refused change; an overlap names the session it runs
 *  into ("Se solapa con la sesión de las 18:20"), with its day if it is
 *  another one. */
function errorText(e: SessionError, near: number): string {
  if (e?.kind === "overlap") {
    const sameDay = isoDate(new Date(e.start)) === isoDate(new Date(near));
    const time = clockTime(e.start);
    return t("errOverlap", { when: sameDay ? time : `${withWeekday(isoDate(new Date(e.start)))}, ${time}` });
  }
  return t(ERRORS[e?.kind] ?? "errDuration");
}

const PENCIL =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10.5 3 13 5.5 6 12.5H3.5V10Z" /></svg>';
const TRASH =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 4.5h9M6.5 4.5V3h3v1.5M5 4.5l.5 8h5l.5-8" /></svg>';

function iconButton(svgText: string, label: string, onClick: () => void): HTMLButtonElement {
  const b = el("button", { className: "icon-button", type: "button", title: label });
  b.setAttribute("aria-label", label);
  b.insertAdjacentHTML("beforeend", svgText);
  b.addEventListener("click", onClick);
  return b;
}

let undoTimer = 0;

function offerUndo(id: number): void {
  window.clearTimeout(undoTimer);
  toast.replaceChildren(
    el("span", { textContent: t("sessionDeleted") }),
    button(t("btnUndo"), () => {
      toast.hidden = true;
      void api.restoreSession(id);
    }, "link"),
  );
  toast.hidden = false;
  undoTimer = window.setTimeout(() => (toast.hidden = true), 6000);
}

function sessionRow(s: Session): HTMLElement {
  const marks: string[] = [];
  if (s.running) marks.push(t("sessionRunning"));
  if (s.edited === 1) marks.push(t("sessionEdited"));
  if (s.edited === 2) marks.push(t("sessionAdded"));
  const row = el(
    "li",
    { className: `session${s.running ? " is-running" : ""}` },
    el("span", { className: "session-time", textContent: `${clockTime(s.start)} – ${s.running ? t("sessionNow") : clockTime(s.end)}` }),
    el("span", { className: "session-dur", textContent: dur(s.secs) }),
    el("span", { className: "session-tag" }, ...(tags.length ? [swatch(s.tag), tagName(s.tag)] : [])),
    el(
      "span",
      { className: "session-meta" },
      ...(s.noise ? [el("span", { className: `noise-dot is-${s.noise}`, title: t(noiseKey(s.noise)) }), t(noiseKey(s.noise))] : []),
      ...(marks.length ? [el("span", { className: "mark", textContent: marks.join(" · ") })] : []),
    ),
  );
  // The session in progress belongs to the stopwatch until it ends.
  const actions = el("span", { className: "session-actions" });
  if (!s.running) {
    actions.append(
      iconButton(PENCIL, t("sessionEdit"), () => {
        row.replaceWith(sessionForm({ session: s, onDone: () => void show() }));
      }),
      iconButton(TRASH, t("sessionDelete"), () => {
        void api.deleteSession(s.id).then(() => offerUndo(s.id));
      }),
    );
  }
  row.append(actions);
  return row;
}

function noiseKey(c: Color): Key {
  return ({ pink: "colorPink", brown: "colorBrown", white: "colorWhite" } as const)[c];
}

function renderSessions(list: SessionDay[], next: number | null): void {
  period.textContent = "";
  onResize = null;
  const toolbar = el(
    "div",
    { className: "toolbar" },
    button(`+ ${t("sessionAdd")}`, () => {
      const open = body.querySelector<HTMLElement>(".session-form.is-new");
      if (open) open.querySelector<HTMLElement>("input")?.focus();
      else toolbar.after(sessionForm({ onDone: () => void show() }));
    }, "btn primary"),
    button(t("exportCsv"), () => void api.exportCsv().then((ok) => ok && say(t("exportCsvOk")))),
  );
  if (list.length === 0) {
    body.replaceChildren(toolbar, card("", emptyCard("emptyFirstTitle", "emptySessionsHint")));
    return;
  }
  const today = isoDate(new Date());
  const days = list.map((d) =>
    el(
      "section",
      { className: "day" },
      el(
        "h2",
        { className: "day-head" },
        el("span", { textContent: d.date === today ? t("today") : capital(longDay(d.date)) }),
        el("span", { className: "label", textContent: dur(d.secs) }),
      ),
      el("ul", { className: "session-list card" }, ...d.sessions.map(sessionRow)),
    ),
  );
  const loadMore =
    next !== null
      ? [button(t("loadMore"), () => void api.sessions(next, PAGE_DAYS).then((page) => {
          shown = [...shown, ...page.days];
          renderSessions(shown, page.next);
        }), "btn subtle more")]
      : [];
  body.replaceChildren(toolbar, ...days, ...loadMore);
}

function say(text: string): void {
  window.clearTimeout(undoTimer);
  toast.replaceChildren(el("span", { textContent: text }));
  toast.hidden = false;
  undoTimer = window.setTimeout(() => (toast.hidden = true), 3000);
}

// ---- Tabs and periods ------------------------------------------------------------

async function show(arriving = true): Promise<void> {
  entering = arriving;
  body.classList.toggle("is-entering", arriving);
  for (const b of tabs.querySelectorAll<HTMLButtonElement>("[role=tab]")) {
    const on = b.dataset.tab === tab;
    b.setAttribute("aria-selected", String(on));
    b.tabIndex = on ? 0 : -1;
  }
  const paged = tab !== "sessions";
  prev.hidden = next.hidden = !paged;
  next.disabled = offsets[tab] === 0;
  prev.setAttribute("aria-label", t(tab === "month" ? "prevMonth" : "prevWeek"));
  next.setAttribute("aria-label", t(tab === "month" ? "nextMonth" : "nextWeek"));
  if (tab === "week") renderWeek(await api.week(offsets.week));
  else if (tab === "month") renderMonth(await api.month(offsets.month));
  else {
    const page = await api.sessions(null, PAGE_DAYS);
    shown = page.days;
    renderSessions(shown, page.next);
  }
}

tabs.addEventListener("click", (e) => {
  const b = (e.target as Element).closest<HTMLButtonElement>("[role=tab]");
  if (!b?.dataset.tab) return;
  tab = b.dataset.tab as Tab;
  try {
    localStorage.setItem("stats-tab", tab);
  } catch {
    // Not remembered this time; nothing else depends on it.
  }
  void show();
});
tabs.addEventListener("keydown", (e) => {
  const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
  if (!step) return;
  const all = [...tabs.querySelectorAll<HTMLButtonElement>("[role=tab]")];
  const i = all.findIndex((b) => b.dataset.tab === tab);
  const target = all[(i + step + all.length) % all.length];
  target.focus();
  target.click();
});
prev.addEventListener("click", () => {
  offsets[tab] += 1;
  void show();
});
next.addEventListener("click", () => {
  offsets[tab] = Math.max(0, offsets[tab] - 1);
  void show();
});
new ResizeObserver(() => onResize?.()).observe(body);
// The history changed under the view on screen: no entrance, just the new numbers.
api.onChanged(() => void show(false));

/** Language, accent and Mica, as in the rest of Rumor. */
function applyContext(cx: StatsContext): void {
  const relabel = cx.lang !== lang;
  lang = cx.lang;
  setLang(cx.lang);
  document.documentElement.dataset.noise = cx.color;
  document.documentElement.toggleAttribute("data-mica", cx.mica);
  applyAccent(cx.systemAccent);
  if (relabel) {
    applyStatic();
    void show();
  }
}

api.onContext(applyContext);
const context = await api.context();
lang = context.lang;
setLang(context.lang);
applyStatic();
applyContext(context);
tags = await api.tags();
await show();
void api.ready();
