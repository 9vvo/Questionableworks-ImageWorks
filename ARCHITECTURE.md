# Architecture

The rules live in `CLAUDE.md` section 3. This file describes how the code
implements them today. It grows with each milestone.

## Crates

| Crate | Role | May depend on UI / windowing / GPU surface |
| --- | --- | --- |
| `crates/iw-engine` | Headless engine: pixel formats, tile store, blend modes, compositor. | No |
| `crates/iw-app` | Desktop shell, binary `imageworks`. Currently the M0 toolkit spike. | Yes |

The engine starts as one crate. Split it (tiles, compositor, document, formats, ...)
when a subsystem has a reason to compile or be depended on separately, not before.
Every engine crate is listed in `scripts/check-layering.sh`.

## Engine modules

| Module | Holds |
| --- | --- |
| `pixel` | `Channel` trait for 8-bit, 16-bit and float samples; premultiplied RGBA `Pixel<C>` |
| `geom` | `Rect` in document pixel coordinates (may be negative) |
| `tile` | 256 x 256 copy-on-write `Tile<C>` and `TileCoord` |
| `raster` | `Raster<C>`: unbounded sparse grid of tiles; a missing tile is transparent |
| `blend` | `BlendMode` and the blend function for each of the 27 modes |
| `compositor` | CPU reference compositor, one tile at a time |

Design points that later milestones rely on:

- **Tiles are reference-counted and copy-on-write.** Cloning a raster copies no
  pixels. An undo record can hold the previous version of only the tiles an edit
  touched (rule 4).
- **Compositing is per tile** (`composite_tile`), so the canvas can recomposite only
  dirty tiles (rule 5). A tile covered by a single opaque Normal layer is returned
  shared, not copied.
- **The stack is accumulated in `f32`** and converted to the channel type once, so
  rounding error does not grow with the number of layers.
- **`blend.rs` is the only definition of the blend modes** (rule 3). The GPU
  compositor in M3 must be tested against this CPU implementation. Formulas and
  known differences from Photoshop are in `docs/BLEND_MODES.md`.

## Enforced rules

- **Engine is headless.** `scripts/check-layering.sh` walks each engine crate's full
  dependency tree and fails on `winit`, `wgpu`, `egui`, `eframe`, `muda`, `rfd` and
  related crates. CI runs it on every push.
- **Licences.** `deny.toml` plus `cargo deny check licenses` in CI.

Rules 2 to 7 of the charter (commands only, one compositor, delta undo, dirty tiles,
workers, atomic saves) have no code yet; each gets its enforcement when its subsystem
lands, and is recorded here.

## Shell (M0 spike)

`iw-app` uses eframe (winit + wgpu) with egui, `egui_dock` for dockable panels, `muda`
for the native menu bar on macOS and Windows, and `rfd` for native file dialogs.

- `main.rs`: entry point; `--version` exits without opening a window (CI launch check).
- `native_menu.rs`: builds the menu and turns menu clicks into `Command` values. Linux
  has no native menu, so development builds show the same commands in-window.
- `spike.rs`: three diagnostic panels (pen probe, event log, display).

Whether this toolkit is kept is decided by the spike results in `STATUS.md`.
