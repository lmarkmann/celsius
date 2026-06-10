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

// Sun-altitude anchors for the clear-sky gradient. Between two anchors the
// bracketing palettes are blended in Oklab, so the sky shifts continuously
// through twilight instead of snapping when an altitude threshold is crossed.
const CLEAR_ANCHORS: [(f64, Palette); 5] = [
    (-12.0, Palette::Night),
    (-7.5, Palette::BlueHour),
    (0.0, Palette::Dawn),
    (6.5, Palette::GoldenHour),
    (12.0, Palette::Day),
];

fn clear_sky_gradient(sun_alt_deg: f64) -> Gradient {
    let (lo_alt, lo_pal) = CLEAR_ANCHORS[0];
    if sun_alt_deg <= lo_alt {
        return gradient_for(lo_pal);
    }
    let (hi_alt, hi_pal) = CLEAR_ANCHORS[CLEAR_ANCHORS.len() - 1];
    if sun_alt_deg >= hi_alt {
        return gradient_for(hi_pal);
    }
    for pair in CLEAR_ANCHORS.windows(2) {
        let (a_alt, a_pal) = pair[0];
        let (b_alt, b_pal) = pair[1];
        if sun_alt_deg <= b_alt {
            let k = (sun_alt_deg - a_alt) / (b_alt - a_alt);
            return gradient_for(a_pal).blend(&gradient_for(b_pal), k);
        }
    }
    gradient_for(hi_pal)
}

/// Continuous sky gradient: clear-sky color by sun altitude, faded toward the
/// cloudy-day then overcast looks as total cover rises. The cloud influence is
/// scaled by a daylight factor so a cloudy night stays a night sky instead of
/// turning daytime-gray.
pub(super) fn sky_gradient(sun_alt_deg: f64, total_cover: f64) -> Gradient {
    let clear = clear_sky_gradient(sun_alt_deg);
    let daylight = smoothstep01((sun_alt_deg + 3.0) / 9.0);
    if daylight <= 0.0 || total_cover <= 0.0 {
        return clear;
    }
    let cover = total_cover.clamp(0.0, 1.0);
    let clouded = if cover <= 0.5 {
        clear.blend(&gradient_for(Palette::CloudyDay), cover / 0.5)
    } else {
        gradient_for(Palette::CloudyDay)
            .blend(&gradient_for(Palette::Overcast), (cover - 0.5) / 0.5)
    };
    clear.blend(&clouded, daylight)
}

fn smoothstep01(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sample a gradient at nine heights so a palette swap anywhere in the
    // vertical profile, not just one stop, counts as a jump.
    fn profile(g: &Gradient) -> [[f64; 3]; 9] {
        let mut prof = [[0.0; 3]; 9];
        for (i, slot) in prof.iter_mut().enumerate() {
            let c = g.sample(i as f64 / 8.0);
            *slot = [c.l, c.a, c.b];
        }
        prof
    }

    fn max_step(a: &[[f64; 3]; 9], b: &[[f64; 3]; 9]) -> f64 {
        let mut worst = 0.0_f64;
        for (pa, pb) in a.iter().zip(b.iter()) {
            for k in 0..3 {
                worst = worst.max((pa[k] - pb[k]).abs());
            }
        }
        worst
    }

    #[test]
    fn clear_sky_has_no_altitude_jump() {
        // Sweep through every old threshold (-12, -3, 3, 10) in fine steps.
        // A step-function palette swap would show up as a large profile jump.
        let mut prev = profile(&sky_gradient(-15.0, 0.0));
        let mut worst = 0.0_f64;
        let mut alt = -15.0;
        while alt <= 30.0 {
            let cur = profile(&sky_gradient(alt, 0.0));
            worst = worst.max(max_step(&prev, &cur));
            prev = cur;
            alt += 0.5;
        }
        // A 0.5deg step rides the steepest anchor ramp (Night->BlueHour) at
        // ~0.05; the old step function jumped by the full palette delta (>0.3)
        // at a single threshold, so this bound separates ramp from jump.
        assert!(
            worst < 0.08,
            "largest 0.5deg color step was {worst}, expected a smooth sweep"
        );
    }

    #[test]
    fn cover_fade_has_no_jump() {
        // At full daylight, sweeping cover 0 -> 1 must stay continuous across the
        // 0.5 cloudy-day / overcast hinge.
        let mut prev = profile(&sky_gradient(40.0, 0.0));
        let mut worst = 0.0_f64;
        let mut cover = 0.0;
        while cover <= 1.0 {
            let cur = profile(&sky_gradient(40.0, cover));
            worst = worst.max(max_step(&prev, &cur));
            prev = cur;
            cover += 0.02;
        }
        assert!(
            worst < 0.05,
            "largest cover step was {worst}, expected a smooth fade"
        );
    }
}
