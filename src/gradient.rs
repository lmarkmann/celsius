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

    /// Cross-fade toward another gradient in Oklab. Samples both at the union
    /// of their stop positions so neither gradient's keyframes are lost, which
    /// keeps a continuous sky transition smooth as `k` sweeps 0 -> 1.
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
