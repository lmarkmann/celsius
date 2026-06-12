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

# Re-lock celsius goldens (PNGs + manifest.toml) from the current renderer.
# Run this only after a deliberate pipeline change you want to bless. The bless
# test renders each scene, writes its PNG, and rewrites manifest.toml.
lock:
    CELSIUS_SCENES="{{scenes}}" cargo test --release --features png --test oracle bless_goldens -- --ignored --nocapture

verify:
    cargo test --release --features png -- --nocapture

bench:
    cargo bench --bench render
