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

## The native format fixture

`crates/iw-engine/tests/fixtures/v1.iwdoc` proves that files saved by format version 1
still open. Never regenerate it. If you change the format so that old readers would
misread new files, raise `VERSION` in `format/native.rs`, keep reading version 1, add
a new fixture and test beside the old ones, and update `docs/FILE_FORMAT.md`.

## Adding a command

Add the variant to `Command`, handle it in `command::apply` (validate first, then
change the document, then return the inverse and the damage), give it a label, and
add it to the list in `history::tests::every_command_undoes_and_redoes_exactly` and
to the random test beside it.

## UI tests

`crates/iw-app/src/ui_tests.rs` drives the whole app through `egui_kittest`: it clicks
menus and buttons found by their accessibility labels and presses shortcuts, with no
window or GPU. They run in `cargo test` on every platform. The app is built with the
in-window menu bar for these tests, so they behave the same on macOS and Windows.

Widgets drawn by hand (layer rows, history rows) must call `response.widget_info(...)`
with a label, or neither screen readers nor the tests can find them.

The harness advances time by 1/60 s per step. Clicks less than 0.6 s apart can count
as a triple click; `run_steps(60)` before a double click avoids that.

## Running the app

    cargo run -p iw-app                 # the application
    cargo run -p iw-app -- file.png     # and open a file
