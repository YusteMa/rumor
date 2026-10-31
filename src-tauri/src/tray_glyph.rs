//! The tray glyph (SPEC §4.1), drawn at the exact pixel size Windows asks for
//! so it is never resampled: four sound bars on whole-pixel columns.
//!
//! - idle: short level bars (a flat wave), dimmed
//! - active: the bars at different heights
//! - in between: play makes the bars rise and pause lets them settle
//!   ([`Pose::blend`]); once there, the glyph keeps still so it never
//!   draws the eye while you work
//! - dot: a dot in the lower right corner, cut out of the bars and always
//!   full ink: the study stopwatch is running, or "¿Seguías estudiando?"
//!   waits (SPEC §4.3c)
//!
//! Pure and testable: no Tauri, no Windows.

/// Bar heights of the still "active" glyph, as a fraction of the icon.
const ACTIVE: [f32; 4] = [0.44, 0.81, 0.56, 0.31];
const IDLE: f32 = 0.31;
const IDLE_OPACITY: f32 = 0.6;
const DOT_RADIUS: f32 = 0.15;
/// Clear space around the dot, in pixels.
const DOT_GAP: f32 = 1.0;

/// Where the bars are: heights as a fraction of the icon, and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    heights: [f32; 4],
    opacity: f32,
    /// Snap the heights to whole pixels (the still glyphs). Glyphs in motion
    /// keep fractional heights so they glide instead of jumping a pixel.
    crisp: bool,
}

impl Pose {
    pub fn idle() -> Self {
        Self {
            heights: [IDLE; 4],
            opacity: IDLE_OPACITY,
            crisp: true,
        }
    }

    pub fn active() -> Self {
        Self {
            heights: ACTIVE,
            opacity: 1.0,
            crisp: true,
        }
    }

    /// `from` at 0, `to` at 1, eased in and out.
    pub fn blend(from: &Self, to: &Self, k: f32) -> Self {
        let k = k.clamp(0.0, 1.0);
        if k == 0.0 {
            return *from;
        }
        if k == 1.0 {
            return *to;
        }
        let e = k * k * (3.0 - 2.0 * k);
        Self {
            heights: std::array::from_fn(|i| {
                from.heights[i] + (to.heights[i] - from.heights[i]) * e
            }),
            opacity: from.opacity + (to.opacity - from.opacity) * e,
            crisp: false,
        }
    }
}

struct Bar {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Bar columns on whole pixels, centred.
fn columns(size: u32) -> (u32, [u32; 4]) {
    let s = size as f32;
    let w = ((s / 8.0).round() as u32).max(2);
    let gap = ((s / 10.0).round() as u32).max(1);
    let left = size.saturating_sub(4 * w + 3 * gap) / 2;
    (w, std::array::from_fn(|i| left + i as u32 * (w + gap)))
}

fn bars(size: u32, pose: &Pose) -> [Bar; 4] {
    let (w, xs) = columns(size);
    let s = size as f32;
    std::array::from_fn(|i| {
        let mut h = (s * pose.heights[i]).max(w as f32);
        if pose.crisp {
            h = h.round();
            if (size as f32 - h) % 2.0 != 0.0 {
                h += 1.0; // centre on whole pixels
            }
        }
        Bar {
            x: xs[i] as f32,
            y: (s - h) / 2.0,
            w: w as f32,
            h,
        }
    })
}

/// Signed distance to a vertical pill (full round caps).
fn pill(px: f32, py: f32, b: &Bar) -> f32 {
    let r = b.w / 2.0;
    let top = b.y + r;
    let bottom = b.y + b.h - r;
    let dy = if py < top {
        py - top
    } else if py > bottom {
        py - bottom
    } else {
        0.0
    };
    (px - (b.x + r)).hypot(dy) - r
}

/// RGBA pixels, `size` × `size`, in `ink`.
pub fn draw(size: u32, ink: [u8; 3], pose: &Pose, dot: bool) -> Vec<u8> {
    const SS: u32 = 4; // supersampling per axis
    let bars = bars(size, pose);
    let s = size as f32;
    let r = (s * DOT_RADIUS).max(2.0);
    let c = s - r - (s / 32.0).max(0.5);
    let mut rgba = vec![0; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut alpha = 0.0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    let d = (x - c).hypot(y - c);
                    if dot && d <= r {
                        alpha += 1.0;
                    } else if dot && d <= r + DOT_GAP {
                        continue;
                    } else if bars.iter().any(|b| pill(x, y, b) <= 0.0) {
                        alpha += pose.opacity;
                    }
                }
            }
            let i = ((py * size + px) * 4) as usize;
            rgba[i..i + 3].copy_from_slice(&ink);
            rgba[i + 3] = (255.0 * alpha / (SS * SS) as f32).round() as u8;
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: [u8; 3] = [1, 2, 3];

    fn alpha(rgba: &[u8], size: u32, x: u32, y: u32) -> u8 {
        rgba[((y * size + x) * 4 + 3) as usize]
    }

    #[test]
    fn every_size_fits_its_bars() {
        for size in 16..=64 {
            let (w, xs) = columns(size);
            assert!(xs[3] + w <= size, "{size} px");
            assert_eq!(
                draw(size, INK, &Pose::active(), true).len(),
                (size * size * 4) as usize
            );
        }
    }

    #[test]
    fn still_glyphs_are_crisp_and_centred() {
        for size in [16, 20, 24, 32, 40, 48] {
            let rgba = draw(size, INK, &Pose::active(), false);
            for b in bars(size, &Pose::active()) {
                assert_eq!(b.y.fract(), 0.0, "{size} px");
                // Bar body: full ink; the column next to it: clear.
                let mid = (b.y + b.h / 2.0) as u32;
                assert_eq!(alpha(&rgba, size, b.x as u32, mid), 255);
                assert_eq!(alpha(&rgba, size, b.x as u32 - 1, mid), 0);
            }
        }
    }

    #[test]
    fn idle_is_level_and_dimmed() {
        let pose = Pose::idle();
        let hs: Vec<f32> = bars(16, &pose).iter().map(|b| b.h).collect();
        assert!(hs.windows(2).all(|p| p[0] == p[1]));
        let rgba = draw(16, INK, &pose, false);
        let peak = rgba.chunks(4).map(|p| p[3]).max().unwrap();
        assert_eq!(peak, (255.0 * IDLE_OPACITY).round() as u8);
    }

    #[test]
    fn the_dot_is_full_ink_with_a_clear_gap() {
        for size in [16, 32] {
            let rgba = draw(size, INK, &Pose::idle(), true);
            let s = size as f32;
            let r = (s * DOT_RADIUS).max(2.0);
            let c = (s - r - (s / 32.0).max(0.5)) as u32;
            assert_eq!(alpha(&rgba, size, c, c), 255);
        }
    }

    #[test]
    fn blending_starts_and_ends_on_the_poses() {
        let (from, to) = (Pose::idle(), Pose::active());
        assert_eq!(Pose::blend(&from, &to, 0.0), from);
        assert_eq!(Pose::blend(&from, &to, 1.0), to);
        let half = Pose::blend(&from, &to, 0.5);
        assert!(half.opacity > IDLE_OPACITY && half.opacity < 1.0);
        // Every bar rises steadily from idle to active.
        let mut last = from;
        for step in 1..=10 {
            let pose = Pose::blend(&from, &to, step as f32 / 10.0);
            for i in 0..4 {
                assert!(pose.heights[i] >= last.heights[i]);
            }
            last = pose;
        }
    }
}
