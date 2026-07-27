//! Live weather: fetch a forecast, resolve a place, synthesize a sky.
//!
//! Three stages, each its own module. `location` turns a place name into coordinates, `forecast` fetches a seven-day hourly window from Open-Meteo, and `state` composes those into a `SkyState` for any instant in that window. `gradients` and `bortle` supply the palettes and the light-pollution model the composer draws on.
//!
//! `WeatherError` distinguishes network, HTTP and decode failures, because the caller's response differs: a network blip is worth retrying, a schema change is not. Nothing here panics on bad input; a failed fetch becomes a dark sky carrying the error message in its footer.

pub mod bortle;
pub mod forecast;
mod gradients;
pub mod location;
pub mod state;

pub use gradients::clear_sky_gradient;
pub use state::ComposeOpts;
pub use state::compose;
pub use state::compose_at;
pub use state::error_sky;
pub use state::turbidity_from_visibility;

use std::sync::LazyLock;
use std::time::Duration;

use thiserror::Error;
use ureq::Agent;

/// One agent for both Open-Meteo endpoints: connection reuse, and explicit timeouts so a stalled network fails the fetch instead of hanging the launch (or the in-TUI retry) forever. Status handling stays manual so error responses keep their body for the Http variant.
pub(crate) static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
});

#[derive(Debug, Clone, Error)]
pub enum WeatherError {
    #[error("network: {0}")]
    Network(String),

    #[error("http {status}: {body}")]
    Http { status: u16, body: String },

    #[error("decode: {0}")]
    Decode(String),
}

impl From<ureq::Error> for WeatherError {
    fn from(err: ureq::Error) -> Self {
        match err {
            // Unreachable while the agent disables http_status_as_error, but kept total so a config change cannot silently misclassify.
            ureq::Error::StatusCode(status) => WeatherError::Http {
                status,
                body: String::new(),
            },
            ureq::Error::Json(e) => WeatherError::Decode(e.to_string()),
            other => WeatherError::Network(other.to_string()),
        }
    }
}
