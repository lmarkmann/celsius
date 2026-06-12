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

pub fn geocode(query: &str) -> Result<Vec<GeoResult>, WeatherError> {
    let mut response = super::AGENT
        .get(ENDPOINT)
        .query("name", query)
        .query("count", "5")
        .query("language", "en")
        .query("format", "json")
        .call()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(WeatherError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let body: GeoResponse = response.body_mut().read_json()?;
    Ok(body.results)
}
