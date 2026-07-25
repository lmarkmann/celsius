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
    // assert_cmd captures stdout (not a TTY), so with no flag the app must fall back to the flat surface, never paint escape codes into the pipe.
    let out = bin().args(["--scene", &scene()]).output().unwrap();
    assert!(out.status.success());
    assert!(!out.stdout.contains(&0x1b));
}

/// The installed-binary case: no repo checkout, no scenes/ directory in reach.
/// Runs from the temp dir so a stray relative path cannot rescue the lookup.
#[test]
fn builtin_scene_renders_with_no_files_on_disk() {
    let out = bin()
        .current_dir(std::env::temp_dir())
        .args(["--scene", "high_noon_clear", "--frame"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.contains(&0x1b));
}

#[test]
fn unknown_scene_name_lists_the_builtins() {
    bin()
        .current_dir(std::env::temp_dir())
        .args(["--scene", "golden_hour"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown scene"))
        .stderr(predicate::str::contains("golden_hour_cumulus"));
}

/// dawn is a work-in-progress scene: reachable by path, deliberately not shipped.
#[test]
fn wip_scene_is_not_a_builtin_but_still_loads_by_path() {
    bin()
        .current_dir(std::env::temp_dir())
        .args(["--scene", "dawn"])
        .assert()
        .failure();
    bin()
        .args(["--scene", &scene(), "--plain"])
        .assert()
        .success();
}

#[test]
fn frame_and_plain_conflict() {
    bin()
        .args(["--scene", &scene(), "--frame", "--plain"])
        .assert()
        .failure();
}
