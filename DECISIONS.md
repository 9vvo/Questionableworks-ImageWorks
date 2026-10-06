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

## 2026-10-06

- **Repository made public by the owner** so GitHub Actions minutes are free. CI had
  stopped starting jobs while the repository was private.
- **MCP support added to the roadmap as M3a**, requested by the owner. Read as: the
  editor runs an MCP server that an assistant connects to, in the way Blender's and
  Roblox Studio's do. Placed straight after M3 because it needs only the command
  engine and a running shell, and it lets an assistant exercise the real app on the
  owner's machine, which is otherwise untestable from CI. Not yet decided: the
  transport and which MCP library to use (licence to be checked then).

## 2026-10-06 (M2)

- **Native extension is `.iwdoc`.** `.iwd` is taken by another well-known format.
- **The native file is a ZIP with a JSON manifest and one entry per tile.** It can be
  inspected and repaired with ordinary tools, extends naturally (embedded files for
  smart objects later), and per-tile entries allow partial loading later. Cost: files
  are larger than a purpose-built binary format would be.
- **Commands are data and return their own inverse.** Chosen over objects with
  `undo()` methods because data can be recorded (actions, M18), sent over MCP (M3a)
  and tested by comparing documents.
- **Layer ids are never reused, even after undo.** Document equality therefore
  ignores the id counter; the counter is still saved in the file.
- **Layers carry an offset.** Moving a layer copies no pixels and needs no undo data
  beyond the old offset. The compositor handles offsets that are not tile-aligned.
- **Groups are pass-through by default**, as in Photoshop, and can be switched to
  isolated with their own blend mode.
- **A locked layer cannot have its pixels edited or be moved.** It can still be
  renamed, hidden, reordered and deleted. One lock flag for now; Photoshop's separate
  transparency/pixels/position locks can be added to the format later without
  breaking it.
- **Alpha channels and paths are stored and undoable now, with no tools yet.** The
  document model and file format carry them so M4 and M11 do not need a format
  change.
- **PNG and JPEG codecs: `png` crate for PNG, `image` crate (JPEG only) for JPEG.**
  `jpeg-encoder` was rejected because its licence adds IJG terms, which are outside
  the allow-list; `image`'s encoder is inside it. `image` always encodes 4:2:2 chroma.
- **JPEG export flattens onto white.** JPEG has no transparency and white is what
  users expect from a transparent canvas.
- **Float documents export to PNG as 16-bit, clamped to 0..1.** PNG has no float
  format.
- **Import size is capped at 2^30 pixels**, to refuse files with absurd dimensions
  before allocating for them.
- **Consequence of premultiplied 8-bit storage, measured:** importing a PNG and
  exporting it again changes colour under partial transparency by at most one 8-bit
  step in the composited result, and colour under fully transparent pixels is not
  kept. Invisible on screen; it would matter for game textures that rely on colour
  bleeding under transparent pixels. Changing it would be a charter section 2 change.
- **Dependencies are optimised in debug builds** (`profile.dev.package."*"`).
  Unoptimised compression made the test suite about ten times slower.
- **The CI smoke test runs the release binary's `convert` command**, not the
  installer. Launching the packaged app on a runner needs a display and comes with
  the real shell in M3.
