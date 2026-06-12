use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub location: Option<LocationPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bortle: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LocationPref {
    Name { name: String },
    Coords { lat: f64, lon: f64 },
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("celsius/config.toml")
}

pub fn load() -> Config {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    match toml::from_str(&text) {
        Ok(cfg) => cfg,
        Err(e) => {
            // Falling back to defaults means the next save overwrites the
            // user's file, so the reset must at least be visible.
            eprintln!("celsius: ignoring malformed config {}: {e}", path.display());
            Config::default()
        }
    }
}

pub fn save(cfg: &Config) -> Result<(), ConfigError> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(cfg)?)?;
    Ok(())
}
