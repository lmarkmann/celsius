use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("celsius").expect("celsius binary")
}

fn scene() -> String {
    format!("{}/scenes/dawn.toml", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn version_matches_manifest() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("celsius {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_documents_examples_without_entering_tui() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("celsius -l Hamburg"));
}

#[test]
fn plain_surface_is_flat_text_with_no_ansi() {
    let out = bin()
        .args(["--scene", &scene(), "--plain"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !out.stdout.contains(&0x1b),
        "ANSI escape leaked into the plain surface"
    );
}

#[test]
fn frame_surface_emits_ansi() {
    let out = bin()
        .args(["--scene", &scene(), "--frame"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stdout.contains(&0x1b),
        "the --frame surface should emit ANSI half-blocks"
    );
}

#[test]
fn piped_stdout_defaults_to_plain() {
    // assert_cmd captures stdout (not a TTY), so with no flag the app must fall
    // back to the flat surface, never paint escape codes into the pipe.
    let out = bin().args(["--scene", &scene()]).output().unwrap();
    assert!(out.status.success());
    assert!(!out.stdout.contains(&0x1b));
}

#[test]
fn frame_and_plain_conflict() {
    bin()
        .args(["--scene", &scene(), "--frame", "--plain"])
        .assert()
        .failure();
}
