//! celsius library root.
//!
//! The architectural contract is `scene.toml -> SkyState -> PixelBuffer
//! -> terminal` (or PNG for oracle tests), three pure stages separated by
//! one typed interface. This module just re-exports the pieces.

pub mod astro;
pub mod colorspace;
pub mod config;
pub mod gradient;
pub mod haze;
pub mod moon;
pub mod noise;
pub mod precipitation;
pub mod render;
pub mod scene;
pub mod stars;
pub mod terminal;
pub mod tui;
pub mod weather;

pub use colorspace::{Oklab, PixelBuffer, Rgb};
pub use gradient::Gradient;
pub use render::render;
pub use scene::{SkyState, load_scene};
