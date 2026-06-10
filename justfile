scene_dir := "scenes"

scenes := "golden_hour_cumulus blue_hour_calm high_noon_clear moonlit_clear_winter stormy_afternoon_advancing overcast_night moonless_darksky"

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

render name:
    mkdir -p out
    cargo run --release --features png -- render --scene {{scene_dir}}/{{name}}.toml --out out/{{name}}.png

render-all:
    for s in {{scenes}}; do just render $s; done

# Re-lock celsius goldens from the current renderer, then rewrite manifest.toml.
# Run this only after a deliberate pipeline change you want to bless.
lock:
    cargo build --release --features png
    mkdir -p tests/goldens
    for s in {{scenes}}; do ./target/release/celsius render --scene {{scene_dir}}/$s.toml --out tests/goldens/$s.png; done
    python3 scripts/write_manifest.py

verify:
    cargo test --release --features png -- --nocapture

bench:
    cargo bench --bench render
