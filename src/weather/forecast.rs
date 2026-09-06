//! The Open-Meteo forecast client and its response types.
//!
//! One request fetches a 192-hour window, 24 hours back and 168 forward from the current hour, rather than seven calendar days from local midnight: with the old window a launch at 22:00 had spent most of its first day already and could scrub only six days ahead. Hourly: temperature, humidity, the three cloud-cover bands, precipitation split into rain, showers and snowfall, the direct beam reaching the ground, convective energy, wind, visibility and WMO weather code. Daily: sunrise, sunset and the day's high and low. That window is also the hard limit on `--at`, since a sky can only be synthesized for an hour the forecast actually covers.
//!
//! Timestamps are unix seconds in UTC (`timeformat=unixtime`), so nothing downstream parses a local wall-clock string. `timezone=auto` stays in the request for `utc_offset_seconds`, which the chrome needs for display, and for the daily block, whose rows are local days.
//!
//! Every hourly field is `Option`, because Open-Meteo genuinely returns nulls for some variables at some locations rather than omitting them, and treating a missing visibility as zero would render fog on a clear day.

use serde::Deserialize;

use super::WeatherError;
use super::aerosol::AerosolForecast;

const ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";

pub(super) const PAST_HOURS: &str = "24";
pub(super) const FORECAST_HOURS: &str = "168";

const HOURLY_FIELDS: &str = concat!(
    "temperature_2m,",
    "relative_humidity_2m,",
    "cloud_cover,",
    "cloud_cover_low,",
    "cloud_cover_mid,",
    "cloud_cover_high,",
    "precipitation,",
    "rain,",
    "showers,",
    "snowfall,",
    "precipitation_probability,",
    "cape,",
    "direct_normal_irradiance_instant,",
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

impl Forecast {
    /// Copy the aerosol optical depth for every hour this forecast covers, matched by timestamp. Hours the aerosol forecast does not cover (its horizon is shorter) stay `None`, and the sky falls back to visibility there.
    pub fn attach_aerosol(&mut self, aerosol: &AerosolForecast) {
        let times = &aerosol.hourly.time;
        let depths = &aerosol.hourly.aerosol_optical_depth;
        let mut j = 0;
        self.hourly.aerosol_optical_depth = self
            .hourly
            .time
            .iter()
            .map(|&t| {
                while j < times.len() && times[j] < t {
                    j += 1;
                }
                if times.get(j) == Some(&t) {
                    depths.get(j).copied().flatten()
                } else {
                    None
                }
            })
            .collect();
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HourlyArrays {
    /// Unix seconds, UTC.
    pub time: Vec<i64>,
    pub temperature_2m: Vec<Option<f64>>,
    /// Over water, which is what Open-Meteo reports and not what the snow morphology diagram is drawn against; `snow::supersaturation` is the conversion.
    #[serde(default)]
    pub relative_humidity_2m: Vec<Option<f64>>,
    #[serde(default)]
    pub cloud_cover: Vec<Option<f64>>,
    pub cloud_cover_low: Vec<Option<f64>>,
    pub cloud_cover_mid: Vec<Option<f64>>,
    pub cloud_cover_high: Vec<Option<f64>>,
    pub precipitation: Vec<Option<f64>>,
    /// The liquid part of `precipitation` that fell from stratiform cloud, in mm. With `showers` it decides whether an hour draws rain streaks at all, which the total cannot, since the total also counts snow.
    #[serde(default)]
    pub rain: Vec<Option<f64>>,
    /// Convective liquid precipitation in mm. Any showers at all make the low deck a cumulonimbus tower, whatever the weather code says.
    #[serde(default)]
    pub showers: Vec<Option<f64>>,
    /// Snow depth accumulating this hour, in cm. Separate from `precipitation`, which reports the same fall as liquid water equivalent and so understates it by roughly ten to one.
    #[serde(default)]
    pub snowfall: Vec<Option<f64>>,
    /// Percent. Scales how much precipitation is drawn, so a twenty percent chance on day six does not paint the same downpour as certain rain in an hour.
    #[serde(default)]
    pub precipitation_probability: Vec<Option<f64>>,
    /// Convective available potential energy in J/kg: how violent a storm the code is announcing.
    #[serde(default)]
    pub cape: Vec<Option<f64>>,
    /// W/m2 at the indicated instant. Against the solar constant this is the share of the beam that reaches the ground, which is what says whether the sun disc is visible and how much of the clear-sky model to draw.
    #[serde(default)]
    pub direct_normal_irradiance_instant: Vec<Option<f64>>,
    pub wind_speed_10m: Vec<Option<f64>>,
    pub wind_direction_10m: Vec<Option<f64>>,
    pub visibility: Vec<Option<f64>>,
    pub weather_code: Vec<Option<u32>>,
    /// Aerosol optical depth at 550 nm. Never present in the forecast response; [`Forecast::attach_aerosol`] fills it from the air-quality endpoint, and it stays empty when that request failed.
    #[serde(default)]
    pub aerosol_optical_depth: Vec<Option<f64>>,
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
    /// Unix seconds of local midnight starting each row's day. Add `utc_offset_seconds` before dividing by 86400 to recover the local date.
    pub time: Vec<i64>,
    /// Unix seconds, UTC. Polar day and night are sentinels rather than nulls: see `daylight_duration`.
    pub sunrise: Vec<i64>,
    pub sunset: Vec<i64>,
    pub daylight_duration: Vec<f64>,
    #[serde(default)]
    pub temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    pub temperature_2m_min: Vec<Option<f64>>,
}

/// Fetch the 192-hour forecast for a coordinate from Open-Meteo: the past 24 hours and the next 168.
///
/// # Errors
///
/// [`WeatherError::Network`] if the request does not complete, [`WeatherError::Http`] for a non-success status, and [`WeatherError::Decode`] if the body is not the JSON shape this crate expects.
pub fn fetch(lat: f64, lon: f64) -> Result<Forecast, WeatherError> {
    let mut response = super::AGENT
        .get(ENDPOINT)
        .query("latitude", lat.to_string())
        .query("longitude", lon.to_string())
        .query("hourly", HOURLY_FIELDS)
        .query("daily", DAILY_FIELDS)
        .query("timezone", "auto")
        .query("timeformat", "unixtime")
        .query("past_hours", PAST_HOURS)
        .query("forecast_hours", FORECAST_HOURS)
        // The daily block is windowed by days, not hours; one day back and eight forward is the smallest span that covers every hourly row whatever the launch hour.
        .query("past_days", "1")
        .query("forecast_days", "8")
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
