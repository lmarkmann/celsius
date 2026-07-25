# celsius-lab

`celsius-lab` is the repository's production-backed scene authoring tool. It is an unpublished Rust workspace crate that loads root `scenes/` through the public Celsius parser and renders them through the same pipeline used by the application and golden oracle.

## Commands

```bash
just lab --help
just lab-render dawn
just lab-contact
just lab-diff dawn --against path/to/reference.jpg
just lab-new harbor_dawn 53.5511 9.9937 2026-04-11T06:14Z --out path/to/drafts/harbor_dawn.toml
just lab-new harbor_dawn 53.5511 9.9937 2026-04-11T06:14Z --visibility 18
just lab place --lat 53.5511 --lon 9.9937 --at 2026-04-11T06:14Z
```

- `render` writes an enlarged nearest-neighbor preview to `out/lab/<scene>.png` while preserving the native 104x50 pixels.
- `contact` renders every root scene into a labeled contact sheet at `out/lab/contact.png`.
- `diff` resizes a reference to 104x50, reports exact RGB disagreement, mean and maximum RGB distance, plus Oklab mean, p95, and maximum distance, and writes a red-to-yellow heatmap.
- `place` prints sun and moon altitude, azimuth, projected screen position, phase, and illumination from the production Meeus implementation.
- `new` writes a root scene from production astronomy and sky gradients, then renders its first preview automatically; pass `--out <path>` to keep an unfinished draft outside the public scene library.

## Repository ownership

- Root `scenes/` is the public scene source directory; `render` and `diff` also accept an existing scene path for temporary draft inspection.
- Root `tests/goldens/` and `just lock` are the only golden-image path.
- `tools/celsius-lab/references/` stores optional comparison photographs and source notes.
- Ignored `out/lab/` stores generated previews, heatmaps, and contact sheets.

The tool intentionally does not reproduce TUI chrome in a synthetic image. Use `celsius --scene scenes/<name>.toml` to inspect the real Ratatui layout after the sky preview reads correctly.
