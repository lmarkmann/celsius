# Changelog

All notable changes to celsius are recorded here. Format roughly follows
Keep a Changelog and versions follow SemVer.
## [0.1.0] - 2026-04-11

### Added

- Config file + first-run location prompt ([#7](https://github.com/lmarkmann/celsius/pull/7))
- ? help overlay, l location overlay, r retry, error sky ([#9](https://github.com/lmarkmann/celsius/pull/9))
- --facing flag for viewer compass bearing ([#6](https://github.com/lmarkmann/celsius/pull/6))
- Cloud drift animation, space to pause ([#4](https://github.com/lmarkmann/celsius/pull/4))

### Ci

- Vendor lab scenes so oracle test runs in CI ([#3](https://github.com/lmarkmann/celsius/pull/3))
- Github actions workflow for fmt + clippy + test

### Dev

- Criterion benchmarks + cargo-audit in CI ([#5](https://github.com/lmarkmann/celsius/pull/5))

### Fixed

- Fix readme: correct flag names, add keybindings table ([#14](https://github.com/lmarkmann/celsius/pull/14))
- Fix ci: replace rustsec/audit-check with taiki-e install + direct cargo audit ([#15](https://github.com/lmarkmann/celsius/pull/15))

### Other

- Add crates.io metadata to Cargo.toml ([#8](https://github.com/lmarkmann/celsius/pull/8))

### Package

- Exclude demos/goldens/tools/benches/tests/.github from crate ([#17](https://github.com/lmarkmann/celsius/pull/17))

### Tui

- Render chrome header and footer bars
