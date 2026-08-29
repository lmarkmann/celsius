use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use celsius::astro::{moon_state, sun_position, to_sky_fracs};
use celsius::atmosphere::turbidity_from_visibility;
use celsius_lab::{
    SKY_HEIGHT, SKY_WIDTH, SceneSpec, analytic_state, compare_images, contact_sheet,
    load_reference, parse_at, render_scene, render_state, repo_root, save_scaled, scene_path,
    scene_toml,
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "celsius-lab",
    about = "Production-backed scene authoring tools for celsius"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print production sun and moon placement for an instant.
    Place {
        #[arg(long)]
        lat: f64,
        #[arg(long)]
        lon: f64,
        #[arg(long)]
        at: String,
        #[arg(long, default_value_t = 180.0)]
        facing: f64,
    },
    /// Scaffold a scene and render its first preview.
    New {
        name: String,
        #[arg(long)]
        lat: f64,
        #[arg(long)]
        lon: f64,
        #[arg(long)]
        at: String,
        #[arg(long, conflicts_with = "turbidity")]
        visibility: Option<f64>,
        #[arg(long)]
        turbidity: Option<f64>,
        #[arg(long, default_value_t = 180.0)]
        facing: f64,
        #[arg(long, default_value_t = 10)]
        stops: usize,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Render a scene with the production renderer.
    Render {
        scene: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 6)]
        scale: u32,
    },
    /// Compare a production render with a reference image.
    Diff {
        scene: String,
        #[arg(long)]
        against: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 6)]
        scale: u32,
    },
    /// Render the analytic sky across sun elevations and turbidities.
    Sweep {
        #[arg(long, value_delimiter = ',', default_value = "3,10,25,55")]
        altitudes: Vec<f64>,
        #[arg(long, value_delimiter = ',', default_value = "2,4,8")]
        turbidities: Vec<f64>,
        /// Sun azimuth offsets from frame centre, in degrees. The radiance field and the drawn disc only disagree off-centre, so a sweep at 0 alone cannot catch them drifting apart.
        #[arg(long, value_delimiter = ',', default_value = "0")]
        sun_az: Vec<f64>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 3)]
        scale: u32,
    },
    /// Preview meteor showers at their peaks, off-season, as a labeled sheet.
    Meteors {
        /// Shower name, or `all` for every shower in the IMO table.
        #[arg(default_value = "all")]
        shower: String,
        #[arg(long, default_value_t = 53.55)]
        lat: f64,
        #[arg(long, default_value_t = 9.99)]
        lon: f64,
        /// Local hour to sample, in UTC. Radiants climb through the night, so 02:00 shows most of them up.
        #[arg(long, default_value_t = 2)]
        hour: u32,
        #[arg(long, default_value_t = 2026)]
        year: i32,
        /// Exposure in seconds. Longer collects more meteors onto the frame.
        #[arg(long, default_value_t = 3600.0)]
        span: f64,
        #[arg(long, default_value_t = 180.0)]
        facing: f64,
        /// Sky width in pixels. Defaults to 3x the 104x50 reference so streaks have room to read.
        #[arg(long, default_value_t = SKY_WIDTH * 3)]
        width: u32,
        /// Sky height in pixels.
        #[arg(long, default_value_t = SKY_HEIGHT * 3)]
        height: u32,
        /// Also write an animated GIF per shower, playing each meteor's flight.
        #[arg(long)]
        gif: bool,
        /// Frames per meteor in the GIF.
        #[arg(long, default_value_t = 10)]
        steps: u32,
        /// Cap on GIF frames, so a Geminid run stays a reasonable file.
        #[arg(long, default_value_t = 300)]
        max_frames: usize,
        #[arg(long, default_value_t = 3)]
        columns: usize,
        #[arg(long, default_value_t = 2)]
        scale: u32,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Render every scene into a labeled contact sheet.
    Contact {
        #[arg(long, default_value_t = 3)]
        columns: usize,
        #[arg(long, default_value_t = 3)]
        scale: u32,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let root = repo_root()?;
    match Cli::parse().command {
        Command::Place {
            lat,
            lon,
            at,
            facing,
        } => place(lat, lon, &at, facing),
        Command::New {
            name,
            lat,
            lon,
            at,
            visibility,
            turbidity,
            facing,
            stops,
            force,
            out,
        } => new_scene(
            &root,
            &name,
            lat,
            lon,
            &at,
            visibility,
            turbidity,
            facing,
            stops,
            force,
            out.as_deref(),
        ),
        Command::Render { scene, out, scale } => {
            render_command(&root, &scene, out.as_deref(), scale)
        }
        Command::Diff {
            scene,
            against,
            out,
            scale,
        } => diff_command(&root, &scene, against.as_deref(), out.as_deref(), scale),
        Command::Sweep {
            altitudes,
            turbidities,
            sun_az,
            out,
            scale,
        } => sweep_command(
            &root,
            &altitudes,
            &turbidities,
            &sun_az,
            out.as_deref(),
            scale,
        ),
        Command::Meteors {
            shower,
            lat,
            lon,
            hour,
            year,
            span,
            facing,
            width,
            height,
            gif,
            steps,
            max_frames,
            columns,
            scale,
            out,
        } => meteors_command(MeteorsArgs {
            root: &root,
            shower: &shower,
            lat,
            lon,
            hour,
            year,
            span,
            facing,
            width,
            height,
            gif,
            steps,
            max_frames,
            columns,
            scale,
            out: out.as_deref(),
        }),
        Command::Contact {
            columns,
            scale,
            out,
        } => contact_command(&root, columns, scale, out.as_deref()),
    }
}

fn place(lat: f64, lon: f64, at: &str, facing: f64) -> Result<()> {
    let unix_utc = parse_at(at)?;
    let sun = sun_position(lat, lon, unix_utc);
    let moon = moon_state(lat, lon, unix_utc);
    let sun_screen = to_sky_fracs(&sun, facing);
    let moon_screen = to_sky_fracs(&moon.altaz, facing);
    let show = |p: Option<(f64, f64)>| match p {
        Some((x, y)) => format!("x={x:.3}  y={y:.3}"),
        None => "behind the viewer".to_string(),
    };
    println!(
        "sun  alt={:.2} deg  az={:.2} deg  {}",
        sun.altitude,
        sun.azimuth,
        show(sun_screen)
    );
    println!(
        "moon alt={:.2} deg  az={:.2} deg  {}  phase={:.3}  illumination={:.3}",
        moon.altaz.altitude,
        moon.altaz.azimuth,
        show(moon_screen),
        moon.phase,
        moon.illumination
    );
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "the CLI exposes each scene seed explicitly"
)]
fn new_scene(
    root: &Path,
    name: &str,
    lat: f64,
    lon: f64,
    at: &str,
    visibility: Option<f64>,
    turbidity: Option<f64>,
    facing: f64,
    stops: usize,
    force: bool,
    out: Option<&Path>,
) -> Result<()> {
    let scene = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| scene_path(root, name));
    if scene.exists() && !force {
        bail!("{} exists; pass --force to replace it", scene.display());
    }
    let unix_utc = parse_at(at)?;
    let turbidity =
        turbidity.unwrap_or_else(|| turbidity_from_visibility(visibility.map(|km| km * 1000.0)));
    let text = scene_toml(&SceneSpec {
        name,
        lat,
        lon,
        unix_utc,
        at_label: at,
        facing,
        turbidity,
        stops,
    })?;
    if let Some(parent) = scene.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&scene, text).with_context(|| format!("writing {}", scene.display()))?;
    let preview = render_scene(&scene)?;
    let output = root.join("out/lab").join(format!("{name}.png"));
    save_scaled(&preview, &output, 6)?;
    println!("scene:   {}", scene.display());
    println!("preview: {}", output.display());
    Ok(())
}

fn render_command(root: &Path, name: &str, out: Option<&Path>, scale: u32) -> Result<()> {
    let scene = scene_path(root, name);
    ensure!(scene.exists(), "scene not found: {}", scene.display());
    let image = render_scene(&scene)?;
    let stem = scene
        .file_stem()
        .and_then(|value| value.to_str())
        .context("scene has no UTF-8 file stem")?;
    let output = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("out/lab").join(format!("{stem}.png")));
    save_scaled(&image, &output, scale)?;
    println!(
        "rendered {} -> {} ({}x{})",
        scene.display(),
        output.display(),
        image.width() * scale,
        image.height() * scale
    );
    Ok(())
}

fn diff_command(
    root: &Path,
    name: &str,
    against: Option<&Path>,
    out: Option<&Path>,
    scale: u32,
) -> Result<()> {
    let scene = scene_path(root, name);
    ensure!(scene.exists(), "scene not found: {}", scene.display());
    let stem = scene
        .file_stem()
        .and_then(|value| value.to_str())
        .context("scene has no UTF-8 file stem")?;
    let reference = against.map(Path::to_path_buf).or_else(|| find_reference(root, stem)).with_context(|| format!("no reference for {stem}; pass --against or add tools/celsius-lab/references/{stem}.jpg"))?;
    let rendered = render_scene(&scene)?;
    let reference_image = load_reference(&reference)?;
    let (metrics, heatmap) = compare_images(&rendered, &reference_image)?;
    let output = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("out/lab").join(format!("{stem}_diff.png")));
    save_scaled(&heatmap, &output, scale)?;
    println!("scene:     {}", scene.display());
    println!("reference: {}", reference.display());
    println!(
        "pixels:    {}/{} differ ({:.2}%)",
        metrics.differing,
        metrics.total,
        metrics.differing as f64 / metrics.total as f64 * 100.0
    );
    println!(
        "RGB:       mean {:.2}  max {:.2} channel levels",
        metrics.mean_rgb_distance, metrics.max_rgb_distance
    );
    println!(
        "Oklab:     mean {:.4}  p95 {:.4}  max {:.4}",
        metrics.mean_delta_e, metrics.p95_delta_e, metrics.max_delta_e
    );
    println!("heatmap:   {}", output.display());
    Ok(())
}

fn sweep_command(
    root: &Path,
    altitudes: &[f64],
    turbidities: &[f64],
    sun_az: &[f64],
    out: Option<&Path>,
    scale: u32,
) -> Result<()> {
    ensure!(
        !altitudes.is_empty() && !turbidities.is_empty() && !sun_az.is_empty(),
        "a sweep needs at least one altitude, turbidity and azimuth"
    );
    let mut tiles = Vec::with_capacity(altitudes.len() * turbidities.len() * sun_az.len());
    for &altitude in altitudes {
        for &offset in sun_az {
            for &turbidity in turbidities {
                let state = analytic_state(altitude, turbidity, offset);
                let image = render_state(&state, SKY_WIDTH, SKY_HEIGHT)?;
                tiles.push((state.name.clone(), image));
            }
        }
    }
    let sheet = contact_sheet(&tiles, turbidities.len(), scale)?;
    let output = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("out/lab/sweep.png"));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    sheet
        .save(&output)
        .with_context(|| format!("writing {}", output.display()))?;
    println!(
        "sweep: {} altitudes x {} turbidities x {} azimuths -> {} ({}x{})",
        altitudes.len(),
        turbidities.len(),
        sun_az.len(),
        output.display(),
        sheet.width(),
        sheet.height()
    );
    Ok(())
}

/// Everything `meteors` needs. A struct rather than sixteen positional parameters, which is the point at which an argument list stops being readable.
struct MeteorsArgs<'a> {
    root: &'a Path,
    shower: &'a str,
    lat: f64,
    lon: f64,
    hour: u32,
    year: i32,
    span: f64,
    facing: f64,
    width: u32,
    height: u32,
    gif: bool,
    steps: u32,
    max_frames: usize,
    columns: usize,
    scale: u32,
    out: Option<&'a Path>,
}

/// One tile per shower, each rendered at its own peak date so the whole year is visible at once. Without this the only way to see a Geminid is to wait until December, because meteors are built from the live forecast and that reaches seven days.
fn meteors_command(args: MeteorsArgs<'_>) -> Result<()> {
    let wanted: Vec<&celsius::meteors::Shower> = if args.shower.eq_ignore_ascii_case("all") {
        celsius::meteors::SHOWERS.iter().collect()
    } else {
        let key = args.shower.replace(['_', '-'], " ").to_lowercase();
        let found: Vec<_> = celsius::meteors::SHOWERS
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&key))
            .collect();
        ensure!(
            !found.is_empty(),
            "no shower matches {:?}; known: {}",
            args.shower,
            celsius::meteors::SHOWERS
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        found
    };

    let dir = args
        .out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| args.root.join("out/lab/showers"));
    fs::create_dir_all(&dir)?;

    let mut tiles = Vec::with_capacity(wanted.len());
    for sh in &wanted {
        let unix_utc = peak_instant(args.year, sh.peak_yday, args.hour)?;
        let state = celsius_lab::meteor_state(
            args.root,
            sh.name,
            unix_utc,
            args.lat,
            args.lon,
            args.facing,
            args.span,
        )?;
        let (image, count) = celsius_lab::render_meteor_map(&state, args.width, args.height)?;
        let altaz = celsius::astro::equatorial_to_altaz(
            sh.ra_deg, sh.dec_deg, args.lat, args.lon, unix_utc,
        );
        // Split on anything that is not alphanumeric rather than substituting each separator, so "S. delta Aquariids" gives s-delta-aquariids and not s--delta-aquariids.
        let slug = sh
            .name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join("-");

        let still = dir.join(format!("{slug}.png"));
        image
            .save(&still)
            .with_context(|| format!("writing {}", still.display()))?;

        let mut gif_note = String::new();
        if args.gif && count > 0 {
            let path = dir.join(format!("{slug}.gif"));
            let frames = celsius_lab::meteor_gif(
                &state,
                args.width,
                args.height,
                args.steps,
                args.max_frames,
                &path,
            )?;
            gif_note = format!("  gif {frames} frames");
        }

        let radiant = match celsius::astro::to_sky_fracs(&altaz, args.facing) {
            Some((rx, ry)) => format!("screen x={rx:+.2} y={ry:+.2}"),
            None => "behind the viewer".to_string(),
        };
        println!(
            "{:<20} zhr {:>5.0}  v {:>2.0}  radiant alt {:>+6.1} az {:>5.1}  {radiant:<26} meteors {:>3}{}",
            sh.name, sh.zhr, sh.v_kms, altaz.altitude, altaz.azimuth, count, gif_note
        );
        tiles.push((
            format!("{} r{:+.0} n{}", sh.name, altaz.altitude, count),
            image,
        ));
    }

    let sheet = contact_sheet(&tiles, args.columns.min(tiles.len().max(1)), args.scale)?;
    let sheet_path = dir.join("_sheet.png");
    sheet
        .save(&sheet_path)
        .with_context(|| format!("writing {}", sheet_path.display()))?;
    println!(
        "\n{} showers at {}x{} -> {}",
        tiles.len(),
        args.width,
        args.height,
        dir.display()
    );
    Ok(())
}

/// A shower's peak day-of-year resolved to a UTC instant in `year`.
fn peak_instant(year: i32, peak_yday: f64, hour: u32) -> Result<i64> {
    let date = chrono::NaiveDate::from_yo_opt(year, peak_yday as u32)
        .with_context(|| format!("day {peak_yday} is not a date in {year}"))?;
    let naive = date
        .and_hms_opt(hour, 0, 0)
        .with_context(|| format!("hour {hour} is not a time"))?;
    Ok(naive.and_utc().timestamp())
}

fn contact_command(root: &Path, columns: usize, scale: u32, out: Option<&Path>) -> Result<()> {
    let mut paths = fs::read_dir(root.join("scenes"))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut scenes = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("scene has no UTF-8 file stem")?
            .to_string();
        scenes.push((name, render_scene(&path)?));
    }
    let sheet = contact_sheet(&scenes, columns, scale)?;
    let output = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("out/lab/contact.png"));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    sheet
        .save(&output)
        .with_context(|| format!("writing {}", output.display()))?;
    println!(
        "contact: {} scenes -> {} ({}x{})",
        scenes.len(),
        output.display(),
        sheet.width(),
        sheet.height()
    );
    Ok(())
}

fn find_reference(root: &Path, name: &str) -> Option<PathBuf> {
    ["jpg", "jpeg", "png", "webp"]
        .into_iter()
        .map(|extension| {
            root.join("tools/celsius-lab/references")
                .join(format!("{name}.{extension}"))
        })
        .find(|path| path.exists())
}
