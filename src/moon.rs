//! Moon disc with phase shading and glow halo.
//!
//! Phase convention: 0 = new moon, 0.5 = full moon, 1 = new moon again.
//! Waxing moons have phase in (0, 0.5) with the right side lit. Waning
//! moons have phase in (0.5, 1) with the left side lit. The terminator is
//! a soft ellipse transition rather than a hard edge so it does not look
//! blocky at terminal resolution. The disc edge also fades softly.

use std::f64::consts::TAU;

use crate::colorspace::{Oklab, lerp_oklab, rgb_u8_to_oklab};
use crate::scene::Moon;

fn lit_color() -> Oklab {
    rgb_u8_to_oklab(242, 238, 222)
}

fn shadow_color() -> Oklab {
    rgb_u8_to_oklab(36, 30, 50)
}

pub fn glow_contribution(
    moon: &Moon,
    px: u32,
    py: u32,
    width: u32,
    height: u32,
) -> (f64, f64, f64) {
    let mx = moon.x_frac * width as f64;
    let my = moon.y_frac * height as f64;
    let dx = (px as f64 - mx) / width as f64;
    let dy = (py as f64 - my) / height as f64;
    let d = (dx * dx + dy * dy * 3.0).sqrt();
    let glow = (1.0 - d / 0.40).max(0.0).powf(2.2);
    (glow * 0.055, glow * -0.004, glow * -0.008)
}

pub fn disc_sample(moon: &Moon, px: u32, py: u32, width: u32, height: u32) -> Option<(Oklab, f64)> {
    let mx = moon.x_frac * width as f64;
    let my = moon.y_frac * height as f64;
    let r = moon.radius;

    let dx = px as f64 - mx;
    let dy = py as f64 - my;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist > r * 1.05 {
        return None;
    }

    let edge = ((dist - r * 0.88).max(0.0) / (r * 0.17)).min(1.0);
    let edge_alpha = (1.0 - edge).max(0.0);
    let edge_alpha = edge_alpha * edge_alpha;

    if dist > r {
        return None;
    }

    let xn = dx / r;
    let yn = dy / r;

    let lit_frac = phase_lit(xn, yn, moon.phase);
    let color = lerp_oklab(shadow_color(), lit_color(), lit_frac);

    Some((color, edge_alpha))
}

fn phase_lit(xn: f64, yn: f64, phase: f64) -> f64 {
    let x_lim = (1.0 - yn * yn).max(0.0).sqrt();
    if x_lim < 1e-4 {
        return 0.5;
    }
    let scale = (TAU * phase).cos();
    let term_x = scale * x_lim;
    let soft_band = x_lim * 0.08 + 0.01;

    let raw = if phase <= 0.5 {
        (xn - term_x) / soft_band
    } else {
        (term_x - xn) / soft_band
    };

    (0.5 + raw * 0.5).clamp(0.0, 1.0)
}
