// The Statistics charts (SPEC §4.3d), drawn as plain SVG at the container's
// real width so text stays at its true size. Colours come from CSS classes
// and the tokens; sizes are read from tokens.css.

import type { DayBar, TagId, TagTime } from "../stats-api";

const NS = "http://www.w3.org/2000/svg";
let uid = 0;

type Attrs = Record<string, string | number>;

export function svg<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: (Node | string)[]
): SVGElementTagNameMap[K] {
  const node = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  node.append(...children);
  return node;
}

/** A length token in px, e.g. `--chart-height`. */
export function px(name: string): number {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

/** The fill for a tag: its `--tag-*` token, or "Sin etiqueta". */
export function tagColor(tag: TagId, colorOf: (tag: number) => string | undefined): string {
  const color = tag === null ? undefined : colorOf(tag);
  return color ? `var(--tag-${color})` : "var(--tag-none)";
}

/** The daily bars' lowest top, in seconds (SPEC §4.3d). */
const SCALE_FLOOR = 6 * 3600;

/** Grid lines on round hours that fit the tallest value. */
function hourTicks(maxSecs: number): number[] {
  const hours = Math.max(1, Math.ceil(maxSecs / 3600));
  const step = hours <= 4 ? 1 : hours <= 8 ? 2 : hours <= 15 ? 3 : 5;
  const top = Math.ceil(hours / step) * step;
  return Array.from({ length: top / step + 1 }, (_, i) => i * step);
}

export interface BarsOptions {
  days: DayBar[];
  /** Stacking order, bottom first (the legend's). */
  order: TagId[];
  fill: (tag: TagId) => string;
  labels: string[];
  today: number | null;
  /** Empty state: outlines of a plausible week, no values. */
  ghost: boolean;
  hourLabel: (h: number) => string;
  /** The floating label's content for a day. */
  tip: (day: DayBar, index: number) => Node;
  /** What a screen reader hears for each bar. */
  describe: (day: DayBar, index: number) => string;
  /** The daily goal: a dashed line with its label. */
  goal: { secs: number; label: string } | null;
  /** The bars rise from the axis: for a view's first draw, not a resize. */
  rise: boolean;
}

const GHOST = [0.55, 0.8, 0.45, 0.95, 0.7, 0.35, 0.25];

/** Daily bars stacked by tag, with rounded tops, a 2 px gap between tags,
 *  a soft side shine, a shadow at the base and a highlight on hover. */
export function dayBars(host: HTMLElement, o: BarsOptions): void {
  const id = ++uid;
  const width = host.clientWidth;
  const height = px("--chart-height");
  const axis = px("--chart-axis");
  // Not laid out yet (hidden window): the resize observer draws it later.
  if (width <= axis) return;
  const radius = px("--chart-bar-radius");
  const gap = px("--chart-seg-gap");
  const floor = px("--chart-floor-height");
  const top = px("--space-3");
  const base = height - px("--space-6");
  const plot = base - top;

  // The same scale from week to week (at least 6 h), so bars compare while
  // paging; it only grows for a week that goes over.
  const goal = o.ghost ? null : o.goal;
  const max = o.ghost ? 3 * 3600 : Math.max(...o.days.map((d) => d.secs), SCALE_FLOOR, goal?.secs ?? 0);
  const ticks = hourTicks(max);
  const scale = plot / (ticks[ticks.length - 1] * 3600);
  const slot = (width - axis) / o.days.length;
  const barW = Math.min(px("--chart-bar-max"), slot * 0.52);

  const root = svg("svg", { width, height, viewBox: `0 0 ${width} ${height}`, class: "chart" });
  root.append(
    svg(
      "defs",
      {},
      svg(
        "linearGradient",
        { id: `shine-${id}`, x1: 0, y1: 0, x2: 1, y2: 0 },
        svg("stop", { offset: 0, class: "shine-light" }),
        svg("stop", { offset: 0.35, class: "shine-mid" }),
        svg("stop", { offset: 0.7, class: "shine-clear" }),
        svg("stop", { offset: 1, class: "shine-shade" }),
      ),
      svg(
        "radialGradient",
        { id: `floor-${id}` },
        svg("stop", { offset: 0, class: "floor-dark" }),
        svg("stop", { offset: 1, class: "floor-clear" }),
      ),
    ),
  );

  const grid = svg("g", { class: "grid", "aria-hidden": "true" });
  for (const h of ticks) {
    const y = Math.round(base - h * 3600 * scale) + 0.5;
    grid.append(svg("line", { x1: axis, x2: width, y1: y, y2: y }));
    // A ghost week has no values to label.
    if (!o.ghost) {
      grid.append(svg("text", { x: axis - px("--space-2"), y: y + 4, class: "axis-label", "text-anchor": "end" }, o.hourLabel(h)));
    }
  }
  root.append(grid);

  const bars = svg("g", { class: `bars${o.ghost ? " is-ghost" : ""}${o.rise ? " is-rising" : ""}` });
  o.days.forEach((day, i) => {
    const x = axis + slot * i + (slot - barW) / 2;
    const g = svg("g", { class: "bar", style: `--i: ${i}` });
    if (o.ghost) {
      const h = GHOST[i % GHOST.length] * plot;
      g.append(svg("rect", { x: x + 0.5, y: base - h + 0.5, width: barW - 1, height: h, rx: radius, class: "ghost" }));
      bars.append(g);
      return;
    }
    if (day.secs <= 0) {
      bars.append(g);
      return;
    }
    const parts = o.order
      .map((tag) => day.byTag.find((t) => t.tag === tag))
      .filter((t): t is TagTime => !!t && t.secs > 0);
    const h = day.secs * scale + gap * (parts.length - 1);
    const y0 = base - h;
    const clip = `bar-${id}-${i}`;
    // The clip rounds the top; it runs past the base so the bottom stays square.
    g.append(
      svg("ellipse", { cx: x + barW / 2, cy: base + floor, rx: barW / 2 + floor, ry: floor, fill: `url(#floor-${id})` }),
      svg("clipPath", { id: clip }, svg("rect", { x, y: y0, width: barW, height: h + radius, rx: radius })),
    );
    const inner = svg("g", { "clip-path": `url(#${clip})`, class: "bar-fill" });
    let y = base;
    parts.forEach((p) => {
      const sh = p.secs * scale;
      y -= sh;
      inner.append(svg("rect", { x, y, width: barW, height: sh, style: `fill: ${o.fill(p.tag)}` }));
      y -= gap;
    });
    inner.append(svg("rect", { x, y: y0, width: barW, height: h, fill: `url(#shine-${id})` }));
    g.append(inner);
    // The whole slot answers the pointer, so short bars are easy to hit.
    g.append(svg("rect", { x: axis + slot * i, y: top, width: slot, height: plot, class: "hit" }));
    g.setAttribute("tabindex", "0");
    g.setAttribute("role", "img");
    g.setAttribute("aria-label", o.describe(day, i));
    const show = () => showTip(host, o.tip(day, i), x + barW / 2, y0);
    g.addEventListener("pointerenter", show);
    g.addEventListener("focus", show);
    g.addEventListener("pointerleave", () => hideTip(host));
    g.addEventListener("blur", () => hideTip(host));
    bars.append(g);
  });
  root.append(bars);

  if (goal) {
    const y = Math.round(base - goal.secs * scale) + 0.5;
    root.append(
      svg(
        "g",
        { class: "goal-line", "aria-hidden": "true" },
        svg("line", { x1: axis, x2: width, y1: y, y2: y }),
        svg("text", { x: width, y: y - px("--space-1"), "text-anchor": "end", class: "axis-label goal-label" }, goal.label),
      ),
    );
  }

  const labels = svg("g", { "aria-hidden": "true" });
  o.labels.forEach((label, i) => {
    labels.append(
      svg(
        "text",
        {
          x: axis + slot * i + slot / 2,
          y: height - px("--space-1"),
          "text-anchor": "middle",
          class: `day-label${i === o.today ? " is-today" : ""}`,
        },
        label,
      ),
    );
  });
  root.append(labels);
  host.querySelector(".chart")?.remove();
  host.prepend(root);
}

function tipOf(host: HTMLElement): HTMLElement {
  let tip = host.querySelector<HTMLElement>(".chart-tip");
  if (!tip) {
    tip = document.createElement("div");
    tip.className = "chart-tip";
    tip.setAttribute("aria-hidden", "true");
    host.append(tip);
  }
  return tip;
}

function showTip(host: HTMLElement, content: Node, x: number, y: number): void {
  const tip = tipOf(host);
  tip.replaceChildren(content);
  tip.classList.add("is-shown");
  const half = tip.offsetWidth / 2;
  const left = Math.min(Math.max(x, half), host.clientWidth - half);
  tip.style.left = `${left}px`;
  tip.style.top = `${y}px`;
}

function hideTip(host: HTMLElement): void {
  host.querySelector(".chart-tip")?.classList.remove("is-shown");
}

/** 0 for nothing, then 1–4 relative to the busiest cell. */
export function level(value: number, max: number): number {
  if (value <= 0 || max <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((value / max) * 4)));
}

export interface HeatOptions {
  /** Rows of hour columns from `firstHour`, then one for the night. */
  rows: number[][];
  rowLabels: string[];
  firstHour: number;
  hourLabel: (h: number) => string;
  nightLabel: string;
  describe: (row: number, col: number, secs: number) => string;
}

/** Hour × weekday, five levels of the accent; the night (outside the day's
 *  hours) gets one column of its own, set a little apart. */
export function heatmap(host: HTMLElement, o: HeatOptions): void {
  const width = host.clientWidth;
  if (width <= px("--space-8")) return;
  const rowH = px("--heat-row");
  const gap = px("--heat-gap");
  const label = px("--space-5");
  const cols = o.rows[0]?.length ?? 0;
  const hours = cols - 1;
  const apart = px("--space-2");
  const cell = (width - label - apart) / cols;
  const height = o.rowLabels.length * rowH + px("--space-5");
  const max = Math.max(...o.rows.flat(), 0);
  const x = (c: number) => label + c * cell + (c === hours ? apart : 0);
  const root = svg("svg", { width, height, viewBox: `0 0 ${width} ${height}`, class: "chart heat" });
  o.rowLabels.forEach((name, r) => {
    root.append(svg("text", { x: 0, y: r * rowH + rowH / 2 + 4, class: "axis-label" }, name));
    for (let c = 0; c < cols; c++) {
      const secs = o.rows[r][c];
      const rect = svg("rect", {
        x: x(c),
        y: r * rowH,
        width: Math.max(1, cell - gap),
        height: rowH - gap,
        rx: px("--heat-radius"),
        class: `heat-${level(secs, max)}`,
      });
      // The same floating label as the bars, instead of the system tooltip.
      const text = o.describe(r, c, secs);
      rect.setAttribute("aria-label", text);
      rect.addEventListener("pointerenter", () => showTip(host, document.createTextNode(text), x(c) + (cell - gap) / 2, r * rowH));
      rect.addEventListener("pointerleave", () => hideTip(host));
      root.append(rect);
    }
  });
  const base = height - px("--space-1");
  // Every four hours, leaving room for the night's label.
  for (let c = 0; c < hours - 2; c += 4) {
    root.append(svg("text", { x: x(c), y: base, class: "axis-label" }, o.hourLabel(o.firstHour + c)));
  }
  root.append(
    svg("text", { x: x(hours) + (cell - gap) / 2, y: base, class: "axis-label", "text-anchor": "middle" }, o.nightLabel),
  );
  host.querySelector(".chart")?.remove();
  host.prepend(root);
}

export interface TrendOptions {
  /** Oldest first; the last one is the week on screen. */
  weeks: { secs: number }[];
  /** The usual week: a dashed line across, when there is one. */
  typical: number | null;
  tip: (index: number) => Node;
  describe: (index: number) => string;
  /** A click (or Enter) on an earlier week opens it. */
  onPick: (index: number) => void;
}

/** The last weeks as small bars beside the week's total: the one on screen
 *  in the accent, the others quieter, each one a way to jump to its week. */
export function weekTrend(host: HTMLElement, o: TrendOptions): void {
  const width = host.clientWidth;
  if (width <= 0 || o.weeks.length === 0) return;
  const height = px("--trend-height");
  const gap = px("--trend-gap");
  const radius = px("--trend-radius");
  const stub = px("--trend-stub");
  const slot = (width + gap) / o.weeks.length;
  const barW = Math.max(1, slot - gap);
  const max = Math.max(...o.weeks.map((w) => w.secs), o.typical ?? 0, 1);
  const last = o.weeks.length - 1;
  const root = svg("svg", { width, height, viewBox: `0 0 ${width} ${height}`, class: "chart trend-chart" });
  o.weeks.forEach((w, i) => {
    const h = w.secs > 0 ? Math.max(stub, (w.secs / max) * height) : stub;
    const x = slot * i;
    const g = svg(
      "g",
      { class: `trend-bar${i === last ? " is-current" : ""}${w.secs > 0 ? "" : " is-empty"}`, tabindex: 0, role: i === last ? "img" : "button", "aria-label": o.describe(i) },
      svg("rect", { x, y: height - h, width: barW, height: h, rx: Math.min(radius, h / 2), class: "trend-fill" }),
      svg("rect", { x: x - gap / 2, y: 0, width: slot, height, class: "hit" }),
    );
    const show = () => showTip(host, o.tip(i), x + barW / 2, height - h);
    g.addEventListener("pointerenter", show);
    g.addEventListener("focus", show);
    g.addEventListener("pointerleave", () => hideTip(host));
    g.addEventListener("blur", () => hideTip(host));
    if (i !== last) {
      g.addEventListener("click", () => o.onPick(i));
      g.addEventListener("keydown", (e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.preventDefault();
        o.onPick(i);
      });
    }
    root.append(g);
  });
  if (o.typical !== null && o.typical > 0) {
    const y = Math.round(height - (o.typical / max) * height) + 0.5;
    root.append(svg("line", { x1: 0, x2: width, y1: y, y2: y, class: "trend-typical", "aria-hidden": "true" }));
  }
  host.querySelector(".chart")?.remove();
  host.prepend(root);
}

/** The weekly goal as a ring: how far the week has got, with the figure
 *  inside. It closes at 100 % and stays closed beyond it. */
export function goalRing(fraction: number, text: string, label: string): SVGSVGElement {
  const size = px("--goal-ring");
  const stroke = px("--goal-ring-stroke");
  const mid = size / 2;
  const r = (size - stroke) / 2;
  const length = 2 * Math.PI * r;
  const done = Math.min(1, Math.max(0, fraction));
  const root = svg(
    "svg",
    { width: size, height: size, viewBox: `0 0 ${size} ${size}`, class: `goal-ring${fraction >= 1 ? " is-met" : ""}`, role: "img", "aria-label": label },
    svg("circle", { cx: mid, cy: mid, r, class: "ring-track" }),
  );
  // Nothing yet: no arc, or its round cap would show as a dot.
  if (done > 0) {
    root.append(
      svg("circle", {
        cx: mid,
        cy: mid,
        r,
        class: "ring-arc",
        transform: `rotate(-90 ${mid} ${mid})`,
        "stroke-dasharray": length,
        "stroke-dashoffset": length * (1 - done),
        style: `--ring-length: ${length}`,
      }),
    );
  }
  root.append(svg("text", { x: mid, y: mid, class: "ring-text", "text-anchor": "middle", "dominant-baseline": "central" }, text));
  return root;
}
