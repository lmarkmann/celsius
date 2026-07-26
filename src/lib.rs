//! The sky above a place, right now, as a truecolor half-block scene.
//!
//! Everything funnels through one type and one function. [`SkyState`] is a complete description of a sky: a gradient or an analytic radiance field, where the sun and moon are, which cloud layers exist, whether it is raining. [`render()`] turns that into a [`PixelBuffer`]. A `SkyState` comes either from a scene TOML via [`load_scene`], which is fixed and reproducible, or from `weather::compose`, which synthesizes one from an Open-Meteo forecast and the observer's coordinates.
//!
//! ```no_run
//! let sky = celsius::load_scene("scenes/high_noon_clear.toml")?;
//! let pixels = celsius::render(&sky, 104, 50);
//! # Ok::<(), celsius::scene::SceneError>(())
//! ```
//!
//! Two rules explain most of the design. **All compositing happens in Oklab**, converting to sRGB only at the final pixel write, because linear interpolation in Oklab is what keeps a dawn gradient from banding. And **the PRNG is bit-compatible with CPython's `random.Random`**, so a given seed produces identical noise, stars and precipitation on every platform; the golden-image tests depend on that exactness.
//!
//! Time-varying effects deliberately live *outside* [`render()`]. Lightning, meteors and the easter egg composite onto a rendered buffer from an app clock, so a still image never catches a flash mid-strike and the expensive pixel pipeline stays cacheable across frames.

pub mod analytic_sky;
pub mod astro;
pub mod colorspace;
pub mod config;
pub mod gradient;
pub mod lightning;
pub mod meteors;
pub mod moon;
pub mod noise;
pub mod pigs;
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
pub use scene::{SkyState, builtin_names, load_builtin_scene, load_scene};
