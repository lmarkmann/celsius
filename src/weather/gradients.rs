//! Lifted sky gradients from the skyterm-lab reference scenes.
//!
//! These are the only gradients celsius is allowed to use. New palettes go
//! into the lab first, get authored against locked goldens, and then land
//! here verbatim. The five below correspond one-for-one to the lab files
//! in `../skyterm-lab/scenes/`.
//!
//! The synthesis layer picks one of these based on sun altitude and total
//! cloud cover. Gaps (dawn, heavy cloudy midday, deep starless night) are
//! intentional and will look imperfect until the lab produces fillers.

use crate::gradient::Gradient;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Palette {
    /// High noon, deep saturated blue, pale horizon.
    Day,
    /// Sun low and warm, whole sky pulled through pink and orange.
    GoldenHour,
    /// Sun just below horizon, cool upper sky with a warm residue below.
    BlueHour,
    /// Astronomical night, near-black with a faint blue cast.
    Night,
    /// Full overcast, gray greenish zenith bleeding to a pale horizon.
    Overcast,
}

const DAY: &[(f64, [u8; 3])] = &[
    (0.00, [22, 58, 132]),
    (0.18, [34, 82, 162]),
    (0.38, [62, 118, 192]),
    (0.58, [114, 166, 216]),
    (0.78, [176, 204, 228]),
    (0.90, [212, 224, 232]),
    (1.00, [224, 230, 232]),
];

const GOLDEN_HOUR: &[(f64, [u8; 3])] = &[
    (0.00, [28, 46, 102]),
    (0.22, [58, 66, 128]),
    (0.42, [132, 88, 144]),
    (0.60, [208, 118, 135]),
    (0.75, [242, 160, 105]),
    (0.85, [252, 170, 92]),
    (0.92, [238, 138, 78]),
    (0.97, [188, 95, 68]),
    (1.00, [118, 62, 55]),
];

const BLUE_HOUR: &[(f64, [u8; 3])] = &[
    (0.00, [22, 28, 82]),
    (0.18, [36, 40, 106]),
    (0.36, [54, 52, 120]),
    (0.52, [74, 64, 130]),
    (0.66, [103, 76, 138]),
    (0.78, [146, 94, 138]),
    (0.86, [188, 120, 116]),
    (0.92, [208, 138, 106]),
    (0.97, [162, 98, 88]),
    (1.00, [108, 66, 70]),
];

const NIGHT: &[(f64, [u8; 3])] = &[
    (0.00, [5, 7, 20]),
    (0.25, [7, 10, 26]),
    (0.50, [9, 12, 30]),
    (0.70, [12, 14, 34]),
    (0.85, [16, 17, 38]),
    (0.94, [20, 20, 42]),
    (1.00, [15, 16, 32]),
];

const OVERCAST: &[(f64, [u8; 3])] = &[
    (0.00, [36, 42, 48]),
    (0.18, [62, 66, 72]),
    (0.42, [96, 94, 90]),
    (0.62, [134, 128, 118]),
    (0.80, [162, 155, 142]),
    (1.00, [190, 182, 168]),
];

pub(super) fn gradient_for(palette: Palette) -> Gradient {
    let stops: &[(f64, [u8; 3])] = match palette {
        Palette::Day => DAY,
        Palette::GoldenHour => GOLDEN_HOUR,
        Palette::BlueHour => BLUE_HOUR,
        Palette::Night => NIGHT,
        Palette::Overcast => OVERCAST,
    };
    Gradient::from_rgb_stops(stops)
}

/// Pick the closest lab palette for the given conditions.
///
/// `sun_alt_deg` is the true solar altitude, `total_cover` is the sum of the
/// three Open-Meteo cloud bands divided by 300 (0..1). Overcast wins over
/// daylight when the sky is mostly opaque and the sun isn't so high that
/// the overcast palette would look silly; at steep sun angles the day
/// palette with heavy cloud layers still looks better than the flat gray.
pub(super) fn select_palette(sun_alt_deg: f64, total_cover: f64) -> Palette {
    if total_cover >= 0.80 && sun_alt_deg < 55.0 && sun_alt_deg > -3.0 {
        return Palette::Overcast;
    }
    if sun_alt_deg >= 10.0 {
        Palette::Day
    } else if sun_alt_deg >= -3.0 {
        Palette::GoldenHour
    } else if sun_alt_deg >= -12.0 {
        Palette::BlueHour
    } else {
        Palette::Night
    }
}
