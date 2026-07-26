//! A vertical sky gradient: colour stops sampled by height.
//!
//! Stops are given at `t` values where 0 is the zenith and 1 the horizon, and [`Gradient::sample`] interpolates between the bracketing pair in Oklab. Stops are stored pre-converted, so the per-row sample in the render loop is a lerp and not a colour-space conversion.
//!
//! Sampling also serves a second purpose: the star builder reads the gradient's lightness at a star's height to decide whether it would be washed out, which is why a dawn scene keeps only its brightest stars without anyone hand-placing them.

use crate::colorspace::{Oklab, lerp_oklab, rgb_u8_to_oklab};

#[derive(Copy, Clone, Debug)]
pub struct Stop {
    pub t: f64,
    pub color: Oklab,
}

#[derive(Clone, Debug)]
pub struct Gradient {
    stops: Vec<Stop>,
}

impl Gradient {
    pub fn from_oklab_stops(stops: Vec<(f64, Oklab)>) -> Self {
        Self {
            stops: stops
                .into_iter()
                .map(|(t, color)| Stop { t, color })
                .collect(),
        }
    }

    pub fn from_rgb_stops(stops: &[(f64, [u8; 3])]) -> Self {
        Self {
            stops: stops
                .iter()
                .map(|(t, rgb)| Stop {
                    t: *t,
                    color: rgb_u8_to_oklab(rgb[0], rgb[1], rgb[2]),
                })
                .collect(),
        }
    }

    pub fn sample(&self, t: f64) -> Oklab {
        let t = t.clamp(0.0, 1.0);
        let stops = &self.stops;
        for i in 0..stops.len() - 1 {
            let s0 = stops[i];
            let s1 = stops[i + 1];
            if t <= s1.t {
                let span = s1.t - s0.t;
                let k = if span > 0.0 { (t - s0.t) / span } else { 0.0 };
                return lerp_oklab(s0.color, s1.color, k);
            }
        }
        stops.last().copied().unwrap().color
    }

    /// Cross-fade toward another gradient in Oklab. Samples both at the union of their stop positions so neither gradient's keyframes are lost, which keeps a continuous sky transition smooth as `k` sweeps 0 -> 1.
    pub fn blend(&self, other: &Gradient, k: f64) -> Gradient {
        let k = k.clamp(0.0, 1.0);
        if k == 0.0 {
            return self.clone();
        }
        if k == 1.0 {
            return other.clone();
        }
        let mut ts: Vec<f64> = self
            .stops
            .iter()
            .chain(other.stops.iter())
            .map(|s| s.t)
            .collect();
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let stops = ts
            .into_iter()
            .map(|t| (t, lerp_oklab(self.sample(t), other.sample(t), k)))
            .collect();
        Gradient::from_oklab_stops(stops)
    }

    pub fn tint_toward_horizon(&mut self, target: Oklab, strength: f64) {
        let strength = strength.clamp(0.0, 1.0);
        if strength == 0.0 {
            return;
        }
        for stop in &mut self.stops {
            let weight = stop.t * strength;
            stop.color = lerp_oklab(stop.color, target, weight);
        }
    }
}

#[cfg(test)]
mod arithmetic_tests {
    use super::*;

    fn ramp() -> Gradient {
        Gradient::from_rgb_stops(&[(0.0, [0, 0, 0]), (1.0, [255, 255, 255])])
    }

    /// Nine mutants survived here against 94.5 percent line coverage: the stops were being sampled but never checked against a known answer, so the interpolation arithmetic could change freely.
    #[test]
    fn sample_interpolates_between_the_bracketing_stops() {
        let g = ramp();
        let bottom = g.sample(0.0).l;
        let middle = g.sample(0.5).l;
        let top = g.sample(1.0).l;
        assert!(bottom < middle && middle < top, "sample must be monotonic");
        assert!(
            (middle - (bottom + top) / 2.0).abs() < 0.02,
            "the midpoint must land between its neighbours, got {middle}"
        );
    }

    #[test]
    fn sample_clamps_outside_the_stop_range() {
        let g = ramp();
        assert!((g.sample(-1.0).l - g.sample(0.0).l).abs() < 1e-12);
        assert!((g.sample(2.0).l - g.sample(1.0).l).abs() < 1e-12);
    }

    /// `blend` is what crossfades one palette into the next as the sun moves, and it had no test at all: seven mutants lived in it and `tint_toward_horizon`.
    #[test]
    fn blend_crosses_from_one_gradient_to_the_other() {
        let dark = Gradient::from_rgb_stops(&[(0.0, [0, 0, 0]), (1.0, [0, 0, 0])]);
        let light = Gradient::from_rgb_stops(&[(0.0, [255, 255, 255]), (1.0, [255, 255, 255])]);

        assert!(
            dark.blend(&light, 0.0).sample(0.5).l < 0.01,
            "k=0 is the left gradient"
        );
        assert!(
            dark.blend(&light, 1.0).sample(0.5).l > 0.99,
            "k=1 is the right one"
        );

        let half = dark.blend(&light, 0.5).sample(0.5).l;
        let quarter = dark.blend(&light, 0.25).sample(0.5).l;
        assert!(
            quarter < half && half < 1.0,
            "the crossfade must be monotonic in k, got {quarter} then {half}"
        );
    }

    /// The light-pollution glow leans a palette toward a colour, and has to lean it harder toward the horizon than the top, or a city sky glows from the wrong end.
    #[test]
    fn tint_toward_horizon_weights_by_height() {
        let mut g = Gradient::from_rgb_stops(&[(0.0, [0, 0, 0]), (1.0, [0, 0, 0])]);
        let before_top = g.sample(0.0).l;
        g.tint_toward_horizon(rgb_u8_to_oklab(255, 255, 255), 1.0);
        assert!(
            (g.sample(0.0).l - before_top).abs() < 1e-9,
            "the top of the sky must be untouched"
        );
        assert!(
            g.sample(1.0).l > 0.5,
            "the horizon must take the full tint, got {}",
            g.sample(1.0).l
        );

        let mut none = Gradient::from_rgb_stops(&[(0.0, [0, 0, 0]), (1.0, [0, 0, 0])]);
        none.tint_toward_horizon(rgb_u8_to_oklab(255, 255, 255), 0.0);
        assert!(
            none.sample(1.0).l < 0.01,
            "zero strength must change nothing"
        );
    }

    #[test]
    fn a_single_stop_is_that_colour_everywhere() {
        let g = Gradient::from_rgb_stops(&[(0.0, [120, 60, 30])]);
        let a = g.sample(0.0);
        let b = g.sample(1.0);
        assert!((a.l - b.l).abs() < 1e-12 && (a.a - b.a).abs() < 1e-12);
    }
}
