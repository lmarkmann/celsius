use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::lightning;
use crate::render::render;
use crate::scene::SkyState;
use crate::tui::widget::SkyWidget;

const TICK: Duration = Duration::from_millis(33);
const MIN_COLS: u16 = 40;
const MIN_ROWS: u16 = 20;

#[derive(Debug, PartialEq)]
pub enum RunOutcome {
    Quit,
    Retry,
    ChangeLocation(String),
}

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

/// The interactive state. Render reads it; key/tick handlers are the only
/// places it mutates. Keeping it terminal-free is what makes the handlers
/// testable without a real terminal (see the tests at the bottom of this file).
pub struct App<'a> {
    timeline: &'a Timeline,
    index: usize,
    display: SkyState,
    drift_paused: bool,
    overlay: Overlay,
    lightning_elapsed: Duration,
    outcome: Option<RunOutcome>,
}

impl<'a> App<'a> {
    pub fn new(timeline: &'a Timeline) -> Self {
        let index = timeline.home;
        let display = timeline.states[index].clone();
        Self {
            timeline,
            index,
            display,
            drift_paused: false,
            overlay: Overlay::None,
            lightning_elapsed: Duration::ZERO,
            outcome: None,
        }
    }

    /// Advance the animation by `elapsed`. Cloud drift and the lightning clock
    /// both freeze while paused, so a paused sky is fully still.
    pub fn tick(&mut self, elapsed: Duration) {
        if self.drift_paused {
            return;
        }
        let delta = self.display.wind_speed_kmh * elapsed.as_secs_f64() * 0.0001;
        for layer in &mut self.display.clouds {
            layer.offset_x += delta;
        }
        self.lightning_elapsed += elapsed;
    }

    pub fn lightning_t(&self) -> f64 {
        self.lightning_elapsed.as_secs_f64()
    }

    /// Dispatch a key press. Quit / Retry / ChangeLocation land in `outcome`
    /// for the event loop to drain; everything else mutates in place. Assumes
    /// the caller already filtered to `KeyEventKind::Press`.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.overlay {
            Overlay::Help => self.overlay = Overlay::None,
            Overlay::Location { .. } => self.handle_location_key(key),
            Overlay::None => self.handle_main_key(key),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) {
        if is_quit_key(&key) {
            self.outcome = Some(RunOutcome::Quit);
            return;
        }
        match key.code {
            KeyCode::Char(' ') => self.drift_paused = !self.drift_paused,
            KeyCode::Char('?') => self.overlay = Overlay::Help,
            KeyCode::Char('l') => {
                self.overlay = Overlay::Location {
                    input: String::new(),
                }
            }
            KeyCode::Char('r') => self.outcome = Some(RunOutcome::Retry),
            _ => {
                let new = scrub_index(&key, self.index, self.timeline);
                if new != self.index {
                    self.index = new;
                    self.display = self.timeline.states[new].clone();
                }
            }
        }
    }

    fn handle_location_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Enter => {
                if let Overlay::Location { input } = &self.overlay {
                    let name = input.trim();
                    if !name.is_empty() {
                        self.outcome = Some(RunOutcome::ChangeLocation(name.to_string()));
                    }
                }
            }
            KeyCode::Char(ch) => {
                if let Overlay::Location { input } = &mut self.overlay {
                    input.push(ch);
                }
            }
            KeyCode::Backspace => {
                if let Overlay::Location { input } = &mut self.overlay {
                    input.pop();
                }
            }
            _ => {}
        }
    }
}

pub fn run(timeline: &Timeline) -> Result<RunOutcome> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut App::new(timeline));
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<RunOutcome> {
    let mut last_tick = Instant::now();
    loop {
        let lightning_t = app.lightning_t();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw_sky(buf, area, &app.display, lightning_t);
                match &app.overlay {
                    Overlay::Help => draw_help_overlay(buf, area),
                    Overlay::Location { input } => draw_location_overlay(buf, area, input),
                    Overlay::None => {}
                }
            })
            .context("drawing frame")?;

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")?
            && let Event::Key(key) = event::read().context("reading input")?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key);
            if let Some(outcome) = app.outcome.take() {
                return Ok(outcome);
            }
        }

        if last_tick.elapsed() >= TICK {
            let elapsed = last_tick.elapsed();
            last_tick = Instant::now();
            app.tick(elapsed);
        }
    }
}

pub fn prompt_location() -> Result<String> {
    let mut terminal = ratatui::init();
    let result = prompt_loop(&mut terminal);
    ratatui::restore();
    match result? {
        Some(name) => Ok(name),
        // Cancelled. The terminal is already restored above, so exiting here is
        // clean; doing it inside the loop would skip restore and corrupt the
        // terminal (the bug this rewrite fixes).
        None => std::process::exit(0),
    }
}

fn prompt_loop(terminal: &mut DefaultTerminal) -> Result<Option<String>> {
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
                KeyCode::Enter if !input.trim().is_empty() => {
                    return Ok(Some(input.trim().to_string()));
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Esc => return Ok(None),
                KeyCode::Char(ch) => input.push(ch),
                KeyCode::Backspace => {
                    input.pop();
                }
                _ => {}
            }
        }
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

fn draw_sky(buf: &mut Buffer, area: Rect, state: &SkyState, lightning_t: f64) {
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
    let mut pixels = render(state, px_width, px_height);
    if let Some(lt) = &state.lightning {
        lightning::overlay(&mut pixels, lt, lightning_t);
    }
    SkyWidget { pixels: &pixels }.render(sky_area, buf);
}

fn draw_chrome_bar(buf: &mut Buffer, area: Rect, left: &str, right: &str) {
    for x in area.x..area.x + area.width {
        let cell = &mut buf[(x, area.y)];
        cell.set_char(' ');
        cell.set_bg(CHROME_BG);
        cell.set_fg(CHROME_FG);
    }
    let style = Style::default().fg(CHROME_FG).bg(CHROME_BG);
    // Measure by display width, not char count, so wide glyphs (CJK place
    // names) right-justify correctly. set_stringn clips to the column budget
    // and handles wide-cell placement, which a per-char set_char loop cannot.
    let right_w = right.width() as u16;
    let right_start = (area.x + area.width).saturating_sub(right_w);
    let max_left = right_start.saturating_sub(area.x + 1);
    buf.set_stringn(area.x, area.y, left, max_left as usize, style);
    buf.set_stringn(right_start, area.y, right, right_w as usize, style);
}

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
    let style = Style::default().fg(fg).bg(OVERLAY_BG);
    buf.set_stringn(x, y, s, max_w as usize, style);
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
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(OVERLAY_BG);
        }
    }
    let inner = draw_overlay_box(buf, area, 46, 7);
    let mut row = inner.y;
    put_str(
        buf,
        inner.x,
        row,
        inner.width,
        "change location",
        OVERLAY_FG,
    );
    row += 2;
    put_str(
        buf,
        inner.x,
        row,
        inner.width,
        "enter place name:",
        OVERLAY_DIM,
    );
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
    let msg_w = msg.width() as u16;
    let row = area.y + area.height / 2;
    let start_x = area.x + area.width.saturating_sub(msg_w) / 2;
    let style = Style::default().fg(Color::Gray).bg(Color::Black);
    buf.set_stringn(start_x, row, &msg, area.width as usize, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradient::Gradient;
    use crate::scene::{Chrome, CloudKind, CloudLayer, Sun};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sky(name: &str) -> SkyState {
        SkyState {
            name: name.to_string(),
            gradient: Gradient::from_rgb_stops(&[(0.0, [10, 10, 30]), (1.0, [200, 180, 160])]),
            sun: Sun {
                x_frac: 0.5,
                y_frac: 0.5,
                radius: 2.0,
                visible: true,
            },
            clouds: vec![CloudLayer {
                cover: 1.0,
                altitude_t: 0.4,
                altitude_sigma: 0.1,
                scale_x: 3.0,
                scale_y: 2.0,
                threshold: 0.5,
                seed: 101,
                kind: CloudKind::Generic,
                flatten: 0.0,
                offset_x: 0.0,
                offset_y: 1.3,
            }],
            chrome: Chrome {
                header_left: "celsius".into(),
                header_right: "testville   today 12:00".into(),
                footer: "10°  clear   wind n 5".into(),
                keys: "q quit".into(),
                status: "Testville 10C clear wind N 5".into(),
            },
            haze: None,
            stars: None,
            moon: None,
            precipitation: None,
            lightning: None,
            horizon_glow: None,
            wind_speed_kmh: 20.0,
        }
    }

    fn timeline() -> Timeline {
        Timeline::new((0..50).map(|i| sky(&format!("s{i}"))).collect(), 10)
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn buffer_text(buf: &Buffer) -> String {
        let area = buf.area;
        let mut s = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn tab_advances_a_day_and_reclones_display() {
        let tl = timeline();
        let mut app = App::new(&tl);
        assert_eq!(app.index, 10);
        app.handle_key(press(KeyCode::Tab));
        assert_eq!(app.index, 34);
        assert_eq!(app.display.name, "s34");
    }

    #[test]
    fn tab_clamps_at_the_end() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Tab));
        app.handle_key(press(KeyCode::Tab));
        assert_eq!(app.index, 49);
    }

    #[test]
    fn arrows_scrub_one_hour_each() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Right));
        assert_eq!(app.index, 11);
        app.handle_key(press(KeyCode::Left));
        app.handle_key(press(KeyCode::Left));
        assert_eq!(app.index, 9);
    }

    #[test]
    fn t_jumps_back_to_home() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Tab));
        app.handle_key(press(KeyCode::Char('t')));
        assert_eq!(app.index, 10);
    }

    #[test]
    fn space_toggles_drift_pause() {
        let tl = timeline();
        let mut app = App::new(&tl);
        assert!(!app.drift_paused);
        app.handle_key(press(KeyCode::Char(' ')));
        assert!(app.drift_paused);
        app.handle_key(press(KeyCode::Char(' ')));
        assert!(!app.drift_paused);
    }

    #[test]
    fn quit_keys_set_quit() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let tl = timeline();
            let mut app = App::new(&tl);
            app.handle_key(press(code));
            assert_eq!(app.outcome, Some(RunOutcome::Quit));
        }
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(app.outcome, Some(RunOutcome::Quit));
    }

    #[test]
    fn r_sets_retry() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('r')));
        assert_eq!(app.outcome, Some(RunOutcome::Retry));
    }

    #[test]
    fn help_opens_and_any_key_closes() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('?')));
        assert!(matches!(app.overlay, Overlay::Help));
        app.handle_key(press(KeyCode::Char('x')));
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.outcome, None);
    }

    #[test]
    fn location_overlay_accumulates_edits_and_confirms() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('l')));
        assert!(matches!(app.overlay, Overlay::Location { .. }));
        for ch in "osloo".chars() {
            app.handle_key(press(KeyCode::Char(ch)));
        }
        app.handle_key(press(KeyCode::Backspace));
        app.handle_key(press(KeyCode::Enter));
        assert_eq!(
            app.outcome,
            Some(RunOutcome::ChangeLocation("oslo".to_string()))
        );
    }

    #[test]
    fn location_overlay_esc_cancels_without_outcome() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('l')));
        app.handle_key(press(KeyCode::Char('x')));
        app.handle_key(press(KeyCode::Esc));
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.outcome, None);
    }

    #[test]
    fn tick_drifts_clouds_running_and_freezes_when_paused() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let before = app.display.clouds[0].offset_x;
        app.tick(Duration::from_millis(100));
        assert!(
            app.display.clouds[0].offset_x > before,
            "clouds should drift while running"
        );

        app.drift_paused = true;
        let held = app.display.clouds[0].offset_x;
        app.tick(Duration::from_millis(100));
        assert_eq!(
            app.display.clouds[0].offset_x, held,
            "a paused sky is fully still"
        );
    }

    #[test]
    fn help_overlay_renders_keybindings() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('?')));
        let mut term = Terminal::new(TestBackend::new(80, 40)).unwrap();
        term.draw(|f| {
            let area = f.area();
            let buf = f.buffer_mut();
            draw_sky(buf, area, &app.display, 0.0);
            draw_help_overlay(buf, area);
        })
        .unwrap();
        assert!(buffer_text(term.backend().buffer()).contains("keybindings"));
    }

    #[test]
    fn cramped_area_shows_the_warning() {
        let tl = timeline();
        let app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(20, 10)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &app.display, 0.0);
        })
        .unwrap();
        assert!(buffer_text(term.backend().buffer()).contains("cramped sky"));
    }

    #[test]
    fn chrome_shows_location() {
        let tl = timeline();
        let app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(80, 40)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &app.display, 0.0);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(content.contains("celsius"));
        assert!(content.contains("testville"));
    }
}
