# Architecture

The rules live in `CLAUDE.md` section 3. This file describes how the code
implements them today. It grows with each milestone.

## Crates

| Crate | Role | May depend on UI / windowing / GPU surface |
| --- | --- | --- |
| `crates/iw-engine` | Headless engine. Currently empty apart from its version. | No |
| `crates/iw-app` | Desktop shell, binary `imageworks`. Currently the M0 toolkit spike. | Yes |

The engine starts as one crate. Split it (tiles, compositor, document, formats, ...)
when a subsystem has a reason to compile or be depended on separately, not before.
Every engine crate is listed in `scripts/check-layering.sh`.

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
