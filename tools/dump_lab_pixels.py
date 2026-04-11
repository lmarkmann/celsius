"""Dump raw 104x50 RGB bytes from the lab for a given scene.

Run from the celsius repo root:
    PYTHONPATH=../skyterm-lab/src ../skyterm-lab/.venv/bin/python \
        tools/dump_lab_pixels.py \
        ../skyterm-lab/scenes/golden_hour_cumulus.toml \
        out/lab_golden_hour_cumulus.raw

The output is width*height*3 bytes in row-major order, no header, no chrome.
Celsius can produce the same layout and we diff byte-for-byte.
"""

import sys
from pathlib import Path

from skyterm_lab.render import render
from skyterm_lab.scene import load_scene

WIDTH = 104
HEIGHT = 50


def main() -> None:
    scene_path = Path(sys.argv[1])
    out_path = Path(sys.argv[2])
    state = load_scene(scene_path)
    pixels = render(state, WIDTH, HEIGHT)
    buf = bytearray(WIDTH * HEIGHT * 3)
    i = 0
    for row in pixels:
        for rgb in row:
            buf[i] = rgb.r
            buf[i + 1] = rgb.g
            buf[i + 2] = rgb.b
            i += 3
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(bytes(buf))
    print(f"{scene_path.name} -> {out_path} ({WIDTH}x{HEIGHT})")


if __name__ == "__main__":
    main()
