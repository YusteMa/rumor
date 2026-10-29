// Generates the tray icons in src-tauri/icons/tray/.
//
//   node scripts/gen-tray-icons.mjs
//
// One PNG per state × taskbar theme × size. Every size is laid out on its own
// pixel grid (integer bar widths, gaps and even heights) so the glyph stays
// crisp at 100–300 % scaling instead of being resampled by Windows.
//
// - active: four sound bars of different heights ("waves")
// - idle:   the same four bars, short and level (a flat wave), dimmed
// Ink colours are the --ink tokens of SPEC §4.5: dark ink on a light taskbar,
// light ink on a dark one.

import { mkdirSync, writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons", "tray");
const SIZES = [16, 20, 24, 32, 40, 48];
const INKS = {
  light: [0x2b, 0x25, 0x23], // for a light taskbar
  dark: [0xf2, 0xec, 0xe9], // for a dark taskbar
};
const ACTIVE_HEIGHTS = [0.44, 0.81, 0.56, 0.31];
const IDLE_HEIGHT = 0.31;
const IDLE_OPACITY = 0.6;

// --- PNG encoding -----------------------------------------------------------

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function png(size, rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // RGBA
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // no filter
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// --- Drawing ----------------------------------------------------------------

// Signed distance to a vertical pill (rounded rectangle with full caps).
function pill(px, py, x, y, w, h) {
  const r = w / 2;
  const cx = x + r;
  const top = y + r;
  const bottom = y + h - r;
  const dy = py < top ? py - top : py > bottom ? py - bottom : 0;
  return Math.hypot(px - cx, dy) - r;
}

function layout(size) {
  const w = Math.max(2, Math.round(size / 8));
  const gap = Math.max(1, Math.round(size / 10));
  const total = 4 * w + 3 * gap;
  const left = Math.floor((size - total) / 2);
  return Array.from({ length: 4 }, (_, i) => ({ x: left + i * (w + gap), w }));
}

function evenHeight(size, fraction, min) {
  let h = Math.max(min, Math.round(size * fraction));
  if ((size - h) % 2 !== 0) h += 1; // centre on whole pixels
  return h;
}

function draw(size, ink, state) {
  const bars = layout(size).map((b, i) => {
    const fraction = state === "active" ? ACTIVE_HEIGHTS[i] : IDLE_HEIGHT;
    const h = evenHeight(size, fraction, b.w);
    return { ...b, y: (size - h) / 2, h };
  });
  const opacity = state === "active" ? 1 : IDLE_OPACITY;
  const SS = 8; // supersampling per axis
  const rgba = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      let hits = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const x = px + (sx + 0.5) / SS;
          const y = py + (sy + 0.5) / SS;
          if (bars.some((b) => pill(x, y, b.x, b.y, b.w, b.h) <= 0)) hits++;
        }
      }
      const i = (py * size + px) * 4;
      rgba[i] = ink[0];
      rgba[i + 1] = ink[1];
      rgba[i + 2] = ink[2];
      rgba[i + 3] = Math.round((255 * opacity * hits) / (SS * SS));
    }
  }
  return png(size, rgba);
}

mkdirSync(OUT, { recursive: true });
for (const state of ["active", "idle"]) {
  for (const [theme, ink] of Object.entries(INKS)) {
    for (const size of SIZES) {
      writeFileSync(join(OUT, `${state}-${theme}-${size}.png`), draw(size, ink, state));
    }
  }
}
console.log(`wrote ${2 * 2 * SIZES.length} icons to ${OUT}`);
