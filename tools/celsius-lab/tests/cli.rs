use std::process::Command;

#[test]
fn help_lists_the_authoring_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_celsius-lab"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        stdout.contains("place")
            && stdout.contains("new")
            && stdout.contains("render")
            && stdout.contains("diff")
            && stdout.contains("sweep")
            && stdout.contains("contact"),
        "unexpected help output: {stdout}"
    );
}

#[test]
fn sweep_writes_a_sheet_for_the_requested_grid() {
    let dir = tempfile::tempdir().unwrap();
    let sheet = dir.path().join("sweep.png");

    let output = Command::new(env!("CARGO_BIN_EXE_celsius-lab"))
        .args(["sweep", "--altitudes", "10,40", "--turbidities", "2"])
        .arg("--scale")
        .arg("1")
        .arg("--out")
        .arg(&sheet)
        .output()
        .unwrap();

    assert!(
        output.status.success() && sheet.exists(),
        "unexpected sweep output: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn place_reports_production_sun_and_moon_coordinates() {
    let output = Command::new(env!("CARGO_BIN_EXE_celsius-lab"))
        .args([
            "place",
            "--lat",
            "53.5511",
            "--lon",
            "9.9937",
            "--at",
            "2026-04-11T06:14Z",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        output.status.success() && stdout.contains("sun  alt=") && stdout.contains("moon alt="),
        "unexpected place output: {stdout}"
    );
}
