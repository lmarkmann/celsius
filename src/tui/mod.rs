mod app;
mod widget;

pub use app::{RunOutcome, Timeline, prompt_location, run};

use std::io::{self, Write};

use crate::render::render;
use crate::scene::SkyState;

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
