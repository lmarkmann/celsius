//! One cell: choose the bipartition, and the two colours that go with it.
//!
//! A glyph is a bitmask over the cell's subpixels, so choosing a glyph is choosing which subpixels share the foreground colour. For a fixed mask the best two colours are the two group means by least squares, so the colours are not free variables and the whole problem collapses to a search over masks.
//!
//! That search is small here, and the reason is worth stating because it is what makes the rest of the design cheap. All sixteen subsets of a quadrant cell have a glyph, so the search is unconstrained; a mask and its complement describe the same partition with the colours swapped, so sixteen candidates are eight. chafa's popcount bitmaps and candidate shortlisting exist to search thousands of arbitrary font glyphs and would be answering a question this does not have.
//!
//! Ordering is the tie-break and it is deliberate. The half-block partition is first and improvement is strict, so a cell that two glyphs describe equally well gets the one every terminal can draw.

use super::Geometry;
use super::lattice::Samples;
use crate::colorspace::{Oklab, Rgb, oklab_to_rgb, rgb_u8_to_oklab};

/// Mask to glyph, indexed so bit `i` names subpixel `i` in reading order: upper left, upper right, lower left, lower right.
const GLYPHS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// Top row foreground, bottom row background: the partition every terminal can draw, and the one the search has to beat by a visible margin before it is displaced.
const HALF_BLOCK: u8 = 0b0011;

/// Reduce one cell's samples to a glyph and its two colours.
///
/// The quadrant arm currently returns the half-block partition. That is a legal choice under the ordering rule rather than a placeholder value, so the data path below is already the final one; what is missing is the eight-candidate search that would sometimes pick a different mask.
pub fn reduce(samples: &Samples, geometry: Geometry) -> (char, Rgb, Rgb) {
    match geometry {
        Geometry::HalfBlock => (
            GLYPHS[HALF_BLOCK as usize],
            samples.values[0],
            samples.values[1],
        ),
        Geometry::Quadrant => {
            let mask = HALF_BLOCK;
            let (fg, bg) = group_means(samples, mask);
            (GLYPHS[mask as usize], fg, bg)
        }
    }
}

/// The two group means for a mask: set bits to the foreground, clear bits to the background.
///
/// An all-set or all-clear mask leaves one group empty, and its colour is then never drawn, so it borrows the other's rather than being left undefined.
fn group_means(samples: &Samples, mask: u8) -> (Rgb, Rgb) {
    let (mut fg, mut bg) = (Accumulator::new(), Accumulator::new());
    for i in 0..samples.len {
        let target = if mask & (1 << i) != 0 {
            &mut fg
        } else {
            &mut bg
        };
        target.add(samples.values[i]);
    }
    match (fg.mean(), bg.mean()) {
        (Some(f), Some(b)) => (f, b),
        (Some(f), None) => (f, f),
        (None, Some(b)) => (b, b),
        (None, None) => (Rgb::BLACK, Rgb::BLACK),
    }
}

/// A running mean in Oklab, which is where every other average in this crate is taken.
struct Accumulator {
    sum: Oklab,
    count: usize,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            sum: Oklab::new(0.0, 0.0, 0.0),
            count: 0,
        }
    }

    fn add(&mut self, c: Rgb) {
        let lab = rgb_u8_to_oklab(c.r, c.g, c.b);
        self.sum.l += lab.l;
        self.sum.a += lab.a;
        self.sum.b += lab.b;
        self.count += 1;
    }

    fn mean(&self) -> Option<Rgb> {
        if self.count == 0 {
            return None;
        }
        let n = self.count as f64;
        Some(oklab_to_rgb(Oklab::new(
            self.sum.l / n,
            self.sum.a / n,
            self.sum.b / n,
        )))
    }
}
