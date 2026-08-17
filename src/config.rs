//! The persisted location preference at `~/.config/celsius/config.toml`.
//!
//! Only what has to survive between runs: the last location, stored either as a name to geocode or as coordinates. Coordinates are preferred once known, since they skip a network round trip and cannot re-resolve to a different city later.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::raster::Geometry;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config serialize: {0}")]
    Serialize(#[from] basic_toml::Error),
}

/// Not exhaustively constructible from outside: this is a settings bag that gains a field every time a preference becomes worth remembering, and each one of those was otherwise a breaking change for anyone writing a struct literal. Build it from `Config::default()` and assign.
#[derive(Debug, Default, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Config {
    // bortle and facing are scalars and must serialize before `location`, which becomes a [location] table; TOML requires every bare key before the first table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bortle: Option<u8>,
    /// Compass bearing to face, when the hemisphere default is not what the viewer wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facing: Option<f64>,
    /// Sub-cell glyph family. Persisted because whether your font has the quadrants is a fact about your setup; colour depth deliberately is not, since a config file gets read over ssh and from other terminals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glyphs: Option<Geometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aa: Option<bool>,
    pub location: Option<LocationPref>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LocationPref {
    // Coords must come before Name: it requires `lat`/`lon` (no defaults), so a name-only table falls through to Name, while a coords table (with or without the optional label) matches here. If Name came first it would greedily claim any table carrying a `name` key and drop the coordinates.
    Coords {
        lat: f64,
        lon: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Name {
        name: String,
    },
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
            // Falling back to defaults means the next save overwrites the user's file, so the reset must at least be visible.
            eprintln!("celsius: ignoring malformed config {}: {e}", path.display());
            Config::default()
        }
    }
}

/// Write the config to `~/.config/celsius/config.toml`, creating the directory if it does not exist.
///
/// # Errors
///
/// [`ConfigError::Io`] if the directory cannot be created or the file cannot be written, and [`ConfigError::Serialize`] if the config does not serialize to TOML.
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
            ..Config::default()
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
                name: Some("Hamburg, Germany".into()),
            }),
            ..Config::default()
        };
        let back = roundtrip(&cfg);
        match back.location {
            Some(LocationPref::Coords { lat, lon, name }) => {
                assert_eq!(lat, 53.55);
                assert_eq!(lon, 9.99);
                assert_eq!(name.as_deref(), Some("Hamburg, Germany"));
            }
            other => panic!("untagged enum picked wrong variant: {other:?}"),
        }
    }

    #[test]
    fn legacy_coords_without_name_parses() {
        // Configs written before the picker carry only lat/lon. They must still load as Coords (name defaulting to None), not get misread as Name.
        let cfg: Config =
            basic_toml::from_str("[location]\nlat = 53.55\nlon = 9.99\n").expect("parse");
        match cfg.location {
            Some(LocationPref::Coords { lat, lon, name }) => {
                assert_eq!(lat, 53.55);
                assert_eq!(lon, 9.99);
                assert_eq!(name, None);
            }
            other => panic!("legacy coords misparsed: {other:?}"),
        }
    }

    #[test]
    fn default_roundtrips() {
        let back = roundtrip(&Config::default());
        assert!(back.location.is_none());
        assert_eq!(back.bortle, None);
    }

    #[test]
    fn every_scalar_and_location_together_roundtrip() {
        // Regression: `location` serializes to a [location] table, so every bare key must come first or basic_toml rejects it with "values must be emitted before tables" and save() fails. Every scalar is set here rather than just the ones that first hit it, so adding another one below `location` fails here rather than at the next save.
        let cfg = Config {
            bortle: Some(5),
            facing: Some(0.0),
            glyphs: Some(Geometry::Quadrant),
            aa: Some(true),
            location: Some(LocationPref::Name {
                name: "Hamburg".into(),
            }),
        };
        let back = roundtrip(&cfg);
        assert_eq!(back.bortle, Some(5));
        assert_eq!(back.facing, Some(0.0));
        assert_eq!(back.glyphs, Some(Geometry::Quadrant));
        assert_eq!(back.aa, Some(true));
        assert!(matches!(
            back.location,
            Some(LocationPref::Name { name }) if name == "Hamburg"
        ));
    }
}
