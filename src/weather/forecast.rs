//! Open-Meteo hourly forecast client.
//!
//! Fetches a seven-day hourly window in UTC for a given (lat, lon). The
//! synthesis layer (Phase 3) indexes into the returned arrays by hour; the
//! TUI scrubs by moving the hour index, not by re-fetching.
//!
//! Nulls in the hourly arrays are legitimate (Open-Meteo can gap individual
//! variables for individual hours) so every value is `Option<f64>`. The
//! synthesis layer falls back to neighbouring hours or defaults when it
//! encounters a null.

use serde::Deserialize;

use super::WeatherError;

const ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";

const HOURLY_FIELDS: &str = concat!(
    "temperature_2m,",
    "cloud_cover_low,",
    "cloud_cover_mid,",
    "cloud_cover_high,",
    "precipitation,",
    "wind_speed_10m,",
    "wind_direction_10m,",
    "visibility,",
    "weather_code"
);

#[derive(Debug, Clone, Deserialize)]
pub struct Forecast {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub elevation: Option<f64>,
    pub timezone: String,
    pub hourly: HourlyArrays,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HourlyArrays {
    /// ISO 8601 local-time strings without zone suffix, e.g. "2026-04-11T00:00".
    /// Because the request uses `timezone=UTC`, these are UTC wall clocks.
    pub time: Vec<String>,
    pub temperature_2m: Vec<Option<f64>>,
    pub cloud_cover_low: Vec<Option<f64>>,
    pub cloud_cover_mid: Vec<Option<f64>>,
    pub cloud_cover_high: Vec<Option<f64>>,
    pub precipitation: Vec<Option<f64>>,
    pub wind_speed_10m: Vec<Option<f64>>,
    pub wind_direction_10m: Vec<Option<f64>>,
    pub visibility: Vec<Option<f64>>,
    pub weather_code: Vec<Option<u32>>,
}

impl HourlyArrays {
    /// Length of the hourly window. Open-Meteo guarantees equal-length arrays
    /// across all requested variables.
    pub fn len(&self) -> usize {
        self.time.len()
    }

    pub fn is_empty(&self) -> bool {
        self.time.is_empty()
    }
}

/// Fetch the 7-day hourly forecast for a location in UTC.
pub fn fetch(lat: f64, lon: f64) -> Result<Forecast, WeatherError> {
    let response = ureq::get(ENDPOINT)
        .query("latitude", &lat.to_string())
        .query("longitude", &lon.to_string())
        .query("hourly", HOURLY_FIELDS)
        .query("timezone", "UTC")
        .query("forecast_days", "7")
        .call()?;
    let body: Forecast = response.into_json()?;
    Ok(body)
}
