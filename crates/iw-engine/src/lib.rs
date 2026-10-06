//! ImageWorks engine.
//!
//! Headless by rule: no UI, windowing or GPU-surface dependencies
//! (enforced by `scripts/check-layering.sh` in CI). Subsystems land here
//! milestone by milestone; see `STATUS.md`.

pub mod blend;
pub mod command;
pub mod compositor;
pub mod document;
pub mod geom;
pub mod history;
pub mod pixel;
pub mod raster;
pub mod tile;

/// Engine version, shown in the app's About/diagnostics output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
