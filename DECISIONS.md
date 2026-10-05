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

## 2026-10-05 (M1)

- **Tile size 256 x 256.** Small enough that a brush stroke touches few tiles, large
  enough to keep per-tile overhead low; also a natural GPU texture upload unit.
- **Rasters are unbounded and sparse, with signed coordinates.** A layer can extend
  past the canvas, and empty areas cost nothing. Canvas clipping is the document's
  job (M2).
- **RGBA only for now.** Document colour mode arrives with the document model (M2)
  and CMYK in M21; adding a colour-model parameter before there is a second model
  would be guesswork.
- **Blend modes use gamma-encoded values in 8-bit and 16-bit documents.** The
  charter asks for linear light "where the mode requires it". None does: matching
  the look users expect from Photoshop requires blending encoded values. 32-bit
  float documents are linear, so blending there is linear. See `docs/BLEND_MODES.md`.
- **Blend formulas follow the PDF 1.7 / W3C Compositing definitions.** They are the
  published definitions of these modes. Differences from Photoshop are listed in
  `docs/BLEND_MODES.md`.
- **The compositor accumulates in `f32` and quantises once.** Avoids rounding error
  building up across layers in 8-bit documents. Results can differ from Photoshop,
  which quantises per layer, by one 8-bit step.
- **Dissolve's pattern is a hash of pixel position.** Stable across recomposites and
  identical on every platform, so it can be golden-tested.
- **Golden PNGs store premultiplied bytes unchanged.** Converting to straight alpha
  loses precision at low alpha, which would hide real differences.
- **`png` crate as a test-only dependency of the engine.** MIT OR Apache-2.0.
- **Packaging runs on demand or on a `v*` tag, not on every push.** The repository
  is private, so Actions minutes are metered, with macOS and Windows runners billed
  at a multiple. Installers are only needed when someone wants to install a build.
