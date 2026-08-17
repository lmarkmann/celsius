//! The PNG sink, behind the `png` feature.
//!
//! Encoding exists for the golden-image oracle and the `render` subcommand, not for everyday use: the TUI is the intended surface. Keeping it feature-gated is what holds the default binary near 3.6 MB.

#[cfg(feature = "png")]
use std::io::Cursor;
#[cfg(feature = "png")]
use std::path::Path;

#[cfg(feature = "png")]
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

#[cfg(feature = "png")]
use crate::colorspace::PixelBuffer;

#[cfg(feature = "png")]
fn raw_rgb(pixels: &PixelBuffer) -> Vec<u8> {
    let mut buf = Vec::with_capacity(pixels.width * pixels.height * 3);
    for p in &pixels.pixels {
        buf.push(p.r);
        buf.push(p.g);
        buf.push(p.b);
    }
    buf
}

/// Why writing a frame out failed.
///
/// The encoder's own error is boxed rather than named. `image` is a 0.x crate, so returning `image::ImageError` put it in this crate's public API and made a routine `image` bump a breaking change for every dependent, which is a break nothing here could have caught: cargo-semver-checks compares this crate against itself with one version of `image` resolved for both sides.
#[cfg(feature = "png")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TerminalError {
    #[error("encoding the frame as PNG: {source}")]
    Encode {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("writing {}: {source}", path.display())]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Encode a buffer as a PNG in memory.
///
/// # Errors
///
/// [`TerminalError::Encode`] if the encoder rejects the buffer.
#[cfg(feature = "png")]
pub fn encode_png(pixels: &PixelBuffer) -> Result<Vec<u8>, TerminalError> {
    let rgb = raw_rgb(pixels);
    let mut out = Cursor::new(Vec::new());
    PngEncoder::new(&mut out)
        .write_image(
            &rgb,
            pixels.width as u32,
            pixels.height as u32,
            ExtendedColorType::Rgb8,
        )
        .map_err(|source| TerminalError::Encode {
            source: Box::new(source),
        })?;
    Ok(out.into_inner())
}

/// Encode a buffer and write it to `path`.
///
/// # Errors
///
/// [`TerminalError::Encode`] if the encoder rejects the buffer, or [`TerminalError::Write`], which names the path, if the file cannot be written.
#[cfg(feature = "png")]
pub fn write_png(pixels: &PixelBuffer, path: impl AsRef<Path>) -> Result<(), TerminalError> {
    let path = path.as_ref();
    std::fs::write(path, encode_png(pixels)?).map_err(|source| TerminalError::Write {
        path: path.to_path_buf(),
        source,
    })
}
