//! Pixels to terminal cells: which glyph a cell gets, and which two colours.
//!
//! [`render`](fn@crate::render) produces square pixels. A terminal produces cells that are roughly twice as tall as they are wide and can carry exactly two colours. This module is the reduction between the two, and it has two knobs that are deliberately independent.
//!
//! [`Geometry`] is how many sub-cell samples a cell is divided into, and [`ColorDepth`] is how many colours the terminal can actually show. They are separate because only one of them is detectable: a terminal will tell you its colour depth, directly or by reputation, but nothing will tell you whether its font has a glyph for U+2596. So colour depth is detected and geometry is chosen.
//!
//! The default, half blocks at truecolor, is exact: each pixel keeps its own colour and nothing is approximated. Every other combination gives some of that up for something the default cannot do.

mod capability;
mod lattice;
mod palette;
mod reduce;

pub use capability::detect_depth;

use serde::{Deserialize, Serialize};

use crate::colorspace::{PixelBuffer, Rgb};

/// How a cell is divided into sub-cell samples.
///
/// The serde names match the `--glyphs` values rather than the variant names, so the config file and the flag say the same word.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Geometry {
    /// One column by two rows, drawn with `▀`. Square subpixels, exact on its sample grid, and the only glyph family with universal coverage.
    #[default]
    #[serde(rename = "half")]
    HalfBlock,
    /// Two columns by two rows, drawn with U+2596..U+259F. Adds horizontal resolution only, since both grids are two rows deep, and its subpixels are vertical slivers rather than squares.
    #[serde(rename = "quad")]
    Quadrant,
}

/// How many colours the terminal can show.
///
/// Sextants and octants are absent from [`Geometry`] for a reason that belongs here too: Windows conhost stores sixteen bits per cell, so anything above U+FFFF is shown as its surrogate decomposition. Both families live in Plane 1, which puts them out of reach of the compatibility floor whatever font is installed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ColorDepth {
    /// 24-bit SGR, `38;2;r;g;b`.
    #[default]
    True,
    /// The xterm 256-entry palette, `38;5;n`: a 6x6x6 cube plus 24 greys. What Apple Terminal below version 464 actually has, whatever it accepts.
    Indexed256,
}

/// Either a direct colour or a palette index, matching the two SGR forms.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CellColor {
    Rgb(Rgb),
    Indexed(u8),
}

/// A finished terminal cell: one glyph and the two colours it is drawn in.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub fg: CellColor,
    pub bg: CellColor,
}

/// What the reduction needs to know, decided once per frame rather than per cell.
#[derive(Copy, Clone, Debug, Default)]
pub struct RasterOpts {
    pub geometry: Geometry,
    pub depth: ColorDepth,
    /// Supersample and average back down, which buys antialiasing without needing a single new glyph and so reaches the compatibility floor as well as everywhere else.
    pub antialias: bool,
}

/// How many samples per pixel, on each axis, these options need from [`render_supersampled`](crate::render::render_supersampled).
///
/// The lattice is always isotropic, and that is load-bearing rather than tidy. Filling a quadrant grid directly would mean a buffer twice as wide but no taller, and five things in the renderer are sized in pixels rather than frame fractions: the sun and moon radii, the star halo, and precipitation's angle and count. On a grid like that the sun stops being a circle. Supersampling both axes leaves every one of them meaning what it meant.
///
/// The resulting `2 * cols` by `4 * rows` lattice is also exactly the octant sub-cell grid, so the tier this build does not ship is a reducer away rather than a resampling change.
///
/// Every combination is spelled out rather than falling through a catch-all. A `_ => 2` arm here quietly gave quadrants and antialiased quadrants the same factor, so `--aa` produced byte-identical output under `--glyphs quad` and the flag did nothing at all. The factor a subpixel needs is set by its own shape: a half block is square and wants `2x2` per subpixel to average, a quadrant is a 1:2 sliver and wants `2x4`, which is a factor of 4 on both axes.
pub const fn sample_factor(opts: RasterOpts) -> u32 {
    match (opts.geometry, opts.antialias) {
        (Geometry::HalfBlock, false) => 1,
        (Geometry::HalfBlock, true) | (Geometry::Quadrant, false) => 2,
        (Geometry::Quadrant, true) => 4,
    }
}

/// The cell at `(col, row)`, reduced from whatever the lattice gathers there.
///
/// `pixels` must be `cols * factor` by `2 * rows * factor` for the `factor` these options report.
///
/// Pure in its arguments, dither included, which is why the dither reads `col` and `row` instead of carrying a residual between cells. An ordered dither is a function of position; an error-diffused one is a function of every cell drawn before it, so a single drifting cloud would reshuffle the pattern across the whole sky. That distinction does not exist for a still image and is the whole game at thirty frames a second.
pub fn cell_at(pixels: &PixelBuffer, opts: RasterOpts, col: usize, row: usize) -> Cell {
    let samples = lattice::gather(pixels, opts, col, row);
    let (glyph, fg, bg) = reduce::reduce(&samples, opts.geometry);
    Cell {
        glyph,
        fg: palette::quantize(fg, opts.depth, col, row * 2),
        bg: palette::quantize(bg, opts.depth, col, row * 2 + 1),
    }
}
