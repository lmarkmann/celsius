//! The sample lattice, and the folds that come off it.
//!
//! One render feeds every geometry. A half-block grid for a `cols x rows` terminal is `cols` by `2 * rows` square pixels; supersampled by two that is `2 * cols` by `4 * rows`. Averaging vertical pairs of that gives the quadrant grid, averaging 2x2 blocks gives antialiased half blocks, and the lattice itself is already the octant grid. The geometries differ only in how they fold the same samples, which is what keeps the renderer from needing to know about any of them.

use super::{Geometry, RasterOpts, sample_factor};
use crate::colorspace::{Oklab, PixelBuffer, Rgb, oklab_to_rgb, rgb_u8_to_oklab};

/// The most subpixels any shipped geometry divides a cell into.
pub const MAX_SUBPIXELS: usize = 4;

/// One cell's subpixels in reading order: left to right, then top to bottom, so bit `i` of a partition mask names `values[i]`.
pub struct Samples {
    pub values: [Rgb; MAX_SUBPIXELS],
    pub len: usize,
}

/// Fold the buffer region under one cell into that cell's subpixels.
///
/// Both geometries are two subpixels deep, so only the column count varies and the vertical span is always `factor`.
pub fn gather(pixels: &PixelBuffer, opts: RasterOpts, col: usize, row: usize) -> Samples {
    let factor = sample_factor(opts) as usize;
    let sub_cols = match opts.geometry {
        Geometry::HalfBlock => 1,
        Geometry::Quadrant => 2,
    };
    let span_x = factor / sub_cols;
    let span_y = factor;
    debug_assert!(
        span_x > 0,
        "sample_factor must give a quadrant cell at least one sample per subpixel"
    );

    let x0 = col * factor;
    let y0 = row * 2 * factor;
    let mut values = [Rgb::BLACK; MAX_SUBPIXELS];
    let mut len = 0;
    for j in 0..2 {
        for i in 0..sub_cols {
            values[len] = average(pixels, x0 + i * span_x, y0 + j * span_y, span_x, span_y);
            len += 1;
        }
    }
    Samples { values, len }
}

/// The mean colour of one subpixel's share of the buffer.
///
/// The single-sample case returns the pixel untouched rather than passing it through Oklab and back. That is not an optimisation: `oklab_to_rgb` rounds on the way out, so a round trip is not guaranteed to be the identity, and half blocks at factor 1 have to be byte-identical to the path this replaces.
///
/// Averaging in Oklab follows the crate rule that every blend happens there, but resampling is arguably not a blend: physically correct antialiasing averages in linear light, and Oklab will read brighter on a hard edge like a star against night sky. Worth revisiting against a contact sheet once there is something to look at.
fn average(pixels: &PixelBuffer, x0: usize, y0: usize, w: usize, h: usize) -> Rgb {
    if w == 1 && h == 1 {
        return pixels.get(x0, y0);
    }
    let (mut l, mut a, mut b) = (0.0, 0.0, 0.0);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let c = pixels.get(x, y);
            let lab = rgb_u8_to_oklab(c.r, c.g, c.b);
            l += lab.l;
            a += lab.a;
            b += lab.b;
        }
    }
    let n = (w * h) as f64;
    oklab_to_rgb(Oklab::new(l / n, a / n, b / n))
}
