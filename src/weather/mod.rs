//! Live weather layer: geocoding + forecast + synthesis into SkyState.
//!
//! Three stages sit upstream of the lab-inherited render pipeline:
//!
//!   query string -> geocode -> (lat, lon, timezone)
//!   (lat, lon)   -> forecast -> hourly arrays
//!   (forecast, lat, lon, time) -> compose -> SkyState
//!
//! Only the first two stages exist in Phase 2. Synthesis (`state::compose`)
//! lands in Phase 3.

pub mod forecast;
mod gradients;
pub mod location;
pub mod state;

pub use state::compose;

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum WeatherError {
    /// Transport-level failure: no connection, DNS, TLS handshake, etc.
    #[error("network: {0}")]
    Network(String),

    /// Non-2xx HTTP response. Open-Meteo returns 400 for malformed queries
    /// and 429 when the free-tier quota is exhausted.
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },

    /// Response body could not be deserialized into the expected shape.
    #[error("decode: {0}")]
    Decode(String),
}

impl From<ureq::Error> for WeatherError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Status(status, response) => {
                let body = response.into_string().unwrap_or_default();
                WeatherError::Http { status, body }
            }
            other => WeatherError::Network(other.to_string()),
        }
    }
}

impl From<std::io::Error> for WeatherError {
    fn from(err: std::io::Error) -> Self {
        WeatherError::Decode(err.to_string())
    }
}
