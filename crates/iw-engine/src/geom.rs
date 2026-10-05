//! Integer geometry in document pixel coordinates.

/// An axis-aligned rectangle. `x`/`y` may be negative: layers can extend
/// past the canvas origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// One past the right edge. `i64` so it cannot overflow.
    pub const fn right(&self) -> i64 {
        self.x as i64 + self.w as i64
    }

    /// One past the bottom edge.
    pub const fn bottom(&self) -> i64 {
        self.y as i64 + self.h as i64
    }

    pub const fn area(&self) -> usize {
        self.w as usize * self.h as usize
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        (x as i64) >= self.x as i64
            && (x as i64) < self.right()
            && (y as i64) >= self.y as i64
            && (y as i64) < self.bottom()
    }

    /// Smallest rectangle covering both. An empty rectangle contributes nothing.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let r = self.right().max(other.right());
        let b = self.bottom().max(other.bottom());
        Rect::new(x, y, (r - x as i64) as u32, (b - y as i64) as u32)
    }

    /// Overlap of the two, or `None` if they do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r > x as i64 && b > y as i64 {
            Some(Rect::new(
                x,
                y,
                (r - x as i64) as u32,
                (b - y as i64) as u32,
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_and_intersect() {
        let a = Rect::new(-10, -10, 20, 20);
        let b = Rect::new(5, 0, 20, 5);
        assert_eq!(a.union(&b), Rect::new(-10, -10, 35, 20));
        assert_eq!(a.intersect(&b), Some(Rect::new(5, 0, 5, 5)));
        assert_eq!(a.intersect(&Rect::new(10, 10, 5, 5)), None);
        assert_eq!(a.union(&Rect::new(100, 100, 0, 0)), a);
    }

    #[test]
    fn contains_uses_half_open_edges() {
        let r = Rect::new(-2, 3, 4, 2);
        assert!(r.contains(-2, 3));
        assert!(r.contains(1, 4));
        assert!(!r.contains(2, 4));
        assert!(!r.contains(1, 5));
    }
}
