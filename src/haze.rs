//! Horizon haze: how much the air itself whitens the sky toward the ground.
//!
//! One exponential falloff from an onset height, standing in for the aerosol column you look through at low angles. The live weather layer derives its strength from Open-Meteo visibility, so 2 km of fog and 30 km of clear air produce visibly different horizons from the same code path.

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
