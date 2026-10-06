# ImageWorks

A professional raster image editor for macOS and Windows, by Questionableworks.
Layers, masks, selections, painting, adjustments, filters, text and vectors, with
non-destructive workflows. No image generation.

**Status: the engine foundation is in place (milestones M0 to M2); the application
shell is next (M3).** The window the binary opens is still the UI toolkit spike. `STATUS.md` is the source of truth for what works.

| Read this | For |
| --- | --- |
| `CLAUDE.md` | Project charter: scope, fixed decisions, roadmap, working rules |
| `STATUS.md` | What is done, partial or not started, and what to do next |
| `DECISIONS.md` | Decisions made along the way, with reasons |
| `ARCHITECTURE.md` | Crate layout and the rules the build enforces |
| `BUILD.md` | Building and packaging from a clean machine |
| `DEVELOPMENT.md` | Day-to-day workflow, checks, adding crates and dependencies |
| `SHORTCUTS.md` | Keyboard shortcuts |
| `docs/FILE_FORMAT.md` | The `.iwdoc` file format |
| `docs/BLEND_MODES.md` | Blend mode formulas and differences from Photoshop |

Quick start: install Rust with [rustup](https://rustup.rs), then `cargo run -p iw-app`.

There is no editing interface yet, but the engine can already convert files:

    cargo run --release -p iw-app -- convert input.png output.iwdoc
    cargo run --release -p iw-app -- convert layered.iwdoc flat.jpg --quality 85
