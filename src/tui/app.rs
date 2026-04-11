//! Event loop: alternate screen + raw mode, tick-based redraw, scrub keys,
//! graceful minimum-size fallback, modal overlays.
//!
//! Resize is handled implicitly: every draw re-samples `frame.area()` and
//! re-renders the sky at the current size.

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
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::render::render;
use crate::scene::SkyState;
use crate::tui::widget::SkyWidget;

const TICK: Duration = Duration::from_millis(33);
const MIN_COLS: u16 = 40;
const MIN_ROWS: u16 = 20;

/// Returned by [`run`] to tell the caller what to do next.
pub enum RunOutcome {
    Quit,
    /// User pressed `r` -- caller should re-fetch weather and call `run` again.
    Retry,
    /// User submitted a new location name from the `l` overlay.
    ChangeLocation(String),
}

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

enum Overlay {
    None,
    Help,
    Location { input: String },
}

pub fn run(timeline: &Timeline) -> Result<RunOutcome> {
    let mut terminal = enter_terminal().context("entering alternate screen")?;
    let _guard = RestoreGuard;

    let mut index = timeline.home;
    let mut display = timeline.states[index].clone();
    let mut drift_paused = false;
    let mut overlay = Overlay::None;
    let mut last_tick = Instant::now();

    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw_sky(buf, area, &display);
                match &overlay {
                    Overlay::Help => draw_help_overlay(buf, area),
                    Overlay::Location { input } => draw_location_overlay(buf, area, input),
                    Overlay::None => {}
                }
            })
            .context("drawing frame")?;

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")? {
            match event::read().context("reading input")? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match &mut overlay {
                        Overlay::Help => {
                            // any key closes help
                            overlay = Overlay::None;
                        }
                        Overlay::Location { input } => {
                            match key.code {
                                KeyCode::Esc => overlay = Overlay::None,
                                KeyCode::Enter if !input.trim().is_empty() => {
                                    let name = input.trim().to_string();
                                    return Ok(RunOutcome::ChangeLocation(name));
                                }
                                KeyCode::Char(ch) => input.push(ch),
                                KeyCode::Backspace => {
                                    input.pop();
                                }
                                _ => {}
                            }
                        }
                        Overlay::None => match key.code {
                            _ if is_quit_key(&key) => return Ok(RunOutcome::Quit),
                            KeyCode::Char(' ') => drift_paused = !drift_paused,
                            KeyCode::Char('?') => overlay = Overlay::Help,
                            KeyCode::Char('l') => {
                                overlay = Overlay::Location {
                                    input: String::new(),
                                }
                            }
                            KeyCode::Char('r') => return Ok(RunOutcome::Retry),
                            _ => {
                                let new = scrub_index(&key, index, timeline);
                                if new != index {
                                    index = new;
                                    display = timeline.states[index].clone();
                                }
                            }
                        },
                    }
                }
                Event::Resize(_, _) | Event::Key(_) => {}
                _ => {}
            }
        }

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64();
            last_tick = Instant::now();
            if !drift_paused {
                let delta = display.wind_speed_kmh * dt * 0.0001;
                for layer in &mut display.clouds {
                    layer.offset_x += delta;
                }
            }
        }
    }
}

/// Full-screen location prompt shown on first run (no config, no flags).
pub fn prompt_location() -> Result<String> {
    let mut terminal = enter_terminal().context("entering alternate screen for prompt")?;
    let _guard = RestoreGuard;

    let mut input = String::new();
    loop {
        let display = input.clone();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_location_overlay(frame.buffer_mut(), area, &display);
            })
            .context("drawing prompt")?;

        if let Event::Key(key) = event::read().context("reading input")? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter if !input.trim().is_empty() => return Ok(input.trim().to_string()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    std::process::exit(0)
                }
                KeyCode::Esc => std::process::exit(0),
                KeyCode::Char(ch) => input.push(ch),
                KeyCode::Backspace => {
                    input.pop();
                }
                _ => {}
            }
        }
    }
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

fn is_quit_key(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

fn scrub_index(key: &KeyEvent, index: usize, timeline: &Timeline) -> usize {
    let cur = index as isize;
    let new = match key.code {
        KeyCode::Left => cur - 1,
        KeyCode::Right => cur + 1,
        KeyCode::Tab => cur + 24,
        KeyCode::BackTab => cur - 24,
        KeyCode::Char('t') => timeline.home as isize,
        _ => return index,
    };
    timeline.clamp(new)
}

const CHROME_BG: Color = Color::Rgb(14, 14, 14);
const CHROME_FG: Color = Color::Rgb(140, 140, 140);
const OVERLAY_BG: Color = Color::Rgb(18, 18, 26);
const OVERLAY_FG: Color = Color::Rgb(210, 210, 210);
const OVERLAY_DIM: Color = Color::Rgb(120, 120, 140);

fn draw_sky(buf: &mut Buffer, area: Rect, state: &SkyState) {
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        draw_too_small(buf, area);
        return;
    }
    let [header, sky_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_chrome_bar(
        buf,
        header,
        &state.chrome.header_left,
        &state.chrome.header_right,
    );
    draw_chrome_bar(buf, footer, &state.chrome.footer, &state.chrome.keys);

    let px_width = sky_area.width as u32;
    let px_height = (sky_area.height as u32) * 2;
    let pixels = render(state, px_width, px_height);
    SkyWidget { pixels: &pixels }.render(sky_area, buf);
}

fn draw_chrome_bar(buf: &mut Buffer, area: Rect, left: &str, right: &str) {
    for x in area.x..area.x + area.width {
        let cell = &mut buf[(x, area.y)];
        cell.set_char(' ');
        cell.set_bg(CHROME_BG);
        cell.set_fg(CHROME_FG);
    }
    let right_chars: Vec<char> = right.chars().collect();
    let right_len = right_chars.len() as u16;
    let right_start = (area.x + area.width).saturating_sub(right_len);
    let max_left = right_start.saturating_sub(area.x + 1) as usize;
    for (i, ch) in left.chars().enumerate().take(max_left) {
        buf[(area.x + i as u16, area.y)].set_char(ch);
    }
    for (i, ch) in right_chars.into_iter().enumerate() {
        let x = right_start + i as u16;
        if x < area.x + area.width {
            buf[(x, area.y)].set_char(ch);
        }
    }
}

/// Draw a solid-background box, return the inner Rect (inset by 2 on each side).
fn draw_overlay_box(buf: &mut Buffer, area: Rect, w: u16, h: u16) -> Rect {
    let bx = area.x + area.width.saturating_sub(w) / 2;
    let by = area.y + area.height.saturating_sub(h) / 2;
    let bw = w.min(area.width);
    let bh = h.min(area.height);
    for y in by..by + bh {
        for x in bx..bx + bw {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(OVERLAY_BG);
            cell.set_fg(OVERLAY_FG);
        }
    }
    Rect {
        x: bx + 2,
        y: by + 1,
        width: bw.saturating_sub(4),
        height: bh.saturating_sub(2),
    }
}

fn put_str(buf: &mut Buffer, x: u16, y: u16, max_w: u16, s: &str, fg: Color) {
    for (i, ch) in s.chars().enumerate().take(max_w as usize) {
        let cell = &mut buf[(x + i as u16, y)];
        cell.set_char(ch);
        cell.set_fg(fg);
        cell.set_bg(OVERLAY_BG);
    }
}

const HELP_LINES: &[(&str, &str)] = &[
    ("←  →", "scrub one hour"),
    ("tab  shift+tab", "+24h / -24h"),
    ("t", "jump to now"),
    ("space", "pause / resume drift"),
    ("l", "change location"),
    ("r", "retry weather fetch"),
    ("?", "this help"),
    ("q  esc", "quit"),
];

fn draw_help_overlay(buf: &mut Buffer, area: Rect) {
    let inner = draw_overlay_box(buf, area, 42, (HELP_LINES.len() as u16) + 4);
    let mut row = inner.y;
    put_str(buf, inner.x, row, inner.width, "keybindings", OVERLAY_FG);
    row += 2;
    for (key, desc) in HELP_LINES {
        let col_w = 18u16;
        put_str(buf, inner.x, row, col_w, key, OVERLAY_FG);
        put_str(
            buf,
            inner.x + col_w,
            row,
            inner.width.saturating_sub(col_w),
            desc,
            OVERLAY_DIM,
        );
        row += 1;
        if row >= inner.y + inner.height {
            break;
        }
    }
}

fn draw_location_overlay(buf: &mut Buffer, area: Rect, input: &str) {
    let inner = draw_overlay_box(buf, area, 46, 7);
    let mut row = inner.y;
    put_str(buf, inner.x, row, inner.width, "change location", OVERLAY_FG);
    row += 2;
    put_str(buf, inner.x, row, inner.width, "enter place name:", OVERLAY_DIM);
    row += 1;
    let cursor = format!("{input}_");
    put_str(buf, inner.x, row, inner.width, &cursor, OVERLAY_FG);
    row += 2;
    put_str(
        buf,
        inner.x,
        row,
        inner.width,
        "enter confirm   esc cancel",
        OVERLAY_DIM,
    );
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
