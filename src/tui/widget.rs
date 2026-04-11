//! PixelBuffer -> ratatui half-block widget.
//!
//! Each terminal cell holds two vertically stacked pixels: the top pixel is
//! the foreground color of `▀`, the bottom pixel is the background. A
//! PixelBuffer sized `W x 2H` fills a `W x H` cell area exactly. This matches
//! the lab's authoring aspect (104x50 pixels -> 104x25 cells) and lets the
//! sky fill any terminal size without stretching.

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
