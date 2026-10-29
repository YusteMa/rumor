// Live spectrum (SPEC §4.2): the measured curve, normalised to 0 dB at 1 kHz
// so it shows the colour's shape rather than the volume, over the dashed
// theoretical slope. On a colour change the curve morphs instead of jumping
// (SPEC §4.5); while paused only the theoretical slope remains.

import { BANDS, F_MAX, F_MIN, bandCentres, type Color } from "../api";

export const SLOPES: Record<Color, number> = { pink: -3, brown: -6, white: 0 };

const REF_BANDS = bandCentres.flatMap((f, i) => (f >= 800 && f <= 1250 ? [i] : []));
/** Per-frame approach to the target: ~150 ms at 60 fps. */
const MORPH = 0.16;
const FADE = 0.08;
/** No data for this long means the stream stopped. */
const STALE_MS = 300;

const xOf = (f: number) => Math.log(f / F_MIN) / Math.log(F_MAX / F_MIN);

interface Palette {
  accent: string;
  soft: string;
  muted: string;
  line: string;
  top: number;
  bottom: number;
  lineWidth: number;
  dash: number;
}

export class Spectrum {
  private readonly ctx: CanvasRenderingContext2D;
  private readonly shown = new Float32Array(BANDS);
  private readonly target = new Float32Array(BANDS);
  private seeded = false;
  private alpha = 0;
  private lastData = 0;
  private playing = false;
  private slope = SLOPES.pink;
  private targetSlope = SLOPES.pink;
  private raf = 0;
  private palette: Palette;
  private readonly reduced = matchMedia("(prefers-reduced-motion: reduce)");

  constructor(private readonly canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no 2d canvas");
    this.ctx = ctx;
    this.palette = this.readPalette();
    matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      this.palette = this.readPalette();
    });
    canvas.parentElement?.querySelectorAll<HTMLElement>("[data-freq]").forEach((label) => {
      label.style.setProperty("--at", String(xOf(Number(label.dataset.freq))));
    });
  }

  private readPalette(): Palette {
    const css = getComputedStyle(document.documentElement);
    const v = (name: string) => css.getPropertyValue(name).trim();
    return {
      accent: v("--accent"),
      soft: v("--accent-soft"),
      muted: v("--muted"),
      line: v("--line"),
      top: Number(v("--spectrum-db-top")) || 36,
      bottom: Number(v("--spectrum-db-bottom")) || -30,
      lineWidth: parseFloat(v("--spectrum-line")) || 1.5,
      dash: parseFloat(v("--spectrum-dash")) || 3,
    };
  }

  /** Re-reads the colours (after a theme or Mica change). */
  refreshPalette(): void {
    this.palette = this.readPalette();
  }

  setColor(color: Color): void {
    this.targetSlope = SLOPES[color];
    if (this.reduced.matches) this.slope = this.targetSlope;
  }

  setPlaying(playing: boolean): void {
    this.playing = playing;
  }

  push(bands: number[]): void {
    const ref = REF_BANDS.reduce((sum, i) => sum + bands[i], 0) / REF_BANDS.length;
    for (let i = 0; i < BANDS; i++) this.target[i] = bands[i] - ref;
    if (!this.seeded || this.reduced.matches) {
      this.shown.set(this.target);
      this.seeded = true;
    }
    this.lastData = performance.now();
  }

  start(): void {
    if (!this.raf) this.raf = requestAnimationFrame(this.frame);
  }

  stop(): void {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  private readonly frame = (): void => {
    const live = this.playing && performance.now() - this.lastData < STALE_MS;
    const goal = live ? 1 : 0;
    this.alpha += Math.sign(goal - this.alpha) * Math.min(FADE, Math.abs(goal - this.alpha));
    if (this.alpha === 0) this.seeded = false;
    for (let i = 0; i < BANDS; i++) this.shown[i] += (this.target[i] - this.shown[i]) * MORPH;
    this.slope += (this.targetSlope - this.slope) * MORPH;
    this.draw();
    this.raf = requestAnimationFrame(this.frame);
  };

  private draw(): void {
    const { canvas, ctx, palette: p } = this;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    const y = (db: number) => ((p.top - Math.max(p.bottom, Math.min(p.top, db))) / (p.top - p.bottom)) * h;

    // Decade grid.
    ctx.strokeStyle = p.line;
    ctx.lineWidth = 1;
    for (const f of [100, 1000, 10_000]) {
      const x = Math.round(xOf(f) * w) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
    }

    // Theoretical slope, always visible.
    ctx.save();
    ctx.setLineDash([p.dash, p.dash]);
    ctx.strokeStyle = p.muted;
    ctx.lineWidth = p.lineWidth;
    ctx.globalAlpha = 0.5 + 0.5 * (1 - this.alpha);
    ctx.beginPath();
    ctx.moveTo(0, y(this.slope * Math.log2(F_MIN / 1000)));
    ctx.lineTo(w, y(this.slope * Math.log2(F_MAX / 1000)));
    ctx.stroke();
    ctx.restore();

    if (this.alpha <= 0) return;

    // Measured curve: smooth path through the band centres.
    const pts = bandCentres.map((f, i) => [xOf(f) * w, y(this.shown[i])] as const);
    const trace = () => {
      ctx.moveTo(0, pts[0][1]);
      ctx.lineTo(pts[0][0], pts[0][1]);
      for (let i = 1; i < pts.length; i++) {
        const [x0, y0] = pts[i - 1];
        const [x1, y1] = pts[i];
        ctx.quadraticCurveTo(x0, y0, (x0 + x1) / 2, (y0 + y1) / 2);
      }
      const last = pts[pts.length - 1];
      ctx.lineTo(w, last[1]);
    };

    ctx.save();
    ctx.globalAlpha = this.alpha;
    ctx.beginPath();
    trace();
    ctx.lineTo(w, h);
    ctx.lineTo(0, h);
    ctx.closePath();
    ctx.fillStyle = p.soft;
    ctx.fill();

    ctx.beginPath();
    trace();
    ctx.strokeStyle = p.accent;
    ctx.lineWidth = p.lineWidth;
    ctx.lineJoin = "round";
    ctx.stroke();
    ctx.restore();
  }
}
