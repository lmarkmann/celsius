//! Open-Meteo geocoding client.
//!
//! Returns up to five candidate matches for a free-form name query. An empty
//! result vector is a legitimate answer (caller decides what "not found"
//! means in its context — the TUI renders a beautiful-error sky, the CLI
//! prints a suggestion).

use serde::Deserialize;

use super::WeatherError;

const ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GeoResult {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub admin1: Option<String>,
    #[serde(default)]
    pub elevation: Option<f64>,
    #[serde(default)]
    pub population: Option<u64>,
}

impl GeoResult {
    /// A short human label like "Hamburg, Germany" or "Hamburg, Arkansas, US".
    pub fn label(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if let Some(admin) = &self.admin1
            && admin != &self.name
        {
            parts.push(admin.clone());
        }
        if let Some(country) = &self.country {
            parts.push(country.clone());
        }
        parts.join(", ")
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeoResponse {
    #[serde(default)]
    pub results: Vec<GeoResult>,
}

/// Look up candidate locations for a free-form name query.
///
/// Returns an empty vector if Open-Meteo has no matches. Returns up to five
/// results otherwise, ordered by Open-Meteo's relevance ranking (which
/// roughly tracks population).
pub fn geocode(query: &str) -> Result<Vec<GeoResult>, WeatherError> {
    let response = ureq::get(ENDPOINT)
        .query("name", query)
        .query("count", "5")
        .query("language", "en")
        .query("format", "json")
        .call()?;
    let body: GeoResponse = response.into_json()?;
    Ok(body.results)
}
