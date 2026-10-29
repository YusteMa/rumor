// Settings → Appearance → "system accent": Windows' accent colour, nudged in
// lightness until it has the same contrast as Rumor's own accents (≥ 3.5:1
// on the light background, ≥ 7:1 on the dark one), with a matching soft tint.

const BG = { light: "#faf7f5", dark: "#1c1a19" };
const TARGET = { light: 3.5, dark: 7 };
/** How much of the accent goes into the soft tint over the background. */
const SOFT_MIX = { light: 0.16, dark: 0.22 };

type Rgb = [number, number, number];

const parse = (hex: string): Rgb =>
  [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255) as Rgb;
const toHex = (c: Rgb) =>
  "#" + c.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, "0")).join("");

function luminance([r, g, b]: Rgb): number {
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

const contrast = (a: Rgb, b: Rgb) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

/** Mixes towards black (t < 0) or white (t > 0). */
const shade = (c: Rgb, t: number): Rgb =>
  c.map((v) => (t < 0 ? v * (1 + t) : v + (1 - v) * t)) as Rgb;

const mix = (a: Rgb, b: Rgb, t: number): Rgb => a.map((v, i) => v * t + b[i] * (1 - t)) as Rgb;

export function adaptAccent(hex: string, theme: "light" | "dark"): { accent: string; soft: string } {
  const bg = parse(BG[theme]);
  let c = parse(hex);
  // Darken on light, lighten on dark, in small steps until it reads.
  for (let i = 0; i < 40 && contrast(c, bg) < TARGET[theme]; i++) {
    c = shade(c, theme === "light" ? -0.04 : 0.04);
  }
  return { accent: toHex(c), soft: toHex(mix(c, bg, SOFT_MIX[theme])) };
}

const dark = matchMedia("(prefers-color-scheme: dark)");
let current: string | null = null;

function paint(): void {
  const root = document.documentElement.style;
  if (!current) {
    root.removeProperty("--accent");
    root.removeProperty("--accent-soft");
    return;
  }
  const { accent, soft } = adaptAccent(current, dark.matches ? "dark" : "light");
  root.setProperty("--accent", accent);
  root.setProperty("--accent-soft", soft);
}

dark.addEventListener("change", paint);

/** `null` goes back to the accent that follows the noise. */
export function applyAccent(systemHex: string | null): void {
  if (systemHex === current) return;
  current = systemHex;
  paint();
}
