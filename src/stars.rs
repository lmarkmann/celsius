use std::collections::HashMap;

use crate::colorspace::Oklab;
use crate::gradient::Gradient;
use crate::noise::Mt19937;
use crate::scene::Stars;

pub type StarMap = HashMap<(u32, u32), Oklab>;

pub fn build_star_map(cfg: &Stars, width: u32, height: u32, gradient: &Gradient) -> StarMap {
    let mut rng = Mt19937::init_by_array(&[cfg.seed as u32]);
    let mut acc: HashMap<(u32, u32), [f64; 3]> = HashMap::new();

    for _ in 0..cfg.count {
        let xf = rng.next_f64();
        let yf = rng.next_f64() * 0.88;
        let mag = rng.next_f64().powf(0.38);
        let hue = -0.018 + 0.036 * rng.next_f64();

        let sky_l = gradient.sample(yf).l;
        let vis = ((cfg.sky_threshold - sky_l) / cfg.sky_threshold).max(0.0);
        let effective = mag * vis * cfg.brightness;
        if effective < 0.04 {
            continue;
        }

        let px = (xf * width as f64) as i32;
        let py = (yf * height as f64) as i32;

        let contrib = [effective * 0.52, hue * 0.45, -hue * 0.72];

        paint(&mut acc, px, py, width, height, contrib);
        if mag > 0.72 {
            let halo = [contrib[0] * 0.18, contrib[1], contrib[2]];
            for (dpx, dpy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                paint(&mut acc, px + dpx, py + dpy, width, height, halo);
            }
        }
    }

    acc.into_iter()
        .map(|(k, v)| (k, Oklab::new(v[0], v[1], v[2])))
        .collect()
}

fn paint(
    acc: &mut HashMap<(u32, u32), [f64; 3]>,
    px: i32,
    py: i32,
    width: u32,
    height: u32,
    contrib: [f64; 3],
) {
    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
        return;
    }
    let key = (px as u32, py as u32);
    let entry = acc.entry(key).or_insert([0.0, 0.0, 0.0]);
    entry[0] += contrib[0];
    entry[1] += contrib[1];
    entry[2] += contrib[2];
}
