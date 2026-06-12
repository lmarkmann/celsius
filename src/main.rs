use std::io::{BufWriter, ErrorKind, IsTerminal, Write, stdout};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use clap::Parser;
#[cfg(feature = "png")]
use clap::Subcommand;
use clap::builder::styling::{AnsiColor, Styles};

use celsius::config::{self, LocationPref};
use celsius::tui::{RunOutcome, Timeline};
use celsius::weather::{ComposeOpts, compose, compose_at, error_sky, forecast, location};
use celsius::{SkyState, load_scene, tui};
#[cfg(feature = "png")]
use celsius::{render, terminal};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default());

#[cfg(feature = "png")]
const AFTER_HELP: &str = "\x1b[1;32mExamples:\x1b[0m
  \x1b[1;36mcelsius -l Hamburg\x1b[0m
  \x1b[1;36mcelsius -l Hamburg --at 17\x1b[0m             today at 17:00 UTC
  \x1b[1;36mcelsius -l Hamburg --at +3h\x1b[0m            three hours from now
  \x1b[1;36mcelsius -l \"Reykjavík\" --at 2026-06-21\x1b[0m  solstice, noon UTC
  \x1b[1;36mcelsius --lat 53.55 --lon 9.99\x1b[0m
  \x1b[1;36mcelsius --scene ../skyterm-lab/scenes/golden_hour_cumulus.toml\x1b[0m
  \x1b[1;36mcelsius render --scene scene.toml --out scene.png\x1b[0m
";

#[cfg(not(feature = "png"))]
const AFTER_HELP: &str = "\x1b[1;32mExamples:\x1b[0m
  \x1b[1;36mcelsius -l Hamburg\x1b[0m
  \x1b[1;36mcelsius -l Hamburg --at 17\x1b[0m             today at 17:00 UTC
  \x1b[1;36mcelsius -l Hamburg --at +3h\x1b[0m            three hours from now
  \x1b[1;36mcelsius -l \"Reykjavík\" --at 2026-06-21\x1b[0m  solstice, noon UTC
  \x1b[1;36mcelsius --lat 53.55 --lon 9.99\x1b[0m
  \x1b[1;36mcelsius --scene ../skyterm-lab/scenes/golden_hour_cumulus.toml\x1b[0m
";

#[derive(Parser)]
#[command(
    name = "celsius",
    version,
    about = "a sky in your terminal",
    styles = STYLES,
    after_help = AFTER_HELP,
)]
struct Cli {
    /// Place name to look up via Open-Meteo geocoding.
    #[arg(short = 'l', long, value_name = "NAME", global = true)]
    location: Option<String>,

    /// Decimal latitude (+N). Pair with --lon to skip geocoding.
    #[arg(long, value_name = "F64", global = true, allow_hyphen_values = true)]
    lat: Option<f64>,

    /// Decimal longitude (+E). Pair with --lat to skip geocoding.
    #[arg(long, value_name = "F64", global = true, allow_hyphen_values = true)]
    lon: Option<f64>,

    /// UTC time to scrub to. Accepts a bare hour ("17"), HH:MM ("17:30"),
    /// a relative offset from now ("+3h", "-30m", "+1d"), a date alone
    /// ("2026-06-21", defaults to 12:00 UTC), or full ISO 8601
    /// ("2026-06-21T17:00:00Z"). Default: now.
    #[arg(long, value_name = "TIME", global = true, allow_hyphen_values = true)]
    at: Option<String>,

    /// Load a lab scene TOML directly instead of synthesizing from weather.
    #[arg(long, value_name = "PATH", global = true)]
    scene: Option<PathBuf>,

    /// Compass bearing the viewer faces: 0=N, 90=E, 180=S, 270=W.
    /// Default 180 (south) suits northern-hemisphere observers.
    #[arg(long, value_name = "DEG", global = true, default_value_t = 180.0)]
    facing: f64,

    /// Bortle dark-sky class for your location: 1 (pristine) to 9 (inner city).
    /// Scales visible star count and tints the horizon with light-pollution glow.
    /// Default: unset (treat as Bortle 1, today's behavior). Falls back to config.
    #[arg(long, value_name = "1..9", global = true, value_parser = clap::value_parser!(u8).range(1..=9))]
    bortle: Option<u8>,

    /// Print a one-line ASCII weather summary and exit, no full-screen UI.
    /// Also the default when stdout is not a terminal or NO_COLOR is set.
    #[arg(long, visible_alias = "no-tui", global = true)]
    plain: bool,

    /// Print one ANSI half-block frame (104x50) and exit: the visual capture
    /// surface, for piping into a file or `less -R`.
    #[arg(long, global = true, conflicts_with = "plain")]
    frame: bool,

    /// Sky model for the live-weather background. `analytic` (default) is the
    /// Preetham physical sky, crossfading to the `palette` gradient through
    /// twilight and night. Pass `--sky palette` to force the hand-tuned gradient.
    #[arg(long, value_enum, default_value_t = SkyModel::Analytic, global = true)]
    sky: SkyModel,

    #[cfg(feature = "png")]
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
enum SkyModel {
    Palette,
    Analytic,
}

#[cfg(feature = "png")]
#[derive(Subcommand)]
enum Command {
    /// Render a scene TOML to a PNG (oracle path, `png` feature).
    Render {
        /// Path to the scene TOML.
        #[arg(short, long)]
        scene: PathBuf,
        /// Output PNG path.
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long, default_value_t = 104)]
        width: u32,
        #[arg(long, default_value_t = 50)]
        height: u32,
    },
}

enum OutputMode {
    Tui,
    Plain,
    Frame,
}

/// Decide how to render. First match wins: an explicit `--frame` beats
/// everything; otherwise anything that means "not an interactive color
/// terminal" (--plain, NO_COLOR, a pipe) falls back to the flat text surface.
fn output_mode(cli: &Cli) -> OutputMode {
    if cli.frame {
        OutputMode::Frame
    // Per the NO_COLOR spec, an empty value means unset.
    } else if cli.plain
        || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
        || !stdout().is_terminal()
    {
        OutputMode::Plain
    } else {
        OutputMode::Tui
    }
}

/// Write one state non-interactively and exit. A broken pipe (`celsius | head`)
/// is a normal way to stop reading, not an error, so it maps to success.
fn write_oneshot(state: &SkyState, mode: &OutputMode) -> Result<()> {
    let mut out = BufWriter::new(stdout().lock());
    let written = match mode {
        OutputMode::Frame => tui::write_frame(state, &mut out),
        _ => tui::write_plain(state, &mut out),
    };
    match written.and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e).context("writing output"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    #[cfg(feature = "png")]
    if let Some(Command::Render {
        scene,
        out,
        width,
        height,
    }) = cli.command
    {
        let state =
            load_scene(&scene).with_context(|| format!("loading scene {}", scene.display()))?;
        let pixels = render(&state, width, height);
        terminal::write_png(&pixels, &out).with_context(|| format!("writing {}", out.display()))?;
        println!("{} -> {} ({}x{})", state.name, out.display(), width, height);
        return Ok(());
    }

    // Scene-file path: single state, no retry loop needed.
    if let Some(scene_path) = cli.scene.as_ref() {
        let state = load_scene(scene_path)
            .with_context(|| format!("loading scene {}", scene_path.display()))?;
        let mode = output_mode(&cli);
        if !matches!(mode, OutputMode::Tui) {
            return write_oneshot(&state, &mode);
        }
        tui::run(&Timeline::single(state)).context("running tui")?;
        return Ok(());
    }

    // Non-interactive (pipe, --plain, --frame, NO_COLOR): build once, no retry.
    // A fetch error here goes to stderr and exits non-zero, the normal CLI shape.
    let mode = output_mode(&cli);
    if !matches!(mode, OutputMode::Tui) {
        let timeline = build_live_timeline(&cli, None).context("building forecast")?;
        return write_oneshot(&timeline.states[timeline.home], &mode);
    }

    // Live TUI: the retry loop handles fetch errors and location changes.
    let mut location_override: Option<String> = None;
    loop {
        let timeline = match build_live_timeline(&cli, location_override.as_deref()) {
            Ok(t) => t,
            Err(e) => Timeline::single(error_sky(&e.to_string())),
        };
        match tui::run(&timeline).context("running tui")? {
            RunOutcome::Quit => return Ok(()),
            RunOutcome::Retry => location_override = None,
            RunOutcome::ChangeLocation(name) => location_override = Some(name),
        }
    }
}

fn build_live_timeline(cli: &Cli, location_override: Option<&str>) -> Result<Timeline> {
    let now_unix = Utc::now().timestamp();
    let at_unix = match cli.at.as_deref() {
        Some(s) => parse_at(s, now_unix)?,
        None => now_unix,
    };

    let location = resolve_location(cli, location_override)?;
    let forecast = forecast::fetch(location.latitude, location.longitude)
        .with_context(|| format!("fetching forecast for {}", location.label()))?;
    let hours = forecast.hourly.len();
    if hours == 0 {
        bail!("forecast returned zero hours for {}", location.label());
    }

    let opts = ComposeOpts {
        center_az: cli.facing,
        bortle: cli.bortle.or_else(|| config::load().bortle),
        analytic: cli.sky == SkyModel::Analytic,
    };
    let mut states: Vec<_> = (0..hours)
        .map(|h| compose(&forecast, &location, h, now_unix, opts))
        .collect::<Result<_, _>>()
        .context("composing sky timeline")?;
    let home = nearest_hour_index(&forecast, at_unix);
    // Render the home slot at the exact requested instant, interpolating between
    // the bracketing hours, so "now" (or any --at) isn't rounded to the hour.
    states[home] = compose_at(&forecast, &location, at_unix, now_unix, opts)
        .context("composing sky for requested time")?;
    Ok(Timeline::new(states, home))
}

fn resolve_location(cli: &Cli, override_name: Option<&str>) -> Result<location::GeoResult> {
    // Location overlay in the TUI takes precedence over everything.
    if let Some(name) = override_name {
        let results = location::geocode(name).with_context(|| format!("geocoding '{name}'"))?;
        return results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no places matched '{name}'"));
    }
    match (cli.lat, cli.lon, cli.location.as_deref()) {
        (Some(lat), Some(lon), name) => Ok(location::GeoResult {
            name: name.unwrap_or("custom").to_string(),
            latitude: lat,
            longitude: lon,
            timezone: "UTC".to_string(),
            country: None,
            admin1: None,
            elevation: None,
            population: None,
        }),
        (None, None, Some(name)) => {
            let results = location::geocode(name).with_context(|| format!("geocoding '{name}'"))?;
            results
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no places matched '{name}'"))
        }
        (Some(_), None, _) | (None, Some(_), _) => {
            bail!("--lat and --lon must be passed together")
        }
        (None, None, None) => resolve_from_config_or_prompt(),
    }
}

fn resolve_from_config_or_prompt() -> Result<location::GeoResult> {
    let cfg = config::load();
    match cfg.location {
        Some(LocationPref::Coords { lat, lon }) => Ok(location::GeoResult {
            name: "saved".to_string(),
            latitude: lat,
            longitude: lon,
            timezone: "UTC".to_string(),
            country: None,
            admin1: None,
            elevation: None,
            population: None,
        }),
        Some(LocationPref::Name { name }) => {
            let results =
                location::geocode(&name).with_context(|| format!("geocoding '{name}'"))?;
            results
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no places matched saved location '{name}'"))
        }
        None => {
            // First run: ask the user for a location, save it, then geocode.
            let name = tui::prompt_location().context("location prompt")?;
            let results =
                location::geocode(&name).with_context(|| format!("geocoding '{name}'"))?;
            let geo = results
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no places matched '{name}'"))?;
            let mut cfg = config::load();
            cfg.location = Some(LocationPref::Name { name });
            config::save(&cfg).context("saving config")?;
            Ok(geo)
        }
    }
}

fn parse_at(raw: &str, now_unix: i64) -> Result<i64> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("--at is empty");
    }

    // Full RFC 3339, e.g. 2026-06-21T17:00:00Z
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).timestamp());
    }

    // Relative offset from now: +3h, -30m, +1d, +90s
    if s.starts_with('+') || s.starts_with('-') {
        return parse_relative(s, now_unix);
    }

    let today = DateTime::<Utc>::from_timestamp(now_unix, 0)
        .ok_or_else(|| anyhow!("system clock out of range"))?
        .date_naive();

    // Be friendly about a trailing Z on otherwise naive inputs.
    let body = s.trim_end_matches('Z');

    // Bare hour: "17", "9"
    if let Ok(h) = body.parse::<u32>()
        && h < 24
    {
        let t = NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        return Ok(NaiveDateTime::new(today, t).and_utc().timestamp());
    }

    // HH:MM or HH:MM:SS today
    if let Some(t) = parse_clock(body) {
        return Ok(NaiveDateTime::new(today, t).and_utc().timestamp());
    }

    // Date only: "2026-06-21" -> noon UTC (most visible sky)
    if let Ok(d) = NaiveDate::parse_from_str(body, "%Y-%m-%d") {
        let t = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        return Ok(NaiveDateTime::new(d, t).and_utc().timestamp());
    }

    // Date + time with either T or space separator.
    // Accepts a bare hour, HH:MM, or HH:MM:SS on the right side.
    if let Some((date_part, time_part)) = body.split_once('T').or_else(|| body.split_once(' '))
        && let Ok(d) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        && let Some(t) = parse_clock(time_part)
    {
        return Ok(NaiveDateTime::new(d, t).and_utc().timestamp());
    }

    bail!(
        "could not parse --at '{raw}' (try '17', '17:30', '+3h', '2026-06-21', or '2026-06-21T17:00:00Z')"
    )
}

fn parse_clock(s: &str) -> Option<NaiveTime> {
    if let Ok(h) = s.parse::<u32>()
        && h < 24
    {
        return NaiveTime::from_hms_opt(h, 0, 0);
    }
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .ok()
}

fn parse_relative(s: &str, now_unix: i64) -> Result<i64> {
    let sign: i64 = if s.starts_with('-') { -1 } else { 1 };
    let rest = &s[1..];
    let unit_pos = rest
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow!("relative offset '{s}' needs a unit (s, m, h, or d)"))?;
    let (num_str, unit) = rest.split_at(unit_pos);
    if num_str.is_empty() {
        bail!("relative offset '{s}' needs a number before the unit");
    }
    let n: i64 = num_str
        .parse()
        .map_err(|_| anyhow!("bad number in relative offset '{s}'"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => bail!("unknown unit '{unit}' in '{s}' (use s, m, h, or d)"),
    };
    Ok(now_unix + sign * secs)
}

fn nearest_hour_index(forecast: &forecast::Forecast, target_unix: i64) -> usize {
    let mut best = 0usize;
    let mut best_dist = i64::MAX;
    for (i, t) in forecast.hourly.time.iter().enumerate() {
        let Ok(naive) = NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M") else {
            continue;
        };
        let unix = naive.and_utc().timestamp();
        let dist = (unix - target_unix).abs();
        if dist < best_dist {
            best_dist = dist;
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd_hms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(hh, mm, ss)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    // Anchor "now" at 2026-04-11T12:00:00Z for all tests that do "today".
    fn now() -> i64 {
        ymd_hms(2026, 4, 11, 12, 0, 0)
    }

    fn at(s: &str) -> i64 {
        parse_at(s, now()).unwrap()
    }

    #[test]
    fn full_rfc3339_with_z() {
        assert_eq!(at("2026-06-21T17:00:00Z"), ymd_hms(2026, 6, 21, 17, 0, 0));
    }

    #[test]
    fn bare_hour_is_today_utc() {
        assert_eq!(at("17"), ymd_hms(2026, 4, 11, 17, 0, 0));
        assert_eq!(at("0"), ymd_hms(2026, 4, 11, 0, 0, 0));
        assert_eq!(at("23"), ymd_hms(2026, 4, 11, 23, 0, 0));
    }

    #[test]
    fn hour_minute_is_today_utc() {
        assert_eq!(at("17:30"), ymd_hms(2026, 4, 11, 17, 30, 0));
        assert_eq!(at("09:05"), ymd_hms(2026, 4, 11, 9, 5, 0));
    }

    #[test]
    fn relative_offsets() {
        let n = now();
        assert_eq!(at("+3h"), n + 3 * 3600);
        assert_eq!(at("-30m"), n - 30 * 60);
        assert_eq!(at("+1d"), n + 86400);
        assert_eq!(at("+90s"), n + 90);
    }

    #[test]
    fn date_only_defaults_to_noon() {
        assert_eq!(at("2026-06-21"), ymd_hms(2026, 6, 21, 12, 0, 0));
    }

    #[test]
    fn date_plus_bare_hour() {
        assert_eq!(at("2026-06-21T17"), ymd_hms(2026, 6, 21, 17, 0, 0));
        assert_eq!(at("2026-06-21 17"), ymd_hms(2026, 6, 21, 17, 0, 0));
    }

    #[test]
    fn date_plus_hhmm_with_trailing_z() {
        assert_eq!(at("2026-06-21T17:30Z"), ymd_hms(2026, 6, 21, 17, 30, 0));
    }

    #[test]
    fn garbage_errors() {
        let n = now();
        assert!(parse_at("not-a-time", n).is_err());
        assert!(parse_at("+h", n).is_err());
        assert!(parse_at("+3x", n).is_err());
        assert!(parse_at("25", n).is_err()); // 25 isn't a valid hour and not a date
    }
}
