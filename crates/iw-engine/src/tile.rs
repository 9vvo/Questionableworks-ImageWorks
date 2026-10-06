//! Fixed-size pixel tiles.
//!
//! A raster is a sparse grid of tiles. Tiles are reference-counted and
//! copy-on-write, so cloning a raster is cheap and an undo record can keep
//! the old version of just the tiles a stroke touched.

use crate::pixel::Texel;
use std::sync::Arc;

/// Tile edge length in pixels.
pub const TILE_SIZE: u32 = 256;
const TILE_AREA: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// Position of a tile in the grid: tile `(tx, ty)` covers pixels
/// `tx * TILE_SIZE ..` by `ty * TILE_SIZE ..`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    // `ty` first so the derived ordering is row-major.
    pub ty: i32,
    pub tx: i32,
}

impl TileCoord {
    pub const fn new(tx: i32, ty: i32) -> Self {
        Self { ty, tx }
    }

    /// The tile containing pixel `(x, y)`, and the pixel's index inside it.
    #[inline]
    pub fn of_pixel(x: i32, y: i32) -> (Self, usize) {
        let size = TILE_SIZE as i32;
        let coord = Self::new(x.div_euclid(size), y.div_euclid(size));
        let index = y.rem_euclid(size) as usize * TILE_SIZE as usize + x.rem_euclid(size) as usize;
        (coord, index)
    }

    /// Pixel coordinate of the tile's top-left corner.
    pub const fn origin(&self) -> (i32, i32) {
        (self.tx * TILE_SIZE as i32, self.ty * TILE_SIZE as i32)
    }
}

/// `TILE_SIZE` x `TILE_SIZE` texels, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile<P: Texel> {
    pixels: Arc<[P]>,
}

impl<P: Texel> Tile<P> {
    /// A tile of default (empty) texels.
    pub fn new() -> Self {
        Self::filled(P::default())
    }

    pub fn filled(pixel: P) -> Self {
        Self {
            pixels: vec![pixel; TILE_AREA].into(),
        }
    }

    /// A tile holding `pixels` (row-major), or `None` if there are not
    /// exactly `TILE_SIZE * TILE_SIZE` of them.
    pub fn from_pixels(pixels: Vec<P>) -> Option<Self> {
        (pixels.len() == TILE_AREA).then(|| Self {
            pixels: pixels.into(),
        })
    }

    #[inline]
    pub fn pixels(&self) -> &[P] {
        &self.pixels
    }

    /// Mutable access. Copies the pixel data first if another tile handle
    /// shares it.
    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [P] {
        Arc::make_mut(&mut self.pixels)
    }

    /// True if both handles point at the same pixel data (no copy has
    /// happened since one was cloned from the other).
    pub fn shares_data_with(&self, other: &Tile<P>) -> bool {
        Arc::ptr_eq(&self.pixels, &other.pixels)
    }

    /// True if every texel is the default (empty) value.
    pub fn is_empty(&self) -> bool {
        let zero = P::default();
        self.pixels.iter().all(|p| *p == zero)
    }
}

impl<P: Texel> Default for Tile<P> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_pixel_handles_negative_coordinates() {
        assert_eq!(TileCoord::of_pixel(0, 0), (TileCoord::new(0, 0), 0));
        assert_eq!(
            TileCoord::of_pixel(255, 255),
            (TileCoord::new(0, 0), TILE_AREA - 1)
        );
        assert_eq!(TileCoord::of_pixel(256, 0), (TileCoord::new(1, 0), 0));
        assert_eq!(
            TileCoord::of_pixel(-1, -1),
            (TileCoord::new(-1, -1), TILE_AREA - 1)
        );
        assert_eq!(
            TileCoord::of_pixel(-256, -257),
            (TileCoord::new(-1, -2), 255 * 256)
        );
    }

    #[test]
    fn clone_shares_until_written() {
        let a: Tile<[u8; 4]> = Tile::filled([1, 2, 3, 4]);
        let mut b = a.clone();
        assert!(a.shares_data_with(&b));
        b.pixels_mut()[0] = [9, 9, 9, 9];
        assert!(!a.shares_data_with(&b));
        assert_eq!(a.pixels()[0], [1, 2, 3, 4]);
        assert_eq!(b.pixels()[0], [9, 9, 9, 9]);
    }

    #[test]
    fn transparency_check() {
        let mut t: Tile<[u16; 4]> = Tile::new();
        assert!(t.is_empty());
        t.pixels_mut()[100] = [0, 0, 0, 1];
        assert!(!t.is_empty());
        // Single-channel tiles work the same way.
        let mut m: Tile<u8> = Tile::new();
        assert!(m.is_empty());
        m.pixels_mut()[0] = 255;
        assert!(!m.is_empty());
    }
}
