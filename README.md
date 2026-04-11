<div align="center">

<!-- TODO: replace with a centered logo or wide terminal screenshot once one exists -->

# celsius

[![CI](https://github.com/lmarkmann/celsius/workflows/CI/badge.svg)](https://github.com/lmarkmann/celsius/actions)
[![Crates.io](https://img.shields.io/crates/v/celsius)](https://crates.io/crates/celsius)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

</div>

<!-- TODO: still iterating on taglines -- two candidates below, pick one once the GIF is in place -->
<!-- README subtitle (above the GIF): "The sky above you, in your terminal." -->
<!-- GitHub repo description: "a sky in your terminal" -->

<img src="demos/demo.gif" alt="celsius demo" width="100%">

*The sky above you, in your terminal.*

Terminal weather as a first-person sky view. You look up into the sky you would see right now at your location, rendered as a truecolor half-block scene directly in your terminal. Clouds drift, the sun tracks its altitude, stars fade in at twilight, rain slants with the wind.

## Install

```sh
cargo install celsius
```

Homebrew tap coming soon.

## Usage

```sh
celsius                          # current sky at your saved location
celsius -l Hamburg               # look up a place by name
celsius --lat 53.55 --lon 9.99   # coordinates
celsius --at 2026-06-21T00:00Z   # scrub to a specific time
celsius --facing 0               # face north (default 180 = south)
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
