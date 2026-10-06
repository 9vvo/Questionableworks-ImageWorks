# Image editor: project charter

This is the standing brief for every session. Each session works on one milestone,
using the session prompt at the bottom.

Lines marked **[confirm]** are defaults the owner has not explicitly confirmed. Work
proceeds on them; changing one is a section 2 change and needs the owner.

---

## 1. What we are building

A professional raster image editor for macOS and Windows: layers, masks, selections,
painting, adjustments, filters, text, vectors, and non-destructive workflows, in the
tradition of Photoshop, Krita and Affinity Photo. It is a real product. Every control
in the UI does what it says.

It has its own name, icon set and visual identity. No Adobe names, logos, icons,
artwork or code. Standard concepts (layers, blend modes, masks, paths) are fine.

**No image generation.** The line: an operation may only rearrange, filter or
reconstruct pixels from the document itself or from classical algorithms. Allowed:
segmentation, background removal, smart selection, edge detection, denoising,
upscaling, patch-based inpainting. Not allowed: text-to-image, diffusion or other
generative models, generative fill or expand, any prompt box.

## 2. Fixed decisions

- **Stack [confirm]:** Rust workspace. Engine crates are pure Rust with no windowing
  dependency. Rendering through wgpu (Metal on macOS, D3D12 on Windows). UI toolkit is
  chosen in milestone M0 by a spike that must demonstrate, on both OSes: dockable
  panels, native menu bar, native file dialogs, HiDPI, and pen pressure from a tablet.
  If no Rust toolkit passes, fall back to a C++/Qt 6 shell over the same engine.
- **Targets [confirm]:** macOS 13+ (arm64 and x64), Windows 10 22H2+ (x64).
- **Dependency licenses:** MIT, Apache-2.0, BSD, zlib, MPL, BSL-1.0, ISC,
  Unicode-3.0. LGPL only when dynamically linked. No GPL. Ask before adding anything
  else. `deny.toml` is the enforced list, including per-crate exceptions.
- **Repo and CI:** GitHub Actions builds and tests on macOS and Windows runners for
  every push. A red build is the first thing fixed in any session.
- **Pixel model:** tile-based storage, premultiplied alpha, generic over 8-bit,
  16-bit and 32-bit float channels from day one, even though only 8-bit is exposed
  at first. Working space is linear-light for blending where the mode requires it;
  document the choice per blend mode.

## 3. Architecture rules

These are enforced by the build, not by convention.

1. Engine crates (document, tiles, compositor, selection, brush, filters, formats,
   history) do not depend on any UI, windowing or GPU-surface crate. They build and
   test headless.
2. The UI never mutates a document directly. Every change is a command submitted to
   the history engine. This is what makes undo, action recording and batch
   processing possible later, so there are no exceptions.
3. One compositor. Blend modes, masks, clipping and opacity live in one place, with a
   CPU reference implementation and a GPU implementation tested against it.
4. Undo stores deltas: touched tiles for raster changes, old and new values for
   property changes. Never a full document copy.
5. The canvas redraws dirty tiles only. Nothing recomposites the whole document on
   mouse move.
6. Anything that can take longer than one frame runs on a worker and is cancellable.
7. Saves are atomic (write temp, fsync, rename).

Module boundaries follow the stack, but responsibilities stay separate: document,
layers, compositor, raster, selection, masks, brush, color, filters, transform, text,
vector, history, file formats, export, automation, plugins, platform, UI.

## 4. How "it works" is proven

- **Golden-image tests** for every blend mode, adjustment, filter, brush stamp and
  transform: fixed input, stored expected output, per-channel tolerance.
- **Round-trip tests** for the native format: build a document using every feature
  marked done, save, load, compare structure and pixels.
- **Undo tests:** apply command, undo, assert identical to the starting state; redo,
  assert identical to the applied state.
- **Smoke test in CI:** the packaged app launches on both OSes, opens a fixture
  document, exports a PNG, exits with code 0.
- **Benchmarks with budgets [confirm]**, run on a document of 8000 x 8000 px with 20
  layers:
  - pan and zoom hold 60 fps
  - a 100 px brush stroke reaches the screen within 16 ms of input
  - undoing a stroke allocates in proportion to the tiles touched
  - opening the document uses less than 2x its uncompressed tile size in RAM
  A benchmark that regresses by more than 20% fails CI.

GUI behaviour that cannot be tested automatically is listed in `STATUS.md` as
"needs manual check" with the exact steps. It is not reported as verified.

## 5. Definition of done, and the status ledger

A feature is done when: the engine implements it, the UI exposes it, it goes through
the command system, undo and redo pass, the native format round-trips it, its tests
are in CI, and CI is green on both OSes.

`STATUS.md` lists every roadmap item as `not started`, `partial` or `done`. Each
`done` row names its tests. Each `partial` row says what is missing.

**The UI exposes only features marked done.** No menu item, button or panel exists
for anything `partial` or `not started`. This replaces any need for placeholder
controls.

## 6. Roadmap

Finish a tier before starting the next. Within a tier, milestones are in order.
Nothing here is optional; the tiers only set the sequence.

### Tier 0: foundation

- **M0** Repo, workspace layout, CI on both OSes, UI toolkit spike, packaging
  skeleton (.app/.dmg, .msi or .exe installer), docs skeleton.
- **M1** Tile store, pixel formats, CPU compositor with all 27 blend modes (Normal,
  Dissolve, Darken, Multiply, Color Burn, Linear Burn, Darker Color, Lighten, Screen,
  Color Dodge, Linear Dodge, Lighter Color, Overlay, Soft Light, Hard Light, Vivid
  Light, Linear Light, Pin Light, Hard Mix, Difference, Exclusion, Subtract, Divide,
  Hue, Saturation, Color, Luminosity). Golden tests.
- **M2** Document model (dimensions, resolution, color mode, bit depth, profile,
  layers, groups, channels, paths, metadata), command and history engine, native
  format with its own extension, atomic save. PNG and JPEG import and export.
- **M3** Application shell: window, menus, toolbar, tool options, document tabs,
  dockable panels, status bar. GPU canvas with zoom, pan, rotate, mirror, dirty-tile
  redraw, cursor coordinates. Layers panel: raster layers, nested groups, visibility,
  opacity, blend mode, lock, rename, duplicate, delete, drag reorder, multi-select.
  History panel. New, Open, Save, Save As, Close, Revert.

- **M3a** MCP server, so an AI assistant can drive the running editor. Off by default;
  local connections only. Every tool maps to an existing command, so each action is
  undoable and appears in the History panel like a user's. First tools: list open
  documents, read document and layer structure, create and edit layers and their
  properties, open and export files, undo and redo, and return a rendered preview of
  the document. Later milestones add a tool for each new command they introduce. The
  server exposes editing commands only: it is not a route to image generation
  (section 1).

### Tier 1: a usable editor

- **M4** Selection engine with one shared mask representation: rectangular and
  elliptical marquee, lasso, polygonal lasso, magic wand, color range; add, subtract,
  intersect; invert, feather, expand, contract, smooth, border, grow, similar. Copy,
  cut, paste, delete honour the selection.
- **M5** Layer masks and clipping masks: selection to mask and back, invert, feather,
  density, paint on mask, mask preview.
- **M6** Brush engine: size, hardness, opacity, flow, spacing, roundness, angle,
  smoothing, pressure, stroke interpolation with no gaps. Brush, pencil, eraser.
  Color picker, eyedropper, foreground and background, hex/RGB/HSV/HSL, swatches.
- **M7** Move and free transform (scale, rotate, skew, flip) with numeric entry,
  proportional lock and origin, high-quality resampling. Crop with aspect
  constraints, fixed size, presets, straighten. Rulers, guides, grid, pixel grid,
  snapping.
- **M8** Adjustment layers: Brightness/Contrast, Levels, Curves, Exposure,
  Hue/Saturation, Color Balance, Invert, Threshold, Posterize. Filter engine with
  preview, cancel, apply: Gaussian, box and motion blur, Sharpen, Unsharp Mask, Add
  Noise, Median.
- **M9** Autosave, recovery files, crash recovery on next launch. Customisable
  keyboard shortcuts with the usual defaults (V, M, L, W, C, I, B, E, G, P, T, Z, H).
  Preferences window. Export As with quality, dimensions, transparency, metadata.

### Tier 2: professional depth

- **M10** Text layers that stay editable: point and paragraph text, font, size,
  weight, italic, tracking, kerning, leading, alignment, color, baseline shift.
- **M11** Vector layers and paths: rectangle, rounded rectangle, ellipse, polygon,
  star, line, pen, editable anchors and handles, fill, stroke, gradients; path to
  selection and back, fill path, stroke path; vector masks.
- **M12** Layer effects, all editable: drop shadow, inner shadow, outer and inner
  glow, bevel and emboss, satin, color, gradient and pattern overlay, stroke. Fill
  layers. Gradient tool.
- **M13** Smart objects: embedded and linked sources, non-destructive transform,
  replace contents, edit contents, shared instances. Smart filters.
- **M14** Remaining adjustments (Vibrance, Black & White, Photo Filter, Channel
  Mixer, Color Lookup, Gradient Map, Selective Color) and filters (radial and surface
  blur, Smart Sharpen, Reduce Noise, Ripple, Wave, Twirl, Spherize, Pinch, Find
  Edges, Emboss, Diffuse, Solarize).
- **M15** Retouching: clone stamp, healing brush, spot healing, patch, red-eye.
  Smudge, dodge, burn, sponge. Brush scatter, texture, dynamics, tilt.
- **M16** Distort, perspective, warp, perspective crop. 16-bit documents exposed in
  the UI. ICC profiles on open, save and export. TIFF, WebP, GIF, BMP.
- **M17** Workspace presets (Essentials, Photography, Digital Painting, Graphic
  Design, UI Design) with save, load, reset, rename, delete. Full preferences.

### Tier 3: automation and extensions

- **M18** Actions: record, stop, play, edit, duplicate, delete, sets. Batch
  processing over folders with resize, action, convert, rename, export.
- **M19** Plugin API for filters, tools, panels, importers, exporters and automation,
  isolated from the core. `PLUGIN_API.md`.
- **M20** Model-assisted selection, background removal, denoise and upscale, running
  locally. Record each model's licence and source in `DECISIONS.md`.
- **M21** Quick selection, magnetic lasso, text on path, warp text, custom shapes.
  CMYK, 32-bit float, PSD import, SVG and PDF export, RAW import.

## 7. Working rules

- One milestone per session. Stop when it is done or blocked; do not start the next.
- Build order inside a milestone: engine, tests, command integration, serialization,
  then UI.
- Commit after each passing step. Never commit with failing tests.
- Decide anything reversible without asking, and log it in `DECISIONS.md` with one
  line of reasoning. Stop and ask before: changing a fixed decision in section 2,
  adding a dependency outside the licence list, or dropping or reordering a roadmap
  item.
- Where a Photoshop behaviour cannot be matched exactly, ship the closest sound
  equivalent and note the difference in `DECISIONS.md`.
- Fixing a bug includes adding the regression test.
- Keep current: `README.md`, `ARCHITECTURE.md`, `BUILD.md`, `DEVELOPMENT.md`,
  `SHORTCUTS.md`, `STATUS.md`, `DECISIONS.md`. `BUILD.md` must be enough to build on
  a clean machine.
- End every session by updating `STATUS.md` and writing a "Next session" note in it:
  what was finished, what is half-done, what to do first next time.

## 8. Session prompt

> Read `CLAUDE.md` and `STATUS.md`. Confirm CI is green; if not, fix that first.
> Then work on milestone **M__**. Follow the build order and the definition of done.
> When finished, update `STATUS.md`, list anything that needs a manual check with
> exact steps, and stop.
