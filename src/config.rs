use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config serialize: {0}")]
    Serialize(#[from] basic_toml::Error),
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    // bortle is a scalar and must serialize before `location`, which becomes a
    // [location] table; TOML requires every bare key before the first table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bortle: Option<u8>,
    pub location: Option<LocationPref>,
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
    match basic_toml::from_str(&text) {
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
    std::fs::write(&path, basic_toml::to_string(cfg)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(cfg: &Config) -> Config {
        let text = basic_toml::to_string(cfg).expect("serialize");
        basic_toml::from_str(&text).expect("deserialize")
    }

    #[test]
    fn name_location_roundtrips() {
        let cfg = Config {
            location: Some(LocationPref::Name {
                name: "Hamburg".into(),
            }),
            bortle: None,
        };
        let back = roundtrip(&cfg);
        assert!(matches!(
            back.location,
            Some(LocationPref::Name { name }) if name == "Hamburg"
        ));
        assert_eq!(back.bortle, None);
    }

    #[test]
    fn coords_location_roundtrips() {
        let cfg = Config {
            location: Some(LocationPref::Coords {
                lat: 53.55,
                lon: 9.99,
            }),
            bortle: None,
        };
        let back = roundtrip(&cfg);
        match back.location {
            Some(LocationPref::Coords { lat, lon }) => {
                assert_eq!(lat, 53.55);
                assert_eq!(lon, 9.99);
            }
            other => panic!("untagged enum picked wrong variant: {other:?}"),
        }
    }

    #[test]
    fn default_roundtrips() {
        let back = roundtrip(&Config::default());
        assert!(back.location.is_none());
        assert_eq!(back.bortle, None);
    }

    #[test]
    fn bortle_and_location_together_roundtrip() {
        // Regression: `location` serializes to a [location] table, so `bortle`
        // (a bare key) must come first or basic_toml rejects it with "values
        // must be emitted before tables" and save() fails.
        let cfg = Config {
            bortle: Some(5),
            location: Some(LocationPref::Name {
                name: "Hamburg".into(),
            }),
        };
        let back = roundtrip(&cfg);
        assert_eq!(back.bortle, Some(5));
        assert!(matches!(
            back.location,
            Some(LocationPref::Name { name }) if name == "Hamburg"
        ));
    }
}
