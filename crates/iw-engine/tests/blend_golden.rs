//! Golden-image tests: one stored image per blend mode.
//!
//! Each test composites a fixed 64x64 source over a fixed backdrop and
//! compares the result with `tests/golden/blend_<mode>.png`. The fixtures
//! cover the full range of both colours and include alpha ramps on both
//! layers.
//!
//! The PNGs hold the engine's premultiplied bytes unchanged, so they look
//! slightly dark at soft edges in an image viewer.
//!
//! To regenerate after an intentional change:
//!
//!     IW_BLESS=1 cargo test -p iw-engine --test blend_golden
//!
//! then review the image diff before committing.

use iw_engine::blend::BlendMode;
use iw_engine::compositor::{composite, Layer};
use iw_engine::geom::Rect;
use iw_engine::pixel::Pixel;
use iw_engine::raster::Raster;
use std::path::PathBuf;

const SIZE: u32 = 64;
/// Allowed difference per 8-bit channel.
const TOLERANCE: u8 = 1;

fn premultiply(rgb: [u32; 3], a: u32) -> Pixel<u8> {
    let m = |c: u32| ((c * a + 127) / 255) as u8;
    [m(rgb[0]), m(rgb[1]), m(rgb[2]), a as u8]
}

/// Integer-only so the fixtures are identical on every platform.
fn fixture(opaque: bool, f: impl Fn(u32, u32) -> ([u32; 3], u32)) -> Raster<u8> {
    let mut pixels = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (rgb, a) = f(x, y);
            pixels.push(premultiply(rgb, if opaque { 255 } else { a }));
        }
    }
    let mut raster = Raster::new();
    raster.write_rect(Rect::new(0, 0, SIZE, SIZE), &pixels);
    raster
}

/// Red rises left to right, green top to bottom; fades out on the right.
fn backdrop(opaque: bool) -> Raster<u8> {
    fixture(opaque, |x, y| {
        let a = if x < 48 { 255 } else { (63 - x) * 255 / 15 };
        ([x * 255 / 63, y * 255 / 63, 128], a)
    })
}

/// Red rises top to bottom, green falls left to right; fades out at the bottom.
fn source(opaque: bool) -> Raster<u8> {
    fixture(opaque, |x, y| {
        let a = if y < 48 { 255 } else { (63 - y) * 255 / 15 };
        ([y * 255 / 63, 255 - x * 255 / 63, (x + y) * 255 / 126], a)
    })
}

fn render(mode: BlendMode, opaque: bool) -> Vec<u8> {
    let (b, s) = (backdrop(opaque), source(opaque));
    let out = composite(&[Layer::new(&b), Layer::new(&s).with_blend(mode)]);
    out.read_rect(Rect::new(0, 0, SIZE, SIZE))
        .into_iter()
        .flatten()
        .collect()
}

fn file_name(prefix: &str, mode: BlendMode) -> String {
    format!(
        "{prefix}_{}.png",
        mode.name().to_lowercase().replace(' ', "_")
    )
}

fn write_png(path: &PathBuf, rgba: &[u8]) {
    let file =
        std::fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(rgba)
        .unwrap();
}

fn read_png(path: &PathBuf) -> Vec<u8> {
    let file = std::fs::File::open(path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run with IW_BLESS=1 to create it",
            path.display()
        )
    });
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().expect("golden image too large")];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (SIZE, SIZE));
    assert_eq!(
        (info.color_type, info.bit_depth),
        (png::ColorType::Rgba, png::BitDepth::Eight)
    );
    buf.truncate(info.buffer_size());
    buf
}

#[test]
fn blend_modes_match_golden_images() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let bless = std::env::var_os("IW_BLESS").is_some();
    let mut failures = Vec::new();

    for mode in BlendMode::ALL {
        let path = dir.join(file_name("blend", mode));
        let actual = render(mode, false);
        if bless {
            write_png(&path, &actual);
            continue;
        }
        let expected = read_png(&path);
        let worst = actual
            .iter()
            .zip(&expected)
            .map(|(a, e)| a.abs_diff(*e))
            .max()
            .unwrap();
        let over = actual
            .iter()
            .zip(&expected)
            .filter(|(a, e)| a.abs_diff(**e) > TOLERANCE)
            .count();
        if over > 0 {
            failures.push(format!(
                "{}: {over} channel values differ, worst by {worst}",
                mode.name()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatches:\n{}",
        failures.join("\n")
    );
}

/// Writes fully opaque renders for `scripts/crosscheck-blend-modes.py`,
/// which compares them with ImageMagick. Not part of the normal test run.
#[test]
#[ignore = "writes files; run via scripts/crosscheck-blend-modes.py"]
fn dump_opaque_renders_for_cross_check() {
    let dir = PathBuf::from(std::env::var_os("IW_DUMP_DIR").expect("set IW_DUMP_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let flat = |r: Raster<u8>| -> Vec<u8> {
        r.read_rect(Rect::new(0, 0, SIZE, SIZE))
            .into_iter()
            .flatten()
            .collect()
    };
    write_png(&dir.join("backdrop.png"), &flat(backdrop(true)));
    write_png(&dir.join("source.png"), &flat(source(true)));
    for mode in BlendMode::ALL {
        write_png(&dir.join(file_name("ours", mode)), &render(mode, true));
    }
}
