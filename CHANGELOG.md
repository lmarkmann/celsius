# Changelog

All notable changes to celsius are recorded here. The format roughly follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-07-27

The sky is now projected the way a camera sees it rather than the way a diagram does, meteors fall on clear nights, and the two parts of the renderer that no test could reach are covered ([#68](https://github.com/lmarkmann/celsius/pull/68), [#69](https://github.com/lmarkmann/celsius/pull/69)).

### Breaking

- `celsius::haze` is gone; its one function folded into `celsius::render`.
- `celsius::lightning::Lightning::new` takes four parameters instead of six.
- `celsius::pigs` is no longer public. It documented where and when a hidden easter egg appears, and a public module published that to docs.rs.
- `SkyState`, `Timeline`, and `Config` each gained a field, so struct literals over them need updating.
- `--facing` no longer carries a fixed default of 180. Passing it explicitly behaves as before.

### Added

- **Meteors on clear nights.** A sporadic background plus the IMO working list of major showers, with each radiant placed from its J2000 coordinates and meteors streaming away from it. Faster showers read cooler and longer. Rates are scaled down twice: to the frame's share of the sky, because ZHR counts a whole hemisphere while every meteor is drawn inside the frame, and again for light pollution when `--bortle` is set.
- **Built-in scene names.** `--scene high_noon_clear` works with no files on disk, so a `cargo install` or `brew install` user can see the locked scene library immediately. Paths still work.
- **`--facing` persists** through `~/.config/celsius/config.toml`, alongside the location and Bortle class.
- **A declared minimum supported Rust version**, `1.88`, found by bisection rather than copied from a dependency, with a CI job pinned to it so the floor cannot drift silently.

### Changed

- **The projection is rectilinear.** It was orthographic. Straight lines in the sky now stay straight on screen, which is what lets a meteor shower's fan be drawn as straight rays from its radiant. The frame covers 110 degrees horizontally and reaches about 69 degrees of altitude rather than the zenith.
- **Gradients, cloud altitudes, haze, and star visibility are placed on an altitude axis** instead of on screen rows. The mapping from row to viewing angle is not linear, so a palette placed by row was being squashed into whatever slice of sky the field of view happened to cover.
- **The analytic sky sets its exposure from the sky it is drawing**, surveying the frame rather than using one constant tuned against a clear noon zenith. Two skies are therefore no longer on a common absolute scale.
- Linux release binaries are now `x86_64-unknown-linux-musl`. The gnu artifacts inherited the build runner's glibc and required `GLIBC_2.39`, so they failed to start on Ubuntu 22.04, Debian 12, RHEL 9, and Alpine, with no error message under our control. The release matrix drops from five targets to three; Intel Mac and ARM Linux fall back to a source build or `cargo install`. Every archive ships a `.sha256` sibling.
- `crossterm` updated to 0.29.

### Fixed

- **The waning half of the moon was inverted.** The terminator expression was negated for phases past full instead of mirrored, which swaps lit for dark as well as left for right: a waning gibbous drew as a dark face inside a bright rim, and a waning crescent as an almost fully lit disc. The quarters are the one place the correct and incorrect forms agree, which is why it went unnoticed. Roughly half of all nights were affected.
- **`--facing` defaulted to due south regardless of hemisphere**, putting every southern-hemisphere viewer at the back of their own sky, watching the half the sun never crosses. It now follows the sign of the latitude.
- **Stars were scattered evenly across the screen rather than across the sky.** Under this projection a pixel near the edge of the frame covers less sky than one at the centre, so an even screen scatter crowded them into the corners by roughly six to one.
- Cloud interiors read as flat colour, because thickness saturated to full opacity as soon as noise passed the layer threshold. Opacity is now Beer-Lambert and thickness also shades the cloud toward its shadow tone.
- Wide terminals showed the same few cloud blobs, larger. Detail now resolves with buffer width, capped where the noise grid would tile.
- The clear-sky model was painting overcast skies.
- Meteor travel and streak length were measured on different bases, so how far a meteor flew depended on which direction it went.

### Performance

- **A night frame renders 26 percent faster**, 136 to 100 microseconds at the reference size. `render()` rebuilt the star field on every call, and placing stars on the sky means rejecting most candidates, so the rebuild had grown to a quarter of the frame while the TUI redrew an unchanged sky thirty times a second. It is now cached on the state that produces it.

### Testing

- `analytic_sky.rs` went from **0 to 100 percent** coverage and `lightning.rs` from 13 to 97. Both were unreachable by the golden oracle: scene files cannot express an analytic sky, and the tick overlays composite outside `render()`.
- The new tests assert properties rather than hashing frames, because a hash only catches a change once someone chooses to relock, at which point the new value silently becomes the truth. The radiance peak must sit within a pixel of where the sun disc is drawn; a quiet lightning frame must be byte-identical to the sky beneath it.
- Surviving mutants fell from 22 to 6, and the remaining six are equivalent mutants that cannot change behaviour. Chasing them is what surfaced the moon bug above.

## [0.4.7] - 2026-07-15

### Security

- Updated `crossbeam-epoch` from 0.9.18 to 0.9.20 to resolve RUSTSEC-2026-0204, which caused the audit job on the v0.4.6 tag to fail.
- Updated `anyhow` from 1.0.102 to 1.0.103 to clear the related RUSTSEC-2026-0190 warning. This release changes dependencies only and does not change celsius runtime behavior ([#64](https://github.com/lmarkmann/celsius/pull/64)).

## [0.4.6] - 2026-07-13

### Testing

- Added a fixture-driven test for the hour-index `compose()` entry point. It locks a clear pre-dawn forecast to its expected `SkyState` and proves that `compose()` agrees with the interpolating `compose_at()` path at the top of an hour.
- Documented that truncating noise seeds to 32 bits is deliberate because it preserves bit parity with the private scene lab. This release contains no runtime behavior changes ([#62](https://github.com/lmarkmann/celsius/pull/62)).

## [0.4.5] - 2026-06-19

### Fixed

- Kept the current sky visible while a newly selected city loads. Forecast fetching and composition now run on a worker thread beneath a cancellable loading overlay, and one terminal session owns the live sky, location picker, and loading state. First launch and retry use the same path ([#58](https://github.com/lmarkmann/celsius/pull/58)).
- Made the minimum-size screen exclusive so an open help overlay can no longer cover the resize instructions ([#60](https://github.com/lmarkmann/celsius/pull/60)).
- Keyed time-gated overlays to the forecast instant on screen instead of the machine clock. Timeline scrubbing and `--at` now keep the displayed sky and its overlays on the same hour ([#61](https://github.com/lmarkmann/celsius/pull/61)).

## [0.4.4] - 2026-06-16

### Added

- Displayed the viewed location's wall clock, sunrise, sunset, and daily high/low against its Open-Meteo UTC offset rather than the computer's local timezone. The astronomy and seed pipeline remain in UTC, and daily lookups use the viewed location's calendar date across midnight ([#56](https://github.com/lmarkmann/celsius/pull/56)).
- Added a hidden location-and-time-gated animated sky Easter egg. It is composited by the TUI outside the deterministic renderer, so scene goldens remain unchanged ([#56](https://github.com/lmarkmann/celsius/pull/56)).

## [0.4.3] - 2026-06-16

### Added

- Replaced the blocking location prompt with a live type-ahead picker. Search is debounced and runs on a worker thread, arrow keys move through a scrolling result list, and the UI distinguishes loading, no matches, and network failure states ([#55](https://github.com/lmarkmann/celsius/pull/55)).
- Ranked ambiguous geocoding matches by population, so a major city is shown before a much smaller place with the same or a similar name. Population is used only for ranking and is not displayed ([#55](https://github.com/lmarkmann/celsius/pull/55)).

### Fixed

- Preserved geocoder capitalization in location names and made compass labels consistently uppercase across the TUI and `--plain` output ([#53](https://github.com/lmarkmann/celsius/pull/53)).

### Testing

- Added fixture tests that lock the forecast-to-`SkyState` mapping before and after sunrise, including star visibility, analytic-sky attachment, and the south-facing projection rule ([#52](https://github.com/lmarkmann/celsius/pull/52)).

### Maintenance

- Updated GitHub Actions that still used the deprecated Node.js 20 runtime, without changing the release or Homebrew workflow inputs ([#51](https://github.com/lmarkmann/celsius/pull/51)).

## [0.4.2] - 2026-06-15

### Fixed

- Wrapped terminal draws in DEC 2026 synchronized output so a resize clear and repaint are presented atomically on supporting terminals. This removes the blank flash seen while dragging a terminal window ([#49](https://github.com/lmarkmann/celsius/pull/49)).
- Coalesced queued resize events before drawing and gated redraws on visible changes. A still sky now remains idle instead of repainting about 30 times per second. Terminals without synchronized-output support still benefit from the reduced repaint count ([#49](https://github.com/lmarkmann/celsius/pull/49)).

## [0.4.1] - 2026-06-15

### Added

- Replaced clipped output in small terminals with a responsive minimum-size screen. It reports the required 60 by 25 dimensions when space allows and degrades to a compact branded message in extremely small windows ([#44](https://github.com/lmarkmann/celsius/pull/44)).
- Added the viewed day's high and low next to the current temperature. Celsius now fetches Open-Meteo's daily maximum and minimum fields and follows the selected day while scrubbing the forecast ([#48](https://github.com/lmarkmann/celsius/pull/48)).
- Made footer chrome width-responsive. Key hints collapse in tiers before any live weather reading is removed, with `? help` retained as the final hint ([#48](https://github.com/lmarkmann/celsius/pull/48)).

### Fixed

- Removed the hard horizontal seam visible at the edge of dense cloud decks. Cloud altitude masks now feather smoothly from a negligible-density cutoff into the body of the layer instead of switching an entire row on at once ([#47](https://github.com/lmarkmann/celsius/pull/47)).
- Replaced a private lab path in CLI help with the public vendored scene path ([#47](https://github.com/lmarkmann/celsius/pull/47)).

### Maintenance

- Replaced the `toml` parser with `basic-toml` to reduce the dependency tree used by scene and config loading ([#46](https://github.com/lmarkmann/celsius/pull/46)).
- Moved golden generation into the ignored Rust `bless_goldens` test. The writer and checker now share one render-and-hash path, and the public repository no longer contains Python tooling ([#46](https://github.com/lmarkmann/celsius/pull/46)).

## [0.4.0] - 2026-06-12

### Added

- Made the Preetham analytic daytime sky the default for live weather. The Perez radiance model derives the zenith-to-horizon falloff, sun-side warmth, and clear-to-hazy color shift from sun position and visibility-derived turbidity. It blends into the palette over the first 8 degrees above the horizon, while `--sky palette` retains the previous renderer ([#35](https://github.com/lmarkmann/celsius/pull/35)).

### Changed

- Replaced stringly precipitation kinds with the `PrecipKind` enum and rejected empty gradients while loading a scene. Invalid scene files now fail at parse time instead of silently changing precipitation or panicking during render ([#41](https://github.com/lmarkmann/celsius/pull/41)).
- Replaced toolchain-dependent `DefaultHasher` weather seeds with stable inline FNV-1a mixing. Live cloud, star, and precipitation layouts are now stable across Rust releases ([#41](https://github.com/lmarkmann/celsius/pull/41)).
- Added `ComposeOpts` to group facing direction, Bortle class, and analytic-sky selection. This is a breaking library API change to the `compose()` and `compose_at()` signatures ([#42](https://github.com/lmarkmann/celsius/pull/42)).

### Fixed

- Added shared 5-second connect and 15-second global timeouts to both Open-Meteo endpoints, so launch and in-TUI retry cannot hang indefinitely on a stalled connection ([#40](https://github.com/lmarkmann/celsius/pull/40)).
- Made error-footer truncation respect UTF-8 character boundaries, preventing a second panic while reporting failures that contain non-ASCII place names ([#40](https://github.com/lmarkmann/celsius/pull/40)).
- Warned when a malformed config file is encountered instead of silently replacing it with defaults on the next save. Config saving now exposes a typed `ConfigError` at the library boundary ([#41](https://github.com/lmarkmann/celsius/pull/41)).
- Continued scanning lightning strikes after a boltless sheet flash, so a later visible bolt in the same schedule is no longer hidden ([#41](https://github.com/lmarkmann/celsius/pull/41)).
- Treated an empty `NO_COLOR` environment variable as unset, matching the specification ([#42](https://github.com/lmarkmann/celsius/pull/42)).

### Performance

- Cached the base `PixelBuffer` until clouds move, the forecast is scrubbed, or the viewport changes. Paused and windless skies no longer rerun the full renderer on every TUI tick, while lightning remains independently animated ([#38](https://github.com/lmarkmann/celsius/pull/38)).
- Hoisted row and layer invariants out of per-pixel loops, stored stars in a row-major vector, cached noise grids by seed, and avoided repeated Oklab conversions. Golden hashes stayed byte-identical; measured night and moonlight benchmarks improved by about 32 and 31 percent respectively ([#39](https://github.com/lmarkmann/celsius/pull/39)).

### Maintenance

- Upgraded to `ureq` 3 with only Rustls and JSON enabled, removed unused URL, international-domain, and compression stacks, and reduced the runtime dependency tree from 131 to 105 crates ([#40](https://github.com/lmarkmann/celsius/pull/40)).
- Moved `sha2` to development dependencies and disabled unused `ratatui` features, reducing the runtime dependency tree from 144 to 131 crates before the `ureq` reduction above ([#37](https://github.com/lmarkmann/celsius/pull/37)).

## [0.3.2] - 2026-06-10

### Added

- Requested daily sunrise, sunset, and daylight duration from Open-Meteo and displayed sunrise and sunset in the header for the viewed forecast day ([#33](https://github.com/lmarkmann/celsius/pull/33)).
- Detected polar day and polar night from daylight duration and displayed those states instead of misleading clock times ([#33](https://github.com/lmarkmann/celsius/pull/33)).

## [0.3.1] - 2026-06-10

### Added

- Used Open-Meteo's total cloud cover, with a union of low, middle, and high layers as fallback, so a full stratus layer no longer appears one-third covered ([#32](https://github.com/lmarkmann/celsius/pull/32)).
- Blended palette transitions continuously in Oklab by sun altitude, then faded toward cloudy or overcast conditions by total cover. Cloudy nights remain on the night palette ([#32](https://github.com/lmarkmann/celsius/pull/32)).
- Added cloud morphology for cirrus, altocumulus, stratus, cumulus, and cumulonimbus, including cover-dependent flattening for solid overcast decks and distinct lit and shadow colors ([#32](https://github.com/lmarkmann/celsius/pull/32)).
- Projected sun and moon positions onto the sky dome, including azimuth foreshortening near the zenith and a bowed solar arc ([#32](https://github.com/lmarkmann/celsius/pull/32)).
- Added a sun-relative warm horizon bias that is strongest near sunrise and sunset and damped by cloud cover ([#32](https://github.com/lmarkmann/celsius/pull/32)).
- Interpolated cloud, precipitation, wind, visibility, and temperature between forecast hours for the current view. The home position now represents the exact current minute instead of the start of the hour ([#32](https://github.com/lmarkmann/celsius/pull/32)).

### Testing

- Added `overcast_night` and `moonless_darksky` to the locked golden set and made the vendored `scenes/` directory the reproducible source used by golden tooling ([#30](https://github.com/lmarkmann/celsius/pull/30)).

## [0.3.0] - 2026-06-10

### Added

- Added `--plain` for a one-line ASCII weather status and `--frame` for an explicit ANSI half-block capture. Pipes, `--no-tui`, and a nonempty `NO_COLOR` now select plain output automatically ([#28](https://github.com/lmarkmann/celsius/pull/28)).
- Added `--version` and launch tests that keep the reported version tied to the package version ([#28](https://github.com/lmarkmann/celsius/pull/28)).

### Changed

- Changed bare piped output from raw ANSI to plain text. Scripts that need the visual frame must now pass `--frame`; this is the breaking behavior change that moved the project from 0.2.x to 0.3.0 ([#28](https://github.com/lmarkmann/celsius/pull/28)).
- Extracted the interactive loop into a testable `App` with explicit key and tick handlers, plus terminal-independent state and frame tests ([#28](https://github.com/lmarkmann/celsius/pull/28)).

### Fixed

- Restored the terminal correctly when the first-run location prompt is cancelled and switched the TUI lifecycle to panic-safe `ratatui` setup and restoration ([#28](https://github.com/lmarkmann/celsius/pull/28)).
- Treated a broken output pipe as a successful exit and made chrome placement display-width aware for wide place names ([#28](https://github.com/lmarkmann/celsius/pull/28)).

### Maintenance

- Changed the Homebrew release workflow to fetch source archives from GitHub's codeload endpoint after the previous archive URL began redirecting ([#27](https://github.com/lmarkmann/celsius/pull/27)).

## [0.2.2] - 2026-06-07

### Fixed

- Corrected the `cargo binstall` package URL, which contained an extra dot before the archive suffix, and declared the binary directory for flat release archives ([commit](https://github.com/lmarkmann/celsius/commit/517d26a1d2dc884b6d1bbf0b2c2cee6e7351c667)).

### Testing

- Extended MT19937 parity coverage across two state refills instead of checking only the first generated values. Boundary values and a full-sequence fold are compared with Python's `getrandbits(32)` behavior ([commit](https://github.com/lmarkmann/celsius/commit/c810c2d640b6bdb43fd137c155aa367cc473a28e)).

### Maintenance

- Updated `dirs` to version 6, moved to `toml` 0.5, and added `cargo-deny` policy for licenses, advisories, bans, and dependency sources ([commit](https://github.com/lmarkmann/celsius/commit/fbde957e3e2314386694291bab21198f99aa837b)).
- Removed the unused `rayon` dependency ([commit](https://github.com/lmarkmann/celsius/commit/8e30db2e45f1f65e67ff7981cf84d371d3f31533)).

## [0.2.1] - 2026-04-25

### Added

- Added deterministic lightning for thunderstorm WMO codes 95 through 99. Storms can produce brief multi-flash illumination and optional branching bolts that are occluded by clouds ([#23](https://github.com/lmarkmann/celsius/pull/23)).
- Kept lightning outside the static renderer and applied it on TUI ticks, so scene PNGs and their locked hashes remain deterministic. Strike schedules are tested for bit parity with the scene lab ([#23](https://github.com/lmarkmann/celsius/pull/23)).

## [0.2.0] - 2026-04-25

### Added

- Added `--bortle 1..9` and a matching config field. The selected Bortle dark-sky class scales star counts along the naked-eye limiting-magnitude curve and adds sun-altitude-gated warm horizon glow for light-polluted night skies. Leaving the setting unset preserves the previous appearance ([#20](https://github.com/lmarkmann/celsius/pull/20)).
- Published prebuilt GitHub Release archives in the naming scheme expected by `cargo binstall`. From this version onward, `cargo binstall celsius` can use a release binary without compiling the crate or requiring a Rust toolchain ([#20](https://github.com/lmarkmann/celsius/pull/20)).

### Changed

- Reorganized development assets into a standard Rust layout: public scenes moved to `scenes/`, goldens to `tests/goldens/`, Open-Meteo fixtures to `tests/`, and development scripts to `scripts/`. The oracle and task runner were updated to use the new paths ([#20](https://github.com/lmarkmann/celsius/pull/20)).

### Fixed

- Removed tracked scripts and goldens from `.gitignore`, allowing release-plz to see package changes and calculate the next release correctly ([#21](https://github.com/lmarkmann/celsius/pull/21)).
- Made release-plz push release branches with a token that can trigger the protected CI checks required before merge ([#19](https://github.com/lmarkmann/celsius/pull/19)).

## [0.1.0] - 2026-04-11

### Added

- Released the first live weather view: a truecolor half-block TUI that turns a seven-day Open-Meteo forecast into a first-person sky with sun, moon, stars, layered clouds, haze, rain, snow, and weather chrome ([initial import](https://github.com/lmarkmann/celsius/commit/23740ded968b7d895fc75412e906b7b7328d3949)).
- Added hour-by-hour timeline scrubbing, day jumps, return-to-now, and a `--facing` bearing so observers can center a different part of the sky. Precipitation slant follows wind relative to the chosen bearing ([#6](https://github.com/lmarkmann/celsius/pull/6)).
- Animated cloud drift from forecast wind speed and added pause and resume control without mutating the canonical forecast timeline ([#4](https://github.com/lmarkmann/celsius/pull/4)).
- Added saved location configuration and a first-run location prompt. CLI names or coordinates take precedence over the saved location ([#7](https://github.com/lmarkmann/celsius/pull/7)).
- Added in-TUI help, location entry, weather retry, and a recoverable error sky instead of exiting immediately on network failures ([#9](https://github.com/lmarkmann/celsius/pull/9)).
- Added distinct dawn and cloudy-day palettes, including a diffuse sun beneath cloud cover and a dedicated dawn transition around the horizon ([#12](https://github.com/lmarkmann/celsius/pull/12)).
- Added flexible `--at` parsing for full timestamps, dates, bare hours, and relative hour offsets, along with documented keybindings and install paths ([#14](https://github.com/lmarkmann/celsius/pull/14), [#16](https://github.com/lmarkmann/celsius/pull/16)).

### Testing

- Added SHA256 golden-image tests for the deterministic scene renderer and vendored the scene fixtures so those tests exercise real renders in CI ([#3](https://github.com/lmarkmann/celsius/pull/3)).
- Added fixture tests for Open-Meteo geocoding and forecast parsing, including nullable forecast fields ([initial import](https://github.com/lmarkmann/celsius/commit/23740ded968b7d895fc75412e906b7b7328d3949)).
- Added Criterion benchmarks for clear and stormy rendering plus noise hot paths, and ran benchmark smoke tests and RustSec audits in CI ([#5](https://github.com/lmarkmann/celsius/pull/5)).

### Maintenance

- Added CI gates for formatting, clippy with warnings denied, the default and PNG feature sets, tests, and oracle rendering ([#1](https://github.com/lmarkmann/celsius/pull/1)).
- Added release-plz versioning and crates.io publication, tag-triggered GitHub releases, and automated Homebrew formula updates ([#16](https://github.com/lmarkmann/celsius/pull/16)).
- Excluded demos, goldens, tools, benchmarks, tests, and workflows from the published crate, reducing it from about 5 MiB to 149 KiB, or 43 KiB compressed ([#17](https://github.com/lmarkmann/celsius/pull/17)).
