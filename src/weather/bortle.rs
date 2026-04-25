use crate::colorspace::rgb_u8_to_oklab;
use crate::gradient::Gradient;

const GLOW_RGB: (u8, u8, u8) = (90, 60, 35);

pub fn count_factor(bortle: u8) -> f64 {
    let b = bortle.clamp(1, 9) as f64;
    10f64.powf(-0.3 * (b - 1.0))
}

pub fn scale_count(count: u32, bortle: Option<u8>) -> u32 {
    let Some(b) = bortle else {
        return count;
    };
    ((count as f64) * count_factor(b)).round().max(1.0) as u32
}

pub fn glow_strength(bortle: u8) -> f64 {
    if bortle <= 2 {
        return 0.0;
    }
    let level = (bortle.clamp(1, 9) - 2) as f64;
    let max_level = 7f64.powf(1.4);
    (level.powf(1.4) / max_level) * 0.8
}

pub fn night_factor(sun_alt_deg: f64) -> f64 {
    (-sun_alt_deg / 12.0).clamp(0.0, 1.0)
}

pub fn apply_glow(gradient: &mut Gradient, bortle: Option<u8>, sun_alt_deg: f64) {
    let Some(b) = bortle else { return };
    let strength = glow_strength(b) * night_factor(sun_alt_deg);
    if strength <= 0.0 {
        return;
    }
    let target = rgb_u8_to_oklab(GLOW_RGB.0, GLOW_RGB.1, GLOW_RGB.2);
    gradient.tint_toward_horizon(target, strength);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_factor_endpoints() {
        assert!((count_factor(1) - 1.0).abs() < 1e-12);
        assert!(count_factor(9) > 0.0);
        assert!(count_factor(9) < 0.01);
    }

    #[test]
    fn count_factor_monotonic() {
        for b in 1..9 {
            assert!(count_factor(b) > count_factor(b + 1));
        }
    }

    #[test]
    fn scale_count_none_passthrough() {
        assert_eq!(scale_count(380, None), 380);
    }

    #[test]
    fn scale_count_floor_is_one() {
        assert_eq!(scale_count(380, Some(9)), 2.max(scale_count(380, Some(9))));
        assert!(scale_count(380, Some(9)) >= 1);
    }

    #[test]
    fn scale_count_city_band() {
        let n = scale_count(280, Some(7));
        assert!((3..=8).contains(&n), "B7 stars should be 3..=8, got {n}");
    }

    #[test]
    fn glow_strength_matches_targets() {
        assert_eq!(glow_strength(1), 0.0);
        assert_eq!(glow_strength(2), 0.0);
        assert!(glow_strength(3) > 0.0 && glow_strength(3) < 0.1);
        let g9 = glow_strength(9);
        assert!((0.7..=0.85).contains(&g9), "B9 strength out of range: {g9}");
    }

    #[test]
    fn night_factor_gating() {
        assert_eq!(night_factor(10.0), 0.0);
        assert_eq!(night_factor(0.0), 0.0);
        assert!(night_factor(-6.0) > 0.4 && night_factor(-6.0) < 0.6);
        assert_eq!(night_factor(-12.0), 1.0);
        assert_eq!(night_factor(-30.0), 1.0);
    }

    #[test]
    fn apply_glow_noop_when_sun_up() {
        let mut g = Gradient::from_rgb_stops(&[(0.0, [10, 10, 30]), (1.0, [80, 60, 90])]);
        let before = g.sample(1.0);
        apply_glow(&mut g, Some(9), 5.0);
        let after = g.sample(1.0);
        assert_eq!(before, after);
    }

    #[test]
    fn apply_glow_noop_when_bortle_none() {
        let mut g = Gradient::from_rgb_stops(&[(0.0, [10, 10, 30]), (1.0, [80, 60, 90])]);
        let before = g.sample(1.0);
        apply_glow(&mut g, None, -20.0);
        let after = g.sample(1.0);
        assert_eq!(before, after);
    }

    #[test]
    fn apply_glow_lifts_horizon_at_night() {
        let mut g = Gradient::from_rgb_stops(&[(0.0, [5, 5, 15]), (1.0, [10, 10, 30])]);
        let before = g.sample(1.0);
        apply_glow(&mut g, Some(7), -20.0);
        let after = g.sample(1.0);
        assert!(
            after.l > before.l,
            "horizon L should rise: {before:?} -> {after:?}"
        );
    }
}
