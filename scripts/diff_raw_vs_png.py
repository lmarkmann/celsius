"""Diff a raw WxHx3 byte dump against a PNG (decoded back to raw).

Usage:
    PYTHONPATH=../skyterm-lab/src ../skyterm-lab/.venv/bin/python \
        scripts/diff_raw_vs_png.py \
        out/lab_golden_hour_cumulus.raw tests/goldens/golden_hour_cumulus.png 104 50
"""

import sys
from pathlib import Path

from PIL import Image


def main() -> None:
    raw_path = Path(sys.argv[1])
    png_path = Path(sys.argv[2])
    width = int(sys.argv[3])
    height = int(sys.argv[4])

    lab = raw_path.read_bytes()
    assert len(lab) == width * height * 3, (
        f"lab raw size {len(lab)} != {width * height * 3}"
    )

    img = Image.open(png_path).convert("RGB")
    if img.size != (width, height):
        print(f"celsius png size {img.size} != ({width}, {height})")
        return
    cel = img.tobytes()

    total = width * height
    differing = 0
    max_dr = max_dg = max_db = 0
    sum_dr = sum_dg = sum_db = 0
    first_diffs = []
    for y in range(height):
        for x in range(width):
            i = (y * width + x) * 3
            lr, lg, lb = lab[i], lab[i + 1], lab[i + 2]
            cr, cg, cb = cel[i], cel[i + 1], cel[i + 2]
            dr, dg, db = abs(lr - cr), abs(lg - cg), abs(lb - cb)
            if dr or dg or db:
                differing += 1
                if len(first_diffs) < 10:
                    first_diffs.append((x, y, (lr, lg, lb), (cr, cg, cb)))
            max_dr = max(max_dr, dr)
            max_dg = max(max_dg, dg)
            max_db = max(max_db, db)
            sum_dr += dr
            sum_dg += dg
            sum_db += db

    pct = 100.0 * differing / total
    print(f"pixels differing: {differing}/{total} ({pct:.2f}%)")
    print(f"max channel diff: r={max_dr} g={max_dg} b={max_db}")
    print(
        f"mean channel diff: "
        f"r={sum_dr / total:.2f} g={sum_dg / total:.2f} b={sum_db / total:.2f}"
    )
    if first_diffs:
        print("first differing pixels (x,y, lab, celsius):")
        for x, y, lab_px, cel_px in first_diffs:
            print(f"  ({x:3d},{y:3d}) lab={lab_px} celsius={cel_px}")


if __name__ == "__main__":
    main()
