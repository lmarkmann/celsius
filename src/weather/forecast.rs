use serde::Deserialize;

use super::WeatherError;

const ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";

const HOURLY_FIELDS: &str = concat!(
    "temperature_2m,",
    "cloud_cover,",
    "cloud_cover_low,",
    "cloud_cover_mid,",
    "cloud_cover_high,",
    "precipitation,",
    "wind_speed_10m,",
    "wind_direction_10m,",
    "visibility,",
    "weather_code"
);

const DAILY_FIELDS: &str = "sunrise,sunset,daylight_duration,temperature_2m_max,temperature_2m_min";

#[derive(Debug, Clone, Deserialize)]
pub struct Forecast {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub elevation: Option<f64>,
    pub timezone: String,
    #[serde(default)]
    pub utc_offset_seconds: i64,
    pub hourly: HourlyArrays,
    #[serde(default)]
    pub daily: Option<DailyArrays>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HourlyArrays {
    pub time: Vec<String>,
    pub temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    pub cloud_cover: Vec<Option<f64>>,
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
    pub fn len(&self) -> usize {
        self.time.len()
    }

    pub fn is_empty(&self) -> bool {
        self.time.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DailyArrays {
    pub time: Vec<String>,
    pub sunrise: Vec<String>,
    pub sunset: Vec<String>,
    pub daylight_duration: Vec<f64>,
    #[serde(default)]
    pub temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    pub temperature_2m_min: Vec<Option<f64>>,
}

pub fn fetch(lat: f64, lon: f64) -> Result<Forecast, WeatherError> {
    let mut response = super::AGENT
        .get(ENDPOINT)
        .query("latitude", lat.to_string())
        .query("longitude", lon.to_string())
        .query("hourly", HOURLY_FIELDS)
        .query("daily", DAILY_FIELDS)
        .query("timezone", "auto")
        .query("forecast_days", "7")
        .call()?;
    let status = response.status();
    if !status.is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(WeatherError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let body: Forecast = response.body_mut().read_json()?;
    Ok(body)
}
