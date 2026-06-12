//! Lightning flashes for thunderstorm scenes (WMO 95-99).
//!
//! Lives outside the render pipeline: composed once when SkyState is built,
//! evaluated each TUI tick by `overlay()`. Bit-parity with celsius-lab's
//! lightning.py for strike scheduling (verified by tests/lightning.rs).

use std::sync::LazyLock;

use crate::colorspace::{Oklab, PixelBuffer, Rgb, oklab_to_rgb, rgb_u8_to_oklab};
use crate::noise::Mt19937;

#[derive(Clone, Debug)]
pub struct FlashParams {
    pub peak: f64,
    pub tau: f64,
    pub sub_count_min: u32,
    pub sub_count_max: u32,
    pub sub_gap_min: f64,
    pub sub_gap_max: f64,
    pub rate: f64,
}

impl Default for FlashParams {
    fn default() -> Self {
        Self {
            peak: 0.30,
            tau: 0.035,
            sub_count_min: 2,
            sub_count_max: 3,
            sub_gap_min: 0.030,
            sub_gap_max: 0.060,
            rate: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubFlash {
    pub t_peak: f64,
}

#[derive(Clone, Debug)]
pub struct Bolt {
    pub points: Vec<(i32, i32)>,
    pub forks: Vec<Bolt>,
}

#[derive(Clone, Debug)]
pub struct Strike {
    pub t_start: f64,
    pub sub_flashes: Vec<SubFlash>,
    pub bolt: Option<Bolt>,
}

#[derive(Clone, Debug)]
pub struct Lightning {
    pub params: FlashParams,
    pub strikes: Vec<Strike>,
}

impl Lightning {
    pub fn new(
        seed: u32,
        intensity: f64,
        duration_s: f64,
        with_bolts: bool,
        width: u32,
        height: u32,
    ) -> Self {
        let params = FlashParams::default();
        let mut strikes = schedule_strikes(seed, duration_s, &params, intensity);
        if with_bolts {
            attach_bolts(&mut strikes, seed, width, height);
        }
        Self { params, strikes }
    }
}

pub fn schedule_strikes(
    seed: u32,
    duration_s: f64,
    params: &FlashParams,
    intensity: f64,
) -> Vec<Strike> {
    let mut rng = Mt19937::init_by_array(&[seed]);
    let rate = params.rate * (0.4 + 1.5 * intensity);
    let mut strikes = Vec::new();
    let mut t = expovariate(&mut rng, rate);
    while t < duration_s {
        let n_sub = rng.randint(params.sub_count_min as i32, params.sub_count_max as i32) as usize;
        let mut sub_flashes = Vec::with_capacity(n_sub);
        sub_flashes.push(SubFlash { t_peak: t });
        let mut cursor = t;
        for _ in 0..n_sub - 1 {
            cursor += uniform(&mut rng, params.sub_gap_min, params.sub_gap_max);
            sub_flashes.push(SubFlash { t_peak: cursor });
        }
        strikes.push(Strike {
            t_start: t,
            sub_flashes,
            bolt: None,
        });
        t += expovariate(&mut rng, rate);
    }
    strikes
}

pub fn attach_bolts(strikes: &mut [Strike], seed: u32, width: u32, height: u32) {
    let mut rng = Mt19937::init_by_array(&[seed ^ 0x80B017]);
    for (i, s) in strikes.iter_mut().enumerate() {
        let bolt_seed = rng.next_u32() ^ (i as u32);
        s.bolt = Some(build_bolt(bolt_seed, width, height));
    }
}

fn build_bolt(seed: u32, width: u32, height: u32) -> Bolt {
    let mut rng = Mt19937::init_by_array(&[seed]);
    let w = width as i32;
    let h = height as i32;

    let x0 = rng.randint((w as f64 * 0.15) as i32, (w as f64 * 0.85) as i32);
    let y0 = rng.randint((h as f64 * 0.20) as i32, (h as f64 * 0.45) as i32);
    let drift = (w as f64 * 0.18) as i32;
    let x1 = (x0 + rng.randint(-drift, drift)).clamp(0, w - 1);
    let y1 = h - 1 - rng.randint(0, 3);

    let amplitude = width as f64 * 0.10;
    let points = midpoint_displace(&mut rng, vec![(x0, y0), (x1, y1)], 5, amplitude);

    let mut forks: Vec<Bolt> = Vec::new();
    let n_segments = points.len().saturating_sub(1);
    if n_segments >= 2 {
        for i in 1..n_segments - 1 {
            if rng.next_f64() < 0.30 {
                let (ax, ay) = points[i];
                let (bx, by) = points[i + 1];
                let dx = (bx - ax) as f64;
                let dy = (by - ay) as f64;
                let seg_len = (dx * dx + dy * dy).sqrt();
                if seg_len < 1.5 {
                    continue;
                }
                let angle = dy.atan2(dx) + uniform(&mut rng, -0.7, 0.7);
                let flen = seg_len * uniform(&mut rng, 2.0, 4.0);
                let fx = (ax as f64 + angle.cos() * flen).round() as i32;
                let fy = (ay as f64 + angle.sin() * flen).round() as i32;
                let fx = fx.clamp(0, w - 1);
                let fy = fy.clamp(0, h - 1);
                let sub =
                    midpoint_displace(&mut rng, vec![(ax, ay), (fx, fy)], 3, width as f64 * 0.04);
                forks.push(Bolt {
                    points: sub,
                    forks: Vec::new(),
                });
            }
        }
    }
    Bolt { points, forks }
}

fn midpoint_displace(
    rng: &mut Mt19937,
    points: Vec<(i32, i32)>,
    depth: u32,
    amplitude: f64,
) -> Vec<(i32, i32)> {
    if depth == 0 {
        return points;
    }
    let mut out: Vec<(i32, i32)> = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for i in 0..points.len() - 1 {
        let a = points[i];
        let b = points[i + 1];
        let mx = (a.0 + b.0) as f64 / 2.0;
        let my = (a.1 + b.1) as f64 / 2.0;
        let dx = (b.0 - a.0) as f64;
        let dy = (b.1 - a.1) as f64;
        let seg_len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if seg_len > 0.0 {
            (-dy / seg_len, dx / seg_len)
        } else {
            (0.0, 0.0)
        };
        let disp = uniform(rng, -amplitude, amplitude);
        out.push((
            (mx + nx * disp).round() as i32,
            (my + ny * disp).round() as i32,
        ));
        out.push(b);
    }
    midpoint_displace(rng, out, depth - 1, amplitude * 0.55)
}

// Matches Python random.Random.expovariate(rate): -ln(1 - random()) / rate.
#[inline]
fn expovariate(rng: &mut Mt19937, rate: f64) -> f64 {
    -(1.0 - rng.next_f64()).ln() / rate
}

// Matches Python random.Random.uniform(a, b): a + (b - a) * random().
#[inline]
fn uniform(rng: &mut Mt19937, a: f64, b: f64) -> f64 {
    a + (b - a) * rng.next_f64()
}

pub fn l_bump_at(strikes: &[Strike], t_now: f64, params: &FlashParams) -> f64 {
    let mut total = 0.0;
    for s in strikes {
        for sf in &s.sub_flashes {
            let dt = t_now - sf.t_peak;
            if dt < -0.005 {
                continue;
            }
            if dt > params.tau * 6.0 {
                continue;
            }
            if dt < 0.0 {
                total += params.peak * (1.0 + dt / 0.005);
            } else {
                total += params.peak * (-dt / params.tau).exp();
            }
        }
    }
    total.min(1.0)
}

fn active_bolt<'a>(strikes: &'a [Strike], t_now: f64, params: &FlashParams) -> Option<&'a Bolt> {
    for s in strikes {
        // A boltless strike must not end the search; `?` here would return
        // None for the whole function at the first sheet flash.
        let Some(bolt) = s.bolt.as_ref() else {
            continue;
        };
        let first = &s.sub_flashes[0];
        let dt = t_now - first.t_peak;
        if (-0.005..=params.tau * 3.0).contains(&dt) {
            return Some(bolt);
        }
    }
    None
}

const BOLT_LAB_R: u8 = 252;
const BOLT_LAB_G: u8 = 250;
const BOLT_LAB_B: u8 = 240;
const HALO_L_BUMP: f64 = 0.18;
const OCCLUSION_L: f64 = 0.78;

static BOLT_COLOR: LazyLock<Rgb> =
    LazyLock::new(|| oklab_to_rgb(rgb_u8_to_oklab(BOLT_LAB_R, BOLT_LAB_G, BOLT_LAB_B)));

pub fn overlay(pixels: &mut PixelBuffer, lightning: &Lightning, t_seconds: f64) {
    let bump = l_bump_at(&lightning.strikes, t_seconds, &lightning.params);
    if bump > 0.0001 {
        apply_l_bump(pixels, bump);
    }
    if let Some(bolt) = active_bolt(&lightning.strikes, t_seconds, &lightning.params) {
        // Snapshot pre-bolt buffer for occlusion / halo blending.
        let base: Vec<crate::colorspace::Rgb> = pixels.pixels.clone();
        draw_bolt_recursive(pixels, &base, bolt, 0);
    }
}

fn apply_l_bump(pixels: &mut PixelBuffer, l_bump: f64) {
    for i in 0..pixels.pixels.len() {
        let p = pixels.pixels[i];
        let lab = rgb_u8_to_oklab(p.r, p.g, p.b);
        let new = Oklab::new((lab.l + l_bump).min(1.0), lab.a, lab.b);
        pixels.pixels[i] = oklab_to_rgb(new);
    }
}

fn draw_bolt_recursive(
    pixels: &mut PixelBuffer,
    base: &[crate::colorspace::Rgb],
    bolt: &Bolt,
    depth: u32,
) {
    let pts = &bolt.points;
    for i in 0..pts.len().saturating_sub(1) {
        draw_segment(pixels, base, pts[i], pts[i + 1], depth);
    }
    if depth < 2 {
        for fork in &bolt.forks {
            draw_bolt_recursive(pixels, base, fork, depth + 1);
        }
    }
}

fn draw_segment(
    pixels: &mut PixelBuffer,
    base: &[crate::colorspace::Rgb],
    a: (i32, i32),
    b: (i32, i32),
    depth: u32,
) {
    let w = pixels.width as i32;
    let h = pixels.height as i32;
    let bolt_color = *BOLT_COLOR;

    let (mut x0, mut y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        if (0..w).contains(&x0) && (0..h).contains(&y0) {
            let idx = (y0 as usize) * pixels.width + (x0 as usize);
            let base_rgb = base[idx];
            let base_lab = rgb_u8_to_oklab(base_rgb.r, base_rgb.g, base_rgb.b);
            if base_lab.l < OCCLUSION_L {
                pixels.pixels[idx] = bolt_color;
                if depth == 0 {
                    stamp_halo(pixels, base, x0, y0);
                }
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn stamp_halo(pixels: &mut PixelBuffer, base: &[crate::colorspace::Rgb], cx: i32, cy: i32) {
    let w = pixels.width as i32;
    let h = pixels.height as i32;
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let x = cx + dx;
            let y = cy + dy;
            if !(0..w).contains(&x) || !(0..h).contains(&y) {
                continue;
            }
            let idx = (y as usize) * pixels.width + (x as usize);
            let base_lab = rgb_u8_to_oklab(base[idx].r, base[idx].g, base[idx].b);
            if base_lab.l >= OCCLUSION_L {
                continue;
            }
            let cur = pixels.pixels[idx];
            let cur_lab = rgb_u8_to_oklab(cur.r, cur.g, cur.b);
            let new = Oklab::new((cur_lab.l + HALO_L_BUMP).min(1.0), cur_lab.a, cur_lab.b);
            pixels.pixels[idx] = oklab_to_rgb(new);
        }
    }
}
