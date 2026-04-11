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

<!-- TODO: replace src with actual demo.gif once a recording exists -->
<img src="demo.gif" alt="celsius demo" width="100%">

*The sky above you, in your terminal.*

Terminal weather as a first-person sky view. You look up into the sky you would see right now at your location, rendered as a truecolor half-block scene directly in your terminal. Clouds drift, the sun tracks its altitude, stars fade in at twilight, rain slants with the wind.

## Usage

`brew install lmarkmann/tap/celsius` or `cargo install celsius`

```sh
celsius              # current sky at your location
celsius --time 18:00 # scrub to a specific hour
```

<!-- TODO: add keybindings table with <kbd> tags once TUI is wired up -->
