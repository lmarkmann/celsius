//! The pixel pipeline: a [`SkyState`] in, a [`PixelBuffer`] out.
//!
//! Compositing order is the physics and cannot be shuffled. Per pixel: the base sky (analytic radiance when the sun is up, otherwise the gradient), then additive stars, sun glow and moon glow, then each cloud layer blended by its density, then haze toward the horizon, and finally the sun and moon discs on top. Precipitation runs afterwards as a full-buffer pass, because a streak crosses many pixels.
//!
//! Everything that can be hoisted out of the per-pixel loop already has been: noise grids are cached per seed, the gradient is sampled once per row, and each cloud layer's altitude mask is a row term rather than a pixel term. That is what keeps a 104x50 frame in the hundreds of microseconds.
//!
//! Note what is *absent*: no time parameter. Animation is expressed by mutating the `SkyState` between calls (cloud drift) or by compositing overlays onto the result (lightning, meteors), never by rendering a different frame for a different instant.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::colorspace::{Oklab, PixelBuffer, Rgb, lerp_oklab, oklab_to_rgb, rgb_u8_to_oklab};
use crate::moon;
use crate::noise::{Noise, smoothstep};
use crate::precipitation;
use crate::scene::SkyState;
use crate::stars::build_star_field;

/// The buffer width the cloud frequencies were tuned against.
const REF_WIDTH: f64 = 104.0;

/// How many extra octaves of cloud detail a buffer of this width earns.
///
/// A cloud subtends a fixed angle, so its size on screen should not change with the terminal: a wider window is not a wider sky. What should change is how much structure resolves inside it, the way a larger print of the same photograph shows more grain rather than more subject.
///
/// The noise is sampled in frame units, so `fbm` produces a fixed *count* of features regardless of resolution, and every extra pixel went into making the same blobs bigger. That is the smudge. Each doubling of width now buys one more octave, which holds the finest structure near the pixel scale instead of stretching it.
///
/// Zero at the reference width, so the golden renders are untouched. Capped at two because the value-noise grid wraps at `NOISE_WIDTH`, and an octave whose sample span passes that starts tiling rather than adding detail.
fn detail_octaves(width: u32) -> u32 {
    ((f64::from(width) / REF_WIDTH).log2().max(0.0).round() as u32).min(2)
}

// A cloud layer's gaussian altitude mask is feathered to zero across this band instead of switching off in one row. Below the floor the layer contributes nothing and is skipped; from floor to knee its density ramps in via smoothstep so a near-uniform high-cover deck fades at its edge rather than leaving a hard horizontal seam where the mask crossed an abrupt cutoff.
const ALTITUDE_FLOOR: f64 = 0.02;
const ALTITUDE_KNEE: f64 = 0.14;

/// How fast optical depth turns into opacity.
///
/// Cloud thickness used to become opacity through `min(1.0)`, which meant the interior of every cloud clipped to exactly 1 and painted flat: with `edge` around 3.6 and `cover` around 1.5, density saturates once the noise passes its threshold by 0.19, and most of a cloud is past that. All the structure inside was computed and then discarded one line later, which is why more octaves did nothing for it.
///
/// Beer-Lambert instead. Opacity approaches 1 without reaching it, so thickness keeps modulating through the dense middle and a deck reads as cloud rather than as a grey shape. Tuned so a typical cloud keeps roughly the opacity it had at the edges.
const OPACITY_K: f64 = 1.7;

/// How much depth darkens a cloud toward its shadow tone.
///
/// Light reaching deep into a cloud has been scattered away, so thickness should shade it as well as hide what is behind it. Without this the tone comes only from distance to the sun, so every pixel of one deck is the same colour, and at night, where the sun term is zero outright, the whole deck is a single flat grey.
const DEPTH_SHADE: f64 = 0.45;

fn sun_disc_color() -> Oklab {
    rgb_u8_to_oklab(255, 242, 205)
}

/// How much the air itself whitens the sky at a given height on the altitude axis.
///
/// One exponential falloff from an onset height, standing in for the aerosol column you look through at low angles. The live weather layer derives `strength` from Open-Meteo visibility, so 2 km of fog and 30 km of clear air produce visibly different horizons from the same expression.
fn haze_blend(alt_t: f64, haze: &crate::scene::Haze) -> f64 {
    if alt_t <= haze.onset_t {
        return 0.0;
    }
    let span = 1.0 - haze.onset_t;
    let k = if span > 0.0 {
        (alt_t - haze.onset_t) / span
    } else {
        1.0
    };
    (haze.strength * k.powf(haze.exponent)).min(1.0)
}

/// A screen row, as a position on the sky's altitude axis: `0` at the top of the frame, `1` at the horizon.
///
/// Everything authored against height uses this, not the raw row: gradient stops, a cloud layer's `altitude_t`, `haze.onset_t`, the horizon-glow band, and the sky-brightness test that decides whether a star survives. The projection from row to viewing angle is nonlinear and stops short of the zenith, so placing a palette by row would squash it into whatever slice of sky the field of view happens to cover, and would move every stop the moment that field of view changed. Both ends are measured from the frame, so no stop is ever wasted off the top.
pub fn altitude_t(tv: f64) -> f64 {
    let alt_at = |v: f64| crate::astro::view_dir(0.5, v)[1].clamp(-1.0, 1.0).asin();
    let top = alt_at(0.0);
    let span = (top - alt_at(1.0)).abs().max(1e-9);
    ((top - alt_at(tv)) / span).clamp(0.0, 1.0)
}

thread_local! {
    // A noise grid depends only on its seed, so animated re-renders (the TUI redraws on every drift tick) reuse grids instead of rebuilding one per layer per frame. Realistic workloads see a few dozen seeds at ~24KB each.
    static NOISE_CACHE: RefCell<HashMap<u64, Rc<Noise>>> = RefCell::new(HashMap::new());
}

fn noise_for(seed: u64) -> Rc<Noise> {
    NOISE_CACHE.with(|cache| {
        Rc::clone(
            cache
                .borrow_mut()
                .entry(seed)
                .or_insert_with(|| Rc::new(Noise::new(seed))),
        )
    })
}

// Per-layer render parameters resolved once from the layer's cloud kind, so the per-pixel loop never reconstructs morphology or re-converts colors.
struct LayerRender {
    noise: Rc<Noise>,
    octaves: u32,
    edge: f64,
    flatten: f64,
    shadow: Oklab,
    lit: Oklab,
    two_sigma_sq: f64,
}

pub fn render(state: &SkyState, width: u32, height: u32) -> PixelBuffer {
    let w = width as usize;
    let h = height as usize;
    let mut pixels = PixelBuffer::filled(w, h, Rgb::BLACK);

    let extra_detail = detail_octaves(width);
    let cloud_layers: Vec<LayerRender> = state
        .clouds
        .iter()
        .map(|l| {
            let m = l.kind.morphology();
            LayerRender {
                noise: noise_for(l.seed),
                octaves: m.octaves + extra_detail,
                edge: m.edge,
                flatten: l.flatten,
                shadow: rgb_u8_to_oklab(m.shadow_rgb[0], m.shadow_rgb[1], m.shadow_rgb[2]),
                lit: rgb_u8_to_oklab(m.lit_rgb[0], m.lit_rgb[1], m.lit_rgb[2]),
                two_sigma_sq: 2.0 * l.altitude_sigma * l.altitude_sigma,
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
    let star_field = state
        .stars
        .as_ref()
        .map(|s| build_star_field(s, width, height, &state.gradient));

    let sun = &state.sun;
    let sun_px = sun.x_frac * width as f64;
    let sun_py = sun.y_frac * height as f64;
    let sun_r = sun.radius;
    let sun_disc = sun_disc_color();

    // Prototype: when an analytic sky is attached, its Preetham radiance field replaces the vertical gradient as the background. Prepared once here; the per-pixel cost is one Perez ratio plus a color conversion.
    let analytic = state.analytic.as_ref().map(crate::analytic_sky::prepare);

    // Row-invariant cloud terms: the altitude gaussian and the noise row coordinate change per row and per layer, never per pixel.
    let mut row_clouds: Vec<(f64, f64)> = vec![(0.0, 0.0); cloud_layers.len()];

    for py in 0..height {
        let tv = py as f64 / (height - 1) as f64;
        let alt_t = altitude_t(tv);
        let grad_row = state.gradient.sample(alt_t);
        for ((layer, lr), slot) in state.clouds.iter().zip(&cloud_layers).zip(&mut row_clouds) {
            let diff = alt_t - layer.altitude_t;
            let alt = (-(diff * diff) / lr.two_sigma_sq).exp();
            let ny = py as f64 / height as f64 * layer.scale_y + layer.offset_y;
            *slot = (alt, ny);
        }
        for px in 0..width {
            let fx = px as f64 / width as f64;
            let base = match &analytic {
                Some(prep) if prep.blend >= 1.0 => prep.sample(px as f64 / (width - 1) as f64, tv),
                Some(prep) => lerp_oklab(
                    grad_row,
                    prep.sample(px as f64 / (width - 1) as f64, tv),
                    prep.blend,
                ),
                None => grad_row,
            };
            let mut l = base.l;
            let mut a = base.a;
            let mut b = base.b;

            if let Some(field) = &star_field
                && let Some(star) = field[(py * width + px) as usize]
            {
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

            // Sun lighting is the same for every layer at this pixel; computed at most once, and only when some layer actually has density.
            let mut sun_lit: Option<f64> = None;
            for ((layer, lr), &(alt, ny)) in state.clouds.iter().zip(&cloud_layers).zip(&row_clouds)
            {
                if alt < ALTITUDE_FLOOR {
                    continue;
                }
                let edge_fade = smoothstep(
                    ((alt - ALTITUDE_FLOOR) / (ALTITUDE_KNEE - ALTITUDE_FLOOR)).min(1.0),
                );
                let nx = fx * layer.scale_x + layer.offset_x;
                let n = lr.noise.warped_fbm_oct(nx, ny, lr.octaves);
                let noise_thickness =
                    ((n - layer.threshold).max(0.0) * lr.edge) * alt * layer.cover;
                // A flat deck ignores the noise gate and fills the altitude band solidly; flatten blends between the two.
                let flat_thickness = alt * layer.cover;
                let thickness = (noise_thickness * (1.0 - lr.flatten)
                    + flat_thickness * lr.flatten)
                    * edge_fade;
                if thickness <= 0.0 {
                    continue;
                }
                // Optical depth to opacity, so the dense middle of a cloud still varies instead of clipping flat.
                let density = 1.0 - (-thickness * OPACITY_K).exp();

                let lit = *sun_lit.get_or_insert_with(|| {
                    let sdx = (sun_px - px as f64) / width as f64;
                    let sdy = (sun_py - py as f64) / height as f64;
                    let sun_dist = (sdx * sdx + sdy * sdy).sqrt();
                    (1.0 - sun_dist * 1.6).clamp(0.0, 1.0)
                });
                // Thickness shades the cloud as well as hiding the sky: a wispy edge keeps its lit tone, a deep part darkens toward shadow. This is what gives an interior any structure at all once it is opaque.
                let depth = 1.0 - (-thickness * DEPTH_SHADE).exp();
                let cl = lerp_oklab(
                    lr.shadow,
                    lr.lit,
                    lit * (1.0 - depth) + (1.0 - depth) * 0.35,
                );
                let inv = 1.0 - density;
                l = l * inv + cl.l * density;
                a = a * inv + cl.a * density;
                b = b * inv + cl.b * density;
            }

            if let (Some(hz), Some(hz_lab)) = (state.haze.as_ref(), haze_lab) {
                let k = haze_blend(alt_t, hz);
                if k > 0.0 {
                    l += (hz_lab.l - l) * k;
                    a += (hz_lab.a - a) * k;
                    b += (hz_lab.b - b) * k;
                }
            }

            if let Some((gx_frac, glow, strength)) = horizon_glow {
                let dx = fx - gx_frac;
                let horiz = (1.0 - dx.abs() / 0.6).max(0.0);
                let band = ((alt_t - 0.45) / 0.55).clamp(0.0, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The axis scene files are authored against has to be linear in viewing angle, or a palette tuned in degrees lands somewhere else. Sampling by screen row instead made the lowest 30 degrees of sky occupy half the frame, and moved every stop whenever the field of view changed.
    #[test]
    fn the_gradient_axis_is_linear_in_altitude() {
        let alt_of = |tv: f64| crate::astro::view_dir(0.5, tv)[1].clamp(-1.0, 1.0).asin();
        let samples: Vec<(f64, f64)> = (0..=10)
            .map(|i| {
                let tv = f64::from(i) / 10.0;
                (altitude_t(tv), alt_of(tv))
            })
            .collect();

        // Rows are not evenly spaced in angle, and are not meant to be. What has to hold is that a step along the axis is always the same number of degrees, wherever in the frame it is taken.
        let slope = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) / (b.1 - a.1);
        let reference = slope(samples[0], samples[10]);
        for pair in samples.windows(2) {
            let local = slope(pair[0], pair[1]);
            assert!(
                (local - reference).abs() < 1e-9,
                "axis moves {local} per radian here but {reference} overall; it is not linear in angle"
            );
        }
        assert!(samples[0].0.abs() < 1e-9, "top of frame must be t = 0");
        assert!(
            (samples[10].0 - 1.0).abs() < 1e-9,
            "the horizon must be t = 1, so no stop is wasted off the top"
        );
    }
}
