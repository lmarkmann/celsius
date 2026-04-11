//! Haze and visibility modulation.
//!
//! Atmospheric scattering degrades contrast and pushes colors toward a
//! diffuse fog tone as the viewing angle approaches the horizon. The blend
//! factor is a power curve in the vertical coordinate tv (0 zenith, 1
//! horizon) with an onset altitude. An exponent greater than one
//! concentrates the effect near the horizon.

use crate::scene::Haze;

pub fn blend_factor(tv: f64, haze: &Haze) -> f64 {
    if tv <= haze.onset_t {
        return 0.0;
    }
    let span = 1.0 - haze.onset_t;
    let k = if span > 0.0 {
        (tv - haze.onset_t) / span
    } else {
        1.0
    };
    (haze.strength * k.powf(haze.exponent)).min(1.0)
}
