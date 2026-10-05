//! Sparse tiled raster.

use crate::geom::Rect;
use crate::pixel::{transparent, Channel, Pixel};
use crate::tile::{Tile, TileCoord, TILE_SIZE};
use std::collections::BTreeMap;

/// An unbounded RGBA image stored as a sparse grid of tiles.
///
/// A missing tile is fully transparent. Cloning is cheap: the clone shares
/// every tile until one side writes to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster<C: Channel> {
    tiles: BTreeMap<TileCoord, Tile<C>>,
}

impl<C: Channel> Default for Raster<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Channel> Raster<C> {
    pub fn new() -> Self {
        Self {
            tiles: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tile(&self, coord: TileCoord) -> Option<&Tile<C>> {
        self.tiles.get(&coord)
    }

    /// The tile at `coord`, created transparent if absent.
    pub fn tile_mut(&mut self, coord: TileCoord) -> &mut Tile<C> {
        self.tiles.entry(coord).or_default()
    }

    /// Replaces the tile at `coord`, returning the previous one.
    pub fn insert_tile(&mut self, coord: TileCoord, tile: Tile<C>) -> Option<Tile<C>> {
        self.tiles.insert(coord, tile)
    }

    pub fn remove_tile(&mut self, coord: TileCoord) -> Option<Tile<C>> {
        self.tiles.remove(&coord)
    }

    /// Coordinates of stored tiles in row-major order.
    pub fn tile_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
        self.tiles.keys().copied()
    }

    pub fn tiles(&self) -> impl Iterator<Item = (TileCoord, &Tile<C>)> + '_ {
        self.tiles.iter().map(|(c, t)| (*c, t))
    }

    pub fn pixel(&self, x: i32, y: i32) -> Pixel<C> {
        let (coord, index) = TileCoord::of_pixel(x, y);
        match self.tiles.get(&coord) {
            Some(tile) => tile.pixels()[index],
            None => transparent(),
        }
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, pixel: Pixel<C>) {
        let (coord, index) = TileCoord::of_pixel(x, y);
        // Writing transparency into a missing tile must not allocate it.
        if pixel == transparent() && !self.tiles.contains_key(&coord) {
            return;
        }
        self.tile_mut(coord).pixels_mut()[index] = pixel;
    }

    /// Bounding box of the stored tiles (tile-granular, not pixel-tight).
    pub fn bounds(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for coord in self.tiles.keys() {
            let (x, y) = coord.origin();
            let r = Rect::new(x, y, TILE_SIZE, TILE_SIZE);
            out = Some(match out {
                Some(o) => o.union(&r),
                None => r,
            });
        }
        out
    }

    /// Copies `rect` out as row-major pixels. Missing tiles read as transparent.
    pub fn read_rect(&self, rect: Rect) -> Vec<Pixel<C>> {
        let mut out = vec![transparent(); rect.area()];
        self.for_each_span(rect, |coord, tile_index, out_index, len| {
            if let Some(tile) = self.tiles.get(&coord) {
                out[out_index..out_index + len]
                    .copy_from_slice(&tile.pixels()[tile_index..tile_index + len]);
            }
        });
        out
    }

    /// Writes row-major `pixels` into `rect`.
    ///
    /// # Panics
    /// If `pixels.len()` is not `rect.area()`.
    pub fn write_rect(&mut self, rect: Rect, pixels: &[Pixel<C>]) {
        assert_eq!(
            pixels.len(),
            rect.area(),
            "pixel count must match the rectangle"
        );
        let zero = transparent::<C>();
        let mut spans = Vec::new();
        self.for_each_span(rect, |coord, tile_index, src_index, len| {
            spans.push((coord, tile_index, src_index, len));
        });
        for (coord, tile_index, src_index, len) in spans {
            let src = &pixels[src_index..src_index + len];
            if !self.tiles.contains_key(&coord) && src.iter().all(|p| *p == zero) {
                continue;
            }
            self.tile_mut(coord).pixels_mut()[tile_index..tile_index + len].copy_from_slice(src);
        }
    }

    /// Drops tiles that have become fully transparent.
    pub fn compact(&mut self) {
        self.tiles.retain(|_, tile| !tile.is_transparent());
    }

    /// Calls `f(tile, index_in_tile, index_in_rect, len)` for every run of
    /// pixels in `rect` that lies within a single tile row.
    fn for_each_span(&self, rect: Rect, mut f: impl FnMut(TileCoord, usize, usize, usize)) {
        if rect.is_empty() {
            return;
        }
        let size = TILE_SIZE as i64;
        for row in 0..rect.h as i64 {
            let y = rect.y as i64 + row;
            let mut x = rect.x as i64;
            while x < rect.right() {
                let (coord, tile_index) = TileCoord::of_pixel(x as i32, y as i32);
                let tile_right = (coord.tx as i64 + 1) * size;
                let len = (tile_right.min(rect.right()) - x) as usize;
                let rect_index = row as usize * rect.w as usize + (x - rect.x as i64) as usize;
                f(coord, tile_index, rect_index, len);
                x += len as i64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pixels_are_transparent() {
        let r: Raster<u8> = Raster::new();
        assert_eq!(r.pixel(12345, -6789), [0, 0, 0, 0]);
        assert!(r.is_empty());
        assert_eq!(r.bounds(), None);
    }

    #[test]
    fn set_and_get_across_tiles_and_negative_coordinates() {
        let mut r: Raster<u8> = Raster::new();
        r.set_pixel(0, 0, [1, 1, 1, 1]);
        r.set_pixel(255, 255, [2, 2, 2, 2]);
        r.set_pixel(256, 0, [3, 3, 3, 3]);
        r.set_pixel(-1, -1, [4, 4, 4, 4]);
        assert_eq!(r.pixel(0, 0), [1, 1, 1, 1]);
        assert_eq!(r.pixel(255, 255), [2, 2, 2, 2]);
        assert_eq!(r.pixel(256, 0), [3, 3, 3, 3]);
        assert_eq!(r.pixel(-1, -1), [4, 4, 4, 4]);
        assert_eq!(r.tile_count(), 3);
        assert_eq!(r.bounds(), Some(Rect::new(-256, -256, 768, 512)));
    }

    #[test]
    fn writing_transparency_does_not_allocate() {
        let mut r: Raster<u8> = Raster::new();
        r.set_pixel(10, 10, [0, 0, 0, 0]);
        r.write_rect(Rect::new(-300, -300, 600, 600), &vec![[0; 4]; 600 * 600]);
        assert!(r.is_empty());
    }

    #[test]
    fn rect_round_trip_spanning_four_tiles() {
        let rect = Rect::new(-20, 240, 50, 40);
        let pixels: Vec<Pixel<u16>> = (0..rect.area())
            .map(|i| {
                let v = (i % 60000) as u16 + 1;
                [v, v / 2, v / 3, u16::MAX]
            })
            .collect();
        let mut r: Raster<u16> = Raster::new();
        r.write_rect(rect, &pixels);
        assert_eq!(r.tile_count(), 4);
        assert_eq!(r.read_rect(rect), pixels);
        // Spot-check individual pixels against the row-major layout.
        assert_eq!(r.pixel(-20, 240), pixels[0]);
        assert_eq!(r.pixel(29, 279), pixels[rect.area() - 1]);
        assert_eq!(r.pixel(0, 256), pixels[16 * 50 + 20]);
        // Just outside the rectangle is untouched.
        assert_eq!(r.pixel(-21, 240), [0; 4]);
        assert_eq!(r.pixel(30, 279), [0; 4]);
    }

    #[test]
    fn clone_is_copy_on_write_per_tile() {
        let mut a: Raster<u8> = Raster::new();
        a.set_pixel(0, 0, [5, 5, 5, 5]);
        a.set_pixel(300, 0, [6, 6, 6, 6]);
        let snapshot = a.clone();
        a.set_pixel(1, 0, [7, 7, 7, 7]);

        let t0 = TileCoord::new(0, 0);
        let t1 = TileCoord::new(1, 0);
        assert!(!a
            .tile(t0)
            .unwrap()
            .shares_data_with(snapshot.tile(t0).unwrap()));
        assert!(a
            .tile(t1)
            .unwrap()
            .shares_data_with(snapshot.tile(t1).unwrap()));
        assert_eq!(snapshot.pixel(1, 0), [0; 4]);
        assert_eq!(a.pixel(1, 0), [7, 7, 7, 7]);
    }

    #[test]
    fn compact_drops_emptied_tiles() {
        let mut r: Raster<f32> = Raster::new();
        r.set_pixel(0, 0, [0.5, 0.5, 0.5, 1.0]);
        r.set_pixel(1000, 1000, [0.1, 0.1, 0.1, 0.2]);
        r.set_pixel(0, 0, [0.0; 4]);
        assert_eq!(r.tile_count(), 2);
        r.compact();
        assert_eq!(r.tile_count(), 1);
        assert_eq!(r.pixel(1000, 1000), [0.1, 0.1, 0.1, 0.2]);
    }
}
