use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use celsius::{load_scene, render, terminal};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const WIDTH: u32 = 104;
const HEIGHT: u32 = 50;

#[derive(Deserialize)]
struct Entry {
    scene_sha256: String,
    png_sha256: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[test]
fn lab_scenes_match_locked_goldens() -> Result<()> {
    let root = repo_root();
    let manifest_path = root.join("tests/goldens/manifest.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: BTreeMap<String, Entry> = toml::from_str(&manifest_text)?;

    for (name, entry) in &manifest {
        let lab_path = root
            .join("../skyterm-lab/scenes")
            .join(format!("{name}.toml"));
        let vendor_path = root.join("scenes").join(format!("{name}.toml"));
        let scene_path = if lab_path.exists() {
            lab_path
        } else {
            vendor_path
        };
        if !scene_path.exists() {
            bail!("{name}: scene not found at lab path or vendored fixture");
        }
        let scene_bytes = fs::read(&scene_path)?;
        let scene_sha = sha256_hex(&scene_bytes);
        if scene_sha != entry.scene_sha256 {
            bail!(
                "{name}: scene TOML drifted\n  expected {}\n  got      {}",
                entry.scene_sha256,
                scene_sha
            );
        }

        let state = load_scene(&scene_path)?;
        let pixels = render(&state, WIDTH, HEIGHT);
        let png = terminal::encode_png(&pixels)?;
        let png_sha = sha256_hex(&png);
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
