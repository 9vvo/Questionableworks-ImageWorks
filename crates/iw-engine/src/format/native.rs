//! The native document format (`.iwdoc`).
//!
//! A ZIP archive, so it can be inspected with ordinary tools:
//!
//! ```text
//! mimetype                 "application/x-imageworks-document", stored
//! document.json            everything except pixel data
//! profile.icc              the ICC profile, if the document has one
//! layers/<id>/<tx>_<ty>    one tile of a layer's pixels
//! channels/<id>/<tx>_<ty>  one tile of an alpha channel
//! ```
//!
//! Tiles are raw samples exactly as held in memory: premultiplied RGBA
//! (or a single channel), row-major, little-endian, at the document's bit
//! depth. Only tiles that exist are stored.
//!
//! `document.json` carries a format `version`. A reader accepts its own
//! version and older ones, ignores fields it does not know, and refuses
//! newer versions rather than guess. Changing the layout in a way old
//! readers would misread requires a new version number.

use super::{malformed, FormatError};
use crate::blend::BlendMode;
use crate::by_depth;
use crate::document::{
    AlphaChannel, Anchor, ChannelId, ColorMode, ColorProfile, Document, Layer, LayerId, LayerKind,
    MaskData, Metadata, PathId, PixelData, Resolution, Subpath, VectorPath,
};
use crate::pixel::{BitDepth, ByDepth, Channel, Pixel, Texel};
use crate::raster::Grid;
use crate::tile::{Tile, TileCoord, TILE_SIZE};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// File extension, without the dot.
pub const EXTENSION: &str = "iwdoc";
pub const MIME_TYPE: &str = "application/x-imageworks-document";
/// The format version this build writes, and the newest it reads.
pub const VERSION: u32 = 1;

const FORMAT_NAME: &str = "imageworks-document";
const TILE_AREA: usize = (TILE_SIZE * TILE_SIZE) as usize;

// ----- The manifest ------------------------------------------------------
//
// These types are the file format. They are deliberately separate from the
// in-memory document so that refactoring the engine cannot silently change
// what is written to disk.

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    /// Which program wrote the file. Informational.
    generator: String,
    width: u32,
    height: u32,
    ppi: f64,
    color_mode: String,
    bit_depth: String,
    tile_size: u32,
    profile: Option<ProfileEntry>,
    metadata: MetadataEntry,
    next_id: u64,
    /// Bottom first.
    layers: Vec<LayerEntry>,
    channels: Vec<ChannelEntry>,
    paths: Vec<PathEntry>,
}

#[derive(Serialize, Deserialize)]
struct ProfileEntry {
    name: String,
    file: String,
}

#[derive(Serialize, Deserialize, Default)]
struct MetadataEntry {
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    copyright: String,
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    modified: Option<i64>,
    #[serde(default)]
    custom: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct LayerEntry {
    id: u64,
    name: String,
    visible: bool,
    opacity: f32,
    blend: String,
    locked: bool,
    #[serde(flatten)]
    kind: KindEntry,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum KindEntry {
    Raster {
        offset: [i32; 2],
        tiles: Vec<[i32; 2]>,
    },
    Group {
        pass_through: bool,
        children: Vec<LayerEntry>,
    },
}

#[derive(Serialize, Deserialize)]
struct ChannelEntry {
    id: u64,
    name: String,
    tiles: Vec<[i32; 2]>,
}

#[derive(Serialize, Deserialize)]
struct PathEntry {
    id: u64,
    name: String,
    subpaths: Vec<SubpathEntry>,
}

#[derive(Serialize, Deserialize)]
struct SubpathEntry {
    closed: bool,
    anchors: Vec<AnchorEntry>,
}

#[derive(Serialize, Deserialize)]
struct AnchorEntry {
    point: [f64; 2],
    handle_in: [f64; 2],
    handle_out: [f64; 2],
}

fn depth_name(depth: BitDepth) -> &'static str {
    match depth {
        BitDepth::U8 => "u8",
        BitDepth::U16 => "u16",
        BitDepth::F32 => "f32",
    }
}

fn tile_entry_name(folder: &str, id: u64, coord: TileCoord) -> String {
    format!("{folder}/{id}/{}_{}", coord.tx, coord.ty)
}

// ----- Texel bytes -------------------------------------------------------

/// A texel that can be stored in a tile entry.
trait Stored: Texel {
    const BYTES: usize;
    fn write(self, out: &mut Vec<u8>);
    fn read(bytes: &[u8]) -> Self;
}

impl<C: Channel> Stored for Pixel<C> {
    const BYTES: usize = 4 * C::BYTES;

    fn write(self, out: &mut Vec<u8>) {
        self.into_iter().for_each(|c| c.write_le(out));
    }

    fn read(bytes: &[u8]) -> Self {
        let n = C::BYTES;
        [
            C::read_le(&bytes[..n]),
            C::read_le(&bytes[n..2 * n]),
            C::read_le(&bytes[2 * n..3 * n]),
            C::read_le(&bytes[3 * n..]),
        ]
    }
}

macro_rules! stored_sample {
    ($($t:ty),*) => {$(
        impl Stored for $t {
            const BYTES: usize = <$t as Channel>::BYTES;

            fn write(self, out: &mut Vec<u8>) {
                self.write_le(out);
            }

            fn read(bytes: &[u8]) -> Self {
                <$t as Channel>::read_le(bytes)
            }
        }
    )*};
}
stored_sample!(u8, u16, f32);

// ----- Writing -----------------------------------------------------------

fn zip_err(e: zip::result::ZipError) -> FormatError {
    match e {
        zip::result::ZipError::Io(e) => FormatError::Io(e),
        other => malformed(other.to_string()),
    }
}

struct Writer<W: Write + Seek> {
    zip: ZipWriter<W>,
}

impl<W: Write + Seek> Writer<W> {
    fn entry(&mut self, name: &str, compress: bool, bytes: &[u8]) -> Result<(), FormatError> {
        // A fixed timestamp keeps the output byte-for-byte reproducible.
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::default())
            .compression_method(if compress {
                CompressionMethod::Deflated
            } else {
                CompressionMethod::Stored
            });
        self.zip.start_file(name, options).map_err(zip_err)?;
        self.zip.write_all(bytes)?;
        Ok(())
    }

    fn tiles<P: Stored>(
        &mut self,
        folder: &str,
        id: u64,
        grid: &Grid<P>,
    ) -> Result<Vec<[i32; 2]>, FormatError> {
        let mut coords = Vec::with_capacity(grid.tile_count());
        let mut bytes = Vec::with_capacity(TILE_AREA * P::BYTES);
        for (coord, tile) in grid.tiles() {
            bytes.clear();
            tile.pixels().iter().for_each(|p| p.write(&mut bytes));
            self.entry(&tile_entry_name(folder, id, coord), true, &bytes)?;
            coords.push([coord.tx, coord.ty]);
        }
        Ok(coords)
    }

    fn layer(&mut self, layer: &Layer) -> Result<LayerEntry, FormatError> {
        let kind = match &layer.kind {
            LayerKind::Raster { pixels, offset } => KindEntry::Raster {
                offset: [offset.0, offset.1],
                tiles: by_depth!(pixels, r => self.tiles("layers", layer.id.raw(), r))?,
            },
            LayerKind::Group {
                children,
                pass_through,
            } => KindEntry::Group {
                pass_through: *pass_through,
                children: children
                    .iter()
                    .map(|c| self.layer(c))
                    .collect::<Result<_, _>>()?,
            },
        };
        Ok(LayerEntry {
            id: layer.id.raw(),
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            blend: layer.blend.id(),
            locked: layer.locked,
            kind,
        })
    }
}

/// Writes `document` as a native file.
pub fn write<W: Write + Seek>(document: &Document, out: W) -> Result<(), FormatError> {
    let mut w = Writer {
        zip: ZipWriter::new(out),
    };
    // First and uncompressed, so the file type can be read at a fixed offset.
    w.entry("mimetype", false, MIME_TYPE.as_bytes())?;

    let layers = document
        .layers()
        .iter()
        .map(|l| w.layer(l))
        .collect::<Result<Vec<_>, _>>()?;
    let channels = document
        .channels()
        .iter()
        .map(|c| {
            Ok(ChannelEntry {
                id: c.id.raw(),
                name: c.name.clone(),
                tiles: by_depth!(&c.data, m => w.tiles("channels", c.id.raw(), m))?,
            })
        })
        .collect::<Result<Vec<_>, FormatError>>()?;

    let profile = match document.profile() {
        Some(profile) => {
            w.entry("profile.icc", true, &profile.icc)?;
            Some(ProfileEntry {
                name: profile.name.clone(),
                file: "profile.icc".into(),
            })
        }
        None => None,
    };

    let meta = document.metadata();
    let manifest = Manifest {
        format: FORMAT_NAME.into(),
        version: VERSION,
        generator: format!("ImageWorks {}", crate::VERSION),
        width: document.width(),
        height: document.height(),
        ppi: document.resolution().ppi,
        color_mode: match document.color_mode() {
            ColorMode::Rgb => "rgb".into(),
        },
        bit_depth: depth_name(document.bit_depth()).into(),
        tile_size: TILE_SIZE,
        profile,
        metadata: MetadataEntry {
            title: meta.title.clone(),
            author: meta.author.clone(),
            description: meta.description.clone(),
            copyright: meta.copyright.clone(),
            created: meta.created,
            modified: meta.modified,
            custom: meta.custom.clone(),
        },
        next_id: document.next_id(),
        layers,
        channels,
        paths: document
            .paths()
            .iter()
            .map(|p| PathEntry {
                id: p.id.raw(),
                name: p.name.clone(),
                subpaths: p
                    .subpaths
                    .iter()
                    .map(|s| SubpathEntry {
                        closed: s.closed,
                        anchors: s
                            .anchors
                            .iter()
                            .map(|a| AnchorEntry {
                                point: a.point,
                                handle_in: a.handle_in,
                                handle_out: a.handle_out,
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| malformed(e.to_string()))?;
    w.entry("document.json", true, &json)?;
    w.zip.finish().map_err(zip_err)?;
    Ok(())
}

// ----- Reading -----------------------------------------------------------

struct Reader<R: Read + Seek> {
    zip: ZipArchive<R>,
}

impl<R: Read + Seek> Reader<R> {
    /// Reads a whole entry, refusing anything larger than `limit` so a
    /// corrupt size field cannot exhaust memory.
    fn entry(&mut self, name: &str, limit: u64) -> Result<Vec<u8>, FormatError> {
        let file = self
            .zip
            .by_name(name)
            .map_err(|_| malformed(format!("missing entry {name}")))?;
        if file.size() > limit {
            return Err(malformed(format!("entry {name} is larger than expected")));
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| malformed(format!("entry {name}: {e}")))?;
        Ok(bytes)
    }

    fn tiles<P: Stored>(
        &mut self,
        folder: &str,
        id: u64,
        coords: &[[i32; 2]],
    ) -> Result<Grid<P>, FormatError> {
        let mut grid = Grid::new();
        let expected = TILE_AREA * P::BYTES;
        for [tx, ty] in coords {
            let coord = TileCoord::new(*tx, *ty);
            let name = tile_entry_name(folder, id, coord);
            let bytes = self.entry(&name, expected as u64)?;
            if bytes.len() != expected {
                return Err(malformed(format!(
                    "tile {name} has {} bytes, expected {expected}",
                    bytes.len()
                )));
            }
            let pixels: Vec<P> = bytes.chunks_exact(P::BYTES).map(P::read).collect();
            let tile = Tile::from_pixels(pixels)
                .ok_or_else(|| malformed(format!("tile {name} is the wrong size")))?;
            if grid.insert_tile(coord, tile).is_some() {
                return Err(malformed(format!("tile {name} is listed twice")));
            }
        }
        Ok(grid)
    }

    fn pixel_data(
        &mut self,
        depth: BitDepth,
        id: u64,
        coords: &[[i32; 2]],
    ) -> Result<PixelData, FormatError> {
        Ok(match depth {
            BitDepth::U8 => ByDepth::U8(self.tiles("layers", id, coords)?),
            BitDepth::U16 => ByDepth::U16(self.tiles("layers", id, coords)?),
            BitDepth::F32 => ByDepth::F32(self.tiles("layers", id, coords)?),
        })
    }

    fn mask_data(
        &mut self,
        depth: BitDepth,
        id: u64,
        coords: &[[i32; 2]],
    ) -> Result<MaskData, FormatError> {
        Ok(match depth {
            BitDepth::U8 => ByDepth::U8(self.tiles("channels", id, coords)?),
            BitDepth::U16 => ByDepth::U16(self.tiles("channels", id, coords)?),
            BitDepth::F32 => ByDepth::F32(self.tiles("channels", id, coords)?),
        })
    }

    fn layer(&mut self, depth: BitDepth, entry: LayerEntry) -> Result<Layer, FormatError> {
        if entry.id == 0 {
            return Err(malformed("layer id 0 is reserved"));
        }
        let blend = BlendMode::from_id(&entry.blend)
            .ok_or_else(|| FormatError::Unsupported(format!("blend mode \"{}\"", entry.blend)))?;
        let kind = match entry.kind {
            KindEntry::Raster { offset, tiles } => LayerKind::Raster {
                pixels: self.pixel_data(depth, entry.id, &tiles)?,
                offset: (offset[0], offset[1]),
            },
            KindEntry::Group {
                pass_through,
                children,
            } => LayerKind::Group {
                pass_through,
                children: children
                    .into_iter()
                    .map(|c| self.layer(depth, c))
                    .collect::<Result<_, _>>()?,
            },
        };
        Ok(Layer {
            id: LayerId::from_raw(entry.id),
            name: entry.name,
            visible: entry.visible,
            opacity: entry.opacity,
            blend,
            locked: entry.locked,
            kind,
        })
    }
}

/// Upper bound on `document.json`, far above any real document.
const MAX_MANIFEST_BYTES: u64 = 256 << 20;
/// Upper bound on an embedded ICC profile.
const MAX_PROFILE_BYTES: u64 = 64 << 20;

/// Reads a native file.
pub fn read<R: Read + Seek>(input: R) -> Result<Document, FormatError> {
    let zip =
        ZipArchive::new(input).map_err(|e| malformed(format!("not a readable archive: {e}")))?;
    let mut r = Reader { zip };

    let mime = r.entry("mimetype", 256)?;
    if mime != MIME_TYPE.as_bytes() {
        return Err(FormatError::Unsupported(
            "the archive is not an ImageWorks document".into(),
        ));
    }

    let json = r.entry("document.json", MAX_MANIFEST_BYTES)?;
    // Check the version before interpreting anything else, so a newer file
    // is reported as newer rather than as damaged.
    #[derive(Deserialize)]
    struct Header {
        format: String,
        version: u32,
    }
    let header: Header =
        serde_json::from_slice(&json).map_err(|e| malformed(format!("document.json: {e}")))?;
    if header.format != FORMAT_NAME {
        return Err(FormatError::Unsupported(format!(
            "document format \"{}\"",
            header.format
        )));
    }
    if header.version > VERSION {
        return Err(FormatError::TooNew {
            version: header.version,
            supported: VERSION,
        });
    }
    if header.version == 0 {
        return Err(malformed("format version 0 does not exist"));
    }
    let m: Manifest =
        serde_json::from_slice(&json).map_err(|e| malformed(format!("document.json: {e}")))?;

    if m.tile_size != TILE_SIZE {
        return Err(FormatError::Unsupported(format!(
            "tile size {}",
            m.tile_size
        )));
    }
    if m.color_mode != "rgb" {
        return Err(FormatError::Unsupported(format!(
            "colour mode \"{}\"",
            m.color_mode
        )));
    }
    let depth = match m.bit_depth.as_str() {
        "u8" => BitDepth::U8,
        "u16" => BitDepth::U16,
        "f32" => BitDepth::F32,
        other => return Err(FormatError::Unsupported(format!("bit depth \"{other}\""))),
    };

    let mut doc = Document::new(m.width, m.height, depth)?;
    doc.set_resolution(Resolution { ppi: m.ppi })?;
    if let Some(profile) = m.profile {
        if profile.file != "profile.icc" {
            return Err(malformed("unexpected profile file name"));
        }
        let icc = r.entry("profile.icc", MAX_PROFILE_BYTES)?;
        doc.set_profile(Some(ColorProfile {
            name: profile.name,
            icc,
        }));
    }
    doc.set_metadata(Metadata {
        title: m.metadata.title,
        author: m.metadata.author,
        description: m.metadata.description,
        copyright: m.metadata.copyright,
        created: m.metadata.created,
        modified: m.metadata.modified,
        custom: m.metadata.custom,
    });

    // Inserting through the document re-checks everything a command would:
    // unique ids, opacity and offset ranges, matching bit depth.
    for (index, entry) in m.layers.into_iter().enumerate() {
        let layer = r.layer(depth, entry)?;
        doc.insert_layer(None, index, layer)?;
    }
    for (index, entry) in m.channels.into_iter().enumerate() {
        if entry.id == 0 {
            return Err(malformed("channel id 0 is reserved"));
        }
        let data = r.mask_data(depth, entry.id, &entry.tiles)?;
        doc.insert_channel(
            index,
            AlphaChannel {
                id: ChannelId::from_raw(entry.id),
                name: entry.name,
                data,
            },
        )?;
    }
    for (index, entry) in m.paths.into_iter().enumerate() {
        if entry.id == 0 {
            return Err(malformed("path id 0 is reserved"));
        }
        let subpaths = entry
            .subpaths
            .into_iter()
            .map(|s| Subpath {
                closed: s.closed,
                anchors: s
                    .anchors
                    .into_iter()
                    .map(|a| Anchor {
                        point: a.point,
                        handle_in: a.handle_in,
                        handle_out: a.handle_out,
                    })
                    .collect(),
            })
            .collect();
        doc.insert_path(
            index,
            VectorPath {
                id: PathId::from_raw(entry.id),
                name: entry.name,
                subpaths,
            },
        )?;
    }
    doc.restore_next_id(m.next_id);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, LayerProps};
    use crate::history::Session;
    use std::io::Cursor;

    /// A document that uses every feature the engine has, at `depth`.
    pub(crate) fn rich_document(depth: BitDepth) -> Document {
        fn fill<C: Channel>(seed: u32) -> Tile<Pixel<C>> {
            let pixels = (0..TILE_AREA as u32)
                .map(|i| {
                    let v =
                        |k: u32| C::from_f32(((i.wrapping_mul(k) ^ seed) % 1000) as f32 / 1000.0);
                    [v(3), v(7), v(11), v(13)]
                })
                .collect();
            Tile::from_pixels(pixels).unwrap()
        }
        fn mask<C: Channel>(seed: u32) -> Tile<C> {
            Tile::from_pixels(
                (0..TILE_AREA as u32)
                    .map(|i| C::from_f32(((i ^ seed) % 256) as f32 / 255.0))
                    .collect(),
            )
            .unwrap()
        }
        let pixels = |seed: u32, coords: &[(i32, i32)]| -> PixelData {
            let mut data = PixelData::empty(depth);
            for (n, (tx, ty)) in coords.iter().enumerate() {
                let coord = TileCoord::new(*tx, *ty);
                let seed = seed + n as u32;
                match &mut data {
                    ByDepth::U8(r) => drop(r.insert_tile(coord, fill(seed))),
                    ByDepth::U16(r) => drop(r.insert_tile(coord, fill(seed))),
                    ByDepth::F32(r) => drop(r.insert_tile(coord, fill(seed))),
                }
            }
            data
        };

        let mut s = Session::new(Document::new(700, 500, depth).unwrap());
        let add = |s: &mut Session, parent, index, layer: Layer| {
            s.execute(Command::AddLayer {
                parent,
                index,
                layer: Box::new(layer),
            })
            .unwrap()
            .added_layers[0]
        };

        add(
            &mut s,
            None,
            0,
            Layer::raster("Background", pixels(1, &[(0, 0), (1, 0), (2, 1), (-1, -1)])),
        );
        // One layer per blend mode, inside a group, with varied properties.
        let modes = add(&mut s, None, 1, Layer::group("All blend modes"));
        for (i, mode) in BlendMode::ALL.into_iter().enumerate() {
            let mut layer = Layer::raster(
                format!("{} layer", mode.name()),
                pixels(100 + i as u32, &[(i as i32 % 3, 0)]),
            );
            layer.blend = mode;
            layer.opacity = 1.0 - i as f32 / 40.0;
            layer.visible = i % 5 != 0;
            layer.locked = i % 7 == 0;
            if let LayerKind::Raster { offset, .. } = &mut layer.kind {
                *offset = (i as i32 * 37 - 400, 256 - i as i32 * 19);
            }
            add(&mut s, Some(modes), i, layer);
        }
        // Nested groups: one isolated with a blend mode, one pass-through, one empty.
        let outer = add(
            &mut s,
            None,
            2,
            Layer::group("Outer \"quoted\" / unicode: é 日本"),
        );
        s.execute(Command::SetLayerProps {
            id: outer,
            props: LayerProps {
                pass_through: Some(false),
                blend: Some(BlendMode::Overlay),
                opacity: Some(0.5),
                ..LayerProps::default()
            },
        })
        .unwrap();
        let inner = add(&mut s, Some(outer), 0, Layer::group("Inner"));
        add(
            &mut s,
            Some(inner),
            0,
            Layer::raster("Deep", pixels(500, &[(1, 1)])),
        );
        add(&mut s, Some(inner), 1, Layer::group("Empty group"));
        add(
            &mut s,
            Some(outer),
            1,
            Layer::raster("Empty layer", PixelData::empty(depth)),
        );

        s.execute(Command::SetResolution(Resolution { ppi: 299.5 }))
            .unwrap();
        s.execute(Command::SetProfile(Some(ColorProfile {
            name: "Test profile".into(),
            icc: (0..=255).collect(),
        })))
        .unwrap();
        s.execute(Command::SetMetadata(Box::new(Metadata {
            title: "Title".into(),
            author: "Author".into(),
            description: "Line one\nLine two".into(),
            copyright: "(c)".into(),
            created: Some(1_700_000_000),
            modified: Some(-5),
            custom: [
                ("key".to_string(), "value".to_string()),
                ("k2".to_string(), String::new()),
            ]
            .into(),
        })))
        .unwrap();

        let mut channel = MaskData::empty(depth);
        match &mut channel {
            ByDepth::U8(m) => drop(m.insert_tile(TileCoord::new(0, 0), mask(9))),
            ByDepth::U16(m) => drop(m.insert_tile(TileCoord::new(0, 0), mask(9))),
            ByDepth::F32(m) => drop(m.insert_tile(TileCoord::new(0, 0), mask(9))),
        }
        for (i, data) in [channel, MaskData::empty(depth)].into_iter().enumerate() {
            s.execute(Command::AddChannel {
                index: i,
                channel: Box::new(AlphaChannel {
                    id: ChannelId::UNASSIGNED,
                    name: format!("Alpha {i}"),
                    data,
                }),
            })
            .unwrap();
        }
        s.execute(Command::AddPath {
            index: 0,
            path: Box::new(VectorPath {
                id: PathId::UNASSIGNED,
                name: "Path".into(),
                subpaths: vec![
                    Subpath {
                        closed: true,
                        anchors: vec![
                            Anchor {
                                point: [10.5, -3.25],
                                handle_in: [9.0, -3.0],
                                handle_out: [12.0, 1e-9],
                            },
                            Anchor {
                                point: [1e6, 0.1],
                                handle_in: [1e6, 0.1],
                                handle_out: [1e6, 0.1],
                            },
                        ],
                    },
                    Subpath {
                        closed: false,
                        anchors: vec![],
                    },
                ],
            }),
        })
        .unwrap();
        // Removing a layer leaves a gap in the ids, which must survive too.
        let gone = add(&mut s, None, 0, Layer::group("Deleted"));
        s.execute(Command::RemoveLayer { id: gone }).unwrap();
        s.into_document()
    }

    /// A small document with the same kinds of content, for tests that
    /// rewrite the archive many times.
    fn small_document() -> Document {
        let mut s = Session::new(Document::new(700, 500, BitDepth::U8).unwrap());
        let layer = |name: &str, seed: u8, blend| {
            let mut r = crate::raster::Raster::<u8>::new();
            r.insert_tile(TileCoord::new(0, 0), Tile::filled([seed, seed, seed, 255]));
            r.insert_tile(TileCoord::new(-1, -1), Tile::filled([seed, 0, 0, seed]));
            let mut layer = Layer::raster(name, ByDepth::U8(r));
            layer.blend = blend;
            layer
        };
        let add = |s: &mut Session, parent, index, layer: Layer| {
            s.execute(Command::AddLayer {
                parent,
                index,
                layer: Box::new(layer),
            })
            .unwrap()
            .added_layers[0]
        };
        add(&mut s, None, 0, layer("Background", 200, BlendMode::Normal));
        let group = add(&mut s, None, 1, Layer::group("Group"));
        add(
            &mut s,
            Some(group),
            0,
            layer("Multiply", 90, BlendMode::Multiply),
        );
        s.execute(Command::SetResolution(Resolution { ppi: 299.5 }))
            .unwrap();
        s.execute(Command::SetProfile(Some(ColorProfile {
            name: "P".into(),
            icc: vec![1, 2, 3],
        })))
        .unwrap();
        s.into_document()
    }

    fn to_bytes(doc: &Document) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write(doc, &mut out).unwrap();
        out.into_inner()
    }

    /// Rewrites an archive, letting `edit` change, add or remove entries.
    fn tamper(bytes: &[u8], edit: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) -> Vec<u8> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut entries = Vec::new();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            entries.push((file.name().to_string(), data));
        }
        edit(&mut entries);
        let mut w = Writer {
            zip: ZipWriter::new(Cursor::new(Vec::new())),
        };
        for (name, data) in &entries {
            w.entry(name, name != "mimetype", data).unwrap();
        }
        w.zip.finish().unwrap().into_inner()
    }

    /// Applies a text replacement to `document.json`.
    fn with_manifest(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
        tamper(bytes, |entries| {
            let json = entries
                .iter_mut()
                .find(|(n, _)| n == "document.json")
                .unwrap();
            let text = String::from_utf8(json.1.clone()).unwrap();
            assert!(text.contains(from), "manifest does not contain {from:?}");
            json.1 = text.replacen(from, to, 1).into_bytes();
        })
    }

    #[test]
    fn a_document_using_every_feature_round_trips_at_every_depth() {
        for depth in [BitDepth::U8, BitDepth::U16, BitDepth::F32] {
            let doc = rich_document(depth);
            let back = read(Cursor::new(to_bytes(&doc))).unwrap();
            assert!(back == doc, "{depth:?} document changed in a save and load");
            assert_eq!(
                back.next_id(),
                doc.next_id(),
                "the id counter must survive so ids are never reused"
            );
            // Spot-check that the comparison is not vacuous.
            assert_eq!(back.all_layers().len(), 34);
            assert_eq!(back.channels().len(), 2);
            assert_eq!(
                back.paths()[0].subpaths[0].anchors[0].handle_out,
                [12.0, 1e-9]
            );
        }
    }

    #[test]
    fn an_empty_document_round_trips() {
        let doc = Document::new(1, 1, BitDepth::U8).unwrap();
        let back = read(Cursor::new(to_bytes(&doc))).unwrap();
        assert!(back == doc);
        assert_eq!(back.next_id(), 1);
    }

    #[test]
    fn saving_twice_gives_identical_bytes() {
        let doc = rich_document(BitDepth::U8);
        assert_eq!(to_bytes(&doc), to_bytes(&doc));
    }

    #[test]
    fn the_archive_is_laid_out_as_documented() {
        let bytes = to_bytes(&rich_document(BitDepth::U8));
        // The type is readable at a fixed offset, without unzipping.
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..38 + MIME_TYPE.len()], MIME_TYPE.as_bytes());

        let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert!(names.contains(&"document.json".to_string()));
        assert!(names.contains(&"profile.icc".to_string()));
        assert!(names.iter().any(|n| n.starts_with("layers/1/")));
        assert!(names.iter().any(|n| n.starts_with("channels/")));
        // Negative tile coordinates are part of the name.
        assert!(names.contains(&"layers/1/-1_-1".to_string()));

        let mut json = String::new();
        zip.by_name("document.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["format"], "imageworks-document");
        assert_eq!(value["version"], 1);
        assert_eq!(value["bit_depth"], "u8");
        assert_eq!(value["layers"][1]["kind"], "group");
        assert_eq!(value["layers"][1]["children"][3]["blend"], "multiply");
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        let doc = rich_document(BitDepth::U8);
        let bytes = with_manifest(
            &to_bytes(&doc),
            "\"width\"",
            "\"added_in_a_later_release\": [1, 2], \"width\"",
        );
        assert!(read(Cursor::new(bytes)).unwrap() == doc);
    }

    #[test]
    fn a_newer_format_version_is_reported_as_newer() {
        let bytes = with_manifest(
            &to_bytes(&rich_document(BitDepth::U8)),
            "\"version\": 1",
            "\"version\": 2",
        );
        match read(Cursor::new(bytes)) {
            Err(FormatError::TooNew {
                version: 2,
                supported: 1,
            }) => {}
            other => panic!("expected TooNew, got {other:?}"),
        }
    }

    #[test]
    fn damaged_or_invalid_files_are_errors_not_panics() {
        let good = to_bytes(&small_document());
        let expect = |bytes: Vec<u8>, what: &str, check: fn(&FormatError) -> bool| match read(
            Cursor::new(bytes),
        ) {
            Ok(_) => panic!("{what}: was accepted"),
            Err(e) => assert!(check(&e), "{what}: unexpected error {e:?}"),
        };
        let malformed = |e: &FormatError| matches!(e, FormatError::Malformed(_));
        let unsupported = |e: &FormatError| matches!(e, FormatError::Unsupported(_));
        let invalid = |e: &FormatError| matches!(e, FormatError::Document(_));

        expect(
            with_manifest(&good, "\"multiply\"", "\"sparkle\""),
            "unknown blend mode",
            unsupported,
        );
        expect(
            with_manifest(&good, "\"tile_size\": 256", "\"tile_size\": 128"),
            "tile size",
            unsupported,
        );
        expect(
            with_manifest(&good, "\"bit_depth\": \"u8\"", "\"bit_depth\": \"u4\""),
            "bit depth",
            unsupported,
        );
        expect(
            with_manifest(&good, "\"color_mode\": \"rgb\"", "\"color_mode\": \"cmyk\""),
            "colour mode",
            unsupported,
        );
        expect(
            with_manifest(&good, "imageworks-document", "something-else"),
            "format name",
            unsupported,
        );
        expect(
            with_manifest(&good, "\"version\": 1", "\"version\": 0"),
            "version 0",
            malformed,
        );
        expect(
            with_manifest(&good, "\"width\": 700", "\"width\": 0"),
            "zero width",
            invalid,
        );
        expect(
            with_manifest(&good, "\"opacity\": 1.0", "\"opacity\": 2.5"),
            "opacity",
            invalid,
        );
        expect(
            with_manifest(&good, "\"id\": 2,", "\"id\": 1,"),
            "duplicate id",
            invalid,
        );
        expect(
            with_manifest(&good, "\"id\": 2,", "\"id\": 0,"),
            "reserved id",
            malformed,
        );
        expect(
            with_manifest(&good, "\"ppi\": 299.5", "\"ppi\": -1"),
            "resolution",
            invalid,
        );
        expect(
            with_manifest(&good, "{", "{ not json"),
            "broken json",
            malformed,
        );
        expect(
            with_manifest(&good, "\"layers\"", "\"lairs\""),
            "missing field",
            malformed,
        );

        expect(
            tamper(&good, |e| e.retain(|(n, _)| n != "layers/1/0_0")),
            "missing tile",
            malformed,
        );
        expect(
            tamper(&good, |e| {
                e.iter_mut()
                    .find(|(n, _)| n == "layers/1/0_0")
                    .unwrap()
                    .1
                    .truncate(100)
            }),
            "short tile",
            malformed,
        );
        expect(
            tamper(&good, |e| {
                e.iter_mut()
                    .find(|(n, _)| n == "layers/1/0_0")
                    .unwrap()
                    .1
                    .push(0)
            }),
            "long tile",
            malformed,
        );
        expect(
            tamper(&good, |e| e.retain(|(n, _)| n != "profile.icc")),
            "missing profile",
            malformed,
        );
        expect(
            tamper(&good, |e| e.retain(|(n, _)| n != "document.json")),
            "missing manifest",
            malformed,
        );
        expect(
            tamper(&good, |e| e.retain(|(n, _)| n != "mimetype")),
            "missing mimetype",
            malformed,
        );
        expect(
            tamper(&good, |e| e[0].1 = b"application/zip".to_vec()),
            "wrong mimetype",
            unsupported,
        );

        // A tile listed twice.
        let twice = tamper(&good, |entries| {
            let json = entries
                .iter_mut()
                .find(|(n, _)| n == "document.json")
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&json.1).unwrap();
            let tiles = value["layers"][0]["tiles"].as_array_mut().unwrap();
            tiles.push(tiles[0].clone());
            json.1 = serde_json::to_vec(&value).unwrap();
        });
        expect(twice, "tile listed twice", malformed);

        // Truncation anywhere, and a flipped byte in the compressed data.
        for cut in [
            0,
            4,
            100,
            good.len() / 3,
            good.len() / 2,
            good.len() - 30,
            good.len() - 1,
        ] {
            assert!(
                read(Cursor::new(&good[..cut])).is_err(),
                "truncated at {cut}"
            );
        }
        let mut flipped = good.clone();
        flipped[good.len() / 4] ^= 0x55;
        assert!(
            read(Cursor::new(flipped)).is_err(),
            "corrupted tile data must fail its checksum"
        );
        assert!(read(Cursor::new(b"PK\x03\x04 followed by rubbish".to_vec())).is_err());
    }

    #[test]
    fn deeply_nested_groups_are_refused_rather_than_overflowing_the_stack() {
        let good = to_bytes(&Document::new(1, 1, BitDepth::U8).unwrap());
        let mut layers = String::new();
        let depth = 5000;
        for i in 0..depth {
            layers.push_str(&format!(
                "{{\"id\": {}, \"name\": \"g\", \"visible\": true, \"opacity\": 1.0, \"blend\": \"normal\", \"locked\": false, \"kind\": \"group\", \"pass_through\": true, \"children\": [",
                i + 1
            ));
        }
        layers.push_str(&"]}".repeat(depth));
        let bytes = with_manifest(&good, "\"layers\": []", &format!("\"layers\": [{layers}]"));
        assert!(read(Cursor::new(bytes)).is_err());
    }
}
