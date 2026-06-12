use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::analytic_sky::AnalyticSky;
use crate::gradient::Gradient;
use crate::lightning::Lightning;

#[derive(Debug, Error)]
pub enum SceneError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("scene {path} has no file stem")]
    NoStem { path: PathBuf },
    #[error("scene {path} declares an empty gradient (needs at least one stop)")]
    EmptyGradient { path: PathBuf },
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sun {
    pub x_frac: f64,
    pub y_frac: f64,
    pub radius: f64,
    #[serde(default = "default_true")]
    pub visible: bool,
}

/// Cloud morphology class. Drives noise detail, edge sharpness, and the
/// lit/shadow colors so a thin cirrus veil, a flat stratus deck, and a dark
/// storm tower no longer share one texture. `Generic` reproduces the
/// pre-morphology render exactly, which keeps vendored scenes and the oracle
/// goldens unchanged.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudKind {
    #[default]
    Generic,
    Cirrus,
    Altocumulus,
    Stratus,
    Cumulus,
    Cumulonimbus,
}

pub struct CloudMorphology {
    pub octaves: u32,
    pub edge: f64,
    pub shadow_rgb: [u8; 3],
    pub lit_rgb: [u8; 3],
}

impl CloudKind {
    pub fn morphology(self) -> CloudMorphology {
        match self {
            CloudKind::Generic => CloudMorphology {
                octaves: 4,
                edge: 3.6,
                shadow_rgb: [78, 74, 108],
                lit_rgb: [252, 215, 172],
            },
            CloudKind::Cirrus => CloudMorphology {
                octaves: 5,
                edge: 2.0,
                shadow_rgb: [150, 152, 168],
                lit_rgb: [244, 240, 235],
            },
            CloudKind::Altocumulus => CloudMorphology {
                octaves: 4,
                edge: 4.5,
                shadow_rgb: [120, 120, 138],
                lit_rgb: [240, 232, 222],
            },
            CloudKind::Stratus => CloudMorphology {
                octaves: 2,
                edge: 2.6,
                shadow_rgb: [128, 128, 134],
                lit_rgb: [196, 196, 198],
            },
            CloudKind::Cumulus => CloudMorphology {
                octaves: 4,
                edge: 6.0,
                shadow_rgb: [120, 118, 132],
                lit_rgb: [252, 220, 178],
            },
            CloudKind::Cumulonimbus => CloudMorphology {
                octaves: 4,
                edge: 5.0,
                shadow_rgb: [40, 40, 50],
                lit_rgb: [150, 150, 162],
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct CloudLayer {
    pub cover: f64,
    pub altitude_t: f64,
    pub altitude_sigma: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub threshold: f64,
    pub seed: u64,
    #[serde(default)]
    pub kind: CloudKind,
    /// 0 = full noise texture, 1 = featureless flat deck. Lets a high-cover
    /// stratus layer render as a solid lid instead of separate blobs.
    #[serde(default)]
    pub flatten: f64,
    #[serde(default = "default_offset_x")]
    pub offset_x: f64,
    #[serde(default = "default_offset_y")]
    pub offset_y: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Haze {
    pub rgb: [u8; 3],
    pub onset_t: f64,
    pub strength: f64,
    #[serde(default = "default_haze_exponent")]
    pub exponent: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Stars {
    pub count: u32,
    pub seed: u64,
    #[serde(default = "default_star_brightness")]
    pub brightness: f64,
    #[serde(default = "default_star_threshold")]
    pub sky_threshold: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Moon {
    pub x_frac: f64,
    pub y_frac: f64,
    pub radius: f64,
    pub phase: f64,
    #[serde(default = "default_true")]
    pub visible: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Chrome {
    pub header_left: String,
    pub header_right: String,
    pub footer: String,
    pub keys: String,
    /// One-line ASCII summary for the `--plain` surface, built from structured
    /// weather data. Empty for scene files, which have no structured weather;
    /// `write_plain` falls back to the decorative chrome there.
    #[serde(default)]
    pub status: String,
}

/// Two kinds only, enforced at parse: a typo like `kind = "Rain"` used to
/// slip through the old stringly field and silently render as snow.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrecipKind {
    Rain,
    Snow,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Precipitation {
    pub kind: PrecipKind,
    pub intensity: f64,
    pub angle_deg: f64,
    pub seed: u64,
    #[serde(default = "default_streak_len")]
    pub streak_len: u32,
    #[serde(default = "default_precip_opacity")]
    pub opacity: f64,
}

/// Warm light concentrated on the horizon at the sun's bearing. The vertical
/// gradient is azimuth-blind, so this is what puts sunrise/sunset color on the
/// side the sun actually is rather than symmetrically across the frame.
#[derive(Clone, Debug)]
pub struct HorizonGlow {
    pub x_frac: f64,
    pub rgb: [u8; 3],
    pub strength: f64,
}

#[derive(Clone, Debug)]
pub struct SkyState {
    pub name: String,
    pub gradient: Gradient,
    pub sun: Sun,
    pub clouds: Vec<CloudLayer>,
    pub chrome: Chrome,
    pub haze: Option<Haze>,
    pub stars: Option<Stars>,
    pub moon: Option<Moon>,
    pub precipitation: Option<Precipitation>,
    pub lightning: Option<Lightning>,
    pub horizon_glow: Option<HorizonGlow>,
    /// Prototype: when present, render computes the sky background from the
    /// Preetham analytic model instead of sampling `gradient`. Set only for the
    /// live-weather daytime path; scenes leave it None.
    pub analytic: Option<AnalyticSky>,
    pub wind_speed_kmh: f64,
}

#[derive(Deserialize)]
struct SceneToml {
    gradient: GradientToml,
    sun: Sun,
    #[serde(default)]
    clouds: CloudsToml,
    chrome: Chrome,
    haze: Option<Haze>,
    stars: Option<Stars>,
    moon: Option<Moon>,
    precipitation: Option<Precipitation>,
}

#[derive(Deserialize)]
struct GradientToml {
    stops: Vec<StopToml>,
}

#[derive(Deserialize)]
struct StopToml {
    t: f64,
    rgb: [u8; 3],
}

#[derive(Default, Deserialize)]
struct CloudsToml {
    #[serde(default)]
    layers: Vec<CloudLayer>,
}

pub fn load_scene(path: impl AsRef<Path>) -> Result<SkyState, SceneError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|e| SceneError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;
    let raw: SceneToml = toml::from_str(&text).map_err(|e| SceneError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;

    // Gradient::sample panics on zero stops; reject it here so a bad scene
    // file fails with a SceneError instead of a render-time panic.
    if raw.gradient.stops.is_empty() {
        return Err(SceneError::EmptyGradient {
            path: path.to_path_buf(),
        });
    }

    let stops: Vec<(f64, [u8; 3])> = raw
        .gradient
        .stops
        .into_iter()
        .map(|s| (s.t, s.rgb))
        .collect();
    let gradient = Gradient::from_rgb_stops(&stops);

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SceneError::NoStem {
            path: path.to_path_buf(),
        })?
        .to_string();

    Ok(SkyState {
        name,
        gradient,
        sun: raw.sun,
        clouds: raw.clouds.layers,
        chrome: raw.chrome,
        haze: raw.haze,
        stars: raw.stars,
        moon: raw.moon,
        precipitation: raw.precipitation,
        lightning: None,
        horizon_glow: None,
        analytic: None,
        wind_speed_kmh: 0.0,
    })
}

fn default_true() -> bool {
    true
}
fn default_offset_x() -> f64 {
    0.4
}
fn default_offset_y() -> f64 {
    1.3
}
fn default_haze_exponent() -> f64 {
    1.8
}
fn default_star_brightness() -> f64 {
    1.0
}
fn default_star_threshold() -> f64 {
    0.35
}
fn default_streak_len() -> u32 {
    4
}
fn default_precip_opacity() -> f64 {
    0.38
}
