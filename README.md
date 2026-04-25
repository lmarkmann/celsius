# celsius

[![CI](https://github.com/lmarkmann/celsius/workflows/CI/badge.svg)](https://github.com/lmarkmann/celsius/actions)
[![Crates.io](https://img.shields.io/crates/v/celsius)](https://crates.io/crates/celsius)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

<img src="demos/demo.gif" alt="celsius demo" width="100%">

Terminal weather as a first-person sky view. You look up into the sky you would see right now at your location, rendered as a truecolor half-block scene directly in your terminal. Clouds drift, the sun tracks its altitude, stars fade in at twilight, rain slants with the wind.

## Install

```sh
cargo install celsius
# or, prebuilt binary (no Rust toolchain), v0.2.0+
cargo binstall celsius
# or
brew install lmarkmann/tap/celsius
```

## Usage

```sh
celsius                          # current sky at your saved location
celsius -l Hamburg               # look up a place by name
celsius --lat 53.55 --lon 9.99   # coordinates
celsius --at 17                  # today at 17:00 UTC
celsius --at +3h                 # three hours from now
celsius --at 2026-06-21          # date alone, noon UTC
celsius --at 2026-06-21T17:00Z   # full ISO 8601
celsius --facing 0               # face north (default 180 = south)
celsius --bortle 7               # adjust visible stars + horizon glow for your sky
```

## Keys

| Key | Action |
|---|---|
| `← →` | scrub one hour |
| `tab` / `shift+tab` | +24h / -24h |
| `t` | jump to now |
| `space` | pause / resume cloud drift |
| `l` | change location |
| `r` | retry weather fetch |
| `?` | keybinding help |
| `q` / `esc` | quit |
