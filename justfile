scene_dir := "scenes"

# Oracle-locked scene set: the single source of truth for `lock` and `render-all`.
# Add a name here only after blessing it; that is what pulls it into the oracle.
scenes := "golden_hour_cumulus blue_hour_calm high_noon_clear moonlit_clear_winter stormy_afternoon_advancing overcast_night moonless_darksky"

# Vendored in scenes/ but intentionally NOT locked yet: not pretty enough to bless.
# Render with `just render-wip`, and promote a name into `scenes` once it is good.
wip_scenes := "dawn cloudy_day"

default: check

# Fast static checks: fmt + clippy on both feature sets (mirrors CI's fmt/clippy steps).
# cargo doc is here rather than in CI because nothing in CI builds the docs at all, so
# `build.warnings = "deny"` was gating only whoever happened to run it locally, and a
# broken intra-doc link reached a branch undetected. In `check` it runs on every commit.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --features png -- -D warnings
    cargo clippy -p celsius-lab --all-targets -- -D warnings
    cargo doc --no-deps --features png

# Everything the CI test job runs, before pushing (dependency policy is separate: `just policy`).
ci: check
    cargo nextest run --features png
    cargo nextest run -p celsius-lab
    cargo bench --bench render -- --test

# Dependency policy: advisories, licences, source registries, and the bans graph.
policy:
    cargo deny check

# The API-compatibility gate release-plz runs, reproduced locally so a breaking change is found before the release PR rather than in it. On 0.x an incompatible change forces 0.x.0 whatever the commit type.
semver:
    cargo semver-checks --baseline-rev main

fmt:
    cargo fmt

# TUI-only release binary (no png sink).
build:
    cargo build --release

# Release binary with the oracle PNG sink compiled in.
build-oracle:
    cargo build --release --features png

# Fat LTO, one codegen unit, dependencies at opt-level "z". About a third smaller than
# `just build` and much slower to link, which is why it is not [profile.release].
# The profile release.yml ships, so this is what a user actually downloads.
dist:
    cargo build --profile dist

# [profile.release] is what the test gate runs, so nothing otherwise proves fat LTO and
# opt-level "z" leave the render bit-identical. They do; this keeps it that way.
# The goldens against the profile we actually ship.
dist-check:
    cargo nextest run --cargo-profile dist --features png

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

# Sweep the analytic sky across sun elevations and turbidities.
lab-sweep *args:
    cargo run -p celsius-lab -- sweep {{args}}

# Scaffold a scene from production astronomy and render its first preview.
lab-new name lat lon at *args:
    cargo run -p celsius-lab -- new {{name}} --lat {{lat}} --lon {{lon}} --at {{at}} {{args}}

# Re-lock celsius golden scene examples (PNGs + manifest.toml) from the current renderer. This should only be run after deliberate changes, of the scenes or how they are rendered.
lock:
    CELSIUS_SCENES="{{scenes}}" cargo test --release --features png --test oracle bless_goldens -- --ignored --nocapture

bench:
    cargo bench --bench render

# Config lives in .cargo/mutants.toml. The whole crate is ~3600 mutants and hours of
# wall clock, so reach for `mutants-file` or `mutants-diff` unless you mean it.
# Every mutant the suite fails to kill: code a test runs but nothing asserts on.
mutants:
    cargo mutants --no-shuffle --in-place

# One module's mutants, which is the shape worth running by hand.
mutants-file file:
    cargo mutants --no-shuffle --in-place --file {{file}}

# What the mutants CI job runs: only the code this branch changed.
mutants-diff:
    git diff $(git merge-base HEAD main) HEAD -- src tests > /tmp/celsius-mutants.diff
    cargo mutants --no-shuffle --in-place --in-diff /tmp/celsius-mutants.diff

# --all-features so the png-gated sinks and the oracle count as covered code
# rather than reading as dead. High coverage here still means the goldens executed
# the line, not that anything checked the result; `mutants` is what checks that.
# Line coverage, which is where the percentages quoted in the docs come from.
cov:
    cargo llvm-cov --locked --all-features --summary-only

# Coverage as lcov, for an editor gutter or CI upload.
cov-lcov:
    cargo llvm-cov --locked --all-features --lcov --output-path lcov.info

# Build and run the benches under cargo-codspeed. Reports no timings off Linux; real numbers come from the CodSpeed job on the PR.
codspeed:
    cargo codspeed build -m simulation -m memory
    cargo codspeed run
