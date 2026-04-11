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
use ratatui::layout::{Constraint, Layout, Rect};
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
    let mut display = timeline.states[index].clone();
    let mut drift_paused = false;
    let mut last_tick = Instant::now();

    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw(buf, area, &display);
            })
            .context("drawing frame")?;

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")? {
            match event::read().context("reading input")? {
                Event::Key(key) if is_quit(&key) => break,
                Event::Key(key) if is_space(&key) => drift_paused = !drift_paused,
                Event::Key(key) => {
                    let new = scrub_index(&key, index, timeline);
                    if new != index {
                        index = new;
                        display = timeline.states[index].clone();
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64();
            last_tick = Instant::now();
            if !drift_paused {
                // 10 km/h wind moves clouds ~0.001 noise units/s; visible over ~15 s
                let delta = display.wind_speed_kmh * dt * 0.0001;
                for layer in &mut display.clouds {
                    layer.offset_x += delta;
                }
            }
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

fn scrub_index(key: &KeyEvent, index: usize, timeline: &Timeline) -> usize {
    if key.kind != KeyEventKind::Press {
        return index;
    }
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

fn is_space(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press && key.code == KeyCode::Char(' ')
}

const CHROME_BG: Color = Color::Rgb(14, 14, 14);
const CHROME_FG: Color = Color::Rgb(140, 140, 140);

fn draw(buf: &mut Buffer, area: Rect, state: &SkyState) {
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

/// Full-screen location prompt shown on first run (no config, no flags).
///
/// Enters alternate screen, draws a centered input box, collects a UTF-8
/// place name, and returns it on Enter. Esc or Ctrl+C exits the process.
pub fn prompt_location() -> Result<String> {
    let mut terminal = enter_terminal().context("entering alternate screen for prompt")?;
    let _guard = RestoreGuard;

    let mut input = String::new();
    loop {
        let display = input.clone();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw_location_prompt(buf, area, &display);
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

fn draw_location_prompt(buf: &mut Buffer, area: Rect, input: &str) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(Color::Rgb(10, 10, 14));
        }
    }
    let label = "enter a location:";
    let cursor = format!("{input}_");
    let box_w = (label.len().max(cursor.len()) + 8) as u16;
    let box_h = 5u16;
    let bx = area.x + area.width.saturating_sub(box_w) / 2;
    let by = area.y + area.height.saturating_sub(box_h) / 2;

    // fill box
    for y in by..by + box_h {
        for x in bx..bx + box_w {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(Color::Rgb(24, 24, 32));
            cell.set_fg(Color::Rgb(200, 200, 200));
        }
    }
    // label row
    for (i, ch) in label.chars().enumerate() {
        let x = bx + 4 + i as u16;
        if x < bx + box_w {
            buf[(x, by + 1)].set_char(ch);
        }
    }
    // input row
    for (i, ch) in cursor.chars().enumerate() {
        let x = bx + 4 + i as u16;
        if x < bx + box_w {
            let cell = &mut buf[(x, by + 3)];
            cell.set_char(ch);
            cell.set_fg(Color::Rgb(255, 255, 255));
        }
    }
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
