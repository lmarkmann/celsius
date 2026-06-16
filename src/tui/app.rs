use std::io::{self, stdout};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

use crate::colorspace::PixelBuffer;
use crate::lightning;
use crate::pigs;
use crate::render::render;
use crate::scene::{Chrome, SkyState};
use crate::tui::widget::SkyWidget;
use crate::weather::location::{GeoResult, geocode, rank};

const TICK: Duration = Duration::from_millis(33);
const MIN_COLS: u16 = 60;
const MIN_ROWS: u16 = 25;

#[derive(Debug, PartialEq)]
pub enum RunOutcome {
    Quit,
    Retry,
    /// The user pressed `l`; the caller runs the location search modal.
    ChangeLocation,
}

pub struct Timeline {
    pub states: Vec<SkyState>,
    pub home: usize,
    /// Viewer coordinates when known; drives the location-gated easter egg.
    /// `None` for scene files and error skies, which never trigger it.
    pub coords: Option<(f64, f64)>,
    /// The viewed location's UTC offset in seconds, so the local-time gate uses
    /// the sky's wall clock rather than the machine's. `0` when unknown.
    pub offset: i64,
}

impl Timeline {
    pub fn single(state: SkyState) -> Self {
        Self {
            states: vec![state],
            home: 0,
            coords: None,
            offset: 0,
        }
    }

    pub fn new(
        states: Vec<SkyState>,
        home: usize,
        coords: Option<(f64, f64)>,
        offset: i64,
    ) -> Self {
        let home = home.min(states.len().saturating_sub(1));
        Self {
            states,
            home,
            coords,
            offset,
        }
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
}

/// The last sky render, reused while nothing visual changes. The pixel
/// pipeline is the expensive part of a frame; ratatui's cell diff already
/// makes repeat draws free downstream of it.
struct SkyCache {
    width: u32,
    height: u32,
    pixels: PixelBuffer,
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
    egg_frame: u64,
    egg_prev: bool,
    outcome: Option<RunOutcome>,
    sky_dirty: bool,
    sky_cache: Option<SkyCache>,
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
            egg_frame: 0,
            egg_prev: false,
            outcome: None,
            sky_dirty: true,
            sky_cache: None,
        }
    }

    /// Whether the flying-pigs egg should show right now: at Kowloon Tong and
    /// inside the 01:28-02:10 local-time window. Recomputed live so it turns on
    /// and off as the real clock crosses the window edges.
    fn egg_active(&self) -> bool {
        let offset = self.timeline.offset;
        self.timeline
            .coords
            .is_some_and(|(lat, lon)| pigs::gate_open(lat, lon, offset))
    }

    /// Advance the animation by `elapsed` and report whether the visible frame
    /// changed, so the event loop can skip redrawing an identical one. Cloud
    /// drift and the lightning clock both freeze while paused, so a paused sky
    /// is fully still. Otherwise the frame changes when clouds drift, and on
    /// every tick of a lightning scene, whose flash is recomputed per frame.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if self.drift_paused {
            return false;
        }
        let mut changed = false;
        let delta = self.display.wind_speed_kmh * elapsed.as_secs_f64() * 0.0001;
        if delta != 0.0 && !self.display.clouds.is_empty() {
            for layer in &mut self.display.clouds {
                layer.offset_x += delta;
            }
            self.sky_dirty = true;
            changed = true;
        }
        self.lightning_elapsed += elapsed;
        self.egg_frame = self.egg_frame.wrapping_add(1);
        // While the egg flies, every tick is a new frame. The transition catches
        // the moment it turns off, so one last redraw clears it from the sky.
        let egg = self.egg_active();
        let egg_changed = egg != self.egg_prev;
        self.egg_prev = egg;
        changed || self.display.lightning.is_some() || egg || egg_changed
    }

    /// Dispatch a key press. Quit / Retry / ChangeLocation land in `outcome`
    /// for the event loop to drain; everything else mutates in place. Assumes
    /// the caller already filtered to `KeyEventKind::Press`.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.overlay {
            Overlay::Help => self.overlay = Overlay::None,
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
            KeyCode::Char('l') => self.outcome = Some(RunOutcome::ChangeLocation),
            KeyCode::Char('r') => self.outcome = Some(RunOutcome::Retry),
            _ => {
                let new = scrub_index(&key, self.index, self.timeline);
                if new != self.index {
                    self.index = new;
                    self.display = self.timeline.states[new].clone();
                    self.sky_dirty = true;
                }
            }
        }
    }
}

/// Owns the terminal for one interactive run: a single alternate-screen enter on
/// `new`, a single leave on `Drop`. The live sky, the location search, and the
/// loading screen all draw to this one surface, so the view never tears down and
/// flashes the shell between them, nor sits bare while a forecast fetches.
pub struct Session {
    terminal: DefaultTerminal,
}

impl Session {
    pub fn new() -> Self {
        Self {
            terminal: ratatui::init(),
        }
    }

    pub fn run(&mut self, timeline: &Timeline) -> Result<RunOutcome> {
        event_loop(&mut self.terminal, &mut App::new(timeline))
    }

    pub fn search_location(&mut self) -> Result<Option<GeoResult>> {
        search_loop(&mut self.terminal)
    }

    /// Hold the sky on screen (drawing `current`, or a plain fill on first
    /// launch) under a loading box while `rx` delivers the next timeline.
    pub fn await_timeline(
        &mut self,
        current: Option<&Timeline>,
        label: &str,
        rx: mpsc::Receiver<Timeline>,
    ) -> Result<Option<Timeline>> {
        await_loop(&mut self.terminal, current, label, rx)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<RunOutcome> {
    let mut last_tick = Instant::now();
    // Draw only when something changed. A still sky then sits idle instead of
    // repainting 30 times a second, and a burst of resize events collapses into
    // one repaint (see the drain loop below).
    let mut needs_redraw = true;
    loop {
        if needs_redraw {
            draw_synchronized(terminal, |frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                draw_sky(buf, area, app);
                match &app.overlay {
                    Overlay::Help => draw_help_overlay(buf, area),
                    Overlay::None => {}
                }
            })
            .context("drawing frame")?;
            needs_redraw = false;
        }

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")? {
            // Drain the whole queue before redrawing. During a window drag the
            // terminal floods us with Resize events; coalescing them means one
            // repaint at the final size, not one per intermediate size.
            loop {
                match event::read().context("reading input")? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        app.handle_key(key);
                        if let Some(outcome) = app.outcome.take() {
                            return Ok(outcome);
                        }
                        needs_redraw = true;
                    }
                    Event::Resize(..) => needs_redraw = true,
                    _ => {}
                }
                if !event::poll(Duration::ZERO).context("polling input")? {
                    break;
                }
            }
        }

        if last_tick.elapsed() >= TICK {
            let elapsed = last_tick.elapsed();
            last_tick = Instant::now();
            if app.tick(elapsed) {
                needs_redraw = true;
            }
        }
    }
}

/// Draw one frame inside a DEC 2026 synchronized update so the terminal presents
/// it atomically. ratatui's resize path clears the screen and resets its diff
/// buffer (a `\e[2J` plus a full repaint); without batching, the gap between the
/// clear and the repaint shows as a flash while a window is dragged. Emulators
/// that lack 2026 ignore the toggles, so this is safe everywhere; the markers
/// are best-effort, and the frame still draws if they fail to write.
fn draw_synchronized<F>(terminal: &mut DefaultTerminal, render: F) -> io::Result<()>
where
    F: FnOnce(&mut Frame),
{
    let _ = execute!(stdout(), BeginSynchronizedUpdate);
    let result = terminal.draw(render);
    let _ = execute!(stdout(), EndSynchronizedUpdate);
    result.map(|_| ())
}

/// How many candidate rows are visible at once; the rest scroll into view.
const SEARCH_WINDOW: usize = 5;
/// Wait this long after the last keystroke before geocoding, so a fast typist
/// makes one request instead of one per character.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(220);
/// Shortest query worth a request.
const MIN_QUERY: usize = 2;
/// How often the loop wakes to drain results and fire debounced searches.
const SEARCH_POLL: Duration = Duration::from_millis(40);
/// Keep retrying a failing query for this long (a slow or just-waking network)
/// before settling on "no connection". The agent's own 5s connect timeout means
/// a single slow attempt already eats most of this; fast failures get retried.
const OFFLINE_AFTER: Duration = Duration::from_millis(2500);

/// Spinner phases for the loading box; the quarter-circle glyphs read as a spin.
const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// Keep the sky on screen under a centered loading box while a background thread
/// fetches the next timeline, returning it the moment `rx` delivers, or `None`
/// if the user cancels with esc. Drawing `current` (or a plain fill on first
/// launch) here is what replaces the old bare-shell gap during a fetch.
fn await_loop(
    terminal: &mut DefaultTerminal,
    current: Option<&Timeline>,
    label: &str,
    rx: mpsc::Receiver<Timeline>,
) -> Result<Option<Timeline>> {
    let mut app = current.map(App::new);
    let mut last_tick = Instant::now();
    let mut spinner = 0usize;
    loop {
        draw_synchronized(terminal, |frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            match &mut app {
                Some(app) => draw_sky(buf, area, app),
                None => fill(buf, area, OVERLAY_BG),
            }
            draw_loading_overlay(buf, area, label, SPINNER[spinner % SPINNER.len()]);
        })
        .context("drawing loading screen")?;

        match rx.try_recv() {
            Ok(timeline) => return Ok(Some(timeline)),
            // The worker dropped its sender without a value (a panic while
            // composing): fall back to the current sky rather than spin forever.
            Err(mpsc::TryRecvError::Disconnected) => return Ok(None),
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("polling input")?
            && let Event::Key(key) = event::read().context("reading input")?
            && key.kind == KeyEventKind::Press
            && is_cancel_key(&key)
        {
            return Ok(None);
        }

        if last_tick.elapsed() >= TICK {
            let elapsed = last_tick.elapsed();
            last_tick = Instant::now();
            if let Some(app) = &mut app {
                app.tick(elapsed);
            }
            spinner = spinner.wrapping_add(1);
        }
    }
}

fn is_cancel_key(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc)
        || (matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn fill(buf: &mut Buffer, area: Rect, bg: Color) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(bg);
        }
    }
}

fn draw_loading_overlay(buf: &mut Buffer, area: Rect, label: &str, spinner: &str) {
    let title = format!("loading {label}");
    let status = format!("{spinner} fetching sky");
    let hint = "esc cancel";
    let content = title.width().max(status.width()).max(hint.width()) as u16;
    let w = (content + 6).min(area.width);
    let inner = draw_overlay_box(buf, area, w, 5);
    put_str(buf, inner.x, inner.y, inner.width, &title, OVERLAY_FG);
    put_str(buf, inner.x, inner.y + 1, inner.width, &status, OVERLAY_DIM);
    put_str(buf, inner.x, inner.y + 2, inner.width, hint, OVERLAY_DIM);
}

/// One geocode reply tagged with the query generation that asked for it, so the
/// loop can discard answers to queries the user has already typed past.
type SearchReply = (
    u64,
    std::result::Result<Vec<GeoResult>, crate::weather::WeatherError>,
);

/// What the body of the search view is currently showing. Separating "loading"
/// from "empty" is what stops a "no matches" flash before the first reply lands.
#[derive(Clone, Copy, PartialEq)]
enum SearchStatus {
    /// Query too short to search yet.
    Idle,
    /// A request is pending or in flight (including offline-grace retries).
    Loading,
    /// Candidates are present.
    Ready,
    /// A search completed with zero matches.
    Empty,
    /// The network was unreachable past the grace window.
    Offline,
}

struct SearchState {
    input: String,
    candidates: Vec<GeoResult>,
    selected: usize,
    status: SearchStatus,
    /// Bumped on every edit; only the latest generation's reply is accepted.
    generation: u64,
    /// Set on edit (and on a grace retry), cleared once the search fires.
    last_edit: Option<Instant>,
    /// When the current query's first request went out; drives the offline grace.
    query_started: Option<Instant>,
}

impl SearchState {
    fn new() -> Self {
        Self {
            input: String::new(),
            candidates: Vec::new(),
            selected: 0,
            status: SearchStatus::Idle,
            generation: 0,
            last_edit: None,
            query_started: None,
        }
    }
}

fn search_loop(terminal: &mut DefaultTerminal) -> Result<Option<GeoResult>> {
    let (tx, rx) = mpsc::channel::<SearchReply>();
    let mut state = SearchState::new();
    loop {
        draw_synchronized(terminal, |frame| {
            let area = frame.area();
            draw_search(frame.buffer_mut(), area, &state);
        })
        .context("drawing search")?;

        // Accept only the newest query's reply; stale generations are dropped.
        while let Ok((generation, reply)) = rx.try_recv() {
            if generation != state.generation {
                continue;
            }
            match reply {
                Ok(results) if !results.is_empty() => {
                    state.candidates = rank(results);
                    state.selected = 0;
                    state.status = SearchStatus::Ready;
                    state.query_started = None;
                }
                Ok(_) => {
                    state.candidates.clear();
                    state.status = SearchStatus::Empty;
                    state.query_started = None;
                }
                Err(crate::weather::WeatherError::Network(_)) => {
                    // A just-waking or flaky network gets retried until the grace
                    // window closes, then we settle on "no connection".
                    let within_grace = state
                        .query_started
                        .is_some_and(|t| t.elapsed() < OFFLINE_AFTER);
                    if within_grace {
                        state.last_edit = Some(Instant::now());
                        state.status = SearchStatus::Loading;
                    } else {
                        state.status = SearchStatus::Offline;
                        state.query_started = None;
                    }
                }
                // Server (HTTP) or decode errors are not connection problems;
                // there is simply nothing usable to show.
                Err(_) => {
                    state.candidates.clear();
                    state.status = SearchStatus::Empty;
                    state.query_started = None;
                }
            }
        }

        // Fire a geocode once typing has settled, on a thread so the input never
        // blocks; stale threads still send but get discarded by generation.
        if let Some(edited) = state.last_edit
            && edited.elapsed() >= SEARCH_DEBOUNCE
        {
            state.last_edit = None;
            let query = state.input.trim().to_string();
            if query.len() >= MIN_QUERY {
                if state.query_started.is_none() {
                    state.query_started = Some(Instant::now());
                }
                state.status = SearchStatus::Loading;
                let generation = state.generation;
                let tx = tx.clone();
                thread::spawn(move || {
                    let _ = tx.send((generation, geocode(&query)));
                });
            }
        }

        if event::poll(SEARCH_POLL).context("polling input")?
            && let Event::Key(key) = event::read().context("reading input")?
            && key.kind == KeyEventKind::Press
        {
            match search_step(&key, &mut state) {
                SearchAction::Choose => {
                    if let Some(choice) = state.candidates.get(state.selected) {
                        return Ok(Some(choice.clone()));
                    }
                }
                SearchAction::Cancel => return Ok(None),
                SearchAction::Edited => {
                    // A new query: invalidate in-flight replies and reset grace.
                    state.generation += 1;
                    state.query_started = None;
                    if state.input.trim().len() < MIN_QUERY {
                        state.candidates.clear();
                        state.status = SearchStatus::Idle;
                    } else {
                        state.status = SearchStatus::Loading;
                    }
                }
                SearchAction::Moved | SearchAction::Ignore => {}
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum SearchAction {
    Edited,
    Moved,
    Choose,
    Cancel,
    Ignore,
}

/// Pure key handler for the search view, factored out so editing and navigation
/// are testable without a terminal or network. Printable keys edit the query, so
/// navigation is the arrow keys (j/k would type, not move); up/down move the
/// selection; enter chooses; esc and ctrl-c cancel. Any edit resets the
/// selection to the top and stamps `last_edit` so the loop can debounce.
fn search_step(key: &KeyEvent, state: &mut SearchState) -> SearchAction {
    match key.code {
        KeyCode::Enter => SearchAction::Choose,
        KeyCode::Esc => SearchAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => SearchAction::Cancel,
        KeyCode::Up => {
            state.selected = state.selected.saturating_sub(1);
            SearchAction::Moved
        }
        KeyCode::Down => {
            let last = state.candidates.len().saturating_sub(1);
            state.selected = (state.selected + 1).min(last);
            SearchAction::Moved
        }
        KeyCode::Backspace => {
            state.input.pop();
            state.selected = 0;
            state.last_edit = Some(Instant::now());
            SearchAction::Edited
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.input.push(ch);
            state.selected = 0;
            state.last_edit = Some(Instant::now());
            SearchAction::Edited
        }
        _ => SearchAction::Ignore,
    }
}

/// First visible row so `selected` stays inside a `window`-row viewport. Zero
/// while the list fits or the selection is in the first window; otherwise it
/// scrolls just far enough to keep the selection on the bottom row, never past
/// the end.
fn scroll_offset(selected: usize, len: usize, window: usize) -> usize {
    if len <= window {
        return 0;
    }
    let max_offset = len - window;
    selected.saturating_sub(window - 1).min(max_offset)
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

const TOO_SMALL_BG: Color = CHROME_BG;
const BRAND: Color = Color::Rgb(252, 215, 172);
const VALUE_OK: Color = Color::Rgb(150, 200, 140);
const VALUE_SHORT: Color = Color::Rgb(220, 90, 90);

fn draw_sky(buf: &mut Buffer, area: Rect, app: &mut App) {
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
        &app.display.chrome.header_left,
        &app.display.chrome.header_right,
    );
    let chrome = &app.display.chrome;
    let (foot_left, foot_keys) = fit_footer(footer.width, chrome);
    draw_chrome_bar(buf, footer, foot_left, foot_keys);

    // Read before the cache borrow below, which mutably borrows app.sky_cache.
    let egg = app.egg_active();
    let egg_frame = app.egg_frame;

    let px_width = sky_area.width as u32;
    let px_height = (sky_area.height as u32) * 2;
    let cache = match &mut app.sky_cache {
        Some(c) if !app.sky_dirty && c.width == px_width && c.height == px_height => c,
        slot => {
            app.sky_dirty = false;
            slot.insert(SkyCache {
                width: px_width,
                height: px_height,
                pixels: render(&app.display, px_width, px_height),
            })
        }
    };
    // Lightning and the pigs egg composite onto a copy so the cached base stays
    // reusable; their timing is per-frame state, not part of the sky render.
    if app.display.lightning.is_some() || egg {
        let mut pixels = cache.pixels.clone();
        if let Some(lt) = &app.display.lightning {
            lightning::overlay(&mut pixels, lt, app.lightning_elapsed.as_secs_f64());
        }
        if egg {
            pigs::overlay(&mut pixels, egg_frame);
        }
        SkyWidget { pixels: &pixels }.render(sky_area, buf);
    } else {
        SkyWidget {
            pixels: &cache.pixels,
        }
        .render(sky_area, buf);
    }
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

// Minimum blank columns kept between the footer payload and the key hints, so
// the two never butt against each other when the bar is tight.
const FOOTER_GAP: usize = 2;

// Widest tier whose display width fits the budget; the last (narrowest) tier is
// the floor when nothing fits, so this never returns empty for a non-empty list.
fn pick_tier(tiers: &[String], budget: usize) -> &str {
    tiers
        .iter()
        .map(String::as_str)
        .find(|s| s.width() <= budget)
        .unwrap_or_else(|| tiers.last().map_or("", String::as_str))
}

// Choose the footer payload and key-hint strings for the current width. The
// payload wins: it is fitted against the width minus the reserved `? help`
// floor, then the hints take the richest tier the remainder allows. Scenes
// carry no tiers, so they fall back to the static footer/keys strings.
fn fit_footer(width: u16, chrome: &Chrome) -> (&str, &str) {
    if chrome.footer_tiers.is_empty() || chrome.keys_tiers.is_empty() {
        return (&chrome.footer, &chrome.keys);
    }
    let total = width as usize;
    let keys_floor = chrome.keys_tiers.last().map_or(0, |s| s.width());
    let left = pick_tier(
        &chrome.footer_tiers,
        total.saturating_sub(keys_floor + FOOTER_GAP),
    );
    let right = pick_tier(
        &chrome.keys_tiers,
        total.saturating_sub(left.width() + FOOTER_GAP),
    );
    (left, right)
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

fn draw_search(buf: &mut Buffer, area: Rect, state: &SearchState) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(OVERLAY_BG);
        }
    }
    let has_list = !state.candidates.is_empty();
    // title, blank, input, blank, body (list rows or one message line), blank,
    // footer, plus the box border.
    let body_rows = if has_list {
        state.candidates.len().min(SEARCH_WINDOW) as u16
    } else {
        1
    };
    let w = 54.min(area.width);
    let box_h = (body_rows + 8).min(area.height);
    let bx = area.x + area.width.saturating_sub(w) / 2;
    // Anchor the top above center so the list grows DOWNWARD (Google-style): the
    // input stays put and only the box's bottom edge moves as results arrive.
    let by = (area.y + area.height / 3).min(area.y + area.height.saturating_sub(box_h));
    for y in by..by + box_h {
        for x in bx..bx + w {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(OVERLAY_BG);
            cell.set_fg(OVERLAY_FG);
        }
    }
    let inner = Rect {
        x: bx + 2,
        y: by + 1,
        width: w.saturating_sub(4),
        height: box_h.saturating_sub(2),
    };

    let mut row = inner.y;
    put_str(
        buf,
        inner.x,
        row,
        inner.width,
        "search location",
        OVERLAY_FG,
    );
    row += 2;

    let cursor = format!("{}_", state.input);
    put_str(buf, inner.x, row, inner.width, &cursor, OVERLAY_FG);
    if state.status == SearchStatus::Loading {
        let tag = "...";
        let tag_x = inner.x + inner.width.saturating_sub(tag.len() as u16);
        put_str(buf, tag_x, row, tag.len() as u16, tag, OVERLAY_DIM);
    }
    row += 2;

    if has_list {
        let offset = scroll_offset(state.selected, state.candidates.len(), SEARCH_WINDOW);
        for (i, cand) in state
            .candidates
            .iter()
            .enumerate()
            .skip(offset)
            .take(SEARCH_WINDOW)
        {
            let marker = if i == state.selected { "▸ " } else { "  " };
            let fg = if i == state.selected {
                OVERLAY_FG
            } else {
                OVERLAY_DIM
            };
            let line = format!("{marker}{}", cand.label());
            put_str(buf, inner.x, row, inner.width, &line, fg);
            row += 1;
        }
    } else {
        let message = match state.status {
            SearchStatus::Loading => "searching...",
            SearchStatus::Empty => "no matches",
            SearchStatus::Offline => "no connection",
            // Idle, or the unreachable Ready-without-candidates: a soft prompt.
            _ => "city or place name",
        };
        put_str(buf, inner.x, row, inner.width, message, OVERLAY_DIM);
        row += 1;
    }
    row += 1;

    // The full nav hint only appears while locations are listed; otherwise just
    // the escape hatch. The position counter is the "more" affordance, shown
    // only when the list overflows the window.
    let footer = if has_list {
        if state.candidates.len() > SEARCH_WINDOW {
            format!(
                "{}/{}   up/down move   enter pick   esc cancel",
                state.selected + 1,
                state.candidates.len()
            )
        } else {
            "up/down move   enter pick   esc cancel".to_string()
        }
    } else {
        "esc cancel".to_string()
    };
    put_str(buf, inner.x, row, inner.width, &footer, OVERLAY_DIM);
}

fn draw_too_small(buf: &mut Buffer, area: Rect) {
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_char(' ');
            cell.set_bg(TOO_SMALL_BG);
        }
    }

    if area.width >= 34 && area.height >= 8 {
        draw_too_small_primary(buf, area);
    } else if area.width >= 12 && area.height >= 3 {
        draw_too_small_secondary(buf, area);
    } else {
        draw_too_small_minimal(buf, area);
    }
}

fn draw_too_small_primary(buf: &mut Buffer, area: Rect) {
    let width_status = status_for(area.width, MIN_COLS);
    let height_status = status_for(area.height, MIN_ROWS);

    let block_height = 7u16;
    let top_y = area.y + (area.height.saturating_sub(block_height)) / 2;

    let brand = [(
        "celsius",
        Style::default()
            .fg(BRAND)
            .bg(TOO_SMALL_BG)
            .add_modifier(Modifier::BOLD),
    )];
    put_centered(buf, area, top_y, &brand);

    let dim = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);
    put_centered(buf, area, top_y + 2, &[("the sky is too cramped", dim)]);

    let width_label = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);
    let width_value = Style::default()
        .fg(width_status.color)
        .bg(TOO_SMALL_BG)
        .add_modifier(Modifier::BOLD);
    let width_status_style = Style::default().fg(width_status.color).bg(TOO_SMALL_BG);
    let width_needs = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);

    let mut y = top_y + 4;
    put_centered(
        buf,
        area,
        y,
        &[
            ("width ", width_label),
            (&format!("{:>3}", area.width), width_value),
            ("  ", Style::default().bg(TOO_SMALL_BG)),
            (&format!("{:<5}", width_status.word), width_status_style),
            (&format!(" needs {}", MIN_COLS), width_needs),
        ],
    );

    y += 1;
    let height_label = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);
    let height_value = Style::default()
        .fg(height_status.color)
        .bg(TOO_SMALL_BG)
        .add_modifier(Modifier::BOLD);
    let height_status_style = Style::default().fg(height_status.color).bg(TOO_SMALL_BG);
    let height_needs = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);
    put_centered(
        buf,
        area,
        y,
        &[
            ("height", height_label),
            (" ", Style::default().bg(TOO_SMALL_BG)),
            (&format!("{:>3}", area.height), height_value),
            ("  ", Style::default().bg(TOO_SMALL_BG)),
            (&format!("{:<5}", height_status.word), height_status_style),
            (&format!(" needs {}", MIN_ROWS), height_needs),
        ],
    );

    put_centered(buf, area, top_y + 6, &[("resize to clear the sky", dim)]);
}

fn draw_too_small_secondary(buf: &mut Buffer, area: Rect) {
    let block_height = 3u16;
    let top_y = area.y + (area.height.saturating_sub(block_height)) / 2;

    let brand = [(
        "celsius",
        Style::default()
            .fg(BRAND)
            .bg(TOO_SMALL_BG)
            .add_modifier(Modifier::BOLD),
    )];
    put_centered(buf, area, top_y, &brand);

    let width_color = if area.width >= MIN_COLS {
        VALUE_OK
    } else {
        VALUE_SHORT
    };
    let height_color = if area.height >= MIN_ROWS {
        VALUE_OK
    } else {
        VALUE_SHORT
    };
    let dim = Style::default().fg(OVERLAY_DIM).bg(TOO_SMALL_BG);

    let size_segments: [(&str, Style); 5] = [
        (
            &format!("{}", area.width),
            Style::default()
                .fg(width_color)
                .bg(TOO_SMALL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        ("x", dim),
        (
            &format!("{}", area.height),
            Style::default()
                .fg(height_color)
                .bg(TOO_SMALL_BG)
                .add_modifier(Modifier::BOLD),
        ),
        (" needs ", dim),
        (&format!("{}x{}", MIN_COLS, MIN_ROWS), dim),
    ];
    put_centered(buf, area, top_y + 1, &size_segments);

    put_centered(buf, area, top_y + 2, &[("resize to clear", dim)]);
}

fn draw_too_small_minimal(buf: &mut Buffer, area: Rect) {
    let y = area.y + area.height / 2;
    put_centered(
        buf,
        area,
        y,
        &[(
            "celsius",
            Style::default()
                .fg(BRAND)
                .bg(TOO_SMALL_BG)
                .add_modifier(Modifier::BOLD),
        )],
    );
}

#[derive(Clone, Copy)]
struct DimStatus {
    color: Color,
    word: &'static str,
}

fn status_for(current: u16, needed: u16) -> DimStatus {
    if current >= needed {
        DimStatus {
            color: VALUE_OK,
            word: "ok",
        }
    } else {
        DimStatus {
            color: VALUE_SHORT,
            word: "short",
        }
    }
}

fn put_centered(buf: &mut Buffer, area: Rect, y: u16, segments: &[(&str, Style)]) {
    let total: usize = segments.iter().map(|(s, _)| s.width()).sum();
    let total = total as u16;
    let start_x = area.x + (area.width.saturating_sub(total)) / 2;
    let mut x = start_x;
    for (text, style) in segments {
        let w = text.width() as u16;
        if x >= area.x + area.width {
            break;
        }
        let max_w = (area.x + area.width).saturating_sub(x) as usize;
        buf.set_stringn(x, y, text, max_w, *style);
        x += w;
    }
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
                footer: "10°  clear   wind N 5".into(),
                keys: "q quit".into(),
                status: "Testville 10C clear wind N 5".into(),
                footer_tiers: Vec::new(),
                keys_tiers: Vec::new(),
            },
            haze: None,
            stars: None,
            moon: None,
            precipitation: None,
            lightning: None,
            horizon_glow: None,
            analytic: None,
            wind_speed_kmh: 20.0,
        }
    }

    fn timeline() -> Timeline {
        Timeline::new(
            (0..50).map(|i| sky(&format!("s{i}"))).collect(),
            10,
            None,
            0,
        )
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
    fn l_requests_a_location_change() {
        // The search itself is a standalone modal run by main; App only signals.
        let tl = timeline();
        let mut app = App::new(&tl);
        app.handle_key(press(KeyCode::Char('l')));
        assert_eq!(app.outcome, Some(RunOutcome::ChangeLocation));
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
    fn tick_marks_sky_dirty_only_when_clouds_move() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.sky_dirty = false;

        app.tick(Duration::from_millis(100));
        assert!(app.sky_dirty, "wind drift must invalidate the cached sky");

        app.sky_dirty = false;
        app.drift_paused = true;
        app.tick(Duration::from_millis(100));
        assert!(!app.sky_dirty, "a paused sky must keep its cache");

        app.drift_paused = false;
        app.display.wind_speed_kmh = 0.0;
        app.tick(Duration::from_millis(100));
        assert!(!app.sky_dirty, "windless clouds do not move");
    }

    #[test]
    fn tick_reports_visible_change_for_redraw_gating() {
        let tl = timeline();
        let mut app = App::new(&tl);

        // Drifting clouds change the frame.
        assert!(app.tick(Duration::from_millis(100)));

        // Paused: nothing moves, so there is nothing to redraw.
        app.drift_paused = true;
        assert!(!app.tick(Duration::from_millis(100)));

        // Running but windless and cloudless leaves the frame identical...
        app.drift_paused = false;
        app.display.wind_speed_kmh = 0.0;
        app.display.clouds.clear();
        assert!(!app.tick(Duration::from_millis(100)));

        // ...until a lightning scene, whose flash is recomputed every tick.
        app.display.lightning = Some(crate::lightning::Lightning::new(
            101, 0.5, 1.0, false, 104, 50,
        ));
        assert!(app.tick(Duration::from_millis(100)));
    }

    #[test]
    fn scrub_marks_sky_dirty() {
        let tl = timeline();
        let mut app = App::new(&tl);
        app.sky_dirty = false;
        app.handle_key(press(KeyCode::Right));
        assert!(app.sky_dirty, "a new hour is a new sky");
    }

    #[test]
    fn draw_reuses_cache_until_dirty() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(80, 40)).unwrap();

        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        assert!(!app.sky_dirty, "drawing consumes the dirty flag");
        let first = term.backend().buffer().clone();

        // Mutate the sky behind the cache's back: a clean draw must not see it.
        app.display.clouds[0].offset_x += 0.5;
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        assert_eq!(
            *term.backend().buffer(),
            first,
            "clean frames must come from the cache"
        );

        app.sky_dirty = true;
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        assert_ne!(
            *term.backend().buffer(),
            first,
            "a dirty frame must re-render and pick up the moved clouds"
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
            draw_sky(buf, area, &mut app);
            draw_help_overlay(buf, area);
        })
        .unwrap();
        assert!(buffer_text(term.backend().buffer()).contains("keybindings"));
    }

    #[test]
    fn cramped_area_shows_the_warning() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(20, 10)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(content.contains("celsius"), "secondary form should brand");
        assert!(
            content.contains("needs 60x25"),
            "secondary form should state required size"
        );
    }

    #[test]
    fn primary_too_small_shows_per_dimension_status() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(62, 20)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(
            content.contains("the sky is too cramped"),
            "primary form should show subtitle"
        );
        assert!(content.contains("ok"), "width should report ok at 62 cols");
        assert!(
            content.contains("short"),
            "height should report short at 20 rows"
        );
        assert!(
            !content.contains("q quit"),
            "chrome footer should not render below the gate"
        );
    }

    #[test]
    fn threshold_exactly_releases_to_sky() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(60, 25)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(
            !content.contains("the sky is too cramped"),
            "exactly the threshold should not show too-small screen"
        );
        assert!(content.contains("celsius"), "sky chrome should render");
    }

    #[test]
    fn minimal_too_small_shows_brand() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(10, 2)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(
            content.contains("celsius"),
            "minimal form should still show brand"
        );
    }

    #[test]
    fn chrome_shows_location() {
        let tl = timeline();
        let mut app = App::new(&tl);
        let mut term = Terminal::new(TestBackend::new(80, 40)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_sky(f.buffer_mut(), area, &mut app);
        })
        .unwrap();
        let content = buffer_text(term.backend().buffer());
        assert!(content.contains("celsius"));
        assert!(content.contains("testville"));
    }

    fn footer_chrome() -> Chrome {
        Chrome {
            header_left: "celsius".into(),
            header_right: String::new(),
            footer: "14°  overcast   wind SW 12".into(),
            keys: "q quit".into(),
            status: String::new(),
            footer_tiers: vec![
                "14°  H22 L15   overcast   wind SW 12".into(),
                "14°  H22 L15   overcast   SW 12".into(),
                "14°  H22 L15   overcast".into(),
                "14°  overcast".into(),
                "14°".into(),
            ],
            keys_tiers: vec![
                "<- -> scrub   tab day   t now   l location   ? help   q quit".into(),
                "tab day   l location   ? help   q quit".into(),
                "? help   q quit".into(),
                "? help".into(),
            ],
        }
    }

    #[test]
    fn fit_footer_wide_shows_everything() {
        let c = footer_chrome();
        let (left, right) = fit_footer(104, &c);
        assert_eq!(left, c.footer_tiers[0], "full payload incl H/L and wind");
        assert_eq!(right, c.keys_tiers[0], "full hints when there is room");
    }

    #[test]
    fn fit_footer_protects_payload_at_gate() {
        let c = footer_chrome();
        let (left, right) = fit_footer(MIN_COLS, &c);
        // The live reading survives untouched; only the hints give way.
        assert_eq!(left, c.footer_tiers[0]);
        assert!(left.contains("H22 L15"), "H/L kept (data-first)");
        assert!(left.contains("SW 12"), "wind kept");
        assert!(right.contains("? help"), "? help held to the gate");
        assert!(
            right.width() < c.keys_tiers[0].width(),
            "hints collapsed from full"
        );
    }

    #[test]
    fn fit_footer_hints_degrade_monotonically() {
        let c = footer_chrome();
        let w104 = fit_footer(104, &c).1.width();
        let w80 = fit_footer(80, &c).1.width();
        let w60 = fit_footer(60, &c).1.width();
        assert!(w104 >= w80 && w80 >= w60, "{w104} {w80} {w60}");
    }

    #[test]
    fn fit_footer_empty_tiers_fall_back() {
        let mut c = footer_chrome();
        c.footer_tiers.clear();
        c.keys_tiers.clear();
        let (left, right) = fit_footer(80, &c);
        assert_eq!(left, c.footer);
        assert_eq!(right, c.keys);
    }

    fn place(name: &str, admin1: &str, country: &str, population: Option<u64>) -> GeoResult {
        GeoResult {
            name: name.to_string(),
            latitude: 0.0,
            longitude: 0.0,
            timezone: "UTC".to_string(),
            country: Some(country.to_string()),
            admin1: Some(admin1.to_string()),
            elevation: None,
            population,
        }
    }

    fn search_state(input: &str, candidates: Vec<GeoResult>) -> SearchState {
        let mut state = SearchState::new();
        state.input = input.to_string();
        state.status = if candidates.is_empty() {
            SearchStatus::Idle
        } else {
            SearchStatus::Ready
        };
        state.candidates = candidates;
        state
    }

    #[test]
    fn search_step_typing_edits_and_resets_selection() {
        let mut state = search_state(
            "",
            vec![place("A", "R", "C", None), place("B", "R", "C", None)],
        );
        state.selected = 1;
        assert_eq!(
            search_step(&press(KeyCode::Char('h')), &mut state),
            SearchAction::Edited
        );
        assert_eq!(state.input, "h");
        assert_eq!(state.selected, 0, "a new query resets to the top result");
        assert!(state.last_edit.is_some(), "editing arms the debounce");
        assert_eq!(
            search_step(&press(KeyCode::Backspace), &mut state),
            SearchAction::Edited
        );
        assert_eq!(state.input, "");
    }

    #[test]
    fn search_step_arrows_move_selection_letters_type() {
        let mut state = search_state(
            "x",
            vec![
                place("A", "R", "C", None),
                place("B", "R", "C", None),
                place("C", "R", "C", None),
            ],
        );
        assert_eq!(
            search_step(&press(KeyCode::Down), &mut state),
            SearchAction::Moved
        );
        assert_eq!(state.selected, 1);
        search_step(&press(KeyCode::Down), &mut state);
        search_step(&press(KeyCode::Down), &mut state);
        assert_eq!(state.selected, 2, "down clamps at the last row");
        assert_eq!(
            search_step(&press(KeyCode::Up), &mut state),
            SearchAction::Moved
        );
        assert_eq!(state.selected, 1);
        // j and k are typed into the query, never used to navigate.
        assert_eq!(
            search_step(&press(KeyCode::Char('j')), &mut state),
            SearchAction::Edited
        );
        assert_eq!(state.input, "xj");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn search_step_chooses_and_cancels() {
        let mut state = search_state("x", vec![place("A", "R", "C", None)]);
        assert_eq!(
            search_step(&press(KeyCode::Enter), &mut state),
            SearchAction::Choose
        );
        assert_eq!(
            search_step(&press(KeyCode::Esc), &mut state),
            SearchAction::Cancel
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(search_step(&ctrl_c, &mut state), SearchAction::Cancel);
    }

    #[test]
    fn scroll_offset_keeps_selection_visible() {
        // Fits the window: never scrolls.
        assert_eq!(scroll_offset(0, 3, 5), 0);
        assert_eq!(scroll_offset(2, 3, 5), 0);
        // Overflows: stays at 0 until the selection leaves the first window,
        // then scrolls just enough, never past the last full window.
        assert_eq!(scroll_offset(4, 8, 5), 0);
        assert_eq!(scroll_offset(5, 8, 5), 1);
        assert_eq!(scroll_offset(7, 8, 5), 3);
        assert_eq!(scroll_offset(7, 8, 5), 8 - 5);
    }

    #[test]
    fn search_view_shows_query_labels_and_marker_not_population() {
        let state = search_state(
            "cape",
            vec![
                place("Cape Town", "Western Cape", "South Africa", Some(3_400_000)),
                place("Capetown", "Ohio", "United States", Some(500)),
            ],
        );
        let mut term = Terminal::new(TestBackend::new(70, 18)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_search(f.buffer_mut(), area, &state);
        })
        .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(
            text.contains("cape_"),
            "the typed query shows with a cursor"
        );
        assert!(text.contains("Cape Town, Western Cape, South Africa"));
        assert!(text.contains("Capetown, Ohio, United States"));
        assert!(text.contains("▸"), "selected row carries the marker");
        assert!(
            text.contains("enter pick"),
            "the full nav hint shows while locations are listed"
        );
        // Population is a ranking signal only; it must never reach the screen.
        assert!(!text.contains("3400000"));
        assert!(!text.contains("500"));
    }

    #[test]
    fn search_view_shows_placeholder_when_empty_and_counter_on_overflow() {
        // Empty query: a soft placeholder, no list, only the escape hint.
        let empty = search_state("", vec![]);
        let mut term = Terminal::new(TestBackend::new(70, 18)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_search(f.buffer_mut(), area, &empty);
        })
        .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(text.contains("city or place name"));
        assert!(text.contains("esc cancel"));
        assert!(
            !text.contains("enter pick"),
            "no list, so no list-navigation hint"
        );

        // An overflowing list shows the position counter as the "more" hint.
        let many = search_state(
            "town",
            (0..8u64)
                .map(|i| place(&format!("Town{i}"), "Region", "Country", Some(i)))
                .collect(),
        );
        term.draw(|f| {
            let area = f.area();
            draw_search(f.buffer_mut(), area, &many);
        })
        .unwrap();
        assert!(buffer_text(term.backend().buffer()).contains("1/8"));
    }

    #[test]
    fn search_view_distinguishes_loading_empty_and_offline() {
        let mut state = search_state("lon", vec![]);
        let mut term = Terminal::new(TestBackend::new(70, 18)).unwrap();
        for (status, message) in [
            (SearchStatus::Loading, "searching..."),
            (SearchStatus::Empty, "no matches"),
            (SearchStatus::Offline, "no connection"),
        ] {
            state.status = status;
            term.draw(|f| {
                let area = f.area();
                draw_search(f.buffer_mut(), area, &state);
            })
            .unwrap();
            let text = buffer_text(term.backend().buffer());
            assert!(text.contains(message), "status should render {message:?}");
            assert!(
                !text.contains("no matches") || status == SearchStatus::Empty,
                "loading must never flash the no-matches message"
            );
        }
    }

    #[test]
    fn loading_overlay_names_the_city_and_offers_cancel() {
        let mut term = Terminal::new(TestBackend::new(70, 18)).unwrap();
        term.draw(|f| {
            let area = f.area();
            draw_loading_overlay(f.buffer_mut(), area, "Paris", SPINNER[0]);
        })
        .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(
            text.contains("loading Paris"),
            "names the city being fetched"
        );
        assert!(text.contains(SPINNER[0]), "shows the current spinner phase");
        assert!(text.contains("esc cancel"), "offers the escape hatch");
    }
}
