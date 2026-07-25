//! The smallest useful embedding of celsius as a library: parse a scene, render it, write one truecolor half-block frame to any `Write`. No network, no feature flags, no terminal setup.
//!
//!     cargo run --example ansi_frame -- scenes/high_noon_clear.toml

use std::io::{self, Write};
use std::process::ExitCode;

use celsius::{load_scene, tui};

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: ansi_frame <scene.toml>");
        return ExitCode::FAILURE;
    };

    let scene = match load_scene(&path) {
        Ok(scene) => scene,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let mut out = io::stdout().lock();
    if let Err(error) = tui::write_frame(&scene, &mut out).and_then(|()| out.flush()) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
