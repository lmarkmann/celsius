lab_scenes := "../skyterm-lab/scenes"

scenes := "golden_hour_cumulus blue_hour_calm high_noon_clear moonlit_clear_winter stormy_afternoon_advancing"

default: check

check:
    cargo clippy --all-targets --features png -- -D warnings
    cargo clippy -- -D warnings
    cargo fmt --check

fmt:
    cargo fmt

build:
    cargo build --release

# Release binary with the oracle PNG sink compiled in.
build-oracle:
    cargo build --release --features png

test:
    cargo test --release --features png

# Render one lab scene to out/<name>.png at the lab's 104x50 authoring size.
render name:
    mkdir -p out
    cargo run --release --features png -- render --scene {{lab_scenes}}/{{name}}.toml --out out/{{name}}.png

# Render all five lab scenes.
render-all:
    for s in {{scenes}}; do just render $s; done

# Re-lock celsius goldens from the current renderer, then rewrite manifest.toml.
# Run this only after a deliberate pipeline change you want to bless.
lock:
    cargo build --release --features png
    mkdir -p goldens
    for s in {{scenes}}; do ./target/release/celsius render --scene {{lab_scenes}}/$s.toml --out goldens/$s.png; done
    python3 tools/write_manifest.py

verify:
    cargo test --release --features png -- --nocapture

bench:
    cargo bench --bench render
