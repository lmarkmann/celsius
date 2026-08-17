//! The terminal surface: the live app, and the two non-interactive fallbacks.
//!
//! Three ways out of the same [`SkyState`]. The full-screen app is the intended one. `write_frame` prints a single 104x50 ANSI half-block frame, which is what `--frame` pipes into a file. `write_plain` prints one line of ASCII with no escape codes at all, which is what a pipe, `NO_COLOR`, or a screen reader gets.
//!
//! The fallbacks are not a lesser mode bolted on; they are the reason `celsius | head` and `celsius > log` behave like ordinary Unix tools.

mod app;
mod widget;

pub use app::{RunOutcome, Session, Timeline};

// Not API: `benches/render.rs` drives the real draw path to measure a frame, and a bench is an external crate. Hidden from docs so it stays off the semver contract that cargo-semver-checks enforces on release.
#[doc(hidden)]
pub use app::{App, draw_frame};

use std::io::{self, Write};

use crate::raster::{self, CellColor, RasterOpts};
use crate::render::render_supersampled;
use crate::scene::SkyState;

/// The captured frame is a fixed 104x50, the size every scene constant is tuned against, rather than the terminal's own size.
const FRAME_WIDTH: u32 = 104;
const FRAME_HEIGHT: u32 = 50;

/// Why the app stopped, when it was not the user asking it to.
///
/// Every failure the loops can reach is the terminal refusing to draw or to hand over an event, so one variant carries all of them along with the operation that was in flight. It is a concrete type rather than `anyhow::Result` because [`Session::run`] is public API: this way a caller can match on the failure, and anyhow stays out of the signature.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TuiError {
    #[error("{during}: {source}")]
    Terminal {
        during: &'static str,
        #[source]
        source: io::Error,
    },
}

impl TuiError {
    /// Label an I/O failure with the operation that was in flight, shaped for `map_err`.
    fn terminal(during: &'static str) -> impl Fn(io::Error) -> Self {
        move |source| Self::Terminal { during, source }
    }
}

/// The flat-text surface for `--plain`, pipes, and `NO_COLOR`: one ASCII status line, no escape codes. Falls back to the decorative chrome for scene files, which carry no structured `status`.
///
/// # Errors
///
/// Propagates the write failure, so a closed pipe reaches the caller rather than becoming a panic.
pub fn write_plain<W: Write>(state: &SkyState, out: &mut W) -> io::Result<()> {
    let line = if state.chrome.status.is_empty() {
        format!(
            "{} {}",
            state.chrome.header_right.trim(),
            state.chrome.footer.trim()
        )
    } else {
        state.chrome.status.clone()
    };
    writeln!(out, "{}", line.trim())
}

/// One captured frame at the default geometry: half blocks at truecolor, the surface `--frame` pipes into a file.
///
/// # Errors
///
/// Propagates the write failure, so a closed pipe reaches the caller rather than becoming a panic.
pub fn write_frame<W: Write>(state: &SkyState, out: &mut W) -> io::Result<()> {
    write_frame_with(state, out, RasterOpts::default())
}

/// One captured frame at a chosen geometry and colour depth.
///
/// A capture is not a live sky, so the two surfaces can reasonably differ here: this one is written once and read later, which is the case where error diffusion would beat the ordered dither the app needs.
///
/// # Errors
///
/// Propagates the write failure, so a closed pipe reaches the caller rather than becoming a panic.
pub fn write_frame_with<W: Write>(
    state: &SkyState,
    out: &mut W,
    opts: RasterOpts,
) -> io::Result<()> {
    let pixels = render_supersampled(
        state,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        raster::sample_factor(opts),
    );
    let cols = FRAME_WIDTH as usize;
    let rows = (FRAME_HEIGHT / 2) as usize;
    for row in 0..rows {
        for col in 0..cols {
            let cell = raster::cell_at(&pixels, opts, col, row);
            out.write_all(b"\x1b[")?;
            write_sgr(out, 38, cell.fg)?;
            out.write_all(b";")?;
            write_sgr(out, 48, cell.bg)?;
            out.write_all(b"m")?;
            write!(out, "{}", cell.glyph)?;
        }
        out.write_all(b"\x1b[0m\n")?;
    }
    Ok(())
}

/// One colour's SGR parameters, without the introducer, so a cell can join its foreground and background into a single escape the way this surface always has.
fn write_sgr<W: Write>(out: &mut W, base: u8, color: CellColor) -> io::Result<()> {
    match color {
        CellColor::Rgb(c) => write!(out, "{base};2;{};{};{}", c.r, c.g, c.b),
        CellColor::Indexed(i) => write!(out, "{base};5;{i}"),
    }
}
