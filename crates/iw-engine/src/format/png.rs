//! PNG import and export.
//!
//! Import accepts every PNG colour type and bit depth and produces a
//! one-layer document: 16-bit files become 16-bit documents, everything
//! else 8-bit. An embedded ICC profile and the pixel density are kept.
//!
//! PNG stores straight alpha and the engine stores premultiplied, so a
//! colour under very low alpha is rounded on import. What is seen when the
//! image is composited is unaffected.

use super::{malformed, FormatError, MAX_IMPORT_PIXELS};
use crate::document::{ColorProfile, Document, Layer, PixelData, Resolution};
use crate::geom::Rect;
use crate::pixel::{ByDepth, Pixel};
use crate::raster::Raster;
use std::borrow::Cow;
use std::io::{Cursor, Write};

const METRES_PER_INCH: f64 = 0.0254;

/// `c * a / max`, rounded to nearest, in integers so it is exact.
fn premultiply(c: u64, a: u64, max: u64) -> u64 {
    (c * a + max / 2) / max
}

/// Inverse of [`premultiply`], clamped. Transparent pixels become black.
fn unpremultiply(c: u64, a: u64, max: u64) -> u64 {
    (c * max + a / 2).checked_div(a).map_or(0, |v| v.min(max))
}

/// Snaps a density that is a whole number of pixels per inch stored in
/// pixels per metre (72 ppi is stored as 2835 and reads back as 72.009).
fn tidy_ppi(ppi: f64) -> f64 {
    if (ppi - ppi.round()).abs() < 0.02 {
        ppi.round()
    } else {
        ppi
    }
}

pub(crate) fn single_layer_document(
    width: u32,
    height: u32,
    pixels: PixelData,
    ppi: Option<f64>,
    icc: Option<Vec<u8>>,
) -> Result<Document, FormatError> {
    let mut doc = Document::new(width, height, pixels.depth())?;
    if let Some(ppi) = ppi.filter(|p| p.is_finite() && *p > 0.0) {
        doc.set_resolution(Resolution { ppi })?;
    }
    if let Some(icc) = icc.filter(|icc| !icc.is_empty()) {
        doc.set_profile(Some(ColorProfile {
            name: "Embedded profile".into(),
            icc,
        }));
    }
    doc.insert_layer(None, 0, Layer::raster("Background", pixels))?;
    Ok(doc)
}

pub(crate) fn check_import_size(width: u32, height: u32) -> Result<(), FormatError> {
    if width == 0 || height == 0 {
        return Err(malformed("image has no pixels"));
    }
    if u64::from(width) * u64::from(height) > MAX_IMPORT_PIXELS {
        return Err(FormatError::Unsupported(format!(
            "image of {width} x {height} pixels is too large"
        )));
    }
    Ok(())
}

/// Reads a PNG.
pub fn read(bytes: &[u8]) -> Result<Document, FormatError> {
    let bad = |e: png::DecodingError| malformed(e.to_string());
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // Palettes become RGB, low bit depths become 8-bit, and a tRNS chunk
    // becomes an alpha channel.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(bad)?;

    let info = reader.info();
    let (width, height) = (info.width, info.height);
    check_import_size(width, height)?;
    let ppi = info.pixel_dims.and_then(|d| match d.unit {
        png::Unit::Meter => Some(tidy_ppi(f64::from(d.xppu) * METRES_PER_INCH)),
        png::Unit::Unspecified => None,
    });
    let icc = info.icc_profile.as_ref().map(|p| p.to_vec());

    let size = reader
        .output_buffer_size()
        .ok_or_else(|| malformed("image is too large to decode"))?;
    let mut buf = vec![0u8; size];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(frame.buffer_size());
    let (color, depth) = reader.output_color_type();

    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(malformed("palette was not expanded")),
    };
    let count = width as usize * height as usize;
    // Expands one decoded pixel's samples to straight RGBA.
    let rgba = |s: &[u64], max: u64| -> [u64; 4] {
        match channels {
            1 => [s[0], s[0], s[0], max],
            2 => [s[0], s[0], s[0], s[1]],
            3 => [s[0], s[1], s[2], max],
            _ => [s[0], s[1], s[2], s[3]],
        }
    };

    let pixels: PixelData = match depth {
        png::BitDepth::Eight => {
            if buf.len() != count * channels {
                return Err(malformed("decoded data is the wrong size"));
            }
            let mut out: Vec<Pixel<u8>> = Vec::with_capacity(count);
            let mut samples = [0u64; 4];
            for px in buf.chunks_exact(channels) {
                for (s, b) in samples.iter_mut().zip(px) {
                    *s = u64::from(*b);
                }
                let [r, g, b, a] = rgba(&samples, 255);
                let m = |c: u64| premultiply(c, a, 255) as u8;
                out.push([m(r), m(g), m(b), a as u8]);
            }
            let mut raster = Raster::new();
            raster.write_rect(Rect::new(0, 0, width, height), &out);
            ByDepth::U8(raster)
        }
        png::BitDepth::Sixteen => {
            if buf.len() != count * channels * 2 {
                return Err(malformed("decoded data is the wrong size"));
            }
            let mut out: Vec<Pixel<u16>> = Vec::with_capacity(count);
            let mut samples = [0u64; 4];
            for px in buf.chunks_exact(channels * 2) {
                // PNG samples are big-endian.
                for (s, b) in samples.iter_mut().zip(px.chunks_exact(2)) {
                    *s = u64::from(u16::from_be_bytes([b[0], b[1]]));
                }
                let [r, g, b, a] = rgba(&samples, 65535);
                let m = |c: u64| premultiply(c, a, 65535) as u16;
                out.push([m(r), m(g), m(b), a as u16]);
            }
            let mut raster = Raster::new();
            raster.write_rect(Rect::new(0, 0, width, height), &out);
            ByDepth::U16(raster)
        }
        other => {
            return Err(malformed(format!(
                "unexpected bit depth {other:?} after expansion"
            )))
        }
    };

    single_layer_document(width, height, pixels, ppi, icc)
}

/// Writes the flattened document as a PNG.
///
/// 8-bit documents are written as 8-bit; 16-bit and float documents as
/// 16-bit (float values are clamped to 0..1). The alpha channel is left
/// out when every pixel is opaque.
pub fn write<W: Write>(document: &Document, out: W) -> Result<(), FormatError> {
    let bad = |e: png::EncodingError| match e {
        png::EncodingError::IoError(e) => FormatError::Io(e),
        other => malformed(other.to_string()),
    };
    let image = document.flatten();

    // Straight RGBA samples, as wide as the output depth.
    let (straight, max, sixteen): (Vec<[u64; 4]>, u64, bool) = match &image.pixels {
        ByDepth::U8(px) => {
            let f = |p: &Pixel<u8>| {
                let a = u64::from(p[3]);
                let c = |v: u8| unpremultiply(u64::from(v), a, 255);
                [c(p[0]), c(p[1]), c(p[2]), a]
            };
            (px.iter().map(f).collect(), 255, false)
        }
        ByDepth::U16(px) => {
            let f = |p: &Pixel<u16>| {
                let a = u64::from(p[3]);
                let c = |v: u16| unpremultiply(u64::from(v), a, 65535);
                [c(p[0]), c(p[1]), c(p[2]), a]
            };
            (px.iter().map(f).collect(), 65535, true)
        }
        ByDepth::F32(px) => {
            let f = |p: &Pixel<f32>| {
                let a = p[3].clamp(0.0, 1.0);
                let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u64;
                let c = |v: f32| if a > 0.0 { q(v / a) } else { 0 };
                [c(p[0]), c(p[1]), c(p[2]), q(a)]
            };
            (px.iter().map(f).collect(), 65535, true)
        }
    };
    let opaque = straight.iter().all(|p| p[3] == max);
    let channels = if opaque { 3 } else { 4 };
    let mut data = Vec::with_capacity(straight.len() * channels * if sixteen { 2 } else { 1 });
    for p in &straight {
        for &v in &p[..channels] {
            if sixteen {
                data.extend_from_slice(&(v as u16).to_be_bytes());
            } else {
                data.push(v as u8);
            }
        }
    }

    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = if opaque {
        png::ColorType::Rgb
    } else {
        png::ColorType::Rgba
    };
    info.bit_depth = if sixteen {
        png::BitDepth::Sixteen
    } else {
        png::BitDepth::Eight
    };
    let per_metre = (document.resolution().ppi / METRES_PER_INCH).round();
    if per_metre >= 1.0 && per_metre <= f64::from(u32::MAX) {
        let ppu = per_metre as u32;
        info.pixel_dims = Some(png::PixelDimensions {
            xppu: ppu,
            yppu: ppu,
            unit: png::Unit::Meter,
        });
    }
    if let Some(profile) = document.profile() {
        info.icc_profile = Some(Cow::Borrowed(&profile.icc));
    }
    let encoder = png::Encoder::with_info(out, info).map_err(bad)?;
    let mut writer = encoder.write_header().map_err(bad)?;
    writer.write_image_data(&data).map_err(bad)?;
    writer.finish().map_err(bad)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerKind;
    use crate::pixel::BitDepth;

    fn encode(info: png::Info<'_>, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        bytes
    }

    fn pixels_u8(doc: &Document) -> Vec<Pixel<u8>> {
        match doc.flatten().pixels {
            ByDepth::U8(p) => p,
            other => panic!("expected 8-bit, got {:?}", other.depth()),
        }
    }

    fn pixels_u16(doc: &Document) -> Vec<Pixel<u16>> {
        match doc.flatten().pixels {
            ByDepth::U16(p) => p,
            other => panic!("expected 16-bit, got {:?}", other.depth()),
        }
    }

    fn doc_from_u8(width: u32, height: u32, pixels: &[Pixel<u8>]) -> Document {
        let mut raster = Raster::new();
        raster.write_rect(Rect::new(0, 0, width, height), pixels);
        single_layer_document(width, height, ByDepth::U8(raster), None, None).unwrap()
    }

    #[test]
    fn integer_alpha_math_is_exact() {
        assert_eq!(premultiply(255, 128, 255), 128);
        assert_eq!(premultiply(128, 128, 255), 64);
        assert_eq!(premultiply(200, 0, 255), 0);
        assert_eq!(unpremultiply(64, 128, 255), 128);
        assert_eq!(unpremultiply(0, 0, 255), 0);
        assert_eq!(
            unpremultiply(300, 128, 255),
            255,
            "invalid premultiplied data is clamped"
        );
        // Every premultiplied value survives the trip out to straight alpha and back.
        for a in 1..=255u64 {
            for c in 0..=a {
                assert_eq!(
                    premultiply(unpremultiply(c, a, 255), a, 255),
                    c,
                    "c={c} a={a}"
                );
            }
        }
    }

    #[test]
    fn opaque_8_bit_round_trip_is_exact_and_omits_alpha() {
        let src: Vec<Pixel<u8>> = (0..12u8)
            .map(|i| [i * 20, 255 - i * 20, i * 7, 255])
            .collect();
        let doc = doc_from_u8(4, 3, &src);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();

        let reader = png::Decoder::new(Cursor::new(&bytes)).read_info().unwrap();
        assert_eq!(
            reader.output_color_type(),
            (png::ColorType::Rgb, png::BitDepth::Eight)
        );

        let back = read(&bytes).unwrap();
        assert_eq!(
            (back.width(), back.height(), back.bit_depth()),
            (4, 3, BitDepth::U8)
        );
        assert_eq!(pixels_u8(&back), src);
        assert_eq!(back.layers().len(), 1);
        assert_eq!(back.layers()[0].name, "Background");
    }

    #[test]
    fn transparency_round_trips_through_straight_alpha() {
        // Premultiplied pixels at several alphas, including fully transparent.
        let src: Vec<Pixel<u8>> = vec![
            [0, 0, 0, 0],
            [10, 5, 0, 10],
            [64, 32, 100, 128],
            [200, 100, 50, 255],
        ];
        let doc = doc_from_u8(2, 2, &src);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let reader = png::Decoder::new(Cursor::new(&bytes)).read_info().unwrap();
        assert_eq!(reader.output_color_type().0, png::ColorType::Rgba);
        assert_eq!(pixels_u8(&read(&bytes).unwrap()), src);
    }

    #[test]
    fn straight_alpha_input_is_premultiplied() {
        let mut info = png::Info::with_size(2, 1);
        info.color_type = png::ColorType::Rgba;
        let bytes = encode(info, &[255, 128, 0, 128, 40, 80, 120, 0]);
        let doc = read(&bytes).unwrap();
        assert_eq!(pixels_u8(&doc), [[128, 64, 0, 128], [0, 0, 0, 0]]);
    }

    #[test]
    fn sixteen_bit_round_trip_is_exact() {
        let src: Vec<Pixel<u16>> = vec![
            [65535, 0, 1234, 65535],
            [100, 200, 300, 65535],
            [0, 0, 0, 0],
            [20000, 10000, 5, 40000],
        ];
        let mut raster = Raster::new();
        raster.write_rect(Rect::new(0, 0, 2, 2), &src);
        let doc = single_layer_document(2, 2, ByDepth::U16(raster), None, None).unwrap();
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.bit_depth(), BitDepth::U16);
        assert_eq!(pixels_u16(&back), src);
    }

    #[test]
    fn float_documents_export_as_16_bit_with_clamping() {
        let src: Vec<Pixel<f32>> = vec![[4.0, 0.5, 0.0, 1.0], [0.25, 0.25, 0.25, 0.5]];
        let mut raster = Raster::new();
        raster.write_rect(Rect::new(0, 0, 2, 1), &src);
        let doc = single_layer_document(2, 1, ByDepth::F32(raster), None, None).unwrap();
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let back = read(&bytes).unwrap();
        // 4.0 clamps to 1.0; straight 0.5 grey at half alpha comes back premultiplied.
        assert_eq!(
            pixels_u16(&back),
            [[65535, 32768, 0, 65535], [16384, 16384, 16384, 32768]]
        );
    }

    #[test]
    fn palette_with_transparency_is_expanded() {
        let mut info = png::Info::with_size(3, 1);
        info.color_type = png::ColorType::Indexed;
        info.bit_depth = png::BitDepth::Two;
        info.palette = Some(Cow::Borrowed(&[255, 0, 0, 0, 255, 0, 0, 0, 255]));
        info.trns = Some(Cow::Borrowed(&[255, 128]));
        // Indices 0, 1, 2 packed two bits each: 00 01 10 00.
        let bytes = encode(info, &[0b0001_1000]);
        let doc = read(&bytes).unwrap();
        assert_eq!(
            pixels_u8(&doc),
            [[255, 0, 0, 255], [0, 128, 0, 128], [0, 0, 255, 255]]
        );
    }

    #[test]
    fn low_bit_depth_greyscale_is_expanded() {
        let mut info = png::Info::with_size(4, 1);
        info.color_type = png::ColorType::Grayscale;
        info.bit_depth = png::BitDepth::One;
        let bytes = encode(info, &[0b1010_0000]);
        let doc = read(&bytes).unwrap();
        let w = [255, 255, 255, 255];
        let k = [0, 0, 0, 255];
        assert_eq!(pixels_u8(&doc), [w, k, w, k]);
    }

    #[test]
    fn sixteen_bit_grey_alpha_is_read() {
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::GrayscaleAlpha;
        info.bit_depth = png::BitDepth::Sixteen;
        let bytes = encode(info, &[0x80, 0x00, 0xFF, 0xFF]);
        assert_eq!(
            pixels_u16(&read(&bytes).unwrap()),
            [[0x8000, 0x8000, 0x8000, 0xFFFF]]
        );
    }

    #[test]
    fn resolution_and_profile_survive() {
        let mut doc = doc_from_u8(1, 1, &[[1, 2, 3, 255]]);
        doc.set_resolution(Resolution { ppi: 300.0 }).unwrap();
        doc.set_profile(Some(ColorProfile {
            name: "Anything".into(),
            icc: vec![7; 200],
        }));
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.resolution().ppi, 300.0);
        assert_eq!(back.profile().unwrap().icc, vec![7; 200]);

        // The default 72 ppi also comes back as exactly 72.
        let doc = doc_from_u8(1, 1, &[[1, 2, 3, 255]]);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.resolution().ppi, 72.0);
        assert!(back.profile().is_none());
    }

    #[test]
    fn export_flattens_layers_and_crops_to_the_canvas() {
        let mut doc = doc_from_u8(2, 2, &[[10, 10, 10, 255]; 4]);
        let mut top = Raster::new();
        top.write_rect(Rect::new(0, 0, 3, 1), &[[200u8, 0, 0, 255]; 3]);
        let mut layer = Layer::raster("Top", ByDepth::U8(top));
        if let LayerKind::Raster { offset, .. } = &mut layer.kind {
            *offset = (1, 1);
        }
        doc.insert_layer(None, 1, layer).unwrap();
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (2, 2));
        let g = [10, 10, 10, 255];
        assert_eq!(pixels_u8(&back), [g, g, g, [200, 0, 0, 255]]);
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        let doc = doc_from_u8(8, 8, &[[1, 2, 3, 255]; 64]);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes).unwrap();
        // Cutting off only the final end-of-file marker is tolerated by the
        // decoder, since every pixel is present; anything earlier is not.
        for cut in [0, 7, 8, 20, 33, bytes.len() / 2] {
            assert!(read(&bytes[..cut]).is_err(), "truncated at {cut}");
        }
        let mut flipped = bytes.clone();
        let idat = flipped.windows(4).position(|w| w == b"IDAT").unwrap();
        flipped[idat + 6] ^= 0xFF;
        assert!(
            read(&flipped).is_err(),
            "corrupted data must fail its checksum"
        );
    }
}
