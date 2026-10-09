# ImageWorks

A professional raster image editor for macOS and Windows, by Questionableworks.
Layers, masks, selections, painting, adjustments, filters, text and vectors, with
non-destructive workflows. No image generation.

**Status: milestones M0 to M3.** The application opens, saves and exports layered
documents, with layers, groups, blend modes, opacity, undo history and a rotatable
canvas. There are no painting or selection tools yet; those start with M4. `STATUS.md` is the source of truth for what works.

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

Files can also be converted without opening a window:

    cargo run --release -p iw-app -- convert input.png output.iwdoc
    cargo run --release -p iw-app -- convert layered.iwdoc flat.jpg --quality 85
