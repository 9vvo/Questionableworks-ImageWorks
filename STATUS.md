# Status

Source of truth for what works. A row is `done` only when it meets the definition of
done in `CLAUDE.md` section 5 and names its evidence.

## Next session

**Milestone M0 is not finished.** Everything that can be checked on a Linux machine
is done; everything that needs macOS, Windows or GitHub Actions has never run.

Do first, in this order:

1. Open the repository's Actions tab. Fix whatever is red in `CI`, then in `Package`.
   Neither workflow has ever executed. `native_menu.rs` has never been compiled for
   macOS or Windows, so start there if the build fails.
2. Run the manual checks below on a Mac and a Windows PC and record the results in
   the spike table.
3. macOS pen pressure is expected to fail (see the spike table). Add a native
   `NSEvent` monitor in `iw-app` that reads tablet pressure, then re-test. If that
   cannot be made to work, the charter's fallback is a C++/Qt 6 shell.
4. Add a real application icon to the packaging config.
5. When every spike row is `pass`, mark M0 done and stop.

## M0: foundation

| Item | State | Evidence or what is missing |
| --- | --- | --- |
| Workspace layout | done | `cargo test --workspace` passes on Linux |
| Engine layering guard | done | `scripts/check-layering.sh`; verified to fail when a windowing crate is added to `iw-engine` and when cargo itself fails |
| Licence policy | done | `cargo deny check licenses` passes |
| Docs skeleton | done | README, ARCHITECTURE, BUILD, DEVELOPMENT, SHORTCUTS, STATUS, DECISIONS |
| CI on macOS and Windows | partial | Workflow written; has never run |
| Packaging skeleton | partial | Config verified by building a `.deb` on Linux; the `.dmg` and NSIS formats have never run. No app icon yet |
| UI toolkit spike | partial | See table below |

### UI toolkit spike (egui/eframe + egui_dock + muda + rfd)

| Requirement | macOS | Windows | Notes |
| --- | --- | --- | --- |
| Renders through wgpu | pass | untested | macOS: Metal on Apple M5 Pro (owner's Mac mini, 2026-10-05). Works on Linux with a software adapter |
| Dockable panels | pass | untested | Works on Linux. Panels dock, split and tab inside the main window; `egui_dock` does not detach a panel into its own OS window, which matters for multi-monitor layouts |
| Native menu bar | pass | untested | macOS: app, Spike and Window menus show in the system menu bar (2026-10-05). The app menu is titled with the window title instead of "ImageWorks"; cosmetic, fix in M3. Never compiled for Windows |
| Native file dialogs | untested | untested | Menu items are visible on macOS; not yet clicked |
| HiDPI | untested | untested | First macOS run was on a 1.0-scale 5120x1440 display, so it proves nothing about Retina |
| Pen pressure | expected fail | untested | winit 0.30 turns Windows pen input into touch events with force, which the pen probe reads. Its macOS backend does not report tablet pressure at all (checked in the winit source), so macOS needs a native hook |

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

## M1 to M21

All `not started`. See the roadmap in `CLAUDE.md` section 6.
