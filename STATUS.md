# Status

Source of truth for what works. A row is `done` only when it meets the definition of
done in `CLAUDE.md` section 5 and names its evidence.

## Next session

**M0 and M1 are complete. Start M2** (document model, command and history engine,
native format, atomic save, PNG and JPEG import and export).

Things M2 should know:

- `Raster<C>` clones are copy-on-write per tile, which is what delta undo needs:
  keep the pre-edit `Tile` handles for the tiles a command touches.
- The compositor takes a flat `&[Layer]`. Groups, masks and clipping are not in it
  yet; extend `compositor.rs` rather than compositing anywhere else (rule 3).
- Golden tests so far cover 8-bit only. 16-bit and float are covered by unit tests
  that check they agree with 8-bit.

Carried forward:

- Nobody has run the app on Windows. Do manual checks 1 to 6 there when a Windows
  machine is available.
- Pen pressure and HiDPI are untested with the risk accepted by the owner (see the
  spike table). Pen pressure must be tested on a tablet before M6 is marked done.
- The macOS app menu shows the window title instead of "ImageWorks". Fix in M3.
- Packaging has no application icon. Add one in M3.
- M3a (MCP server) was added to the roadmap after M3 at the owner's request. Design
  M2's command API with it in mind: commands need stable names and serialisable
  parameters, which actions (M18) need anyway.
- No performance numbers exist yet. The charter's budgets apply from M3, when there
  is a canvas to measure.

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

## M2 to M21, including M3a

All `not started`. See the roadmap in `CLAUDE.md` section 6.
