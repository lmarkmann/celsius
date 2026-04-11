//! Terminal UI: ratatui + crossterm event loop that displays a SkyState,
//! plus a non-TTY path that emits a single frame as truecolor ANSI to any
//! writer (for `celsius --scene X | cat`, redirects, CI snapshots).
//!
//! Phase 1 surface: load a SkyState (from `--scene` for now, from synthesis
//! later), render it full-screen on every frame, respond to resize, quit on
//! `q` / `Esc` / `Ctrl+C`. No keybindings beyond quit yet; scrubbing, the
//! location overlay, and the help overlay land alongside the weather layer.

mod app;
mod widget;

pub use app::{Timeline, prompt_location, run};

use std::io::{self, Write};

use crate::render::render;
use crate::scene::SkyState;

/// Render one frame at the lab-canonical 104x50 and write it to `out` as
/// truecolor ANSI half-blocks, each row terminated by a reset + newline.
/// No alternate screen, no raw mode, no chrome: this is what the binary
/// falls back to when stdout is not a tty (piped or redirected).
///
/// Callers should wrap the writer in a `BufWriter`; the emit loop issues
/// roughly 2.6k escape sequences per frame and unbuffered stdout locks on
/// every one.
pub fn write_frame<W: Write>(state: &SkyState, out: &mut W) -> io::Result<()> {
    let pixels = render(state, 104, 50);
    let width = pixels.width;
    let rows = pixels.height / 2;
    for row in 0..rows {
        let y_top = row * 2;
        let y_bot = y_top + 1;
        for col in 0..width {
            let top = pixels.pixels[y_top * width + col];
            let bot = pixels.pixels[y_bot * width + col];
            write!(
                out,
                "\x1b[38;2;{};{};{};48;2;{};{};{}m▀",
                top.r, top.g, top.b, bot.r, bot.g, bot.b
            )?;
        }
        out.write_all(b"\x1b[0m\n")?;
    }
    Ok(())
}
