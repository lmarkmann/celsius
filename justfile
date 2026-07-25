scene_dir := "scenes"

# Oracle-locked scene set: the single source of truth for `lock` and `render-all`.
# Add a name here only after blessing it; that is what pulls it into the oracle.
scenes := "golden_hour_cumulus blue_hour_calm high_noon_clear moonlit_clear_winter stormy_afternoon_advancing overcast_night moonless_darksky"

# Vendored in scenes/ but intentionally NOT locked yet: not pretty enough to bless.
# Render with `just render-wip`, and promote a name into `scenes` once it is good.
wip_scenes := "dawn cloudy_day"

default: check

# Fast static checks: fmt + clippy on both feature sets (mirrors CI's fmt/clippy steps).
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --features png -- -D warnings
    cargo clippy -p celsius-lab --all-targets -- -D warnings

# Everything the CI test job runs, before pushing (dependency policy is separate: `just policy`).
ci: check
    cargo nextest run --features png
    cargo nextest run -p celsius-lab
    cargo bench --bench render -- --test

# Dependency policy: advisories, licences, source registries, and the bans graph.
policy:
    cargo deny check

fmt:
    cargo fmt

# TUI-only release binary (no png sink).
build:
    cargo build --release

# Release binary with the oracle PNG sink compiled in.
build-oracle:
    cargo build --release --features png

# Release tests incl. the golden oracle (the deterministic scene-lock path).
test:
    cargo nextest run --release --features png

# Release tests with captured output shown (use to see which scene the oracle reports on failure).
verify:
    cargo nextest run --release --features png --no-capture

# Render one scene to out/NAME.png.
render name:
    mkdir -p out
    cargo run --release --features png -- render --scene {{scene_dir}}/{{name}}.toml --out out/{{name}}.png

# Render every locked scene to out/.
render-all:
    for s in {{scenes}}; do just render $s; done

# Render the held-back WIP scenes to out/ to judge whether they are ready to bless.
render-wip:
    for s in {{wip_scenes}}; do just render $s; done

# Run any production-backed scene-authoring command.
lab *args:
    cargo run -p celsius-lab -- {{args}}

# Render one scene to an enlarged PNG in out/lab/.
lab-render name *args:
    cargo run -p celsius-lab -- render {{name}} {{args}}

# Compare one scene with a reference image and write an Oklab heatmap.
lab-diff name *args:
    cargo run -p celsius-lab -- diff {{name}} {{args}}

# Render every root scene into a labeled contact sheet.
lab-contact *args:
    cargo run -p celsius-lab -- contact {{args}}

# Scaffold a scene from production astronomy and render its first preview.
lab-new name lat lon at *args:
    cargo run -p celsius-lab -- new {{name}} --lat {{lat}} --lon {{lon}} --at {{at}} {{args}}

# Re-lock celsius golden scene examples (PNGs + manifest.toml) from the current renderer. This should only be run after deliberate changes, of the scenes or how they are rendered.
lock:
    CELSIUS_SCENES="{{scenes}}" cargo test --release --features png --test oracle bless_goldens -- --ignored --nocapture

bench:
    cargo bench --bench render
