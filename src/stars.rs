//! A procedural star field, built once per render into a flat lookup.
//!
//! Stars are drawn from the seeded MT19937, given a magnitude from a power-law draw and a slight warm or cool tint, then painted into a row-major array the render loop can index directly. A hash map would be the obvious structure and the wrong one: this is read once per pixel per frame, so it has to be an array load.
//!
//! Visibility is decided against the gradient rather than a flag. A star's contribution scales by how much darker the sky behind it is than the scene's `sky_threshold`, so the same field thins out naturally from midnight through dawn. The brightest stars get a four-neighbour halo, which is what makes them read as bright rather than merely lighter.

use crate::colorspace::Oklab;
use crate::gradient::Gradient;
use crate::noise::Mt19937;
use crate::scene::Stars;

/// Star contributions on a flat row-major grid, None where nothing painted. Render looks this up for every pixel of every frame, so it must be an array load, not a hash.
pub type StarField = Vec<Option<Oklab>>;

pub fn build_star_field(cfg: &Stars, width: u32, height: u32, gradient: &Gradient) -> StarField {
    let mut rng = Mt19937::init_by_array(&[cfg.seed as u32]);
    let mut field: StarField = vec![None; (width * height) as usize];

    // The frame is a rectilinear projection, where a pixel at the edge covers less sky than one at the centre by cos^3 of its angle off the axis. Scattering stars uniformly across the screen therefore crowds them into the corners by about six to one. Keeping the draw in screen space and rejecting against that ratio puts them back on the sky instead, and costs one extra random number per candidate.
    let axis = crate::astro::view_dir(0.5, 0.5);
    let mut placed = 0u32;
    // Corners are rejected around 85 percent of the time, so the budget has to be generous; it exists only so a pathological field cannot spin forever.
    let mut budget = cfg.count.saturating_mul(50);

    while placed < cfg.count && budget > 0 {
        budget -= 1;
        let xf = rng.next_f64();
        let yf = rng.next_f64();
        let dir = crate::astro::view_dir(xf, yf);
        let cos_off_axis = (dir[0] * axis[0] + dir[1] * axis[1] + dir[2] * axis[2]).clamp(0.0, 1.0);
        if rng.next_f64() > cos_off_axis.powi(3) {
            continue;
        }
        placed += 1;

        let mag = rng.next_f64().powf(0.38);
        let hue = -0.018 + 0.036 * rng.next_f64();

        // Ask the gradient how bright the sky is on the same altitude axis the renderer paints it on, not on the screen row. Sampling by row would test a star against a colour that is drawn somewhere else.
        let sky_l = gradient.sample(crate::render::altitude_t(yf)).l;
        let vis = ((cfg.sky_threshold - sky_l) / cfg.sky_threshold).max(0.0);
        let effective = mag * vis * cfg.brightness * extinction(dir[1]);
        if effective < 0.04 {
            continue;
        }

        let px = (xf * width as f64) as i32;
        let py = (yf * height as f64) as i32;

        let contrib = [effective * 0.52, hue * 0.45, -hue * 0.72];

        paint(&mut field, px, py, width, height, contrib);
        if mag > 0.72 {
            let halo = [contrib[0] * 0.18, contrib[1], contrib[2]];
            for (dpx, dpy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                paint(&mut field, px + dpx, py + dpy, width, height, halo);
            }
        }
    }

    field
}

/// How much of a star's light survives the air it is seen through, normalised so the zenith is unchanged.
///
/// This replaces a flat `yf * 0.88` cap that simply refused to draw anything in the bottom eighth of the screen. Real stars do appear near the horizon; they are just dimmed, because a line of sight at five degrees passes through eleven times the air of one straight up. Extinction is about 0.2 magnitudes per airmass at a decent site, so the field now fades toward the horizon instead of stopping at an invisible line.
fn extinction(sin_alt: f64) -> f64 {
    const PER_AIRMASS_MAG: f64 = 0.2;
    // Plane-parallel airmass, floored so a star exactly on the horizon stays finite.
    let airmass = 1.0 / sin_alt.max(0.02);
    let transmission = 10f64.powf(-0.4 * PER_AIRMASS_MAG * airmass);
    let at_zenith = 10f64.powf(-0.4 * PER_AIRMASS_MAG);
    (transmission / at_zenith).clamp(0.0, 1.0)
}

fn paint(
    field: &mut [Option<Oklab>],
    px: i32,
    py: i32,
    width: u32,
    height: u32,
    contrib: [f64; 3],
) {
    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
        return;
    }
    let idx = (py as u32 * width + px as u32) as usize;
    let star = field[idx].get_or_insert(Oklab::new(0.0, 0.0, 0.0));
    star.l += contrib[0];
    star.a += contrib[1];
    star.b += contrib[2];
}
