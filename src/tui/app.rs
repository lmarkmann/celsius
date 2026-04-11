//! Event loop: alternate screen + raw mode, tick-based redraw, scrub keys,
//! graceful minimum-size fallback. Resize is handled implicitly: every draw
//! re-samples `frame.area()` and re-renders the sky at the current size, so
//! pulling the terminal corner just changes the next frame.

use std::io::{self, Stdout, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::render::render;
use crate::scene::SkyState;
use crate::tui::widget::SkyWidget;

const TICK: Duration = Duration::from_millis(33);
const MIN_COLS: u16 = 40;
const MIN_ROWS: u16 = 20;

/// A scrubable timeline of pre-composed sky states. `home` is the index that
/// the `t` key snaps back to (typically the hour matching wall-clock now).
pub struct Timeline {
    pub states: Vec<SkyState>,
    pub home: usize,
}

impl Timeline {
    pub fn single(state: SkyState) -> Self {
        Self {
            states: vec![state],
            home: 0,
        }
    }

    pub fn new(states: Vec<SkyState>, home: usize) -> Self {
        let home = home.min(states.len().saturating_sub(1));
        Self { states, home }
    }

    fn len(&self) -> usize {
        self.states.len()
    }

    fn clamp(&self, idx: isize) -> usize {
        idx.clamp(0, self.len() as isize - 1) as usize
    }
}

pub fn run(timeline: &Timeline) -> Result<()> {
    let mut terminal = enter_terminal().context("entering alternate screen")?;
    let _guard = RestoreGuard;

    let mut index = timeline.home;
    let mut last_tick = Instant::now();
    loop {
        let state = &timeline.states[index];
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw(buf, area, state);
            })
            .context("drawing frame")?;

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")? {
            match event::read().context("reading input")? {
                Event::Key(key) if is_quit(&key) => break,
                Event::Key(key) => handle_scrub(&key, &mut index, timeline),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

struct RestoreGuard;

impl Drop for RestoreGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = stdout.flush();
    }
}

fn is_quit(key: &KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

fn handle_scrub(key: &KeyEvent, index: &mut usize, timeline: &Timeline) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    let cur = *index as isize;
    let new = match key.code {
        KeyCode::Left => cur - 1,
        KeyCode::Right => cur + 1,
        KeyCode::Tab => cur + 24,
        KeyCode::BackTab => cur - 24,
        KeyCode::Char('t') => timeline.home as isize,
        _ => return,
    };
    *index = timeline.clamp(new);
}

fn draw(buf: &mut Buffer, area: Rect, state: &SkyState) {
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        draw_too_small(buf, area);
        return;
    }
    let px_width = area.width as u32;
    let px_height = (area.height as u32) * 2;
    let pixels = render(state, px_width, px_height);
    SkyWidget { pixels: &pixels }.render(area, buf);
}

fn draw_too_small(buf: &mut Buffer, area: Rect) {
    let msg = format!(
        "cramped sky: needs {}x{}, yours is {}x{}",
        MIN_COLS, MIN_ROWS, area.width, area.height
    );
    let msg_len = msg.chars().count() as u16;
    let row = area.y + area.height / 2;
    let start_x = area.x + area.width.saturating_sub(msg_len) / 2;
    for (i, ch) in msg.chars().enumerate() {
        let x = start_x + i as u16;
        if x >= area.x + area.width {
            break;
        }
        let cell = &mut buf[(x, row)];
        cell.set_char(ch);
        cell.set_fg(Color::Gray);
        cell.set_bg(Color::Black);
    }
}
