# Building

## Requirements

- [rustup](https://rustup.rs). The compiler version is pinned in `rust-toolchain.toml`
  and installed automatically on first build.
- macOS: Xcode Command Line Tools (`xcode-select --install`).
- Windows: Visual Studio Build Tools with the "Desktop development with C++" workload.
- Linux (development only, not a release target): a desktop with X11 or Wayland and
  Vulkan or OpenGL drivers.

No other system libraries are needed.

## Development build

    cargo run -p iw-app

## Production build

    cargo build --release -p iw-app

The binary is `target/release/imageworks` (`imageworks.exe` on Windows).
`imageworks --version` prints the version and exits without opening a window.

## Packaging

    cargo install cargo-packager --locked --version 0.11.8
    cargo packager --release -p iw-app --formats dmg     # macOS
    cargo packager --release -p iw-app --formats nsis    # Windows installer (.exe)

Output goes to `dist/`. Configuration is under `[package.metadata.packager]` in
`crates/iw-app/Cargo.toml`. The `Package` workflow runs this on every push to `main`
and uploads the installers as artifacts.

Packages are unsigned. Code signing and notarisation are not set up yet.

## Environments without access to static.rust-lang.org

If rustup cannot download the pinned toolchain, build with an installed one of at
least the workspace `rust-version`: `RUSTUP_TOOLCHAIN=stable cargo build`.
