//! Aerosol optical depth from Open-Meteo's air-quality endpoint, the column measure the analytic sky's turbidity actually describes.
//!
//! Visibility is a boundary-layer number that most models cap near 24 km, so on most days it sits at the cap and turbidity sits at 2 whatever the air is doing above the first kilometre. Optical depth at 550 nm from CAMS is the whole column, hourly and worldwide, and it moves with haze, dust and smoke. It comes from a different host with a shorter horizon, about five days, so it is a second request that the sky can do without: a failure here leaves `Forecast::attach_aerosol` uncalled and turbidity falls back to visibility.

use serde::Deserialize;

use super::WeatherError;
use super::forecast::{FORECAST_HOURS, PAST_HOURS};

const ENDPOINT: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";

#[derive(Debug, Clone, Deserialize)]
pub struct AerosolForecast {
    pub hourly: AerosolHourly,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AerosolHourly {
    /// Unix seconds, UTC, on the same hourly grid as the forecast.
    pub time: Vec<i64>,
    /// Dimensionless; 0.05 is alpine air, 0.3 a hazy summer afternoon, above 1 is smoke or a dust storm.
    pub aerosol_optical_depth: Vec<Option<f64>>,
}

/// Fetch the aerosol optical depth over the same window as the forecast.
///
/// # Errors
///
/// [`WeatherError::Network`] if the request does not complete, [`WeatherError::Http`] for a non-success status, and [`WeatherError::Decode`] if the body is not the JSON shape this crate expects.
pub fn fetch(lat: f64, lon: f64) -> Result<AerosolForecast, WeatherError> {
    let mut response = super::AGENT
        .get(ENDPOINT)
        .query("latitude", lat.to_string())
        .query("longitude", lon.to_string())
        .query("hourly", "aerosol_optical_depth")
        .query("timeformat", "unixtime")
        .query("past_hours", PAST_HOURS)
        .query("forecast_hours", FORECAST_HOURS)
        .call()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(WeatherError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let body: AerosolForecast = response.body_mut().read_json()?;
    Ok(body)
}
