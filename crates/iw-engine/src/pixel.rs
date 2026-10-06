//! Pixel formats.
//!
//! Every raster in the engine is RGBA with **premultiplied alpha**, generic
//! over the channel type. Three channel types exist: 8-bit, 16-bit and
//! 32-bit float. Integer channels are normalised (0 = 0.0, max = 1.0).
//! Float channels are stored as-is and may exceed 1.0.

use std::fmt::Debug;

/// Bits per channel of a document or raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BitDepth {
    U8,
    U16,
    F32,
}

/// A channel sample type.
pub trait Channel: Copy + Default + PartialEq + Debug + Send + Sync + 'static {
    const DEPTH: BitDepth;
    /// The value representing 0.0 (also the default).
    const ZERO: Self;
    /// The value representing 1.0.
    const ONE: Self;

    /// Normalised value. Integers map to `0.0..=1.0`; floats pass through.
    fn to_f32(self) -> f32;

    /// Inverse of [`Channel::to_f32`]. Integers clamp and round to nearest;
    /// floats pass through, except NaN becomes 0.
    fn from_f32(v: f32) -> Self;
}

impl Channel for u8 {
    const DEPTH: BitDepth = BitDepth::U8;
    const ZERO: Self = 0;
    const ONE: Self = u8::MAX;

    #[inline]
    fn to_f32(self) -> f32 {
        self as f32 / 255.0
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        // `as` saturates and maps NaN to 0, so no explicit clamp is needed.
        (v * 255.0 + 0.5) as u8
    }
}

impl Channel for u16 {
    const DEPTH: BitDepth = BitDepth::U16;
    const ZERO: Self = 0;
    const ONE: Self = u16::MAX;

    #[inline]
    fn to_f32(self) -> f32 {
        self as f32 / 65535.0
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        (v * 65535.0 + 0.5) as u16
    }
}

impl Channel for f32 {
    const DEPTH: BitDepth = BitDepth::F32;
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;

    #[inline]
    fn to_f32(self) -> f32 {
        self
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        if v.is_nan() {
            0.0
        } else {
            v
        }
    }
}

/// One premultiplied RGBA pixel: `[r, g, b, a]`.
pub type Pixel<C> = [C; 4];

/// Anything a tile can hold: an RGBA [`Pixel`], or a single [`Channel`]
/// sample for masks. The default value is the "empty" value a missing tile
/// reads as.
pub trait Texel: Copy + Default + PartialEq + Debug + Send + Sync + 'static {}

impl<T: Copy + Default + PartialEq + Debug + Send + Sync + 'static> Texel for T {}

/// One value per supported bit depth. Documents choose their depth at run
/// time, while rasters and the compositor are generic over it; this is the
/// bridge between the two.
#[derive(Clone, Debug, PartialEq)]
pub enum ByDepth<A, B, C> {
    U8(A),
    U16(B),
    F32(C),
}

impl<A, B, C> ByDepth<A, B, C> {
    pub fn depth(&self) -> BitDepth {
        match self {
            ByDepth::U8(_) => BitDepth::U8,
            ByDepth::U16(_) => BitDepth::U16,
            ByDepth::F32(_) => BitDepth::F32,
        }
    }
}

/// Evaluates `$body` with `$x` bound to the payload of whichever variant
/// `$value` holds. The body must type-check for all three channel types.
#[macro_export]
macro_rules! by_depth {
    ($value:expr, $x:ident => $body:expr) => {
        match $value {
            $crate::pixel::ByDepth::U8($x) => $body,
            $crate::pixel::ByDepth::U16($x) => $body,
            $crate::pixel::ByDepth::F32($x) => $body,
        }
    };
}

/// Like [`by_depth!`], but wraps each result back in the same variant.
#[macro_export]
macro_rules! map_depth {
    ($value:expr, $x:ident => $body:expr) => {
        match $value {
            $crate::pixel::ByDepth::U8($x) => $crate::pixel::ByDepth::U8($body),
            $crate::pixel::ByDepth::U16($x) => $crate::pixel::ByDepth::U16($body),
            $crate::pixel::ByDepth::F32($x) => $crate::pixel::ByDepth::F32($body),
        }
    };
}

/// The fully transparent pixel.
#[inline]
pub fn transparent<C: Channel>() -> Pixel<C> {
    [C::ZERO; 4]
}

/// Converts a pixel to normalised floats.
#[inline]
pub fn to_f32<C: Channel>(p: Pixel<C>) -> [f32; 4] {
    [p[0].to_f32(), p[1].to_f32(), p[2].to_f32(), p[3].to_f32()]
}

/// Converts normalised floats to a pixel.
#[inline]
pub fn from_f32<C: Channel>(p: [f32; 4]) -> Pixel<C> {
    [
        C::from_f32(p[0]),
        C::from_f32(p[1]),
        C::from_f32(p[2]),
        C::from_f32(p[3]),
    ]
}

/// Builds a premultiplied pixel from straight (unassociated) normalised RGBA.
#[inline]
pub fn from_straight<C: Channel>(rgba: [f32; 4]) -> Pixel<C> {
    let a = rgba[3];
    from_f32([rgba[0] * a, rgba[1] * a, rgba[2] * a, a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_round_trips_are_exact() {
        for v in 0..=u8::MAX {
            assert_eq!(u8::from_f32(v.to_f32()), v);
        }
        for v in 0..=u16::MAX {
            assert_eq!(u16::from_f32(v.to_f32()), v);
        }
    }

    #[test]
    fn integers_clamp_and_floats_pass_through() {
        assert_eq!(u8::from_f32(-0.5), 0);
        assert_eq!(u8::from_f32(7.0), 255);
        assert_eq!(u16::from_f32(f32::NAN), 0);
        assert_eq!(f32::from_f32(3.5), 3.5);
        assert_eq!(f32::from_f32(f32::NAN), 0.0);
    }

    #[test]
    fn from_straight_premultiplies() {
        let p: Pixel<u8> = from_straight([1.0, 0.5, 0.0, 0.5]);
        assert_eq!(p, [128, 64, 0, 128]);
    }
}
