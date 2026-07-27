//! The half-block widget: two sky pixels per terminal cell.
//!
//! A terminal cell is roughly twice as tall as it is wide, so drawing one pixel per cell gives a squashed sky. Printing `▀` with the upper pixel as foreground and the lower as background instead packs two rows into one cell, which both doubles vertical resolution and squares the aspect ratio. A 104x50 buffer therefore occupies 104 columns and 25 rows.
//!
//! This is why every scene constant is expressed against a 104x50 pixel buffer rather than against terminal rows.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::PixelBuffer;

pub struct SkyWidget<'a> {
    pub pixels: &'a PixelBuffer,
}

impl Widget for SkyWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = area.width as usize;
        let rows = area.height as usize;
        let pw = self.pixels.width;
        let ph = self.pixels.height;
        debug_assert_eq!(pw, cols, "pixel width must match terminal cols");
        debug_assert_eq!(ph, rows * 2, "pixel height must be 2 * terminal rows");

        for row in 0..rows {
            for col in 0..cols {
                let y_top = row * 2;
                let y_bot = y_top + 1;
                let top = self.pixels.pixels[y_top * pw + col];
                let bot = self.pixels.pixels[y_bot * pw + col];
                let cell = &mut buf[(area.x + col as u16, area.y + row as u16)];
                cell.set_char('▀');
                cell.set_fg(Color::Rgb(top.r, top.g, top.b));
                cell.set_bg(Color::Rgb(bot.r, bot.g, bot.b));
            }
        }
    }
}
