# verification scenes

Representative (location, time) pairs for manually smoke-testing the weather
synthesis layer once Phase 2/3 are in. These are deliberately **not** a CLI
flag (no `--tour`) because baking place names into the binary is arbitrary
taste and adds no real value over a copy-paste list here. When you want to
eyeball synthesis across conditions, open this file and run the commands by
hand, one at a time, inside the TUI.

All timestamps are ISO 8601 UTC. Dates use 2026 because the summer/winter
solstice UTC seconds in that year are close to what celsius will see in any
given year and the sun math is good to arcminute accuracy over 1950-2050.

## Golden-hour warm mid-cloud — Hamburg, summer evening

    celsius -l "Hamburg" --at 2026-06-18T19:00:00Z

Expected: sun near +5 to +10 deg altitude, warm gradient stops pulled up
through pink/orange, scattered cumulus catching the warm band.

## Midnight sun — Reykjavík, June solstice

    celsius -l "Reykjavík" --at 2026-06-21T00:00:00Z

Expected: sun positive altitude at "midnight" local time, golden-hour-like
color (because altitude is low even though it's the middle of the night),
chrome label showing `golden hour` despite the clock saying 00:00. This is
the label-vs-clock test case — polar locations should render accurate-but-
weird labels without special-casing.

## Polar night — Svalbard, December solstice

    celsius --lat 78.2232 --lon 15.6267 --at 2026-12-21T12:00:00Z

Expected: sun altitude deeply negative, chrome label `night`, full star
field, moon if positive, haze low. Daytime clock, nighttime sky — the other
side of the polar edge case.

## Noon sun from the southern hemisphere — Sydney

    celsius -l "Sydney" --at 2026-06-21T02:00:00Z

Expected: sun is visibly biased north, not south. With view direction
hardcoded to south-facing in v0 (see memory
`project_view_direction_bias.md`), this scene will look "backwards" — sun
moving the wrong way across the sky. That is expected v0 behavior and is
the reason a `--facing` flag is tracked for later.

## Clear dark night, high elevation — Atacama

    celsius --lat -24.6282 --lon -70.4036 --at 2026-04-15T04:00:00Z

Expected: deep night gradient, dense star field (clear air, low haze,
altitude elevation doesn't currently affect the keyframe-sampled gradient
but should be noted if stars look muted), moon if positive.

## High noon clear — Washington, DC, June solstice

    celsius -l "Washington, DC" --at 2026-06-21T17:00:00Z

Expected: sun at ~74 deg altitude (matches the `sun_washington_solstice_noon`
test in `astro.rs`), full daylight gradient, no stars, no moon if below
horizon. A known-good anchor for synthesis daylight.

## Scene-file validation (no network)

When you need to isolate terminal rendering from synthesis bugs, boot
celsius directly into a lab scene via `--scene`:

    celsius --scene ../skyterm-lab/scenes/golden_hour_cumulus.toml
    celsius --scene ../skyterm-lab/scenes/high_noon_clear.toml
    celsius --scene ../skyterm-lab/scenes/blue_hour_calm.toml
    celsius --scene ../skyterm-lab/scenes/moonlit_clear_winter.toml
    celsius --scene ../skyterm-lab/scenes/stormy_afternoon_advancing.toml

These scenes are byte-identical against their locked goldens, so the only
thing that can look wrong in the TUI when using them is the PixelBuffer ->
terminal-cell path (half-block truecolor quantization). If one of these
looks wrong in celsius but the corresponding PNG looks right in an image
viewer, the bug is in `src/tui/widget.rs`.
