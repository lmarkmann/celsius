pub mod forecast;
mod gradients;
pub mod location;
pub mod state;

pub use state::compose;
pub use state::error_sky;

use thiserror::Error;

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
