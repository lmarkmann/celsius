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

    for _ in 0..cfg.count {
        let xf = rng.next_f64();
        let yf = rng.next_f64() * 0.88;
        let mag = rng.next_f64().powf(0.38);
        let hue = -0.018 + 0.036 * rng.next_f64();

        // Ask the gradient how bright the sky is on the same altitude axis the renderer paints it on, not on the screen row. Sampling by row would test a star against a colour that is drawn somewhere else.
        let sky_l = gradient.sample(crate::render::altitude_t(yf)).l;
        let vis = ((cfg.sky_threshold - sky_l) / cfg.sky_threshold).max(0.0);
        let effective = mag * vis * cfg.brightness;
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
