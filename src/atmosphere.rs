//! What the air is doing, held once so every sky model reads the same state instead of each deriving its own.
//!
//! Turbidity and horizon haze are one aerosol loading seen two ways, and both were computed from the same reported visibility in two places on two unrelated curves: turbidity ramps across 2 to 24 km while haze switches off entirely above 12 km. At 13 km the analytic sky is told the air is fairly hazy and the haze layer is told there is none. Both curves are unchanged here; what changes is that they now read one value and that disagreement sits in one type rather than split across two functions.
//!
//! `#[non_exhaustive]` is the reason this type exists at all, rather than a precaution. Turbidity lived on `AnalyticSky`, which is the Preetham parameter block and does not survive the move to Hosek-Wilkie; ground albedo is the next axis (Hosek-Wilkie fits it, Preetham has no place to put it) and aerosol species the one after (no model in the Perez family carries it). Each of those would otherwise break every caller writing a struct literal.

/// The atmospheric state one sky is rendered through.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Atmosphere {
    /// Horizontal visibility in metres, when a forecast reported one. `None` for a sky whose turbidity was chosen directly, which is every sweep the lab renders.
    pub visibility_m: Option<f64>,
    /// Preetham turbidity: total optical thickness over its molecular part, so 1 is a pure Rayleigh atmosphere and 2 to 3 is a clear day.
    pub turbidity: f64,
}

impl Atmosphere {
    /// The live path, where everything follows from what the forecast reported.
    #[must_use]
    pub fn from_visibility(visibility_m: Option<f64>) -> Self {
        Self {
            visibility_m,
            turbidity: turbidity_from_visibility(visibility_m),
        }
    }

    /// The authored path, for sweeping a model across turbidities that no visibility reading has to justify.
    #[must_use]
    pub fn from_turbidity(turbidity: f64) -> Self {
        Self {
            visibility_m: None,
            turbidity,
        }
    }
}

// Open-Meteo visibility tops out near 24 km on clear days and falls to a few km in haze/fog. Map clear -> low turbidity (~2), hazy -> high (~9).
/// Map visibility in metres to the turbidity range used by the analytic sky.
#[must_use]
pub fn turbidity_from_visibility(vis_m: Option<f64>) -> f64 {
    let vis_km = vis_m.unwrap_or(24_000.0) / 1000.0;
    (2.0 + (24.0 - vis_km.clamp(2.0, 24.0)) / 22.0 * 7.0).clamp(2.0, 9.0)
}
