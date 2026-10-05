# Blend modes

The definitions implemented in `crates/iw-engine/src/blend.rs`. `b` is the backdrop
and `s` the source, as straight (non-premultiplied) colour in 0..1.

## Working space

Blending is done on the document's stored values:

- **8-bit and 16-bit documents are gamma-encoded**, so every mode blends encoded
  values. This is what Photoshop does by default and what gives Multiply, Screen,
  Overlay and the rest their familiar look. No mode is switched to linear light.
- **32-bit float documents are linear** by convention, so there every mode blends
  linear values. Until 32-bit is exposed (M21), modes other than Normal clamp their
  inputs to 0..1; Normal passes values above 1 through.

## Compositing

With premultiplied colours `cs`, `cb`, alphas `as`, `ab` (layer opacity already
multiplied into the source) and blend function `B`:

    co = (1 - ab) * cs + (1 - as) * cb + as * ab * B(Cb, Cs)
    ao = as + ab - as * ab

Where there is no backdrop (`ab = 0`) every mode reduces to plain source-over.

## Separable modes (per channel)

| Mode | B(b, s) |
| --- | --- |
| Normal | `s` |
| Darken | `min(b, s)` |
| Multiply | `b * s` |
| Color Burn | `1` if `b = 1`; else `0` if `s = 0`; else `1 - min(1, (1 - b) / s)` |
| Linear Burn | `max(0, b + s - 1)` |
| Lighten | `max(b, s)` |
| Screen | `b + s - b * s` |
| Color Dodge | `0` if `b = 0`; else `1` if `s = 1`; else `min(1, b / (1 - s))` |
| Linear Dodge (Add) | `min(1, b + s)` |
| Overlay | Hard Light with `b` and `s` swapped |
| Soft Light | `s <= 0.5`: `b - (1 - 2s) * b * (1 - b)`; else `b + (2s - 1) * (D(b) - b)`, with `D(b) = ((16b - 12) * b + 4) * b` for `b <= 0.25`, else `sqrt(b)` |
| Hard Light | `s <= 0.5`: `2 * b * s`; else `Screen(b, 2s - 1)` |
| Vivid Light | `s <= 0.5`: `ColorBurn(b, 2s)`; else `ColorDodge(b, 2s - 1)` |
| Linear Light | `clamp(b + 2s - 1, 0, 1)` |
| Pin Light | `s <= 0.5`: `min(b, 2s)`; else `max(b, 2s - 1)` |
| Hard Mix | `1` if `b + s >= 1`, else `0` |
| Difference | `abs(b - s)` |
| Exclusion | `b + s - 2 * b * s` |
| Subtract | `max(0, b - s)` |
| Divide | `0` if `b = 0`; else `1` if `s = 0`; else `min(1, b / s)` |

## Whole-colour modes

`Lum(c) = 0.3 R + 0.59 G + 0.11 B`, `Sat(c) = max - min`, and `SetLum`, `SetSat`,
`ClipColor` as defined in the PDF 1.7 and W3C Compositing specifications.

| Mode | Result |
| --- | --- |
| Darker Color | whichever of `b`, `s` has the lower `Lum` |
| Lighter Color | whichever of `b`, `s` has the higher `Lum` |
| Hue | `SetLum(SetSat(s, Sat(b)), Lum(b))` |
| Saturation | `SetLum(SetSat(b, Sat(s)), Lum(b))` |
| Color | `SetLum(s, Lum(b))` |
| Luminosity | `SetLum(b, Lum(s))` |

## Dissolve

Not a colour function. Each document pixel has a fixed pseudo-random threshold in
0..1 derived from its position. The source pixel is drawn fully opaque where its
alpha (times layer opacity) exceeds the threshold, and not drawn otherwise. The
pattern depends only on position, so it does not shift when layers are edited.

## Differences from Photoshop

- **Soft Light** uses the PDF/W3C formula. Photoshop is reported to use `sqrt(b)`
  across the whole range, which differs slightly where `b <= 0.25`. Not checked
  against Photoshop itself.
- **Hard Mix** thresholds `b + s`. Photoshop gives the same result at 100% fill; its
  fill-opacity behaviour for Hard Mix is not reproduced (there is no fill opacity
  yet).
- **Dissolve** uses its own noise pattern; the speckle will not match Photoshop's
  pixel for pixel.
- **Divide** by black is defined as white, except black over black, which is black.

## How these are verified

- Unit tests compare each function with values worked out by hand from the tables
  above, and check neutral colours, commutativity and output range.
- `tests/blend_golden.rs` compares a full composite per mode with a stored image.
- `scripts/crosscheck-blend-modes.py` compares 20 modes with ImageMagick on opaque
  images. Result on 2026-10-05: all 20 agree to within rounding, except that at
  `b = 0, s = 1` Color Dodge and Vivid Light give 0 here (per the specification) and
  1 in ImageMagick. ImageMagick truncates where the engine rounds to nearest.
  The six whole-colour modes and Dissolve are not comparable: ImageMagick defines
  them differently.
