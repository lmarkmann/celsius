# Changelog

All notable changes to celsius are recorded here. Format roughly follows
Keep a Changelog and versions follow SemVer.

## [0.4.2] - 2026-06-15

### Fixed

- Present TUI frames atomically to remove resize flicker ([#49](https://github.com/lmarkmann/celsius/pull/49))

## [0.4.1] - 2026-06-15

### Added

- Responsive footer chrome with daily high/low ([#48](https://github.com/lmarkmann/celsius/pull/48))
- Responsive minimum terminal size screen ([#44](https://github.com/lmarkmann/celsius/pull/44))

### Build

- Replace toml with basic-toml, generate goldens in Rust ([#46](https://github.com/lmarkmann/celsius/pull/46))

### Fixed

- Remove horizontal cloud seam, plus release-prep cleanup ([#47](https://github.com/lmarkmann/celsius/pull/47))

## [0.4.0] - 2026-06-12

### Added

- Bundle compose options into ComposeOpts ([#42](https://github.com/lmarkmann/celsius/pull/42))
- Validate scenes at parse, stabilize seeds, surface config errors ([#41](https://github.com/lmarkmann/celsius/pull/41))
- Physically-based analytic sky (Preetham) for the live daytime view ([#35](https://github.com/lmarkmann/celsius/pull/35))

### Build

- Move sha2 to dev-dependencies and trim ratatui features ([#37](https://github.com/lmarkmann/celsius/pull/37))

### Fixed

- Timeouts on weather fetches, ureq 3, panic-free error sky ([#40](https://github.com/lmarkmann/celsius/pull/40))

### Other

- Release PR body is just the changelog entry ([#43](https://github.com/lmarkmann/celsius/pull/43))

### Performance

- Hoist loop invariants out of the render hot paths ([#39](https://github.com/lmarkmann/celsius/pull/39))
- Cache the rendered sky and skip re-rendering idle frames ([#38](https://github.com/lmarkmann/celsius/pull/38))

## [0.3.2] - 2026-06-10

### Added

- Show sunrise and sunset times in the header ([#33](https://github.com/lmarkmann/celsius/pull/33))

## [0.3.1] - 2026-06-10

### Added

- Higher-fidelity sky synthesis (cover, palette, clouds, projection, sub-hour now) ([#32](https://github.com/lmarkmann/celsius/pull/32))

### Test

- Add overcast_night and moonless_darksky oracle goldens ([#30](https://github.com/lmarkmann/celsius/pull/30))

## [0.3.0] - 2026-06-10

### Added

- Add --plain/--frame surfaces and a testable TUI App ([#28](https://github.com/lmarkmann/celsius/pull/28))

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
