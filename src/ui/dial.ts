// The sphere (SPEC §4.3b): the study card's clock and, small, the mini
// clock's button. A ring on the accent's soft tint round a raised face; on
// the ring, the Pomodoro phase as an arc with a knob at its tip, or the
// stopwatch's short trail going round once a minute. The knob glides from
// one second to the next (`--dur-tick`); every size comes from tokens.css.

const NS = "http://www.w3.org/2000/svg";

export interface DialView {
  /** Pomodoro phase done, 0–1; `null` without a phase. */
  progress: number | null;
  /** Stopwatch: seconds worked, for the trail; `null` without one. */
  trail: number | null;
  look: "running" | "paused" | "break" | "idle";
}

/** Token names for each size of sphere. */
const SIZES = {
  panel: { size: "--dial-size", stroke: "--dial-stroke", knob: "--dial-knob", ring: "--dial-knob-ring", gap: "--dial-face-gap" },
  mini: { size: "--mini-dial", stroke: "--mini-dial-stroke", knob: "--mini-knob", ring: "--mini-knob-ring", gap: "--mini-face-gap" },
} as const;

function token(name: string): number {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const node = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  return node;
}

/** Turns a group without ever spinning backwards: a smaller angle (a new
 *  phase or session) jumps there instead of gliding. */
function turner(group: SVGGElement) {
  let last = 0;
  return (deg: number) => {
    if (deg < last) {
      group.style.transition = "none";
      group.style.transform = `rotate(${deg}deg)`;
      void group.getBoundingClientRect();
      group.style.transition = "";
    } else {
      group.style.transform = `rotate(${deg}deg)`;
    }
    last = deg;
  };
}

export function dial(host: HTMLElement, kind: keyof typeof SIZES) {
  const t = SIZES[kind];
  const size = token(t.size);
  const stroke = token(t.stroke);
  const knob = token(t.knob);
  const c = size / 2;
  const r = c - stroke / 2 - 1;
  const face = r - stroke / 2 - token(t.gap);
  const tail = token("--dial-tail");

  const root = svg("svg", { class: "dial-svg", viewBox: `0 0 ${size} ${size}`, width: size, height: size, "aria-hidden": "true" });
  const ring = (cls: string, extra: Record<string, string | number> = {}) =>
    svg("circle", { class: cls, cx: c, cy: c, r, "stroke-width": stroke, pathLength: 100, ...extra });
  const tip = () => svg("circle", { class: "dial-knob", cx: c + r, cy: c, r: knob, "stroke-width": token(t.ring) });

  // From the top, clockwise: the ring is drawn turned a quarter back.
  const rotor = svg("g", { class: "dial-marks", transform: `rotate(-90 ${c} ${c})` });
  const arc = ring("dial-arc", { "stroke-dasharray": "100 100", "stroke-dashoffset": 100 });
  const hand = svg("g", { class: "dial-hand" });
  hand.append(tip());
  const trail = svg("g", { class: "dial-hand dial-trail" });
  trail.append(ring("dial-tail", { "stroke-dasharray": `${tail} ${100 - tail}`, "stroke-dashoffset": tail }), tip());
  rotor.append(arc, hand, trail);
  root.append(ring("dial-track"), svg("circle", { class: "dial-face", cx: c, cy: c, r: face }), rotor);
  host.prepend(root);

  const turnHand = turner(hand);
  const turnTrail = turner(trail);
  let lastProgress = 0;

  return {
    render(v: DialView) {
      host.dataset.look = v.look;
      const p = v.progress;
      hand.style.visibility = arc.style.visibility = p === null ? "hidden" : "";
      trail.style.visibility = v.trail === null ? "hidden" : "";
      if (p !== null) {
        // As a style, so it transitions with the knob.
        if (p < lastProgress) {
          arc.style.transition = "none";
          arc.style.strokeDashoffset = String(100 - p * 100);
          void arc.getBoundingClientRect();
          arc.style.transition = "";
        } else {
          arc.style.strokeDashoffset = String(100 - p * 100);
        }
        lastProgress = p;
        turnHand(p * 360);
      }
      if (v.trail !== null) turnTrail(v.trail * 6);
    },
  };
}
