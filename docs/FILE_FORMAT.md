# The `.iwdoc` file format

Implemented in `crates/iw-engine/src/format/native.rs`. Current version: **1**.

An `.iwdoc` file is a ZIP archive. Rename one to `.zip` and it opens in any archive
tool.

| Entry | Contents |
| --- | --- |
| `mimetype` | The text `application/x-imageworks-document`. Always the first entry and stored uncompressed, so the file type can be recognised from its first bytes. |
| `document.json` | Everything except pixel data. |
| `profile.icc` | The ICC colour profile, if the document has one. |
| `layers/<id>/<tx>_<ty>` | One tile of a raster layer. |
| `channels/<id>/<tx>_<ty>` | One tile of an alpha channel. |

All entries except `mimetype` are Deflate-compressed. Entry timestamps are fixed, so
saving the same document twice gives identical bytes.

## Tiles

A tile is 256 x 256 texels, row by row from the top, with no header. Tile `(tx, ty)`
covers pixels `tx*256 ..` by `ty*256 ..` in the layer's own coordinates; both can be
negative. Only tiles that exist are stored; a missing tile is transparent.

| Bit depth | Layer texel | Channel texel |
| --- | --- | --- |
| `u8` | 4 bytes: R, G, B, A | 1 byte |
| `u16` | 8 bytes: four little-endian `u16` | 2 bytes |
| `f32` | 16 bytes: four little-endian `f32` | 4 bytes |

Layer colour is **premultiplied** by alpha. A tile entry must be exactly the right
length or the file is rejected.

## `document.json`

    {
      "format": "imageworks-document",
      "version": 1,
      "generator": "ImageWorks 0.0.1",
      "width": 320, "height": 240,
      "ppi": 144.0,
      "color_mode": "rgb",
      "bit_depth": "u8",
      "tile_size": 256,
      "profile": { "name": "...", "file": "profile.icc" },
      "metadata": { "title": "", "author": "", "description": "", "copyright": "",
                    "created": 1759700000, "modified": null, "custom": { "key": "value" } },
      "next_id": 8,
      "layers": [ ... ],
      "channels": [ { "id": 6, "name": "...", "tiles": [[0, 0]] } ],
      "paths": [ { "id": 7, "name": "...", "subpaths": [
        { "closed": true, "anchors": [
          { "point": [10.0, 10.0], "handle_in": [10.0, 10.0], "handle_out": [10.0, 10.0] } ] } ] } ]
    }

- `layers` lists top-level layers **bottom first**.
- `profile` is `null` when there is no profile (the document is then sRGB).
- `created` and `modified` are seconds since the Unix epoch, or `null`.
- `next_id` is the next id the document will hand out. Ids are shared by layers,
  channels and paths, are never 0, and are never reused.

A layer is one of:

    { "id": 1, "name": "Background", "visible": true, "opacity": 1.0,
      "blend": "normal", "locked": false,
      "kind": "raster", "offset": [0, 0], "tiles": [[0, 0], [1, 0]] }

    { "id": 2, "name": "Effects", "visible": true, "opacity": 1.0,
      "blend": "normal", "locked": false,
      "kind": "group", "pass_through": true, "children": [ ... ] }

- `blend` is a blend mode id: the mode's name in lower case with underscores, such as
  `color_burn` (see `docs/BLEND_MODES.md` for the 27 modes).
- `offset` is where the layer's pixel origin sits on the canvas.
- A group with `pass_through: true` is only a folder and its `blend` is unused.

## Versioning rules

- A reader opens its own version and every older one.
- A reader **ignores fields it does not know**. Adding an optional field that old
  readers can safely skip does not need a new version.
- A reader **refuses a higher `version`** and says the file is from a newer release.
  Any change an old reader would misread (a new layer kind, a different tile layout, a
  field whose absence changes the picture) must raise the version.
- `crates/iw-engine/tests/fixtures/v1.iwdoc` is a version 1 file kept in the
  repository. A test opens it on every run. It is never regenerated: if that test
  fails, the reader has broken compatibility with files people already have.

## What a reader must reject

Wrong `mimetype`; unknown `format`, `color_mode`, `bit_depth`, `tile_size` or blend id;
a missing, short or over-long tile; a tile listed twice; an id of 0 or one used
twice; an opacity outside 0..1; a canvas size of 0 or above 300,000. The reader
re-checks all of this through the same code that validates edits.

## Known inefficiency

Tiles are compressed as raw samples with no prediction filter, so a layered file of
smooth images is much larger than the same pixels as PNG. This is a size cost only
and can be improved in a later format version.
