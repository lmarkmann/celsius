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

# Everything the CI test job runs, before pushing (audit still lives only in CI).
ci: check
    cargo test --features png
    cargo bench --bench render -- --test

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
    cargo test --release --features png

# Release tests with captured output shown (use to see which scene the oracle reports on failure).
verify:
    cargo test --release --features png -- --nocapture

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

# Re-lock celsius goldens (PNGs + manifest.toml) from the current renderer. Run this only after a deliberate pipeline change you want to bless. The bless test renders each scene, writes its PNG, and rewrites manifest.toml.
lock:
    CELSIUS_SCENES="{{scenes}}" cargo test --release --features png --test oracle bless_goldens -- --ignored --nocapture

# Criterion render + noise benchmarks.
bench:
    cargo bench --bench render
