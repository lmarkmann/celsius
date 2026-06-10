//! Prototype: an analytic daytime sky (Preetham et al. 1999), the closed-form
//! member of the Hosek-Wilkie family. Sky radiance is computed per pixel from
//! the sun's position and an atmospheric turbidity via the Perez function, so
//! the zenith-to-horizon falloff and the sun-side brightening fall out of one
//! physical model instead of hand-tuned palettes. Daytime only: `build_sky`
//! attaches this only when the sun is above the horizon, and `render` falls
//! back to the gradient otherwise (Preetham's zenith formula is undefined once
//! the solar zenith angle passes 90 degrees).

use std::f64::consts::PI;

use crate::colorspace::Oklab;

/// Tone-map gain. Preetham radiance is in kcd/m^2 with enormous dynamic range;
/// this is the one knob that decides how that maps into terminal brightness.
/// Tuned by eye against a clear-noon zenith; this is the value to tweak.
const EXPOSURE: f64 = 0.045;

/// Horizontal field of view across the frame width, in radians (~140 deg). The
/// frame is a horizon-facing window, not a fisheye, so azimuth maps rectilinearly
/// across x and the whole frame is sky (no orthographic dome / dark corners).
const FOV_H: f64 = 2.443;

/// Parameters for one analytic sky, filled from live weather in `build_sky`.
#[derive(Clone, Debug)]
pub struct AnalyticSky {
    pub sun_alt: f64,
    pub sun_az: f64,
    pub center_az: f64,
    pub turbidity: f64,
    /// Crossfade weight toward the analytic sky, 0..1. Ramps up from 0 at the
    /// horizon to 1 a few degrees above it, so the model fades into the palette
    /// through twilight instead of popping in at sunrise. render lerps the
    /// palette gradient toward `sample()` by this amount.
    pub blend: f64,
}

struct Coeffs {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
}

// Preetham distribution coefficients, all linear in turbidity T.
fn lum_coeffs(t: f64) -> Coeffs {
    Coeffs {
        a: 0.1787 * t - 1.4630,
        b: -0.3554 * t + 0.4275,
        c: -0.0227 * t + 5.3251,
        d: 0.1206 * t - 2.5771,
        e: -0.0670 * t + 0.3703,
    }
}
fn cx_coeffs(t: f64) -> Coeffs {
    Coeffs {
        a: -0.0193 * t - 0.2592,
        b: -0.0665 * t + 0.0008,
        c: -0.0004 * t + 0.2125,
        d: -0.0641 * t - 0.8989,
        e: -0.0033 * t + 0.0452,
    }
}
fn cy_coeffs(t: f64) -> Coeffs {
    Coeffs {
        a: -0.0167 * t - 0.2608,
        b: -0.0950 * t + 0.0092,
        c: -0.0079 * t + 0.2102,
        d: -0.0441 * t - 1.6537,
        e: -0.0109 * t + 0.0529,
    }
}

// Perez F(theta, gamma): theta enters through its cosine (the view ray's zenith
// angle), gamma is the angle between the view ray and the sun.
fn perez(cos_theta: f64, gamma: f64, k: &Coeffs) -> f64 {
    let cos_theta = cos_theta.max(0.001); // guard the secant at the horizon
    let cg = gamma.cos();
    (1.0 + k.a * (k.b / cos_theta).exp()) * (1.0 + k.c * (k.d * gamma).exp() + k.e * cg * cg)
}

fn zenith_luminance(t: f64, theta_sun: f64) -> f64 {
    let chi = (4.0 / 9.0 - t / 120.0) * (PI - 2.0 * theta_sun);
    (4.0453 * t - 4.9710) * chi.tan() - 0.2155 * t + 2.4192
}

fn zenith_cx(t: f64, ts: f64) -> f64 {
    let (ts3, ts2) = (ts * ts * ts, ts * ts);
    t * t * (0.00166 * ts3 - 0.00375 * ts2 + 0.00209 * ts)
        + t * (-0.02903 * ts3 + 0.06377 * ts2 - 0.03202 * ts + 0.00394)
        + (0.11693 * ts3 - 0.21196 * ts2 + 0.06052 * ts + 0.25886)
}
fn zenith_cy(t: f64, ts: f64) -> f64 {
    let (ts3, ts2) = (ts * ts * ts, ts * ts);
    t * t * (0.00275 * ts3 - 0.00610 * ts2 + 0.00317 * ts)
        + t * (-0.04214 * ts3 + 0.08970 * ts2 - 0.04153 * ts + 0.00516)
        + (0.15346 * ts3 - 0.26756 * ts2 + 0.06670 * ts + 0.26688)
}

/// Per-sky constants computed once, so the per-pixel loop only does the Perez
/// ratio and the color conversion.
pub struct Prepared {
    sun: [f64; 3],
    lum: Coeffs,
    cx: Coeffs,
    cy: Coeffs,
    lum_z: f64,
    cx_z: f64,
    cy_z: f64,
    denom_lum: f64,
    denom_cx: f64,
    denom_cy: f64,
    pub blend: f64,
}

// View / sun direction as a unit vector in (east, up, forward), matching
// astro::to_sky_fracs so the analytic sun lines up with the drawn sun disc.
fn dir_from_altaz(alt_deg: f64, az_deg: f64, center_az: f64) -> [f64; 3] {
    let alt = alt_deg.to_radians();
    let az_delta = (((az_deg - center_az + 180.0).rem_euclid(360.0)) - 180.0).to_radians();
    [
        alt.cos() * az_delta.sin(),
        alt.sin(),
        alt.cos() * az_delta.cos(),
    ]
}

pub fn prepare(sky: &AnalyticSky) -> Prepared {
    let t = sky.turbidity;
    let theta_sun = (90.0 - sky.sun_alt).to_radians();
    let lum = lum_coeffs(t);
    let cx = cx_coeffs(t);
    let cy = cy_coeffs(t);
    // Normalize so the zenith value equals the analytic zenith (Preetham's
    // value(theta,gamma) = Z * perez(theta,gamma) / perez(0, theta_sun)).
    let denom_lum = perez(1.0, theta_sun, &lum);
    let denom_cx = perez(1.0, theta_sun, &cx);
    let denom_cy = perez(1.0, theta_sun, &cy);
    Prepared {
        sun: dir_from_altaz(sky.sun_alt, sky.sun_az, sky.center_az),
        lum_z: zenith_luminance(t, theta_sun),
        cx_z: zenith_cx(t, theta_sun),
        cy_z: zenith_cy(t, theta_sun),
        lum,
        cx,
        cy,
        denom_lum,
        denom_cx,
        denom_cy,
        blend: sky.blend.clamp(0.0, 1.0),
    }
}

impl Prepared {
    pub fn sample(&self, x_frac: f64, y_frac: f64) -> Oklab {
        // Vertical position is altitude (orthographic up = sin(alt), matching the
        // sun-disc placement); horizontal is a rectilinear azimuth sweep so the
        // whole rectangular frame is sky.
        let up = (1.0 - y_frac).clamp(0.0, 1.0);
        let alt = up.asin();
        let az_delta = (x_frac - 0.5) * FOV_H;
        let ca = alt.cos();
        let view = [ca * az_delta.sin(), up, ca * az_delta.cos()];
        let cos_theta = up;
        let dot = (view[0] * self.sun[0] + view[1] * self.sun[1] + view[2] * self.sun[2])
            .clamp(-1.0, 1.0);
        let gamma = dot.acos();

        let lum = self.lum_z * perez(cos_theta, gamma, &self.lum) / self.denom_lum;
        let cx = self.cx_z * perez(cos_theta, gamma, &self.cx) / self.denom_cx;
        let cy = self.cy_z * perez(cos_theta, gamma, &self.cy) / self.denom_cy;
        xyy_to_oklab(cx, cy, lum.max(0.0))
    }
}

fn xyy_to_oklab(cx: f64, cy: f64, lum: f64) -> Oklab {
    let cy = cy.max(1e-4);
    let big_x = (cx / cy) * lum;
    let big_z = ((1.0 - cx - cy) / cy) * lum;
    // XYZ (D65) -> linear sRGB.
    let r = (3.2404542 * big_x - 1.5371385 * lum - 0.4985314 * big_z).max(0.0);
    let g = (-0.9692660 * big_x + 1.8760108 * lum + 0.0415560 * big_z).max(0.0);
    let b = (0.0556434 * big_x - 0.2040259 * lum + 1.0572252 * big_z).max(0.0);
    // Exposure tone-map: 1 - exp(-c*E) keeps the huge near-sun range in gamut.
    let r = 1.0 - (-r * EXPOSURE).exp();
    let g = 1.0 - (-g * EXPOSURE).exp();
    let b = 1.0 - (-b * EXPOSURE).exp();
    lin_rgb_to_oklab(r, g, b)
}

// Linear sRGB -> Oklab. colorspace::rgb_to_oklab gamma-decodes first; our input
// is already linear, so apply the LMS matrix directly.
fn lin_rgb_to_oklab(lr: f64, lg: f64, lb: f64) -> Oklab {
    let l = 0.412_221_470_8 * lr + 0.536_332_536_3 * lg + 0.051_445_992_9 * lb;
    let m = 0.211_903_498_2 * lr + 0.680_699_545_1 * lg + 0.107_396_956_6 * lb;
    let s = 0.088_302_461_9 * lr + 0.281_718_837_6 * lg + 0.629_978_700_5 * lb;
    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();
    Oklab {
        l: 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        a: 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        b: 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    }
}
