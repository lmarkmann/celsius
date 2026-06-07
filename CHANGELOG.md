# Changelog

All notable changes to celsius are recorded here. Format roughly follows
Keep a Changelog and versions follow SemVer.

## [0.2.2] - 2026-06-07

### Fixed

- Binstall pkg-url had a stray dot before archive-suffix, add bin-dir for flat archives

### Other

- Drop rayon, nothing uses it
- Dirs 6 (dedups windows-sys), toml 0.5 (drops toml_edit), add cargo-deny config

### Test

- Draw past the mt19937 refill boundary

## [0.2.1] - 2026-04-25

### Added

- Lightning flashes for thunderstorm scenes ([#23](https://github.com/lmarkmann/celsius/pull/23))

## [0.2.0] - 2026-04-25

### Added

- Bortle dark-sky class input via `--bortle 1..9` flag and `bortle` config field. Scales visible star count along the NELM curve and tints the gradient horizon with a warm sodium/LED glow when the sun is below the horizon. Default = unset = pre-0.2.0 behavior. ([#20](https://github.com/lmarkmann/celsius/pull/20))
- Prebuilt release binaries for `cargo binstall celsius`. From this release onward, `cargo binstall celsius` resolves directly to GitHub Release tarballs without a Rust toolchain. ([#20](https://github.com/lmarkmann/celsius/pull/20))

### Changed

- Layout reorg: `tools/` -> `scripts/`, `tests/fixtures/scenes/` -> `scenes/`, `goldens/` -> `tests/goldens/`, `tests/fixtures/open-meteo-*.json` -> `tests/`. Cleaner standard-Rust layout; oracle test, weather test, and justfile path imports updated to match. ([#20](https://github.com/lmarkmann/celsius/pull/20))

### Fixed

- Remove `scripts/` and `tests/goldens/` from .gitignore so release-plz can compute the next version (both directories contain tracked files). ([#21](https://github.com/lmarkmann/celsius/pull/21))
