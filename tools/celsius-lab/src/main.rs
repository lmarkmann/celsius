use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use celsius::astro::{moon_state, sun_position, to_sky_fracs};
use celsius::weather::turbidity_from_visibility;
use celsius_lab::{
    SceneSpec, compare_images, contact_sheet, load_reference, parse_at, render_scene, repo_root,
    save_scaled, scene_path, scene_toml,
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
    let (sun_x, sun_y) = to_sky_fracs(&sun, facing);
    let (moon_x, moon_y) = to_sky_fracs(&moon.altaz, facing);
    println!(
        "sun  alt={:.2} deg  az={:.2} deg  x={sun_x:.3}  y={sun_y:.3}",
        sun.altitude, sun.azimuth
    );
    println!(
        "moon alt={:.2} deg  az={:.2} deg  x={moon_x:.3}  y={moon_y:.3}  phase={:.3}  illumination={:.3}",
        moon.altaz.altitude, moon.altaz.azimuth, moon.phase, moon.illumination
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
