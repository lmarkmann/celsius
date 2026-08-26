# celsius

[![Built With Ratatui](https://ratatui.rs/built-with-ratatui/badge.svg)](https://ratatui.rs/)
[![Crates.io](https://img.shields.io/crates/v/celsius?logo=rust)](https://crates.io/crates/celsius)
[![CI](https://github.com/lmarkmann/celsius/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/lmarkmann/celsius/actions/workflows/ci.yml?query=branch%3Amain)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

<img src="demos/demo.gif" alt="celsius demo" width="100%">

Terminal weather as a first-person sky view. You look up into the sky you would see right now at your location, drawn in truecolor half-blocks directly in your terminal. Clouds drift, the sun tracks its altitude, stars fade in at twilight, rain slants with the wind, and thunderstorms flash.

## Install

```sh
# prebuilt binary, no Rust toolchain needed
curl -fsSL https://raw.githubusercontent.com/lmarkmann/celsius/main/install.sh | sh
# or
brew install lmarkmann/tap/celsius
# or, prebuilt binary, v0.2.0+
cargo binstall -y celsius
# or, from source
cargo install celsius
```

The script needs `curl` or `wget`, verifies the SHA-256 published with the release, and installs into `$XDG_BIN_HOME`, or `~/.local/bin` if that is unset. Options go after `-s --`, so `| sh -s -- --install-dir ~/bin` or `| sh -s -- --version v0.5.0`. It covers arm64 macOS and x86_64 Linux. Intel Mac and ARM Linux have no prebuilt binary, so Homebrew and `cargo install` build from source there. Windows has a prebuilt binary but no installer: take the `x86_64-pc-windows-msvc` zip from the [latest release](https://github.com/lmarkmann/celsius/releases/latest).

## Usage

```sh
celsius                          # current sky will open in the TUI
celsius -l Hamburg               # look up a place (can be done inside TUI as well)
celsius --lat 53.55 --lon 9.99   # coordinates
celsius --at 17                  # today at 17:00 UTC
celsius --at +3h                 # three hours from now
celsius --at 2026-06-21          # date alone, noon UTC
celsius --at 2026-06-21T17:00Z   # full ISO 8601
celsius --facing 0               # face north (default 180 = south)
celsius --bortle 7               # adjust visible stars + horizon glow for your sky
celsius --scene high_noon_clear  # a built-in sky, no network needed
celsius --scene ./my_sky.toml    # your own scene file
```

Seven scenes are compiled into the binary, so `--scene` works offline and on a fresh install: `blue_hour_calm`, `golden_hour_cumulus`, `high_noon_clear`, `moonless_darksky`, `moonlit_clear_winter`, `overcast_night`, `stormy_afternoon_advancing`. Pass a path instead of a name to render a scene TOML of your own.

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

## Scene development

The unpublished Rust tool under `tools/celsius-lab/` scaffolds scenes from production astronomy, renders previews, compares reference photographs in Oklab, and builds a labeled contact sheet without maintaining a second renderer.

```sh
just lab --help
just lab-render dawn
just lab-contact
just lab-diff dawn --against path/to/reference.jpg
just lab-new harbor_dawn 53.5511 9.9937 2026-04-11T06:14Z --out path/to/drafts/harbor_dawn.toml
just lab-new harbor_dawn 53.5511 9.9937 2026-04-11T06:14Z --visibility 18
```

See [the celsius-lab guide](tools/celsius-lab/README.md) for the scene and output layout.
