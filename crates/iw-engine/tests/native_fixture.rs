//! Backward compatibility: a document saved by format version 1 must keep
//! opening, unchanged, in every later version of the engine.
//!
//! `tests/fixtures/v1.iwdoc` was written once and is never regenerated.
//! If this test fails, the reader broke compatibility with existing files;
//! fix the reader, do not replace the fixture. (To create the fixture for
//! a *new* format version, add a new file and test alongside this one.)

use iw_engine::blend::BlendMode;
use iw_engine::command::{Command, LayerProps};
use iw_engine::document::{
    AlphaChannel, Anchor, ChannelId, ColorProfile, Document, Layer, LayerId, MaskData, Metadata,
    PathId, PixelData, Resolution, Subpath, VectorPath,
};
use iw_engine::format::{self, ExportOptions, FileFormat};
use iw_engine::geom::Rect;
use iw_engine::history::Session;
use iw_engine::pixel::{BitDepth, ByDepth, Pixel};
use iw_engine::raster::{Mask, Raster};
use std::path::PathBuf;

/// The document the fixture holds, built through the public command API.
fn expected() -> Document {
    let mut s = Session::new(Document::new(320, 240, BitDepth::U8).unwrap());
    let add = |s: &mut Session, parent: Option<LayerId>, index: usize, layer: Layer| {
        s.execute(Command::AddLayer {
            parent,
            index,
            layer: Box::new(layer),
        })
        .unwrap()
        .added_layers[0]
    };
    let block = |rect: Rect, f: &dyn Fn(u32, u32) -> Pixel<u8>| {
        let mut r: Raster<u8> = Raster::new();
        let pixels: Vec<Pixel<u8>> = (0..rect.h)
            .flat_map(|y| (0..rect.w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect();
        r.write_rect(rect, &pixels);
        PixelData::U8(r)
    };

    // An opaque gradient background covering the canvas.
    add(
        &mut s,
        None,
        0,
        Layer::raster(
            "Background",
            block(Rect::new(0, 0, 320, 240), &|x, y| {
                [(x * 255 / 319) as u8, (y * 255 / 239) as u8, 128, 255]
            }),
        ),
    );
    // A group holding a Multiply layer and a half-transparent Screen layer.
    let group = add(&mut s, None, 1, Layer::group("Effects"));
    let multiply = add(
        &mut s,
        Some(group),
        0,
        Layer::raster(
            "Shade",
            block(Rect::new(0, 0, 100, 100), &|_, _| [128, 128, 128, 255]),
        ),
    );
    let screen = add(
        &mut s,
        Some(group),
        1,
        Layer::raster(
            "Glow",
            block(Rect::new(0, 0, 60, 60), &|x, _| {
                let a = (x * 4) as u8;
                [a, a / 2, 0, a]
            }),
        ),
    );
    s.execute(Command::SetLayerProps {
        id: multiply,
        props: LayerProps {
            blend: Some(BlendMode::Multiply),
            offset: Some((40, 30)),
            ..LayerProps::default()
        },
    })
    .unwrap();
    s.execute(Command::SetLayerProps {
        id: screen,
        props: LayerProps {
            blend: Some(BlendMode::Screen),
            opacity: Some(0.75),
            offset: Some((230, 150)),
            ..LayerProps::default()
        },
    })
    .unwrap();
    add(&mut s, None, 2, {
        let mut hidden = Layer::raster(
            "Hidden note",
            block(Rect::new(-10, -10, 20, 20), &|_, _| [255, 0, 0, 255]),
        );
        hidden.visible = false;
        hidden.locked = true;
        hidden
    });

    s.execute(Command::SetResolution(Resolution { ppi: 144.0 }))
        .unwrap();
    s.execute(Command::SetProfile(Some(ColorProfile {
        name: "Fixture profile".into(),
        icc: vec![0xAB; 64],
    })))
    .unwrap();
    s.execute(Command::SetMetadata(Box::new(Metadata {
        title: "Format v1 fixture".into(),
        author: "ImageWorks tests".into(),
        created: Some(1_759_700_000),
        custom: [("purpose".to_string(), "backward compatibility".to_string())].into(),
        ..Metadata::default()
    })))
    .unwrap();
    let mut mask: Mask<u8> = Mask::new();
    mask.write_rect(Rect::new(10, 10, 4, 1), &[0, 85, 170, 255]);
    s.execute(Command::AddChannel {
        index: 0,
        channel: Box::new(AlphaChannel {
            id: ChannelId::UNASSIGNED,
            name: "Saved selection".into(),
            data: MaskData::U8(mask),
        }),
    })
    .unwrap();
    s.execute(Command::AddPath {
        index: 0,
        path: Box::new(VectorPath {
            id: PathId::UNASSIGNED,
            name: "Triangle".into(),
            subpaths: vec![Subpath {
                closed: true,
                anchors: [[10.0, 10.0], [200.5, 20.25], [100.0, 180.0]]
                    .into_iter()
                    .map(|p| Anchor {
                        point: p,
                        handle_in: p,
                        handle_out: p,
                    })
                    .collect(),
            }],
        }),
    })
    .unwrap();
    s.into_document()
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1.iwdoc")
}

#[test]
fn the_version_1_fixture_still_opens_unchanged() {
    let path = fixture_path();
    let expected = expected();
    if std::env::var_os("IW_CREATE_FIXTURE").is_some() {
        assert!(
            !path.exists(),
            "the fixture already exists and must not be replaced"
        );
        format::save(
            &expected,
            &path,
            FileFormat::Native,
            &ExportOptions::default(),
        )
        .unwrap();
    }
    let loaded = format::open(&path).expect("the v1 fixture must open");
    assert!(
        loaded == expected,
        "the v1 fixture no longer loads as the document it was saved from"
    );
    assert_eq!(loaded.next_id(), expected.next_id());

    // And what it looks like is pinned too: a few composited pixels.
    let ByDepth::U8(pixels) = loaded.flatten().pixels else {
        panic!("expected 8-bit")
    };
    let at = |x: usize, y: usize| pixels[y * 320 + x];
    assert_eq!(at(0, 0), [0, 0, 128, 255], "plain background");
    assert_eq!(
        at(319, 239),
        [255, 255, 128, 255],
        "plain background, far corner"
    );
    // Under the Multiply layer (50% grey): background (39, 42, 128) halves.
    assert_eq!(at(50, 40), [20, 21, 64, 255]);
    // The hidden layer must not show.
    assert_eq!(at(5, 5), [3, 5, 128, 255]);
}
