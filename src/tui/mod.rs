//! The terminal surface: the live app, and the two non-interactive fallbacks.
//!
//! Three ways out of the same [`SkyState`]. The full-screen app is the intended one. `write_frame` prints a single 104x50 ANSI half-block frame, which is what `--frame` pipes into a file. `write_plain` prints one line of ASCII with no escape codes at all, which is what a pipe, `NO_COLOR`, or a screen reader gets.
//!
//! The fallbacks are not a lesser mode bolted on; they are the reason `celsius | head` and `celsius > log` behave like ordinary Unix tools.

mod app;
mod widget;

pub use app::{RunOutcome, Session, Timeline};

use std::io::{self, Write};

use crate::render::render;
use crate::scene::SkyState;

/// The flat-text surface for `--plain`, pipes, and `NO_COLOR`: one ASCII status line, no escape codes. Falls back to the decorative chrome for scene files, which carry no structured `status`.
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
