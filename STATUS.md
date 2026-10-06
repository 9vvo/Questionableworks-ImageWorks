# Status

Source of truth for what works. A row is `done` only when it meets the definition of
done in `CLAUDE.md` section 5 and names its evidence.

## Next session

**M0, M1 and M2's engine work are complete. Start M3** (application shell: window,
menus, panels, GPU canvas, Layers panel, History panel, New/Open/Save).

Before starting, confirm `CI` is green for the last M2 commit.

Things M3 should know:

- The app must change documents only through `Session::execute`; the compiler will
  not let it do otherwise.
- `Effects::damage` from execute, undo and redo says which tiles to recomposite.
  `Document::composite_tile` produces one tile; a single opaque layer's tile comes
  back shared, so uploading it to the GPU needs no copy.
- `Session::states`, `position` and `jump_to` are what the History panel needs.
  `Session::is_modified` drives the unsaved-changes prompt.
- `format::open` and `format::save` do the file work; `FileFormat::from_path` and
  `extensions` feed the file dialogs. Saving as PNG or JPEG flattens, so the shell
  should treat those as Export, not Save.
- Layer lists are **bottom first** in the engine; a Layers panel shows them reversed.
- The M0 spike (`spike.rs`) is to be replaced, not extended.

Carried forward:

- Nobody has run the app on Windows. Do the M0 manual checks 1 to 6 there when a
  Windows machine is available.
- Pen pressure and HiDPI are untested with the risk accepted by the owner (see the
  spike table). Pen pressure must be tested on a tablet before M6 is marked done.
- The macOS app menu shows the window title instead of "ImageWorks". Fix in M3.
- Packaging has no application icon. Add one in M3.
- M3a (MCP server) follows M3. Commands are already plain data with stable blend-mode
  ids, which is what it needs.
- No performance numbers exist yet. The charter's budgets apply from M3, when there
  is a canvas to measure. The CPU compositor is a reference implementation and has
  not been profiled.
- `.iwdoc` files are larger than they need to be (see `docs/FILE_FORMAT.md`).
- JPEG import does not keep EXIF data (only applies its orientation), and reads
  resolution from the JFIF header only. Metadata handling is M9.

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

## M3 to M21, including M3a

All `not started`. See the roadmap in `CLAUDE.md` section 6.
