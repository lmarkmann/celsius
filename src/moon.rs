use std::f64::consts::TAU;
use std::sync::LazyLock;

use crate::colorspace::{Oklab, lerp_oklab, rgb_u8_to_oklab};
use crate::scene::Moon;

static LIT: LazyLock<Oklab> = LazyLock::new(|| rgb_u8_to_oklab(242, 238, 222));
static SHADOW: LazyLock<Oklab> = LazyLock::new(|| rgb_u8_to_oklab(36, 30, 50));

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
    let falloff = (1.0 - d / 0.40).max(0.0);
    if falloff == 0.0 {
        // Same signs the powf path yields for a zero base, so the caller's
        // additive blend stays bit-identical while most pixels skip the powf.
        return (0.0, -0.0, -0.0);
    }
    let glow = falloff.powf(2.2);
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
    let color = lerp_oklab(*SHADOW, *LIT, lit_frac);

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
