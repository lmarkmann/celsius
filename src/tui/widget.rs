//! The sky widget: one terminal cell per glyph, whatever the glyph turns out to be.
//!
//! A terminal cell is roughly twice as tall as it is wide, so drawing one pixel per cell gives a squashed sky. Printing `▀` with the upper pixel as foreground and the lower as background packs two rows into one cell, which both doubles vertical resolution and squares the aspect ratio. A 104x50 buffer therefore occupies 104 columns and 25 rows.
//!
//! This is why every scene constant is expressed against a 104x50 pixel buffer rather than against terminal rows.
//!
//! Which glyph a cell gets is no longer fixed, but the geometry above is: every shipped sub-cell family is two subpixels deep, so a cell is always two logical pixels tall and the reference size means what it always meant. All the choosing happens in `raster`; this widget only places the result.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::PixelBuffer;
use crate::raster::{CellColor, RasterOpts, cell_at, sample_factor};

pub struct SkyWidget<'a> {
    pub pixels: &'a PixelBuffer,
    pub opts: RasterOpts,
}

impl Widget for SkyWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = area.width as usize;
        let rows = area.height as usize;
        let factor = sample_factor(self.opts) as usize;
        debug_assert_eq!(
            self.pixels.width,
            cols * factor,
            "pixel width must be cols * the sample factor these options ask for"
        );
        debug_assert_eq!(
            self.pixels.height,
            rows * 2 * factor,
            "pixel height must be 2 * rows * the sample factor these options ask for"
        );

        for row in 0..rows {
            for col in 0..cols {
                let sky = cell_at(self.pixels, self.opts, col, row);
                let cell = &mut buf[(area.x + col as u16, area.y + row as u16)];
                cell.set_char(sky.glyph);
                cell.set_fg(style(sky.fg));
                cell.set_bg(style(sky.bg));
            }
        }
    }
}

/// The two `CellColor` forms map onto the two ratatui colours that emit the two SGR forms, which is the only reason the distinction is carried this far down.
fn style(c: CellColor) -> Color {
    match c {
        CellColor::Rgb(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        CellColor::Indexed(i) => Color::Indexed(i),
    }
}
