//! JPEG import and export.
//!
//! Import produces an 8-bit, one-layer, opaque document. The EXIF
//! orientation is applied, so photos open the right way up. An embedded
//! ICC profile and the JFIF pixel density are kept.
//!
//! JPEG has no transparency: export flattens the document onto white.

use super::png::{check_import_size, single_layer_document};
use super::{malformed, FormatError};
use crate::document::Document;
use crate::geom::Rect;
use crate::pixel::{ByDepth, Pixel};
use crate::raster::Raster;
use image::codecs::jpeg::{JpegDecoder, JpegEncoder, PixelDensity};
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder};
use std::io::{Cursor, Write};

/// Largest width or height the JPEG format can hold.
const MAX_JPEG_DIMENSION: u32 = 65_535;

fn image_err(e: image::ImageError) -> FormatError {
    match e {
        image::ImageError::IoError(e) => FormatError::Io(e),
        image::ImageError::Unsupported(e) => FormatError::Unsupported(e.to_string()),
        image::ImageError::Limits(e) => FormatError::Unsupported(e.to_string()),
        other => malformed(other.to_string()),
    }
}

/// Pixels per inch from a JFIF header, if the file starts with one and it
/// gives a real density rather than only an aspect ratio.
fn jfif_ppi(bytes: &[u8]) -> Option<f64> {
    let header = bytes.get(..18)?;
    if header[..4] != [0xFF, 0xD8, 0xFF, 0xE0] || &header[6..11] != b"JFIF\0" {
        return None;
    }
    let density = f64::from(u16::from_be_bytes([header[14], header[15]]));
    match header[13] {
        1 => Some(density),
        2 => Some(density * 2.54),
        _ => None,
    }
    .filter(|ppi| *ppi > 0.0)
}

/// Reads a JPEG.
pub fn read(bytes: &[u8]) -> Result<Document, FormatError> {
    let mut decoder = JpegDecoder::new(Cursor::new(bytes)).map_err(image_err)?;
    let (width, height) = decoder.dimensions();
    check_import_size(width, height)?;
    let icc = decoder.icc_profile().ok().flatten();
    // A missing or unreadable orientation means "as stored".
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);

    let mut image = DynamicImage::from_decoder(decoder).map_err(image_err)?;
    image.apply_orientation(orientation);
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();

    let pixels: Vec<Pixel<u8>> = rgb.pixels().map(|p| [p[0], p[1], p[2], 255]).collect();
    let mut raster = Raster::new();
    raster.write_rect(Rect::new(0, 0, width, height), &pixels);
    single_layer_document(width, height, ByDepth::U8(raster), jfif_ppi(bytes), icc)
}

/// Writes the flattened document as a JPEG over a white background.
/// `quality` runs from 1 (smallest file) to 100 (best).
pub fn write<W: Write>(document: &Document, out: W, quality: u8) -> Result<(), FormatError> {
    let image = document.flatten();
    if image.width > MAX_JPEG_DIMENSION || image.height > MAX_JPEG_DIMENSION {
        return Err(FormatError::Unsupported(format!(
            "JPEG cannot hold an image of {} x {} pixels (the limit is {MAX_JPEG_DIMENSION} per side)",
            image.width, image.height
        )));
    }

    // Premultiplied colour over white is `c + (1 - a)`.
    let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    match &image.pixels {
        ByDepth::U8(px) => {
            for p in px {
                let gap = 255 - u32::from(p[3]);
                rgb.extend(p[..3].iter().map(|c| (u32::from(*c) + gap).min(255) as u8));
            }
        }
        ByDepth::U16(px) => {
            for p in px {
                let gap = 65535 - u64::from(p[3]);
                rgb.extend(p[..3].iter().map(|c| {
                    let v = (u64::from(*c) + gap).min(65535);
                    ((v * 255 + 32767) / 65535) as u8
                }));
            }
        }
        ByDepth::F32(px) => {
            for p in px {
                let gap = 1.0 - p[3].clamp(0.0, 1.0);
                rgb.extend(
                    p[..3]
                        .iter()
                        .map(|c| ((c + gap).clamp(0.0, 1.0) * 255.0 + 0.5) as u8),
                );
            }
        }
    }

    let mut encoder = JpegEncoder::new_with_quality(out, quality.clamp(1, 100));
    let ppi = document.resolution().ppi.round();
    if (1.0..=65535.0).contains(&ppi) {
        encoder.set_pixel_density(PixelDensity::dpi(ppi as u16));
    }
    if let Some(profile) = document.profile() {
        // The encoder accepts any profile; an error would mean it cannot
        // embed one at all, which is not worth failing the export for.
        let _ = encoder.set_icc_profile(profile.icc.clone());
    }
    encoder
        .write_image(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
        .map_err(image_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ColorProfile, Resolution};
    use crate::pixel::BitDepth;

    fn doc_from(width: u32, height: u32, f: impl Fn(u32, u32) -> Pixel<u8>) -> Document {
        let pixels: Vec<Pixel<u8>> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        let mut raster = Raster::new();
        raster.write_rect(Rect::new(0, 0, width, height), &pixels);
        single_layer_document(width, height, ByDepth::U8(raster), None, None).unwrap()
    }

    fn pixels(doc: &Document) -> Vec<Pixel<u8>> {
        match doc.flatten().pixels {
            ByDepth::U8(p) => p,
            _ => panic!("expected 8-bit"),
        }
    }

    fn mean_abs_diff(a: &[Pixel<u8>], b: &[Pixel<u8>]) -> f64 {
        let total: u64 = a
            .iter()
            .zip(b)
            .flat_map(|(p, q)| p.iter().zip(q))
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        total as f64 / (a.len() * 4) as f64
    }

    #[test]
    fn round_trip_is_close_at_high_quality() {
        // A smooth gradient, which JPEG handles well.
        let doc = doc_from(64, 48, |x, y| [(x * 4) as u8, (y * 5) as u8, 128, 255]);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes, 95).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(
            (back.width(), back.height(), back.bit_depth()),
            (64, 48, BitDepth::U8)
        );
        let diff = mean_abs_diff(&pixels(&doc), &pixels(&back));
        assert!(
            diff < 2.0,
            "mean difference {diff} is too large for quality 95"
        );
        assert!(
            pixels(&back).iter().all(|p| p[3] == 255),
            "JPEG import must be opaque"
        );
    }

    #[test]
    fn lower_quality_makes_a_smaller_file() {
        let doc = doc_from(96, 96, |x, y| {
            [
                ((x * 37 + y * 11) % 256) as u8,
                ((x * 5) ^ (y * 9)) as u8,
                (y * 2) as u8,
                255,
            ]
        });
        let (mut high, mut low) = (Vec::new(), Vec::new());
        write(&doc, &mut high, 95).unwrap();
        write(&doc, &mut low, 20).unwrap();
        assert!(low.len() < high.len());
    }

    #[test]
    fn transparency_is_flattened_onto_white() {
        // Left half transparent, right half half-transparent black.
        let doc = doc_from(
            32,
            32,
            |x, _| if x < 16 { [0, 0, 0, 0] } else { [0, 0, 0, 128] },
        );
        let mut bytes = Vec::new();
        write(&doc, &mut bytes, 100).unwrap();
        let back = pixels(&read(&bytes).unwrap());
        let at = |x: usize, y: usize| back[y * 32 + x];
        for c in &at(4, 16)[..3] {
            assert!(*c >= 250, "transparent should become white, got {c}");
        }
        for c in &at(28, 16)[..3] {
            assert!(
                (120..=135).contains(c),
                "half-transparent black over white should be mid grey, got {c}"
            );
        }
    }

    #[test]
    fn resolution_and_profile_survive() {
        let mut doc = doc_from(8, 8, |_, _| [100, 150, 200, 255]);
        doc.set_resolution(Resolution { ppi: 300.0 }).unwrap();
        doc.set_profile(Some(ColorProfile {
            name: "Any".into(),
            icc: vec![9; 500],
        }));
        let mut bytes = Vec::new();
        write(&doc, &mut bytes, 90).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.resolution().ppi, 300.0);
        assert_eq!(back.profile().unwrap().icc, vec![9; 500]);
    }

    #[test]
    fn jfif_density_units() {
        let mut header = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 1, 0, 150, 0, 150,
        ];
        assert_eq!(jfif_ppi(&header), Some(150.0));
        header[13] = 2; // dots per centimetre
        assert_eq!(jfif_ppi(&header), Some(381.0));
        header[13] = 0; // aspect ratio only
        assert_eq!(jfif_ppi(&header), None);
        assert_eq!(jfif_ppi(&header[..10]), None);
        assert_eq!(jfif_ppi(b"not a jpeg at all, just text"), None);
    }

    /// A camera stores a portrait photo sideways and records how to turn it.
    #[test]
    fn exif_orientation_is_applied() {
        // Stored image: 64 wide, 32 tall, left half red, right half blue.
        let mut rgb = Vec::new();
        for _y in 0..32 {
            for x in 0..64 {
                rgb.extend_from_slice(if x < 32 { &[255, 0, 0] } else { &[0, 0, 255] });
            }
        }
        // Minimal EXIF: little-endian TIFF, one IFD entry, Orientation = 6
        // ("rotate 90 degrees clockwise to display").
        let mut exif = Vec::new();
        exif.extend_from_slice(b"II*\0");
        exif.extend_from_slice(&8u32.to_le_bytes());
        exif.extend_from_slice(&1u16.to_le_bytes());
        exif.extend_from_slice(&0x0112u16.to_le_bytes());
        exif.extend_from_slice(&3u16.to_le_bytes());
        exif.extend_from_slice(&1u32.to_le_bytes());
        exif.extend_from_slice(&6u16.to_le_bytes());
        exif.extend_from_slice(&0u16.to_le_bytes());
        exif.extend_from_slice(&0u32.to_le_bytes());

        let mut bytes = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 100);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(&rgb, 64, 32, ExtendedColorType::Rgb8)
            .unwrap();

        let doc = read(&bytes).unwrap();
        assert_eq!(
            (doc.width(), doc.height()),
            (32, 64),
            "rotated image should be portrait"
        );
        let px = pixels(&doc);
        let at = |x: usize, y: usize| px[y * 32 + x];
        // Rotating clockwise moves the left (red) half to the top.
        let top = at(16, 8);
        let bottom = at(16, 56);
        assert!(
            top[0] > 200 && top[2] < 60,
            "top should be red, got {top:?}"
        );
        assert!(
            bottom[2] > 200 && bottom[0] < 60,
            "bottom should be blue, got {bottom:?}"
        );
    }

    #[test]
    fn oversized_documents_are_refused() {
        let doc = Document::new(70_000, 1, BitDepth::U8).unwrap();
        assert!(matches!(
            write(&doc, Vec::new(), 90),
            Err(FormatError::Unsupported(_))
        ));
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        let doc = doc_from(16, 16, |x, y| [(x * 16) as u8, (y * 16) as u8, 0, 255]);
        let mut bytes = Vec::new();
        write(&doc, &mut bytes, 90).unwrap();
        for cut in [0, 2, 3, 10, 40] {
            assert!(read(&bytes[..cut]).is_err(), "truncated at {cut}");
        }
        assert!(
            read(&[0xFF, 0xD8, 0xFF, 0xD9]).is_err(),
            "a JPEG with no image data"
        );
    }
}
