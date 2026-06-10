use crate::colorspace::{Oklab, PixelBuffer, Rgb, lerp_oklab, oklab_to_rgb, rgb_u8_to_oklab};
use crate::haze;
use crate::moon;
use crate::noise::Noise;
use crate::precipitation;
use crate::scene::SkyState;
use crate::stars::build_star_map;

const ALTITUDE_CUTOFF: f64 = 0.04;

fn sun_disc_color() -> Oklab {
    rgb_u8_to_oklab(255, 242, 205)
}

// Per-layer render parameters resolved once from the layer's cloud kind, so the
// per-pixel loop never reconstructs morphology or re-converts colors.
struct LayerRender {
    noise: Noise,
    octaves: u32,
    edge: f64,
    flatten: f64,
    shadow: Oklab,
    lit: Oklab,
}

pub fn render(state: &SkyState, width: u32, height: u32) -> PixelBuffer {
    let w = width as usize;
    let h = height as usize;
    let mut pixels = PixelBuffer::filled(w, h, Rgb::BLACK);

    let cloud_layers: Vec<LayerRender> = state
        .clouds
        .iter()
        .map(|l| {
            let m = l.kind.morphology();
            LayerRender {
                noise: Noise::new(l.seed),
                octaves: m.octaves,
                edge: m.edge,
                flatten: l.flatten,
                shadow: rgb_u8_to_oklab(m.shadow_rgb[0], m.shadow_rgb[1], m.shadow_rgb[2]),
                lit: rgb_u8_to_oklab(m.lit_rgb[0], m.lit_rgb[1], m.lit_rgb[2]),
            }
        })
        .collect();
    let haze_lab = state
        .haze
        .as_ref()
        .map(|h| rgb_u8_to_oklab(h.rgb[0], h.rgb[1], h.rgb[2]));
    let horizon_glow = state.horizon_glow.as_ref().map(|g| {
        (
            g.x_frac,
            rgb_u8_to_oklab(g.rgb[0], g.rgb[1], g.rgb[2]),
            g.strength,
        )
    });
    let star_map = state
        .stars
        .as_ref()
        .map(|s| build_star_map(s, width, height, &state.gradient))
        .unwrap_or_default();

    let sun = &state.sun;
    let sun_px = sun.x_frac * width as f64;
    let sun_py = sun.y_frac * height as f64;
    let sun_r = sun.radius;
    let sun_disc = sun_disc_color();

    for py in 0..height {
        let tv = py as f64 / (height - 1) as f64;
        for px in 0..width {
            let base = state.gradient.sample(tv);
            let mut l = base.l;
            let mut a = base.a;
            let mut b = base.b;

            if let Some(star) = star_map.get(&(px, py)) {
                l = (l + star.l).min(1.0);
                a += star.a;
                b += star.b;
            }

            if sun.visible {
                let dx = (px as f64 - sun_px) / width as f64;
                let dy = (py as f64 - sun_py) / height as f64;
                let d = (dx * dx + dy * dy * 3.2).sqrt();
                let glow = (1.0 - d / 0.60).max(0.0).powi(2);
                l += glow * 0.11;
                a += glow * 0.020;
                b += glow * 0.055;
            }

            if let Some(m) = state.moon.as_ref().filter(|m| m.visible) {
                let (dl, da, db) = moon::glow_contribution(m, px, py, width, height);
                l += dl;
                a += da;
                b += db;
            }

            for (layer, lr) in state.clouds.iter().zip(cloud_layers.iter()) {
                let diff = tv - layer.altitude_t;
                let sigma = layer.altitude_sigma;
                let alt = (-(diff * diff) / (2.0 * sigma * sigma)).exp();
                if alt < ALTITUDE_CUTOFF {
                    continue;
                }
                let nx = px as f64 / width as f64 * layer.scale_x + layer.offset_x;
                let ny = py as f64 / height as f64 * layer.scale_y + layer.offset_y;
                let n = lr.noise.warped_fbm_oct(nx, ny, lr.octaves);
                let noise_density = ((n - layer.threshold).max(0.0) * lr.edge) * alt * layer.cover;
                // A flat deck ignores the noise gate and fills the altitude band
                // solidly; flatten blends between the two.
                let flat_density = (alt * layer.cover).min(1.0);
                let mut density = noise_density * (1.0 - lr.flatten) + flat_density * lr.flatten;
                if density <= 0.0 {
                    continue;
                }
                density = density.min(1.0);

                let sdx = (sun_px - px as f64) / width as f64;
                let sdy = (sun_py - py as f64) / height as f64;
                let sun_dist = (sdx * sdx + sdy * sdy).sqrt();
                let lit = (1.0 - sun_dist * 1.6).clamp(0.0, 1.0);
                let cl = lerp_oklab(lr.shadow, lr.lit, lit);
                let inv = 1.0 - density;
                l = l * inv + cl.l * density;
                a = a * inv + cl.a * density;
                b = b * inv + cl.b * density;
            }

            if let (Some(hz), Some(hz_lab)) = (state.haze.as_ref(), haze_lab) {
                let k = haze::blend_factor(tv, hz);
                if k > 0.0 {
                    l += (hz_lab.l - l) * k;
                    a += (hz_lab.a - a) * k;
                    b += (hz_lab.b - b) * k;
                }
            }

            if let Some((gx_frac, glow, strength)) = horizon_glow {
                let dx = px as f64 / width as f64 - gx_frac;
                let horiz = (1.0 - dx.abs() / 0.6).max(0.0);
                let band = ((tv - 0.45) / 0.55).clamp(0.0, 1.0);
                let k = strength * horiz * horiz * band * band * 0.6;
                if k > 0.0 {
                    l += (glow.l - l) * k;
                    a += (glow.a - a) * k;
                    b += (glow.b - b) * k;
                }
            }

            if sun.visible {
                let ex = px as f64 - sun_px;
                let ey = py as f64 - sun_py;
                let sd = (ex * ex + ey * ey).sqrt();
                if sd < sun_r {
                    let k = (1.0 - (sd / sun_r).powi(2)).max(0.0);
                    let inv = 1.0 - k;
                    l = l * inv + sun_disc.l * k;
                    a = a * inv + sun_disc.a * k;
                    b = b * inv + sun_disc.b * k;
                }
            }

            if let Some(m) = state.moon.as_ref().filter(|m| m.visible)
                && let Some((color, alpha)) = moon::disc_sample(m, px, py, width, height)
            {
                let inv = 1.0 - alpha;
                l = l * inv + color.l * alpha;
                a = a * inv + color.a * alpha;
                b = b * inv + color.b * alpha;
            }

            pixels.set(px as usize, py as usize, oklab_to_rgb(Oklab::new(l, a, b)));
        }
    }

    if let Some(p) = state.precipitation.as_ref() {
        precipitation::overlay(&mut pixels, p);
    }

    pixels
}
