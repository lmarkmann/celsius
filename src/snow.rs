//! Falling snow: flake form from the weather, motion from the clock.
//!
//! Snow used to be rain with the colour swapped, which is wrong in the way that matters most: a raindrop is a blurred line because it falls fast enough to smear across an exposure, and a snowflake is not, because it does not. What a flake looks like instead is set by the air it grew in, and the relationship is the Nakaya morphology diagram (Libbrecht 2006): temperature picks the habit and supersaturation picks how branched it is. [`FlakeForm::select`] is that diagram as a lookup, so a -15 C sky snows dendrites and a -5 C sky snows needles.
//!
//! This is the one effect drawn both in stills and in motion, and the reason is worth stating because it is the opposite of lightning's. A still that catches a flash mid-strike is wrong, so lightning lives outside `render()`; a snowfall scene with no flakes in its PNG is not a scene, so snow lives inside it, at `t = 0`. The TUI takes the same function with a real clock. One function, two callers, so the frozen instant a PNG shows is a frame the animation actually passes through.
//!
//! Flakes are held as frame fractions rather than buffer pixels, per `rules/reference-size.md`: `count` is an absolute over a fixed field of view, so a wider terminal shows the same snowfall in bigger flakes rather than more of them. It is the drawn size that follows the buffer, which is the correction that rule spells out for `Stars.count` and that the old area-scaled drop count had backwards.

use std::f64::consts::TAU;

use crate::colorspace::{Oklab, PixelBuffer, oklab_to_rgb, rgb_u8_to_oklab};
use crate::noise::Mt19937;

/// Gas constant for water vapour, J/(kg K).
const R_V: f64 = 461.5;

/// Where the diagram's humidity axis is cut in two. The morphology diagram's y axis runs from ice saturation up to the water-saturation line, which Libbrecht gives as the humidity inside dense winter clouds, so half way up it is the honest split between "low" and "high" for a two-column table.
const BRANCHING_FRACTION: f64 = 0.5;

/// Snowfall rate that anchors `flake_count`, in cm/h. Moderate snow.
const REFERENCE_RATE_CM_H: f64 = 0.5;

/// Flakes in shot at the reference rate, over the frame's fixed field of view.
const REFERENCE_COUNT: f64 = 140.0;

fn flake_color() -> Oklab {
    rgb_u8_to_oklab(242, 244, 248)
}

/// The crystal habits the morphology diagram separates, plus the one it does not: `Aggregate` is several flakes stuck together on the way down, which is what falls when the air is near or above freezing and is why wet snow arrives in clumps rather than crystals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlakeForm {
    Plate,
    SectoredPlate,
    Column,
    Needle,
    Dendrite,
    Aggregate,
}

/// Saturation vapour pressure over water, hPa, from Alduchov and Eskridge (1996).
#[must_use]
pub fn vapour_pressure_water(t_c: f64) -> f64 {
    6.1094 * (17.625 * t_c / (t_c + 243.04)).exp()
}

/// Saturation vapour pressure over ice, hPa. Below freezing it sits under the water curve, and that gap is the whole reason a crystal in a cloud of supercooled droplets grows at all.
#[must_use]
pub fn vapour_pressure_ice(t_c: f64) -> f64 {
    6.1121 * (22.587 * t_c / (t_c + 273.86)).exp()
}

/// Excess water vapour over ice saturation, g/m^3, which is the morphology diagram's y axis.
///
/// Open-Meteo reports relative humidity over *water*, and the diagram is drawn against excess density over *ice*, so the conversion is not optional: at -15 C, air at 90 percent relative humidity is already well supersaturated with respect to ice.
#[must_use]
pub fn supersaturation(t_c: f64, relative_humidity: f64) -> f64 {
    let vapour = relative_humidity / 100.0 * vapour_pressure_water(t_c);
    (vapour - vapour_pressure_ice(t_c)) * 100.0 / (R_V * (t_c + 273.15)) * 1000.0
}

impl FlakeForm {
    /// The morphology diagram, read as a table. Temperature is the primary axis and supersaturation the secondary one, exactly as Libbrecht draws it: "complex, branched crystals appear when the humidity is high, while low humidity levels yield simpler, faceted crystals".
    ///
    /// The bands come from the diagram itself: plates just below freezing, columns and needles a few degrees colder, the large branched crystals at the -15 C peak, and smaller plates and columns colder still.
    #[must_use]
    pub fn select(temperature_c: f64, relative_humidity: f64) -> Self {
        if temperature_c > 0.5 {
            return Self::Aggregate;
        }
        // Against the water-saturation line rather than against a fixed density, because the amount of vapour the air can hold collapses as it cools: one absolute threshold would call every cold sky dry.
        let ceiling = supersaturation(temperature_c, 100.0);
        let branched = ceiling > 0.0
            && supersaturation(temperature_c, relative_humidity) > ceiling * BRANCHING_FRACTION;

        match temperature_c {
            t if t > -3.5 => {
                if branched {
                    Self::SectoredPlate
                } else {
                    Self::Plate
                }
            }
            t if t > -8.0 => {
                if branched {
                    Self::Needle
                } else {
                    Self::Column
                }
            }
            t if t > -12.0 => {
                if branched {
                    Self::Plate
                } else {
                    Self::Column
                }
            }
            t if t > -18.0 => {
                if branched {
                    Self::Dendrite
                } else {
                    Self::Plate
                }
            }
            t if t > -25.0 => {
                if branched {
                    Self::SectoredPlate
                } else {
                    Self::Plate
                }
            }
            _ => {
                if branched {
                    Self::Plate
                } else {
                    Self::Column
                }
            }
        }
    }

    /// Fall speed in frame heights per second.
    ///
    /// The ordering is the measured terminal velocities: a dendrite falls at 0.3 to 0.5 m/s where a wet aggregate manages 1 to 2, because drag scales with area and mass does not. The range between them is deliberately compressed rather than reproduced, since a frame is about ten metres of sky and a true 4x spread puts the fastest flake through it faster than a 30fps terminal can draw it moving.
    fn fall_speed(self) -> f64 {
        match self {
            Self::Dendrite => 1.0 / 7.0,
            Self::SectoredPlate => 1.0 / 6.0,
            Self::Plate => 1.0 / 5.5,
            Self::Column => 1.0 / 4.2,
            Self::Needle => 1.0 / 3.8,
            Self::Aggregate => 1.0 / 3.0,
        }
    }

    /// Side-to-side flutter as (amplitude in frame widths, frequency in Hz).
    ///
    /// A flake flutters because it is a light thing with a large face, so it stalls and slips rather than falling straight. That is a drag-to-mass ordering, not a simulation: a dendrite is nearly all area and wanders visibly, a needle is a splinter and drops almost straight, and an aggregate is heavy enough to swing slowly rather than quickly.
    fn flutter(self) -> (f64, f64) {
        match self {
            Self::Dendrite => (0.035, 0.35),
            Self::SectoredPlate => (0.028, 0.40),
            Self::Plate => (0.020, 0.45),
            Self::Aggregate => (0.012, 0.30),
            Self::Column => (0.008, 0.60),
            Self::Needle => (0.003, 0.70),
        }
    }
}

/// Snow falling over a finished sky.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Snowfall {
    pub form: FlakeForm,
    /// Flakes in shot. An absolute over the frame's fixed field of view, not a density over its pixels.
    pub count: u32,
    pub seed: u64,
    /// Sideways travel in frame widths per second, signed, from the crosswind.
    #[serde(default)]
    pub drift: f64,
    #[serde(default = "default_snow_opacity")]
    pub opacity: f64,
}

fn default_snow_opacity() -> f64 {
    0.75
}

/// How much a flake's rate of fall varies between the nearest and furthest in shot. Parallax stands in for depth: a flake close to the viewer crosses faster and reads brighter, which is what stops a field of identical flakes looking like a moving wallpaper.
const DEPTH_RANGE: (f64, f64) = (0.65, 1.15);

/// Composite `snow` onto `pixels` at `t_seconds` since the fall started.
///
/// `t = 0` is what `render()` passes, so a still is the instant the animation begins rather than a separate arrangement.
pub fn overlay(pixels: &mut PixelBuffer, snow: &Snowfall, t_seconds: f64) {
    if snow.count == 0 || snow.opacity <= 0.0 {
        return;
    }
    let (w, h) = (pixels.width as f64, pixels.height as f64);
    // One pixel at the 104-wide reference, growing with the buffer so a flake keeps roughly the angular size it was drawn at instead of shrinking into the grid.
    let scale = (w / 104.0).max(1.0);
    let radius = scale.round().max(1.0) as i32;

    let mut rng = Mt19937::init_by_array(&[snow.seed as u32]);
    let (amplitude, frequency) = snow.form.flutter();
    let base_fall = snow.form.fall_speed();
    let color = flake_color();

    for _ in 0..snow.count {
        let x0 = rng.next_f64();
        let y0 = rng.next_f64();
        let phase = rng.next_f64() * TAU;
        let depth = DEPTH_RANGE.0 + (DEPTH_RANGE.1 - DEPTH_RANGE.0) * rng.next_f64();

        let sway = amplitude * (TAU * frequency * t_seconds + phase).sin();
        let fx = (x0 + snow.drift * t_seconds + sway).rem_euclid(1.0);
        let fy = (y0 + base_fall * depth * t_seconds).rem_euclid(1.0);

        let alpha = (snow.opacity * depth).clamp(0.0, 1.0);
        draw_flake(
            pixels,
            snow.form,
            (fx * w) as i32,
            (fy * h) as i32,
            radius,
            snow.drift,
            alpha,
            color,
        );
    }
}

/// One flake, as much of its shape as a terminal cell can carry.
///
/// A flake at this size is a few pixels, so the forms are told apart by silhouette rather than by structure: a column is longer than it is wide, a needle leans into the wind, and the branched habits get a halo their faceted counterparts do not. That is the part of the diagram a viewer can actually see.
#[allow(clippy::too_many_arguments)]
fn draw_flake(
    pixels: &mut PixelBuffer,
    form: FlakeForm,
    x: i32,
    y: i32,
    radius: i32,
    drift: f64,
    alpha: f64,
    color: Oklab,
) {
    body(pixels, x, y, radius, alpha, color);
    match form {
        FlakeForm::Plate => {}
        FlakeForm::Column => {
            body(pixels, x, y + radius, radius, alpha, color);
        }
        FlakeForm::Needle => {
            // Leaning with the drift rather than straight down: a needle is the one habit whose long axis is visibly set by the wind carrying it.
            let lean = if drift < 0.0 { -radius } else { radius };
            body(pixels, x + lean, y + radius, radius, alpha * 0.8, color);
            body(pixels, x - lean, y - radius, radius, alpha * 0.8, color);
        }
        FlakeForm::SectoredPlate | FlakeForm::Dendrite => {
            let arm = if form == FlakeForm::Dendrite {
                0.55
            } else {
                0.4
            };
            body(pixels, x + radius, y, radius, alpha * arm, color);
            body(pixels, x - radius, y, radius, alpha * arm, color);
            body(pixels, x, y + radius, radius, alpha * arm, color);
            body(pixels, x, y - radius, radius, alpha * arm, color);
        }
        FlakeForm::Aggregate => {
            body(pixels, x + radius, y, radius, alpha * 0.7, color);
            body(pixels, x, y + radius, radius, alpha * 0.7, color);
            body(pixels, x + radius, y + radius, radius, alpha * 0.5, color);
        }
    }
}

/// One mark of the flake, at whatever size this buffer draws a flake.
///
/// A flake subtends a fixed angle, so its size on screen has to follow the buffer or a wide terminal renders it as a speck and the snow disappears into the grid. This is the same correction `rules/reference-size.md` prescribes for a star's halo, and it is why `count` must *not* follow the buffer: bigger flakes, not more of them.
fn body(pixels: &mut PixelBuffer, x: i32, y: i32, radius: i32, alpha: f64, color: Oklab) {
    let half = radius / 2;
    for dy in -half..=half {
        for dx in -half..=half {
            blend(pixels, x + dx, y + dy, alpha, color);
        }
    }
}

fn blend(pixels: &mut PixelBuffer, x: i32, y: i32, alpha: f64, color: Oklab) {
    if alpha <= 0.0
        || !(0..pixels.width as i32).contains(&x)
        || !(0..pixels.height as i32).contains(&y)
    {
        return;
    }
    let idx = (y as usize) * pixels.width + (x as usize);
    let base = pixels.pixels[idx];
    let lab = rgb_u8_to_oklab(base.r, base.g, base.b);
    let inv = 1.0 - alpha;
    pixels.pixels[idx] = oklab_to_rgb(Oklab::new(
        lab.l * inv + color.l * alpha,
        lab.a * inv + color.a * alpha,
        lab.b * inv + color.b * alpha,
    ));
}

/// Flakes in shot for a snowfall rate in cm/h.
///
/// The exponent compresses the tail so a blizzard reads as dense rather than as a white screen: at four times the reference rate the count not quite triples. The floor keeps light snow visible at all, and the ceiling is where more flakes stop adding anything a 104x50 frame can resolve.
#[must_use]
pub fn flake_count(snowfall_cm_h: f64) -> u32 {
    let ratio = (snowfall_cm_h.max(0.0) / REFERENCE_RATE_CM_H).powf(0.75);
    (REFERENCE_COUNT * ratio).round().clamp(24.0, 420.0) as u32
}
