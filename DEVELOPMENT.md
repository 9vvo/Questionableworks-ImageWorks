# Development

Read `CLAUDE.md` first; it sets the order of work and the definition of done.

## Checks to run before every commit

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    bash scripts/check-layering.sh
    cargo deny check licenses        # cargo install cargo-deny --locked

CI runs the same commands on macOS and Windows, then builds the release binary and
runs `imageworks --version`.

## Adding a dependency

Run `cargo deny check licenses`. If it fails, do not add an exception yourself: the
charter requires asking the owner first.

## Adding an engine crate

Create it under `crates/`, add it to the workspace members and to `ENGINE` in
`scripts/check-layering.sh`, and describe it in `ARCHITECTURE.md`.

## Running the shell without a desktop (Linux)

    xvfb-run -a cargo run -p iw-app

Rendering falls back to a software adapter. Useful for checking that the window comes
up; not a substitute for testing on macOS and Windows.

## Golden images

`crates/iw-engine/tests/golden/` holds one expected image per blend mode. If a test
fails, the engine's output changed. If the change is intended:

    IW_BLESS=1 cargo test -p iw-engine --test blend_golden

then look at the image diff before committing it. Never bless to make a failure go
away without understanding it.

`python3 scripts/crosscheck-blend-modes.py` compares the blend modes with
ImageMagick (needs ImageMagick, Pillow and numpy). Run it after changing `blend.rs`
or `compositor.rs`.
