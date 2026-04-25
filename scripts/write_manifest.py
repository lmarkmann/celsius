"""Write tests/goldens/manifest.toml from the current scenes and locked PNGs.

Call after `cargo run -- render` has produced tests/goldens/<name>.png for every
scene in scenes/. Uses stdlib only so it's fine to run with plain python3.
"""

import hashlib
from pathlib import Path

SCENES = [
    "golden_hour_cumulus",
    "blue_hour_calm",
    "high_noon_clear",
    "moonlit_clear_winter",
    "stormy_afternoon_advancing",
]

HEADER = """\
# celsius goldens: raw 104x50 renders (no chrome).
# scene_sha256 is the SHA-256 of the lab scene TOML the PNG was rendered from;
# png_sha256 is the SHA-256 of the locked PNG. The oracle test recomputes both
# and refuses to pass if either drifts. Regenerate with:
#   just lock
# Lab scene TOMLs live in ../skyterm-lab/scenes/ (sibling of celsius in ~/Documents).
"""


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    lines = [HEADER]
    for name in SCENES:
        scene = root.parent / "skyterm-lab" / "scenes" / f"{name}.toml"
        png = root / "tests" / "goldens" / f"{name}.png"
        lines.append("")
        lines.append(f"[{name}]")
        lines.append(f'scene_sha256 = "{sha256(scene)}"')
        lines.append(f'png_sha256 = "{sha256(png)}"')
    (root / "tests" / "goldens" / "manifest.toml").write_text("\n".join(lines) + "\n")
    print(f"wrote {root / 'tests' / 'goldens' / 'manifest.toml'}")


if __name__ == "__main__":
    main()
