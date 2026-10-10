# Status

Source of truth for what works. A row is `done` only when it meets the definition of
done in `CLAUDE.md` section 5 and names its evidence.

## Next session

**M0 to M3 are complete in code and tests, but M3's GUI has only been run on Linux
under a virtual display.** First, the owner's manual checks for M3 below (they need a
Mac). Fix anything they turn up, then start **M3a** (MCP server).

Things M3a should know:

- Every user action already goes through `App::perform(Action)` or a panel intent
  that carries a `Command`. MCP tools should call the same paths, so they appear in
  the History panel and undo like a user's actions.
- `Command` and `LayerProps` are plain data, and blend modes have stable ids
  (`BlendMode::id`), which is what tool parameters need.
- The renderer can produce composited tiles off the UI thread; a "rendered preview"
  tool can flatten a snapshot (`Document::flatten`) on a worker the same way.

Carried forward:

- Nobody has run the app on Windows. CI builds and tests it there, including the UI
  tests, but no person has seen it.
- Pen pressure and HiDPI are untested with the risk accepted by the owner. Pen
  pressure must be tested on a tablet before M6 is marked done.
- No GPU compositor yet (rule 3; see `DECISIONS.md`). Needed by M6 or M8.
- No performance measurements yet. The charter's benchmark budgets need a benchmark
  harness and a machine with a GPU in CI; neither exists.
- Layer thumbnails, Photoshop's separate lock types, and a zoomed-out image pyramid
  for very large documents are not built.
- `.iwdoc` files are larger than they need to be (see `docs/FILE_FORMAT.md`).
- JPEG import does not keep EXIF data. Metadata handling is M9.

## M0: foundation (complete)

| Item | State | Evidence or what is missing |
| --- | --- | --- |
| Workspace layout | done | `cargo test --workspace` passes on Linux |
| Engine layering guard | done | `scripts/check-layering.sh`; verified to fail when a windowing crate is added to `iw-engine` and when cargo itself fails |
| Licence policy | done | `cargo deny check licenses` passes |
| Docs skeleton | done | README, ARCHITECTURE, BUILD, DEVELOPMENT, SHORTCUTS, STATUS, DECISIONS |
| CI on macOS and Windows | done | `CI` workflow green on `macos-latest` and `windows-latest` (run 37348860484, 2026-10-05) |
| Packaging skeleton | done | `Package` workflow green: builds the `.dmg` and the NSIS installer (run 37348860376). Installers not yet opened by a person; no app icon yet |
| UI toolkit spike | done | Toolkit accepted by the owner; see table below for what passed and what is an accepted risk |

### UI toolkit spike (egui/eframe + egui_dock + muda + rfd)

| Requirement | macOS | Windows | Notes |
| --- | --- | --- | --- |
| Renders through wgpu | pass | untested | macOS: Metal on Apple M5 Pro (owner's Mac mini, 2026-10-05). Works on Linux with a software adapter |
| Dockable panels | pass | untested | Works on Linux. Panels dock, split and tab inside the main window; `egui_dock` does not detach a panel into its own OS window, which matters for multi-monitor layouts |
| Native menu bar | pass | untested | macOS: app, Spike and Window menus show in the system menu bar (2026-10-05). The app menu is titled with the window title instead of "ImageWorks"; cosmetic, fix in M3. Never compiled for Windows |
| Native file dialogs | pass | untested | macOS: open and save dialogs both return the chosen path (2026-10-05) |
| HiDPI | untested, risk accepted | untested | Owner has no Retina or scaled display; accepted 2026-10-05. Toolkit handles scaling itself |
| Pen pressure | untested, risk accepted | untested | Owner has no tablet; accepted 2026-10-05. Expected to fail on macOS: winit 0.30 reports pen force on Windows but no tablet pressure on macOS, so a native `NSEvent` hook is needed. Must be written and tested on a real tablet before M6 (brush engine) is marked done |

### Needs manual check

Run `cargo run --release -p iw-app` on each OS.

1. **Window and GPU.** The window opens with a dark UI. The Display panel's "GPU
   adapter" row names a Metal adapter on macOS and a Dx12 adapter on Windows.
2. **Docking.** Drag the "Event log" tab onto the left edge of the Pen probe panel;
   it docks there. Drag a divider; panels resize. Choose Window > Reset Panel Layout;
   the original layout returns.
3. **Native menu.** macOS: the menu bar at the top of the screen shows ImageWorks,
   Spike and Window. Windows: the window has a Spike and Window menu bar under the
   title bar. The Display panel's "Menu bar" row reads "native menu bar installed".
4. **File dialogs.** Spike > Test Open Dialog shows the system file picker; pick a
   file and its path appears in the Event log. Same for Test Save Dialog (nothing is
   written to disk).
5. **HiDPI.** On a Retina or scaled display, text is sharp and "Scale factor (OS)"
   matches the display setting (2.0 on Retina, 1.5 at 150%). Drag the window to a
   monitor with a different scale; it stays sharp and the value updates.
6. **Mouse drawing.** Drag in the Pen probe panel; a thin line follows the cursor.
   Passed on macOS 2026-10-05.
7. **Pen pressure.** With a tablet, draw a stroke going from light to hard pressure.
   The line thickens along its length and the panel header shows "pressure received".
   A hairline with "no pressure data received yet" is a fail.
8. **Installer.** Download the artifacts from the `Package` workflow. macOS: open the
   `.dmg`, drag the app to Applications, launch it. Windows: run the installer, launch
   from the Start menu. Both are unsigned, so expect a Gatekeeper or SmartScreen
   warning.

## M1: tiles, pixel formats, compositor (complete)

M1 is engine-only. By the charter's definition a feature is `done` only once the UI,
commands, undo and the file format also handle it, so the rows below stay `partial`
until M2 and M3 supply those parts. The engine work itself is finished and tested;
`CI` is green on macOS and Windows (run 37370995492, 2026-10-06).

| Item | State | Evidence or what is missing |
| --- | --- | --- |
| Pixel formats (8-bit, 16-bit, float; premultiplied RGBA) | partial | Engine done: `pixel::tests`. Missing: document bit depth (M2), UI (M3, M16) |
| Tile store (sparse, copy-on-write) | partial | Engine done: `tile::tests`, `raster::tests`. Missing: use by documents and undo (M2) |
| 27 blend modes | partial | Engine done: `blend::tests` (hand-computed values), `tests/blend_golden.rs` (27 golden images), ImageMagick cross-check of 20 modes. Missing: layer property, command, file format (M2), UI (M3) |
| CPU compositor | partial | Engine done: `compositor::tests`. Missing: groups, masks, clipping (M2, M5), GPU implementation tested against it (M3) |

Verified only by unit tests and golden images, with no independent reference:
Dissolve, Darker Color, Lighter Color, Hue, Saturation, Color, Luminosity.
Nothing has been compared with Photoshop itself.

## M2: document, history, native format, PNG and JPEG (complete)

Engine-only, like M1: by the charter's definition the rows stay `partial` until the
shell in M3 exposes them.

| Item | State | Evidence or what is missing |
| --- | --- | --- |
| Document model (canvas, resolution, colour mode, bit depth, profile, layers, groups, channels, paths, metadata) | partial | Engine done: `history::tests`, `format::native::tests`. Missing: UI (M3). Colour mode is RGB only; channels and paths have no tools yet (M4, M11) |
| Group compositing (pass-through and isolated) and layer offsets | partial | Engine done: `compositor::tests`. Missing: UI (M3), move tool (M7) |
| Command and history engine | partial | Engine done: `history::tests`, including every command applied, undone and redone, and 1,600 random commands replayed both ways. Missing: History panel and menu items (M3) |
| Delta undo (rule 4) | done | `history::tests::a_pixel_edit_keeps_only_the_tiles_it_replaced` |
| Native format `.iwdoc`, version 1 | partial | Engine done: `format::native::tests` (full-feature round trip at 8-bit, 16-bit and float; 30 kinds of damaged file), `tests/native_fixture.rs` (pinned v1 file). Reachable from `imageworks convert`. Missing: Open and Save in the UI (M3) |
| Atomic save (rule 7) | done | `io::tests`, `format::tests::a_failed_save_leaves_the_existing_file_alone` |
| PNG import and export | partial | Engine done: `format::png::tests`; checked against Pillow (see below). Reachable from `imageworks convert`. Missing: UI (M3), export options (M9) |
| JPEG import and export | partial | Engine done: `format::jpeg::tests`; checked against Pillow. Reachable from `imageworks convert`. Missing: UI (M3), EXIF preservation and export options (M9) |
| Headless open-and-export smoke test in CI | done | `ci.yml` step "Open a document and export it, without a display", on macOS and Windows. Runs the release binary, not the installer |

Checked by hand against Pillow on 2026-10-06 (not in CI):

- PNG and JPEG written by the engine open in Pillow with the right size, pixels,
  resolution and ICC profile.
- PNGs written by Pillow (RGB, RGBA, grey, grey+alpha, palette, palette with
  transparency, 1-bit, 16-bit) import with the same pixels. With partial transparency
  the composited result differs by at most one 8-bit step, the known cost of
  premultiplied storage.
- JPEGs written by Pillow (baseline, progressive, greyscale, CMYK) import within the
  normal difference between two JPEG decoders (up to 3 levels with chroma
  subsampling).
- All eight EXIF orientations import the same way up as Pillow shows them.

### Needs manual check

1. **A real photo.** On the Mac, run
   `cargo run --release -p iw-app -- convert <a phone photo>.jpg ~/Desktop/test.png`
   and open `test.png` in Preview. It should be the right way up and look the same
   as the original.
2. **A real PNG with transparency** (a game asset, say):
   `... convert asset.png ~/Desktop/asset.iwdoc`, then
   `... convert ~/Desktop/asset.iwdoc ~/Desktop/asset-back.png`. The result should
   look identical to the original over any background.

## M3: application shell (complete; needs manual checks on macOS)

| Item | State | Evidence or what is missing |
| --- | --- | --- |
| Window, native menu bar, toolbar, tool options, status bar | done | Built from one action list (`actions::tests`). UI tests use the same menus in-window (`ui_tests`) |
| Document tabs and dockable panels | done | `egui_dock`; documents and panels dock, split and tab. `ui_tests::starts_with_the_start_screen` |
| GPU canvas: zoom, pan, rotate, mirror, cursor coordinates | done | `view::tests` (coordinate maths, zoom about a point, fit, rotation, mirror). Drawing checked on Linux under a virtual display only |
| Dirty-tile redraw (rule 5) | done | `ui_tests::view_changes_composite_nothing_and_edits_only_their_tiles` |
| Background compositing (rule 6) | done | `renderer::tests` (results, cancellation of stale tiles, 16-bit and float display) |
| Layers panel: raster layers, nested groups, visibility, opacity, blend mode, lock, rename, duplicate, delete, drag reorder, multi-select | done | `layers_model::tests` (rows, moves, grouping, selection), `ui_tests` (buttons, toggles, blend mode, rename by double-click, drag to reorder, menu commands with undo and redo) |
| History panel | done | `ui_tests::layer_menu_commands_undo_and_redo` (clicking a row jumps); `history::tests` |
| New, Open, Save, Save As, Close, Revert | done | `ui_tests` (New dialog, open from the command line, open errors, Save to the original file, close with the unsaved-changes prompt). Open, Save As and Export As use native dialogs: manual check |
| Export As (PNG, JPEG) | done | Engine tests from M2; menu item opens the native dialog: manual check |
| App icon in the window and installers | done | Window icon set at start-up; `Package` workflow builds the `.dmg` and NSIS installer with the icon (run 38008572087). How it looks in Finder and the Start menu: manual check |
| GPU compositor tested against the CPU one (rule 3) | not started | Deferred to M6/M8; see `DECISIONS.md` |
| Performance budgets (charter section 4) | not started | No benchmark harness |

Fixed carry-overs from M0: the macOS app menu is titled "ImageWorks" (the first window
title is the app name), and installers include the app icon.

### Needs manual check (macOS)

Run `cargo run --release -p iw-app` after `git pull`.

1. **Menus.** The menu bar shows ImageWorks, File, Edit, Layer, View, Window. The
   ImageWorks menu has About, Hide, and Quit. Items that cannot be used are greyed
   (for example Undo in a new document).
2. **New.** File > New…, choose "Texture, 1024 × 1024", Create. A white canvas
   appears in a tab named Untitled-1, and the Layers panel shows Background.
3. **Layers.** Press Cmd+Shift+N twice; drag "Layer 2" below Background; Cmd-click two
   layers and press Cmd+G; double-click a name and rename it; toggle an eye and a lock;
   change the blend mode and drag the opacity slider. Each step appears once in the
   History panel. Cmd+Z and Cmd+Shift+Z step through them, and Edit > Undo names the
   step.
4. **Canvas.** Pinch or Cmd+scroll zooms at the pointer; two-finger scroll pans; hold
   Space and drag to pan; press R and drag to rotate (Shift snaps); View > Flip View
   Horizontally mirrors; Cmd+0 fits; Cmd+1 is 100%. The status bar shows the pointer's
   pixel coordinates, zoom and angle. When zoomed far in, pixels are sharp squares.
5. **Files.** Open a PNG with transparency (checkerboard shows through), a JPEG
   photo (upright), and `crates/iw-engine/tests/fixtures/v1.iwdoc` (five layers).
   Drag an image file onto the window: it opens. Save As writes an `.iwdoc` that
   reopens identically. Export As writes a PNG and a JPEG.
6. **Unsaved changes.** Edit a document, then close its tab, then press Cmd+Q: each
   asks to save first, and Cancel keeps everything open.
7. **Large document.** File > New…, 8000 × 8000. The canvas fills in within a few
   seconds while the window stays responsive, and panning and zooming stay smooth.
8. **Dock and window.** The Dock shows the ImageWorks icon; panels can be dragged to
   other edges; Window > Reset Panel Layout restores them.

## M3a to M21

All `not started`. See the roadmap in `CLAUDE.md` section 6.

All `not started`. See the roadmap in `CLAUDE.md` section 6.
