#!/usr/bin/env python3
"""Compare the engine's blend modes with ImageMagick's, on opaque images.

An independent check that the formulas are right, not just stable. It is a
manual tool, not part of CI: it needs ImageMagick 6 or 7, Pillow and numpy.

    python3 scripts/crosscheck-blend-modes.py

Modes ImageMagick defines differently, or does not have, are listed with
the reason instead of being compared.
"""
import os
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

# Engine mode name -> ImageMagick -compose operator.
COMPARABLE = {
    "Normal": "Over",
    "Darken": "Darken",
    "Multiply": "Multiply",
    "Color Burn": "ColorBurn",
    "Linear Burn": "LinearBurn",
    "Lighten": "Lighten",
    "Screen": "Screen",
    "Color Dodge": "ColorDodge",
    "Linear Dodge": "LinearDodge",
    "Overlay": "Overlay",
    "Soft Light": "SoftLight",
    "Hard Light": "HardLight",
    "Vivid Light": "VividLight",
    "Linear Light": "LinearLight",
    "Pin Light": "PinLight",
    "Hard Mix": "HardMix",
    "Difference": "Difference",
    "Exclusion": "Exclusion",
    "Subtract": "MinusSrc",
    "Divide": "DivideSrc",
}

NOT_COMPARED = {
    "Dissolve": "ImageMagick's dissolve is a cross-fade, not a coverage pattern",
    "Darker Color": "ImageMagick compares intensity with different channel weights",
    "Lighter Color": "ImageMagick compares intensity with different channel weights",
    "Hue": "ImageMagick works in HCL; the engine uses the PDF/W3C Lum and Sat model",
    "Saturation": "ImageMagick works in HCL; the engine uses the PDF/W3C Lum and Sat model",
    "Color": "ImageMagick works in HCL; the engine uses the PDF/W3C Lum and Sat model",
    "Luminosity": "ImageMagick works in HCL; the engine uses the PDF/W3C Lum and Sat model",
}

# One 8-bit step of rounding on each side.
TOLERANCE = 2


def main() -> int:
    magick = shutil.which("magick") or shutil.which("convert")
    if not magick:
        print("ImageMagick not found", file=sys.stderr)
        return 2
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = tempfile.mkdtemp(prefix="iw-crosscheck-")
    subprocess.run(
        ["cargo", "test", "-q", "-p", "iw-engine", "--test", "blend_golden", "--",
         "--ignored", "dump_opaque_renders_for_cross_check"],
        cwd=root, env={**os.environ, "IW_DUMP_DIR": out}, check=True, stdout=subprocess.DEVNULL,
    )

    def load(path):
        return np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)

    failed = 0
    print(f"{'mode':14} {'max diff':>8} {'mean diff':>9} {'over tol':>8}")
    for mode, operator in COMPARABLE.items():
        slug = mode.lower().replace(" ", "_")
        theirs = os.path.join(out, f"im_{slug}.png")
        subprocess.run(
            [magick, os.path.join(out, "backdrop.png"), os.path.join(out, "source.png"),
             "-compose", operator, "-composite", "-depth", "8", theirs],
            check=True,
        )
        diff = np.abs(load(os.path.join(out, f"ours_{slug}.png")) - load(theirs))
        over = int((diff > TOLERANCE).sum())
        failed += over > 0
        flag = "" if over == 0 else "  <-- differs"
        print(f"{mode:14} {int(diff.max()):8d} {diff.mean():9.3f} {over:8d}{flag}")
    for mode, why in NOT_COMPARED.items():
        print(f"{mode:14} not compared: {why}")
    print(f"renders kept in {out}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
