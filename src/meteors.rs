//! Meteors (shooting stars): an additive overlay for clear night skies.
//!
//! Same shape as `lightning`: composed once when SkyState is built, evaluated
//! each TUI tick by `overlay()`, so a still PNG never shows one. A sporadic
//! background (~10/hr, Lynch & Livingston 6.15) plus any active showers from the
//! IMO working list. A shower's meteors stream away from its radiant, which is
//! placed with `astro::equatorial_to_altaz`; faster showers read cooler and
//! longer. Mt19937-seeded so a given (place, day) always yields the same sky.

use std::f64::consts::TAU;

use crate::astro;
use crate::colorspace::{Oklab, PixelBuffer, lerp_oklab, oklab_to_rgb, rgb_u8_to_oklab};
use crate::noise::Mt19937;

/// One shower from the IMO working list. Radiant is J2000 (RA, Dec in degrees);
/// `peak_yday` is the day-of-year of maximum, `window_days` the Gaussian
/// half-width over which it stays active, `zhr` the peak zenithal hourly rate,
/// `v_kms` the geocentric entry velocity (sets streak speed and color).
#[derive(Clone, Copy, Debug)]
pub struct Shower {
    pub name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub peak_yday: f64,
    pub window_days: f64,
    pub zhr: f64,
    pub v_kms: f64,
}

// The International Meteor Organization working list of major showers, pulled 2026-06-19, with radiants cross-checked against Wikipedia's "List of meteor showers". Where the two disagreed on ZHR the IMO peak value wins.
// peak_yday is a non-leap day-of-year; a leap-boundary off-by-one is immaterial
// to a day-resolution rate.
#[rustfmt::skip]
pub const SHOWERS: &[Shower] = &[
    Shower { name: "Quadrantids",        ra_deg: 230.0, dec_deg:  49.0, peak_yday:   3.0, window_days: 1.0, zhr: 120.0, v_kms: 40.0 },
    Shower { name: "Lyrids",             ra_deg: 271.0, dec_deg:  34.0, peak_yday: 112.0, window_days: 2.0, zhr:  18.0, v_kms: 49.0 },
    Shower { name: "Eta Aquariids",      ra_deg: 338.0, dec_deg:  -1.0, peak_yday: 126.0, window_days: 5.0, zhr:  50.0, v_kms: 66.0 },
    Shower { name: "S. delta Aquariids", ra_deg: 340.0, dec_deg: -16.0, peak_yday: 211.0, window_days: 7.0, zhr:  25.0, v_kms: 41.0 },
    Shower { name: "Perseids",           ra_deg:  48.0, dec_deg:  58.0, peak_yday: 225.0, window_days: 5.0, zhr: 100.0, v_kms: 59.0 },
    Shower { name: "Orionids",           ra_deg:  95.0, dec_deg:  16.0, peak_yday: 294.0, window_days: 6.0, zhr:  20.0, v_kms: 66.0 },
    Shower { name: "Leonids",            ra_deg: 152.0, dec_deg:  22.0, peak_yday: 321.0, window_days: 2.0, zhr:  15.0, v_kms: 70.0 },
    Shower { name: "Geminids",           ra_deg: 112.0, dec_deg:  33.0, peak_yday: 348.0, window_days: 3.0, zhr: 150.0, v_kms: 35.0 },
    Shower { name: "Ursids",             ra_deg: 217.0, dec_deg:  76.0, peak_yday: 356.0, window_days: 2.0, zhr:  10.0, v_kms: 33.0 },
];

const SPORADIC_HR: f64 = 10.0;
const VMIN: f64 = 12.0;
const VMAX: f64 = 75.0;

/// One scheduled meteor, in frame fractions rather than pixels.
///
/// Everything here is 0..1 across the frame, so the same meteor lands in the same place whatever size the terminal is. Storing pixels instead meant a schedule built for the 104x50 reference drew into the top-left corner of any larger buffer, and stopped matching the sky after a resize. Mapping fractions to pixels is linear, so the fan still converges exactly on its radiant.
#[derive(Clone, Debug)]
pub struct Meteor {
    pub t_start: f64,
    pub life: f64,
    /// Where the head starts, as a fraction of the frame.
    pub from: (f64, f64),
    /// Travel direction, a unit vector in frame fractions.
    pub dir: (f64, f64),
    /// How far the head travels over its life, in frame fractions.
    pub travel: f64,
    /// Length of the glowing trail behind the head, in frame fractions.
    pub streak: f64,
    pub peak_l: f64,
    pub color: Oklab,
    pub train: bool,
}

#[derive(Clone, Debug)]
pub struct Meteors {
    pub meteors: Vec<Meteor>,
}

#[derive(Clone, Copy)]
struct ActiveShower {
    rate_hr: f64,
    radiant: (f64, f64),
    v_kms: f64,
}

impl Meteors {
    pub fn new(
        seed: u32,
        unix_utc: i64,
        lat: f64,
        lon: f64,
        center_az: f64,
        duration_s: f64,
    ) -> Self {
        let mut rng = Mt19937::init_by_array(&[seed]);

        // Active showers: tapered by proximity to peak, scaled by radiant
        // altitude (rate folds to zero as the radiant sets), projected to screen.
        let yday = day_of_year(unix_utc);
        let mut active: Vec<ActiveShower> = Vec::new();
        for sh in SHOWERS {
            let taper = peak_taper(yday, sh.peak_yday, sh.window_days);
            if taper <= 0.0 {
                continue;
            }
            let altaz = astro::equatorial_to_altaz(sh.ra_deg, sh.dec_deg, lat, lon, unix_utc);
            if altaz.altitude <= 0.0 {
                continue;
            }
            // A radiant off the edge of the frame is normal and still sets the direction meteors travel, so keep it rather than culling; only a radiant behind the viewing plane has no usable position. Bound it so a grazing angle cannot throw the fan geometry to infinity.
            let Some((fx, fy)) = astro::to_sky_fracs(&altaz, center_az) else {
                continue;
            };
            active.push(ActiveShower {
                rate_hr: sh.zhr * taper * altaz.altitude.to_radians().sin(),
                radiant: (fx.clamp(-3.0, 4.0), fy.clamp(-3.0, 4.0)),
                v_kms: sh.v_kms,
            });
        }

        let total_hr = SPORADIC_HR + active.iter().map(|a| a.rate_hr).sum::<f64>();
        let rate_s = total_hr / 3600.0;

        let mut meteors = Vec::new();
        let mut t = expovariate(&mut rng, rate_s);
        while t < duration_s {
            // Pick the source by rate share: sporadic, else one shower.
            let mut pick = rng.next_f64() * total_hr;
            let (radiant, v_kms) = if pick < SPORADIC_HR || active.is_empty() {
                (None, uniform(&mut rng, 25.0, 60.0))
            } else {
                pick -= SPORADIC_HR;
                let mut chosen = active[active.len() - 1];
                for a in &active {
                    if pick < a.rate_hr {
                        chosen = *a;
                        break;
                    }
                    pick -= a.rate_hr;
                }
                (Some(chosen.radiant), chosen.v_kms)
            };

            meteors.push(spawn(&mut rng, t, radiant, v_kms));
            t += expovariate(&mut rng, rate_s);
        }

        Self { meteors }
    }
}

fn spawn(rng: &mut Mt19937, t_start: f64, radiant: Option<(f64, f64)>, v_kms: f64) -> Meteor {
    let from = (uniform(rng, 0.0, 1.0), uniform(rng, 0.0, 1.0));
    let dir = match radiant {
        Some((rx, ry)) => {
            let (dx, dy) = (from.0 - rx, from.1 - ry);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-3 {
                let ang = uniform(rng, 0.0, TAU);
                (ang.cos(), ang.sin())
            } else {
                (dx / len, dy / len)
            }
        }
        None => {
            let ang = uniform(rng, 0.0, TAU);
            (ang.cos(), ang.sin())
        }
    };

    let vf = ((v_kms - VMIN) / (VMAX - VMIN)).clamp(0.0, 1.0);
    let life = uniform(rng, 0.35, 0.9) * (1.0 - 0.35 * vf);
    let travel = (0.12 + 0.30 * vf) * uniform(rng, 0.7, 1.2);
    // Streak length as a fraction of the frame, from the pixel lengths this was tuned at against the 104-wide reference.
    let streak = (2.5 + 6.0 * vf) / 104.0;
    // Brightness skewed faint: most meteors are dim, the odd one flares.
    let u = rng.next_f64();
    let peak_l = 0.22 + 0.6 * u * u;
    let train = peak_l > 0.6 && rng.next_f64() < 0.5;

    Meteor {
        t_start,
        life,
        from,
        dir,
        travel,
        streak,
        peak_l,
        color: meteor_color(vf),
        train,
    }
}

// Slow meteors burn warm (sodium/iron), fast ones read cool blue-white.
fn meteor_color(vf: f64) -> Oklab {
    let warm = rgb_u8_to_oklab(255, 200, 140);
    let cool = rgb_u8_to_oklab(205, 222, 255);
    lerp_oklab(warm, cool, vf)
}

// Day-of-year (1..=366) of the UTC date. Shower activity is date-keyed, so the
// sub-day offset between UTC and local time does not matter here.
fn day_of_year(unix_utc: i64) -> f64 {
    use chrono::{Datelike, TimeZone, Utc};
    Utc.timestamp_opt(unix_utc, 0)
        .single()
        .map(|dt| dt.ordinal() as f64)
        .unwrap_or(1.0)
}

// Gaussian falloff around the peak, wrapping the year boundary, cut at 3 sigma.
fn peak_taper(yday: f64, peak_yday: f64, window_days: f64) -> f64 {
    let raw = (yday - peak_yday).abs();
    let d = raw.min(365.0 - raw);
    if d > window_days * 3.0 {
        0.0
    } else {
        (-0.5 * (d / window_days).powi(2)).exp()
    }
}

// Python random.Random parity, same as lightning's scheduler.
#[inline]
fn expovariate(rng: &mut Mt19937, rate: f64) -> f64 {
    -(1.0 - rng.next_f64()).ln() / rate
}

#[inline]
fn uniform(rng: &mut Mt19937, a: f64, b: f64) -> f64 {
    a + (b - a) * rng.next_f64()
}

pub fn overlay(pixels: &mut PixelBuffer, meteors: &Meteors, t: f64) {
    // Fractions become pixels here and nowhere else, so a resize moves the meteors with the sky instead of stranding them in a corner.
    let w = pixels.width as f64;
    let h = pixels.height as f64;
    for m in &meteors.meteors {
        let dt = t - m.t_start;
        if dt < 0.0 || dt > m.life {
            continue;
        }
        let p = dt / m.life;
        let brightness = m.peak_l * 4.0 * p * (1.0 - p); // parabola, peaks mid-flight
        if brightness <= 0.001 {
            continue;
        }
        // Head position in pixels, and the travel direction measured in this buffer's pixels so the trail is drawn one pixel at a time however wide the terminal is.
        let (fx, fy) = (m.from.0 * w, m.from.1 * h);
        let (px_dx, px_dy) = (m.dir.0 * w, m.dir.1 * h);
        let px_len = (px_dx * px_dx + px_dy * px_dy).sqrt().max(1e-9);
        let (ux, uy) = (px_dx / px_len, px_dy / px_len);
        let travel_px = m.travel * px_len;
        let streak_px = (m.streak * w).max(1.0);

        let hx = fx + ux * travel_px * p;
        let hy = fy + uy * travel_px * p;

        // Glowing streak: from the head back toward the radiant, fading.
        let steps = streak_px.ceil() as i32;
        for s in 0..=steps {
            let f = s as f64;
            let falloff = (1.0 - f / streak_px).max(0.0);
            add_glow(
                pixels,
                (hx - ux * f).round() as i32,
                (hy - uy * f).round() as i32,
                brightness * falloff,
                m.color,
            );
        }

        // Bright meteors leave a faint persistent train along the flown path.
        if m.train {
            let flown = (travel_px * p) as i32;
            for s in 0..flown {
                let f = s as f64;
                add_glow(
                    pixels,
                    (fx + ux * f).round() as i32,
                    (fy + uy * f).round() as i32,
                    0.05 * brightness,
                    m.color,
                );
            }
        }
    }
}

fn add_glow(pixels: &mut PixelBuffer, x: i32, y: i32, l_bump: f64, color: Oklab) {
    if l_bump <= 0.0 {
        return;
    }
    let w = pixels.width as i32;
    let h = pixels.height as i32;
    if !(0..w).contains(&x) || !(0..h).contains(&y) {
        return;
    }
    let idx = (y as usize) * pixels.width + (x as usize);
    let base = pixels.pixels[idx];
    let lab = rgb_u8_to_oklab(base.r, base.g, base.b);
    let l = (lab.l + l_bump).min(1.0);
    let mix = (0.6 * l_bump).clamp(0.0, 1.0);
    let a = lab.a + (color.a - lab.a) * mix;
    let b = lab.b + (color.b - lab.b) * mix;
    pixels.pixels[idx] = oklab_to_rgb(Oklab::new(l, a, b));
}
