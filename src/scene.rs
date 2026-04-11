use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::gradient::Gradient;

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
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sun {
    pub x_frac: f64,
    pub y_frac: f64,
    pub radius: f64,
    #[serde(default = "default_true")]
    pub visible: bool,
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
}

#[derive(Clone, Debug, Deserialize)]
pub struct Precipitation {
    pub kind: String,
    pub intensity: f64,
    pub angle_deg: f64,
    pub seed: u64,
    #[serde(default = "default_streak_len")]
    pub streak_len: u32,
    #[serde(default = "default_precip_opacity")]
    pub opacity: f64,
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
