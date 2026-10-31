// Generates the app icon in src-tauri/icons/ (installer, Start menu, .exe).
//
//   node scripts/gen-app-icon.mjs
//
// A warm dark tile with three sound bars, one per noise and in its accent
// (SPEC §4.5): pink, brown and white, with a 1 px rim. Every size is laid out on its own
// pixel grid (integer bar widths and gaps, heights centred on whole pixels) so
// the small sizes stay crisp instead of being scaled down from a big one.
// icon.ico carries every size Windows asks for, as PNG entries.

import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256];
const PNGS = { "32x32.png": 32, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512 };

// Dark-theme tokens of SPEC §4.5: --bg, a rim just above --line and the three
// accents.
const TILE = [0x1c, 0x1a, 0x19];
const RIM = [0x3a, 0x34, 0x32];
const BARS = [
  [0xe8, 0x89, 0x9f],
  [0xe5, 0xa0, 0x5e],
  [0xaf, 0xba, 0xcb],
];
// As a fraction of the bar area: the middle one tallest, so the glyph sits
// centred.
const HEIGHTS = [0.6, 1, 0.72];
const CORNER = 0.225; // of the tile
// Of the tile, for the tallest bar; taller at the small sizes, where the short
// bars would otherwise shrink to dots.
const BAR_AREA = 0.6;
const BAR_AREA_SMALL = 0.72;
const BAR_WIDTH = 0.15;
const BAR_GAP = 0.09;

// --- PNG and ICO encoding -----------------------------------------------------

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

function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2); // icon
  header.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, data }) => {
    const e = Buffer.alloc(16);
    e[0] = size >= 256 ? 0 : size;
    e[1] = size >= 256 ? 0 : size;
    e.writeUInt16LE(1, 4); // planes
    e.writeUInt16LE(32, 6); // bits per pixel
    e.writeUInt32LE(data.length, 8);
    e.writeUInt32LE(offset, 12);
    offset += data.length;
    return e;
  });
  return Buffer.concat([header, ...entries, ...images.map((i) => i.data)]);
}

// --- Drawing ----------------------------------------------------------------

// Signed distance to a rounded square.
function roundedSquare(px, py, x, y, s, r) {
  const qx = Math.abs(px - (x + s / 2)) - (s / 2 - r);
  const qy = Math.abs(py - (y + s / 2)) - (s / 2 - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
}

// Signed distance to a vertical pill (full round caps).
function pill(px, py, x, y, w, h) {
  const r = w / 2;
  const top = y + r;
  const bottom = y + h - r;
  const dy = py < top ? py - top : py > bottom ? py - bottom : 0;
  return Math.hypot(px - (x + r), dy) - r;
}

function layout(size) {
  const margin = Math.floor(size / 16);
  const tile = size - 2 * margin;
  let w = Math.max(2, Math.round(tile * BAR_WIDTH));
  const gap = Math.max(1, Math.round(tile * BAR_GAP));
  // An even leftover splits evenly: the bars sit exactly in the middle.
  if ((tile - 3 * w - 2 * gap) % 2 !== 0) w += 1;
  const left = margin + (tile - 3 * w - 2 * gap) / 2;
  const area = tile * (size <= 24 ? BAR_AREA_SMALL : BAR_AREA);
  const bars = HEIGHTS.map((f, i) => {
    let h = Math.max(w, Math.round(area * f));
    if ((size - h) % 2 !== 0) h += 1; // centre on whole pixels
    return { x: left + i * (w + gap), y: (size - h) / 2, w, h, color: BARS[i] };
  });
  return { margin, tile, radius: tile * CORNER, bars };
}

function draw(size) {
  const { margin, tile, radius, bars } = layout(size);
  // A 1 px rim keeps the tile from melting into dark backgrounds; 2 px on the
  // 512 px master, which is only ever shown at half size or less.
  const rim = Math.max(1, size / 256);
  const SS = 8; // supersampling per axis
  const rgba = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      const sum = [0, 0, 0, 0];
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const x = px + (sx + 0.5) / SS;
          const y = py + (sy + 0.5) / SS;
          const d = roundedSquare(x, y, margin, margin, tile, radius);
          if (d > 0) continue;
          const bar = bars.find((b) => pill(x, y, b.x, b.y, b.w, b.h) <= 0);
          const c = bar ? bar.color : d > -rim ? RIM : TILE;
          sum[0] += c[0];
          sum[1] += c[1];
          sum[2] += c[2];
          sum[3] += 1;
        }
      }
      const i = (py * size + px) * 4;
      if (sum[3] > 0) {
        rgba[i] = Math.round(sum[0] / sum[3]);
        rgba[i + 1] = Math.round(sum[1] / sum[3]);
        rgba[i + 2] = Math.round(sum[2] / sum[3]);
      }
      rgba[i + 3] = Math.round((255 * sum[3]) / (SS * SS));
    }
  }
  return png(size, rgba);
}

for (const [name, size] of Object.entries(PNGS)) writeFileSync(join(OUT, name), draw(size));
writeFileSync(join(OUT, "icon.ico"), ico(ICO_SIZES.map((size) => ({ size, data: draw(size) }))));
console.log(`wrote ${Object.keys(PNGS).length} PNGs and icon.ico (${ICO_SIZES.join(", ")} px) to ${OUT}`);
