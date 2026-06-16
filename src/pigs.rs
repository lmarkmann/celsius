//! Three flying pigs: a hidden easter egg.
//!
//! Three fat pink pigs with angel wings and halos fly single file across the
//! sky, each leaving a short Nyan-Cat rainbow contrail. It appears only at
//! Kowloon Tong, Kowloon, Hong Kong, between 01:28 and 02:10 local time; any
//! other place or time renders the normal sky untouched.
//!
//! Like lightning, this lives outside the render pipeline: `overlay()` composites
//! onto a copy of the rendered sky each TUI tick. Positions are pixel-exact at
//! the 104x50 reference size and scale the flight path to the actual buffer.

use std::sync::LazyLock;

use chrono::{Timelike, Utc};

use crate::colorspace::{PixelBuffer, Rgb, lerp_oklab, oklab_to_rgb, rgb_u8_to_oklab};

const W: i32 = 24;
const H: i32 = 22;
const START_X: i32 = -W; // sprite enters fully off the left edge

const BODY: Rgb = Rgb::new(244, 158, 184);
const BELLY: Rgb = Rgb::new(212, 120, 150);
const SNOUT: Rgb = Rgb::new(252, 194, 206);
const NOSTRIL: Rgb = Rgb::new(150, 70, 95);
const EYE: Rgb = Rgb::new(38, 26, 34);
const EAR: Rgb = Rgb::new(228, 132, 162);
const TAIL: Rgb = Rgb::new(234, 142, 172);
const FOOT: Rgb = Rgb::new(198, 108, 138);
const WING: Rgb = Rgb::new(250, 250, 255);
const WING_EDGE: Rgb = Rgb::new(204, 206, 230);
const HALO: Rgb = Rgb::new(255, 216, 120);

// Wingless pig, one char per cell ('.' transparent). The back is solid (row 8)
// so a swung-away wing never leaves a hole there.
const BASE: [&str; 22] = [
    "........................",
    "........................",
    "..............ooo.......",
    ".............o...o......",
    "..............ooo.......",
    "........................",
    "........................",
    "..............rr........",
    ".......BBBBBBBrrr.......",
    "..t..BBBBBBBBBBBe.......",
    ".t.tBBBBBBBBBBBeee......",
    ".t..BBBBBBBBBBBBBSS.....",
    "..tBBBBBBBBBBBBBBSSS....",
    "...BBBBBBBBBBBBBBSSn....",
    "....BBBBBBBBBBBBBBSS....",
    "....bbbbbbbbbbbbb.......",
    ".....bbbbbbbbbbb........",
    ".......bbbbbbb..........",
    ".......f..f...f.........",
    ".......f..f...f.........",
    "........................",
    "........................",
];

fn palette(ch: char) -> Option<Rgb> {
    match ch {
        'B' => Some(BODY),
        'b' => Some(BELLY),
        'S' => Some(SNOUT),
        'n' => Some(NOSTRIL),
        'e' => Some(EYE),
        'r' => Some(EAR),
        't' => Some(TAIL),
        'f' => Some(FOOT),
        'o' => Some(HALO),
        _ => None,
    }
}

// Near wing in three poses; the tip travels high (up) -> out-left (mid) -> low
// (down) so the flap sweeps. Trailing-edge cells get the feathered tint.
const WING_UP: &[(i32, i32)] = &[
    (6, 1),
    (7, 1),
    (8, 1),
    (5, 2),
    (6, 2),
    (7, 2),
    (8, 2),
    (5, 3),
    (6, 3),
    (7, 3),
    (8, 3),
    (9, 3),
    (6, 4),
    (7, 4),
    (8, 4),
    (9, 4),
    (10, 4),
    (6, 5),
    (7, 5),
    (8, 5),
    (9, 5),
    (10, 5),
    (7, 6),
    (8, 6),
    (9, 6),
    (10, 6),
    (11, 6),
    (9, 7),
    (10, 7),
    (11, 7),
    (10, 8),
    (11, 8),
];
const WING_UP_EDGE: &[(i32, i32)] = &[(9, 2), (10, 3), (11, 4), (11, 5), (12, 6), (12, 7), (12, 8)];

const WING_MID: &[(i32, i32)] = &[
    (10, 5),
    (11, 5),
    (12, 5),
    (8, 6),
    (9, 6),
    (10, 6),
    (11, 6),
    (12, 6),
    (5, 7),
    (6, 7),
    (7, 7),
    (8, 7),
    (9, 7),
    (10, 7),
    (11, 7),
    (12, 7),
    (4, 8),
    (5, 8),
    (6, 8),
    (7, 8),
    (8, 8),
    (9, 8),
    (10, 8),
    (11, 8),
    (5, 9),
    (6, 9),
    (7, 9),
    (8, 9),
    (9, 9),
    (10, 9),
    (7, 10),
    (8, 10),
    (9, 10),
];
const WING_MID_EDGE: &[(i32, i32)] = &[(4, 8), (5, 9), (7, 10), (12, 5), (12, 6), (12, 7)];

const WING_DOWN: &[(i32, i32)] = &[
    (10, 8),
    (11, 8),
    (12, 8),
    (8, 9),
    (9, 9),
    (10, 9),
    (11, 9),
    (12, 9),
    (7, 10),
    (8, 10),
    (9, 10),
    (10, 10),
    (11, 10),
    (6, 11),
    (7, 11),
    (8, 11),
    (9, 11),
    (10, 11),
    (11, 11),
    (6, 12),
    (7, 12),
    (8, 12),
    (9, 12),
    (10, 12),
    (5, 13),
    (6, 13),
    (7, 13),
    (8, 13),
    (9, 13),
    (5, 14),
    (6, 14),
    (7, 14),
    (8, 14),
    (5, 15),
    (6, 15),
    (7, 15),
];
const WING_DOWN_EDGE: &[(i32, i32)] = &[
    (12, 8),
    (12, 9),
    (11, 10),
    (11, 11),
    (10, 12),
    (9, 13),
    (8, 14),
    (7, 15),
];

type Sprite = Vec<Option<Rgb>>;

fn build_frame(wing: &[(i32, i32)], edge: &[(i32, i32)]) -> Sprite {
    let mut spr: Sprite = vec![None; (W * H) as usize];
    for (y, row) in BASE.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if let Some(c) = palette(ch) {
                spr[y * W as usize + x] = Some(c);
            }
        }
    }
    for &(x, y) in wing {
        spr[(y * W + x) as usize] = Some(WING);
    }
    for &(x, y) in edge {
        spr[(y * W + x) as usize] = Some(WING_EDGE);
    }
    spr
}

// up, mid, down. The flap cycle (below) indexes into this.
static FRAMES: LazyLock<[Sprite; 3]> = LazyLock::new(|| {
    [
        build_frame(WING_UP, WING_UP_EDGE),
        build_frame(WING_MID, WING_MID_EDGE),
        build_frame(WING_DOWN, WING_DOWN_EDGE),
    ]
});

const RAINBOW: [Rgb; 6] = [
    Rgb::new(255, 70, 70),
    Rgb::new(255, 150, 50),
    Rgb::new(250, 225, 70),
    Rgb::new(95, 205, 75),
    Rgb::new(75, 160, 240),
    Rgb::new(150, 95, 220),
];

const SPEED: f64 = 2.0; // px per tick
const FLAP_K: i64 = 3; // ticks per wing pose
const CYCLE: [usize; 4] = [0, 1, 2, 1]; // up, mid, down, mid -> FRAMES index
const CYCLE_FRAMES: i64 = FLAP_K * 4;
const BOB_AMP: f64 = 2.0; // px; bob lifts the pig and staggers the contrail
const NUM_PIGS: i64 = 3;
const SPACING: i32 = 36; // px between pigs; paired with TRAIL_LEN
const TRAIL_LEN: i32 = 18; // px the contrail persists
const TRAIL_ATTACH: i32 = 4; // rear offset where emission starts
const TRAIL_TOP: i32 = 10; // band top relative to the bobbing pig origin

// Match Python's round() (round half to even); the sandbox is authored against
// it, so odd-distance trail columns would land 1px off with away-from-zero.
fn iround(x: f64) -> i64 {
    x.round_ties_even() as i64
}

fn pos_x(i: i64, start_x: i32) -> i32 {
    iround(start_x as f64 + SPEED * i as f64) as i32
}

fn bob_at(i: i64) -> i32 {
    let phase = std::f64::consts::TAU * (i.rem_euclid(CYCLE_FRAMES)) as f64 / CYCLE_FRAMES as f64;
    iround(-BOB_AMP * phase.cos()) as i32
}

fn wing_index(i: i64) -> usize {
    CYCLE[i.div_euclid(FLAP_K).rem_euclid(4) as usize]
}

fn lag_frames(k: i64) -> i64 {
    iround(SPACING as f64 * k as f64 / SPEED)
}

fn alpha(dx: i32) -> f64 {
    (1.0 - dx as f64 / TRAIL_LEN as f64).max(0.0)
}

fn blend(bg: Rgb, fg: Rgb, a: f64) -> Rgb {
    if a >= 1.0 {
        return fg;
    }
    if a <= 0.0 {
        return bg;
    }
    oklab_to_rgb(lerp_oklab(
        rgb_u8_to_oklab(bg.r, bg.g, bg.b),
        rgb_u8_to_oklab(fg.r, fg.g, fg.b),
        a,
    ))
}

// Long enough for the rearmost pig's whole trail to clear the right edge, so the
// loop wraps on empty sky with no rainbow popping at the edge.
fn frame_count(width: i32) -> i64 {
    let rear_reach = SPACING * (NUM_PIGS as i32 - 1) + TRAIL_LEN;
    iround(((width - START_X) + rear_reach) as f64 / SPEED) + 1
}

fn base_y(height: i32) -> i32 {
    (height - H) / 2 - 3
}

fn draw_trail(pixels: &mut PixelBuffer, t: i64, lag: i64, height: i32) {
    let w = pixels.width as i32;
    let rear_x = pos_x(t - lag, START_X) + TRAIL_ATTACH - 1;
    let by = base_y(height);
    for dx in 0..TRAIL_LEN {
        let x = rear_x - dx;
        if !(0..w).contains(&x) {
            continue;
        }
        let a = alpha(dx);
        if a <= 0.0 {
            continue;
        }
        let emitted = iround(t as f64 - dx as f64 / SPEED);
        let y_top = by + TRAIL_TOP + bob_at(emitted - lag);
        for (row, color) in RAINBOW.iter().enumerate() {
            let y = y_top + row as i32;
            if !(0..height).contains(&y) {
                continue;
            }
            let idx = (y as usize) * pixels.width + (x as usize);
            let bg = pixels.pixels[idx];
            pixels.pixels[idx] = blend(bg, *color, a);
        }
    }
}

fn paste_body(pixels: &mut PixelBuffer, frame: &Sprite, px: i32, py: i32) {
    let w = pixels.width as i32;
    let h = pixels.height as i32;
    for sy in 0..H {
        let y = py + sy;
        if !(0..h).contains(&y) {
            continue;
        }
        for sx in 0..W {
            let Some(c) = frame[(sy * W + sx) as usize] else {
                continue;
            };
            let x = px + sx;
            if !(0..w).contains(&x) {
                continue;
            }
            let idx = (y as usize) * pixels.width + (x as usize);
            pixels.pixels[idx] = c;
        }
    }
}

/// Composite the egg onto an already-rendered sky for the given tick. Trails for
/// all pigs first, then bodies on top, so each pig sits over every trail.
pub fn overlay(pixels: &mut PixelBuffer, tick: u64) {
    let w = pixels.width as i32;
    let h = pixels.height as i32;
    let fc = frame_count(w);
    let t = (tick % fc as u64) as i64;
    let by = base_y(h);
    for k in 0..NUM_PIGS {
        draw_trail(pixels, t, lag_frames(k), h);
    }
    for k in 0..NUM_PIGS {
        let lag = lag_frames(k);
        let frame = &FRAMES[wing_index(t - lag)];
        paste_body(pixels, frame, pos_x(t - lag, START_X), by + bob_at(t - lag));
    }
}

// --- trigger gate ---

const KOWLOON_TONG: (f64, f64) = (22.337, 114.176); // lat, lon
const GATE_RADIUS: f64 = 0.05; // degrees; absorbs geocoding precision
const WINDOW_START: u32 = 88; // 01:28, minutes since midnight
const WINDOW_END: u32 = 130; // 02:10

fn at_kowloon_tong(lat: f64, lon: f64) -> bool {
    (lat - KOWLOON_TONG.0).abs() <= GATE_RADIUS && (lon - KOWLOON_TONG.1).abs() <= GATE_RADIUS
}

fn in_window(local_minutes: u32) -> bool {
    (WINDOW_START..=WINDOW_END).contains(&local_minutes)
}

// Wall-clock minutes since midnight at the viewed location, from its UTC offset.
fn local_minutes(offset: i64) -> u32 {
    let local = Utc::now() + chrono::Duration::seconds(offset);
    local.hour() * 60 + local.minute()
}

/// Whether the egg should show right now for a viewer at `(lat, lon)` whose
/// local time runs `offset` seconds ahead of UTC.
pub fn gate_open(lat: f64, lon: f64, offset: i64) -> bool {
    at_kowloon_tong(lat, lon) && in_window(local_minutes(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KT: (f64, f64) = KOWLOON_TONG;
    const ELSEWHERE: (f64, f64) = (52.520, 13.405); // Berlin

    #[test]
    fn place_match_is_radius_bounded() {
        assert!(at_kowloon_tong(KT.0, KT.1));
        assert!(at_kowloon_tong(KT.0 + 0.04, KT.1 - 0.04));
        assert!(!at_kowloon_tong(ELSEWHERE.0, ELSEWHERE.1));
        assert!(!at_kowloon_tong(KT.0 + 1.0, KT.1));
    }

    #[test]
    fn window_boundaries() {
        assert!(!in_window(87)); // 01:27 just outside
        assert!(in_window(88)); // 01:28 just inside
        assert!(in_window(130)); // 02:10 just inside
        assert!(!in_window(131)); // 02:11 just outside
        assert!(!in_window(720)); // noon, well outside
    }

    #[test]
    fn frame_count_reference_size() {
        // At the 104-wide reference, the loop is 110 ticks (matches the sandbox).
        assert_eq!(frame_count(104), 110);
    }

    #[test]
    fn three_pigs_have_distinct_lags() {
        assert_eq!(lag_frames(0), 0);
        assert_eq!(lag_frames(1), 18);
        assert_eq!(lag_frames(2), 36);
    }

    #[test]
    fn overlay_changes_pixels_when_pigs_are_on_screen() {
        let sky = PixelBuffer::filled(104, 50, Rgb::new(10, 12, 28));
        let mut shown = sky.clone();
        // Tick 50: the formation is mid-pass, all three on screen.
        overlay(&mut shown, 50);
        assert_ne!(shown.pixels, sky.pixels);
    }
}
