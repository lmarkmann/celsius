use std::io::{BufWriter, IsTerminal, Write, stdout};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use clap::Parser;
#[cfg(feature = "png")]
use clap::Subcommand;
use clap::builder::styling::{AnsiColor, Styles};

use celsius::tui::{RunOutcome, Timeline};
use celsius::weather::{compose, error_sky, forecast, location};
use celsius::{load_scene, tui};
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
  \x1b[1;36mcelsius -l \"Reykjavík\" --at 2026-06-21T00:00:00Z\x1b[0m
  \x1b[1;36mcelsius --lat 53.55 --lon 9.99\x1b[0m
  \x1b[1;36mcelsius --scene ../skyterm-lab/scenes/golden_hour_cumulus.toml\x1b[0m
  \x1b[1;36mcelsius render --scene scene.toml --out scene.png\x1b[0m
";

#[cfg(not(feature = "png"))]
const AFTER_HELP: &str = "\x1b[1;32mExamples:\x1b[0m
  \x1b[1;36mcelsius -l Hamburg\x1b[0m
  \x1b[1;36mcelsius -l \"Reykjavík\" --at 2026-06-21T00:00:00Z\x1b[0m
  \x1b[1;36mcelsius --lat 53.55 --lon 9.99\x1b[0m
  \x1b[1;36mcelsius --scene ../skyterm-lab/scenes/golden_hour_cumulus.toml\x1b[0m
";

#[derive(Parser)]
#[command(
    name = "celsius",
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

    /// ISO 8601 UTC timestamp to start scrubbing from. Default: now.
    #[arg(long, value_name = "ISO8601", global = true)]
    at: Option<String>,

    /// Load a lab scene TOML directly instead of synthesizing from weather.
    #[arg(long, value_name = "PATH", global = true)]
    scene: Option<PathBuf>,

    /// Compass bearing the viewer faces: 0=N, 90=E, 180=S, 270=W.
    /// Default 180 (south) suits northern-hemisphere observers.
    #[arg(long, value_name = "DEG", global = true, default_value_t = 180.0)]
    facing: f64,

    #[cfg(feature = "png")]
    #[command(subcommand)]
    command: Option<Command>,
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
        let timeline = Timeline::single(state);
        if stdout().is_terminal() {
            match tui::run(&timeline).context("running tui")? {
                RunOutcome::Quit | RunOutcome::Retry => {}
                RunOutcome::ChangeLocation(_) => {}
            }
        } else {
            let mut out = BufWriter::new(stdout().lock());
            tui::write_frame(&timeline.states[timeline.home], &mut out)
                .context("writing frame to non-tty stdout")?;
            out.flush().context("flushing stdout")?;
        }
        return Ok(());
    }

    // Live weather path: retry loop handles fetch errors and location changes.
    let mut location_override: Option<String> = None;
    loop {
        let timeline = match build_live_timeline(&cli, location_override.as_deref()) {
            Ok(t) => t,
            Err(e) => Timeline::single(error_sky(&e.to_string())),
        };

        if !stdout().is_terminal() {
            let mut out = BufWriter::new(stdout().lock());
            tui::write_frame(&timeline.states[timeline.home], &mut out)
                .context("writing frame to non-tty stdout")?;
            out.flush().context("flushing stdout")?;
            return Ok(());
        }

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
        Some(s) => parse_iso8601_utc(s)?,
        None => now_unix,
    };

    let location = resolve_location(cli, location_override)?;
    let forecast = forecast::fetch(location.latitude, location.longitude)
        .with_context(|| format!("fetching forecast for {}", location.label()))?;
    let hours = forecast.hourly.len();
    if hours == 0 {
        bail!("forecast returned zero hours for {}", location.label());
    }

    let center_az = cli.facing;
    let states: Vec<_> = (0..hours)
        .map(|h| compose(&forecast, &location, h, now_unix, center_az))
        .collect::<Result<_, _>>()
        .context("composing sky timeline")?;
    let home = nearest_hour_index(&forecast, at_unix);
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
        (None, None, None) => {
            bail!("no location given; pass -l NAME, --lat F --lon F, or --scene PATH")
        }
    }
}

fn parse_iso8601_utc(s: &str) -> Result<i64> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).timestamp());
    }
    let trimmed = s.trim_end_matches('Z');
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M"))
        .map_err(|e| anyhow!("could not parse --at '{s}': {e}"))?;
    Ok(naive.and_utc().timestamp())
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
