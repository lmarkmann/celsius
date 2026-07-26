//! The moon: its glow, its disc, and the phase terminator.
//!
//! Two contributions, kept separate because they composite at different stages. The glow is additive and lands early, before clouds, so a cloud deck can pass in front of it. The disc composites last, on top of everything, since nothing in this scene is in front of the moon.
//!
//! Phase is a 0..1 value where 0.5 is full, and the terminator is evaluated per pixel across the disc rather than drawn as a shape, which is what gives a gibbous moon its curved edge for free. The disc is deliberately flat: the real moon shows no limb darkening, so a uniform face is the accurate choice as well as the cheap one.

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
        // Same signs the powf path yields for a zero base, so the caller's additive blend stays bit-identical while most pixels skip the powf.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn moon_at(phase: f64) -> Moon {
        Moon {
            x_frac: 0.5,
            y_frac: 0.5,
            radius: 10.0,
            phase,
            visible: true,
        }
    }

    /// The golden renders execute this file but never assert on what it drew, so its arithmetic was free to change: mutation testing flipped signs and comparisons here twelve times without a single test noticing.
    #[test]
    fn the_disc_ends_where_its_radius_says() {
        let m = moon_at(0.5);
        // Centre of a 100x100 frame, radius 10 px.
        assert!(
            disc_sample(&m, 50, 50, 100, 100).is_some(),
            "centre is disc"
        );
        assert!(
            disc_sample(&m, 59, 50, 100, 100).is_some(),
            "just inside the radius is still disc"
        );
        assert!(
            disc_sample(&m, 61, 50, 100, 100).is_none(),
            "past the radius is sky, not disc"
        );
    }

    /// Edge alpha exists to soften the rim. Flat inside, falling to nothing at the edge.
    #[test]
    fn the_rim_softens_outward() {
        let m = moon_at(0.5);
        let (_, centre) = disc_sample(&m, 50, 50, 100, 100).unwrap();
        let (_, rim) = disc_sample(&m, 59, 50, 100, 100).unwrap();
        assert!(
            (centre - 1.0).abs() < 1e-9,
            "the face is flat: the real moon shows no limb darkening, got {centre}"
        );
        assert!(
            rim < centre,
            "the rim must fade, got {rim} against {centre}"
        );
    }

    /// Phase is the whole point of the module, and was the least defended part of it.
    #[test]
    fn phase_lights_the_side_the_sun_is_on() {
        let full = moon_at(0.5);
        let (west, _) = disc_sample(&full, 45, 50, 100, 100).unwrap();
        let (east, _) = disc_sample(&full, 55, 50, 100, 100).unwrap();
        assert!(
            (west.l - east.l).abs() < 1e-9,
            "a full moon is lit evenly across"
        );

        // First quarter: one limb lit, the other in shadow.
        let quarter = moon_at(0.25);
        let (left, _) = disc_sample(&quarter, 43, 50, 100, 100).unwrap();
        let (right, _) = disc_sample(&quarter, 57, 50, 100, 100).unwrap();
        assert!(
            (left.l - right.l).abs() > 0.05,
            "a quarter moon must have a lit limb and a dark one, got {} and {}",
            left.l,
            right.l
        );

        // And the opposite quarter lights the opposite limb.
        let last = moon_at(0.75);
        let (last_left, _) = disc_sample(&last, 43, 50, 100, 100).unwrap();
        assert!(
            (last_left.l - left.l).abs() > 0.05,
            "last quarter must light the limb first quarter left dark"
        );
    }

    /// The glow is a separate contribution that lands before the clouds, so it has to reach beyond the disc and fall to nothing.
    #[test]
    fn the_glow_falls_off_with_distance() {
        let m = moon_at(0.5);
        let (near, _, _) = glow_contribution(&m, 52, 50, 100, 100);
        let (far, _, _) = glow_contribution(&m, 70, 50, 100, 100);
        let (gone, _, _) = glow_contribution(&m, 99, 50, 100, 100);
        assert!(near > far, "glow must decrease outward: {near} then {far}");
        assert!(far > 0.0, "glow must reach past the disc itself");
        assert!(gone == 0.0, "and must reach zero, got {gone}");
    }
}
