use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use celsius::{load_scene, render, terminal};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const WIDTH: u32 = 104;
const HEIGHT: u32 = 50;

const MANIFEST_HEADER: &str = "\
# celsius goldens: raw 104x50 renders (no chrome).
# scene_sha256 is the SHA-256 of the scene TOML the PNG was rendered from;
# png_sha256 is the SHA-256 of the locked PNG. The oracle test recomputes both
# and refuses to pass if either drifts. Regenerate with:
#   just lock
# Scene TOMLs are vendored in scenes/; the oracle reads them there.
";

#[derive(Deserialize)]
struct Entry {
    scene_sha256: String,
    png_sha256: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scene_path(root: &Path, name: &str) -> PathBuf {
    root.join("scenes").join(format!("{name}.toml"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Render a scene the way the oracle does and return its scene-TOML hash, the
/// PNG bytes, and the PNG hash. The checker and the bless writer both go through
/// here, so a locked manifest can never disagree with what the test verifies.
fn render_golden(scene: &Path) -> Result<(String, Vec<u8>, String)> {
    let scene_bytes = fs::read(scene)?;
    let scene_sha = sha256_hex(&scene_bytes);
    let state = load_scene(scene)?;
    let pixels = render(&state, WIDTH, HEIGHT);
    let png = terminal::encode_png(&pixels)?;
    let png_sha = sha256_hex(&png);
    Ok((scene_sha, png, png_sha))
}

#[test]
fn lab_scenes_match_locked_goldens() -> Result<()> {
    let root = repo_root();
    let manifest_path = root.join("tests/goldens/manifest.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: BTreeMap<String, Entry> = basic_toml::from_str(&manifest_text)?;

    for (name, entry) in &manifest {
        let scene = scene_path(&root, name);
        if !scene.exists() {
            bail!("{name}: vendored scene scenes/{name}.toml not found");
        }
        let (scene_sha, _png, png_sha) =
            render_golden(&scene).with_context(|| format!("rendering {name}"))?;
        if scene_sha != entry.scene_sha256 {
            bail!(
                "{name}: scene TOML drifted\n  expected {}\n  got      {}",
                entry.scene_sha256,
                scene_sha
            );
        }
        if png_sha != entry.png_sha256 {
            bail!(
                "{name}: render drifted\n  expected {}\n  got      {}",
                entry.png_sha256,
                png_sha
            );
        }
    }
    Ok(())
}

/// Regenerate `tests/goldens/<NAME>.png` and `manifest.toml` for every scene in
/// the space-separated `CELSIUS_SCENES`. Ignored by default; `just lock` runs it
/// with the scene list from the justfile. This is the old write_manifest.py,
/// moved into Rust so the repo carries no Python and the writer shares the
/// renderer/hash path with the checker above.
#[test]
#[ignore = "writes goldens; run via `just lock`"]
fn bless_goldens() -> Result<()> {
    let Ok(scenes) = std::env::var("CELSIUS_SCENES") else {
        bail!("set CELSIUS_SCENES (space-separated scene names); run via `just lock`");
    };
    let root = repo_root();
    let goldens = root.join("tests/goldens");
    fs::create_dir_all(&goldens)?;

    let mut manifest = String::from(MANIFEST_HEADER);
    let mut count = 0;
    for name in scenes.split_whitespace() {
        let (scene_sha, png, png_sha) =
            render_golden(&scene_path(&root, name)).with_context(|| format!("blessing {name}"))?;
        fs::write(goldens.join(format!("{name}.png")), &png)?;
        manifest.push_str(&format!(
            "\n[{name}]\nscene_sha256 = \"{scene_sha}\"\npng_sha256 = \"{png_sha}\"\n"
        ));
        count += 1;
    }
    fs::write(goldens.join("manifest.toml"), &manifest)?;
    println!("blessed {count} scenes into {}", goldens.display());
    Ok(())
}
