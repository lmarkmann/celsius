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

#[cfg(feature = "png")]
pub fn encode_png(pixels: &PixelBuffer) -> Result<Vec<u8>, image::ImageError> {
    let rgb = raw_rgb(pixels);
    let mut out = Cursor::new(Vec::new());
    PngEncoder::new(&mut out).write_image(
        &rgb,
        pixels.width as u32,
        pixels.height as u32,
        ExtendedColorType::Rgb8,
    )?;
    Ok(out.into_inner())
}

#[cfg(feature = "png")]
pub fn write_png(pixels: &PixelBuffer, path: impl AsRef<Path>) -> Result<(), image::ImageError> {
    std::fs::write(path, encode_png(pixels)?)?;
    Ok(())
}
