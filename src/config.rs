use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return Config::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

pub fn save(cfg: &Config) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(cfg)?)?;
    Ok(())
}
