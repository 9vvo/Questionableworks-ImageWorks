# Architecture

The rules live in `CLAUDE.md` section 3. This file describes how the code
implements them today. It grows with each milestone.

## Crates

| Crate | Role | May depend on UI / windowing / GPU surface |
| --- | --- | --- |
| `crates/iw-engine` | Headless engine: pixels, tiles, compositor, document, commands and history, file formats. | No |
| `crates/iw-app` | Desktop application, binary `imageworks`. | Yes |

The engine starts as one crate. Split it (tiles, compositor, document, formats, ...)
when a subsystem has a reason to compile or be depended on separately, not before.
Every engine crate is listed in `scripts/check-layering.sh`.

## Engine modules

| Module | Holds |
| --- | --- |
| `pixel` | `Channel` trait for 8-bit, 16-bit and float samples; premultiplied RGBA `Pixel<C>` |
| `geom` | `Rect` in document pixel coordinates (may be negative) |
| `tile` | 256 x 256 copy-on-write `Tile<C>` and `TileCoord` |
| `raster` | `Grid<P>`: unbounded sparse grid of tiles. `Raster<C>` is the RGBA form, `Mask<C>` the single-channel form; a missing tile is empty |
| `blend` | `BlendMode` and the blend function for each of the 27 modes |
| `compositor` | CPU reference compositor, one tile at a time: layers at any offset, pass-through and isolated groups |
| `document` | `Document`: canvas, resolution, colour mode, bit depth, profile, layer tree, alpha channels, paths, metadata |
| `command` | `Command`: every possible change to a document, and how each is applied and reversed |
| `history` | `Session`: owns a document, executes commands, undo/redo, history states, modified flag |
| `format` | Native `.iwdoc` (`docs/FILE_FORMAT.md`), PNG and JPEG readers and writers |
| `io` | Atomic file replacement |

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

## How a change reaches a document

    UI or tool  --Command-->  Session::execute  -->  command::apply  -->  Document
                                    |                      |
                              keeps the inverse      returns Effects
                              on the undo stack      (what to redraw, new ids)

- **`Document` is read-only outside the engine crate.** Its mutating methods are
  `pub(crate)`; the app crate cannot call them, so it has to go through
  `Session::execute` (rule 2). This is enforced by the compiler, not by review.
- **A command is plain data** and applying it returns its inverse, also a command.
  Undo applies the inverse and keeps *its* inverse for redo.
- **Inverses are deltas** (rule 4): an old property value, the tiles an edit replaced
  (`ReplaceTiles`), or the removed layer itself. Because tiles are copy-on-write, the
  untouched tiles of an edited layer are shared with the history, not copied.
- **Commands validate before they change anything**, so a rejected command leaves
  the document untouched. `Batch` applies several commands as one undo step and
  rolls back if any of them fails.
- **`Effects::damage`** says which tiles need recompositing after a command, undo or
  redo. The canvas in M3 uses it to redraw only those (rule 5).
- **Layer ids are never reused**, including after undo, so anything holding an id
  (selection state, an MCP client) can never end up pointing at a different layer.
- **Pixel edits will come from tools as `ReplaceTiles`**, usually inside a `Batch`
  named after the tool. Moving a layer changes its `offset` and copies no pixels.

## Files

- **Saving is atomic** (rule 7): `io::atomic_write` writes a temporary file beside
  the target, flushes it to disk, then renames it over the target. A failed save
  leaves the existing file as it was. All of `format::save` goes through it.
- **Opening goes by content, not file name**: `format::read` looks at the first
  bytes to pick a reader.
- **Loading re-validates.** The native reader builds the document through the same
  insert functions commands use, so a hand-edited or damaged file cannot produce a
  document the engine would not allow.
- **The file format is defined by its own types** in `format/native.rs`, separate
  from the in-memory structs, so refactoring the engine cannot change saved files by
  accident. A version 1 fixture in the repository is opened by a test on every run.

## Enforced rules

- **Engine is headless.** `scripts/check-layering.sh` walks each engine crate's full
  dependency tree and fails on `winit`, `wgpu`, `egui`, `eframe`, `muda`, `rfd` and
  related crates. CI runs it on every push.
- **Licences.** `deny.toml` plus `cargo deny check licenses` in CI.

- **Commands only (rule 2).** Compiler-enforced through `pub(crate)`; see above.
- **Delta undo (rule 4).** `history::tests::a_pixel_edit_keeps_only_the_tiles_it_replaced`.
- **Atomic saves (rule 7).** `io::tests` and `format::tests::a_failed_save_leaves_the_existing_file_alone`.

Rule 5 (dirty tiles) has its data (`Effects::damage`) but no canvas yet, and rule 6
(workers) has nothing to apply to until M3.

## Application shell

`iw-app` uses eframe (winit + wgpu) with egui, `egui_dock` for docking, `muda` for the
native menu bar on macOS and Windows, and `rfd` for native file dialogs.

| Module | Holds |
| --- | --- |
| `main.rs` | Entry point. Command-line arguments are handled first and never open a window |
| `cli.rs` | `imageworks convert`, `--version`, `--help` |
| `app.rs` | `App`: open documents, the dock layout, tools, dialogs, file tasks, the close and quit flow |
| `actions.rs` | `Action`: every menu command and shortcut, with its label and key |
| `native_menu.rs` | The native menu bar, and the in-window menu bar used on Linux and in tests, both built from `actions::menus()` |
| `canvas.rs` | Drawing a document's tiles with the view transform; the Hand, Zoom and Rotate View tools |
| `view.rs` | `View`: zoom, rotation, mirror and centre, and the screen/document maths |
| `renderer.rs` | Worker threads that composite tiles off the UI thread |
| `layers_model.rs` | The Layers panel's rows and the commands its gestures produce |
| `panels/` | The Layers and History panels |
| `icons.rs`, `theme.rs` | Painter-drawn icons and the visual theme |
| `ui_tests.rs` | End-to-end tests that drive the app through its UI |

### How the canvas draws

    edit -> Session -> Effects::damage -> Renderer (worker threads)
                                              | composite_tile on a snapshot
                                              v
                    Canvas texture cache <- 8-bit display tile
                              |
                    each frame: one textured quad per visible tile,
                    placed by the View (zoom, rotate, mirror, pan)

- **Compositing happens on worker threads** (rule 6). The app sends the damaged tile
  coordinates and a snapshot of the document (cheap: tiles are shared). A tile that is
  requested again before its job runs is skipped, so stale work is cancelled.
- **Only damaged tiles are recomposited** (rule 5). Zoom, pan, rotate and mirror only
  move the existing textures. `ui_tests::view_changes_composite_nothing_and_edits_only_their_tiles`
  checks this.
- **The GPU draws, the CPU composites.** Each composited tile is a texture; the GPU
  places and filters them (crisp pixels when zoomed in, mipmapped when zoomed out).
  The blend modes still run in the CPU compositor. A GPU compositor (rule 3) is not
  built yet; see `DECISIONS.md`.
- **16-bit and float documents display as 8-bit**, converted when the tile is ready.

### How commands reach a document

The panels never touch the document: they return intents (`panels::layers::Intent`)
holding commands, and `App` executes them through the document's `Session`. Continuous
gestures (dragging the opacity slider) use `Session::execute_coalescing`, so a whole
drag is one undo step. Menu items, shortcuts and panel buttons all go through
`App::perform`, which checks the same enabled state the menus show.

### Files and closing

- Open and save run on background threads; the UI shows progress in the status bar.
  A save writes a snapshot and records the history state it saved
  (`Session::mark_saved_at`), so editing during a save is safe.
- Closing a document, closing the window and Quit all go through one queue that asks
  about unsaved documents one at a time and can be cancelled.
- Save writes only `.iwdoc`. A document opened from PNG or JPEG has no save path, so
  Save asks for one; Export As writes PNG or JPEG without changing what Save does.

### Shortcuts and the menu

On macOS the native menu's key equivalents fire the menu items, so the app does not
also handle Cmd shortcuts there. On Windows the menu shows the shortcuts but the app
handles the keys, because winit does not run the menu translation step. Shortcuts
without a modifier (tool keys, F12) are never put on menu items and are ignored while
typing in a text field.
