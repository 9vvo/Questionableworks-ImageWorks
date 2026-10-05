# Decisions

Newest last. One entry per decision: what, why, and whether the owner confirmed it.

## 2026-10-05 (M0)

- **Product name is ImageWorks.** Taken from the repository name. Reversible until
  the native file extension and bundle identifier ship in a release.
- **Bundle identifier `com.questionableworks.imageworks`.** Studio name plus product.
- **Stack, targets and benchmark budgets from the charter are in use unconfirmed.**
  Work started without changes to the `[confirm]` lines, so they stand as defaults.
- **Engine is a single crate (`iw-engine`) for now.** There is nothing to separate
  yet; splitting empty crates up front would be guesswork about boundaries.
- **UI toolkit under test: egui/eframe 0.36 + egui_dock + muda + rfd.** It is the
  only pure-Rust combination that covers wgpu rendering, docking, native menus and
  native dialogs with compatible licences. Kept or dropped on the spike results in
  `STATUS.md`.
- **Licences BSL-1.0, ISC and Unicode-3.0 added to the allow-list; OFL-1.1 and Ubuntu
  Font Licence allowed for `epaint_default_fonts` only.** Owner approved. The first
  three are permissive and arrive through `clipboard-win`, `libloading` and
  `unicode-ident`. The font licences cover egui's bundled default fonts.
- **Licence check covers shipping targets only** (`deny.toml` `[graph] targets`).
  Linux is a development platform; its GTK-era dependencies never ship.
- **Compiler pinned to 1.97.0** in `rust-toolchain.toml` for reproducible builds.
  Workspace `rust-version` is 1.95, the minimum eframe 0.36 accepts.
- **Packaging with cargo-packager**, `.dmg` on macOS and an NSIS `.exe` installer on
  Windows. NSIS over MSI because it needs no WiX toolchain on the build machine.
- **Packaging runs in its own workflow**, so an installer failure cannot mask a test
  failure in CI.
- **Linux development builds show menu commands in-window.** `muda` needs GTK on
  Linux, which is outside the licence policy's spirit for no shipping benefit.
- **egui/eframe + egui_dock + muda + rfd is the UI toolkit.** On the owner's Mac it
  passed wgpu rendering (Metal), docking, native menu bar and both file dialogs.
- **Pen pressure and HiDPI accepted as untested risks.** Owner has no tablet or
  Retina display and chose to proceed. Pen pressure is expected to need a native
  macOS hook; it has to be proven on hardware before M6 is done.
- **The app is for personal use, possibly shared with a few friends.** Stated by the
  owner. Consequence: code signing and notarisation are not planned; unsigned builds
  with a Gatekeeper or SmartScreen warning are acceptable.
- **Markdown-only pushes do not trigger CI or packaging.** Status updates were
  cancelling and restarting in-progress runs.
