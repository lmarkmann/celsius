//! Colour depth: what to send when the terminal cannot take the colour we computed.
//!
//! On a truecolor terminal this is the identity and costs nothing. On a 256-colour one it is the part of the pipeline that matters most, because the sky is mostly a smooth gradient and a smooth gradient is exactly what a coarse palette destroys. A dusk ramp down fifty rows crosses eight or nine entries of the xterm cube and shows every one of the boundaries.
//!
//! The fix is not a better nearest-entry search. Snapping each cell to its closest entry is what produces the bands in the first place, however good the metric. It is to pick two entries that straddle the target and alternate them across the sub-cell lattice, so a cell averages out to a colour the palette does not contain. That makes the sub-cell sites a dither lattice as much as a resolution gain, which is the one place the sky being smooth works in our favour rather than against.

use super::{CellColor, ColorDepth};
use crate::colorspace::Rgb;

/// Map one colour to what the terminal can actually show.
///
/// `x` and `y` are the colour's position in half-block pixel space, which is the ordered dither's only input beyond the colour itself. They are unused until that lands, and they are in the signature now because taking them later would mean changing every caller.
pub fn quantize(color: Rgb, depth: ColorDepth, _x: usize, _y: usize) -> CellColor {
    match depth {
        ColorDepth::True => CellColor::Rgb(color),
        // Passes through for now, so choosing this depth is currently a no-op rather than a downgrade. The cube, the Oklab nearest-entry search and the Bayer threshold all land here.
        ColorDepth::Indexed256 => CellColor::Rgb(color),
    }
}
