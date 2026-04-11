use crate::gradient::Gradient;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Palette {
    Day,
    Dawn,
    GoldenHour,
    BlueHour,
    Night,
    CloudyDay,
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

const DAWN: &[(f64, [u8; 3])] = &[
    (0.00, [22, 30, 72]),
    (0.18, [36, 44, 94]),
    (0.36, [58, 58, 112]),
    (0.52, [88, 72, 122]),
    (0.66, [138, 96, 120]),
    (0.78, [186, 122, 104]),
    (0.88, [218, 152, 100]),
    (0.94, [230, 168, 98]),
    (0.98, [210, 140, 84]),
    (1.00, [172, 106, 72]),
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

const CLOUDY_DAY: &[(f64, [u8; 3])] = &[
    (0.00, [168, 172, 180]),
    (0.30, [172, 172, 174]),
    (0.60, [172, 170, 165]),
    (0.82, [174, 170, 160]),
    (1.00, [176, 170, 158]),
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
        Palette::Dawn => DAWN,
        Palette::GoldenHour => GOLDEN_HOUR,
        Palette::BlueHour => BLUE_HOUR,
        Palette::Night => NIGHT,
        Palette::CloudyDay => CLOUDY_DAY,
        Palette::Overcast => OVERCAST,
    };
    Gradient::from_rgb_stops(stops)
}

pub(super) fn select_palette(sun_alt_deg: f64, total_cover: f64) -> Palette {
    if total_cover >= 0.80 {
        if (-3.0..55.0).contains(&sun_alt_deg) {
            return Palette::Overcast;
        }
    } else if total_cover >= 0.50 && (3.0..55.0).contains(&sun_alt_deg) {
        return Palette::CloudyDay;
    }
    if sun_alt_deg >= 10.0 {
        Palette::Day
    } else if sun_alt_deg >= 3.0 {
        Palette::GoldenHour
    } else if sun_alt_deg >= -3.0 {
        Palette::Dawn
    } else if sun_alt_deg >= -12.0 {
        Palette::BlueHour
    } else {
        Palette::Night
    }
}
