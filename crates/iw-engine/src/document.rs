//! The document model.
//!
//! A [`Document`] is everything that is saved: canvas, colour settings,
//! the layer tree, alpha channels, paths and metadata. It is independent of
//! any UI.
//!
//! Outside this crate a document is read-only. The only way to change one
//! is to submit a [`Command`](crate::command::Command) to a
//! [`Session`](crate::history::Session) (architecture rule 2); the mutating
//! methods here are crate-private for that reason.

use crate::blend::BlendMode;
use crate::by_depth;
use crate::compositor::{self, GroupBlend, Node};
use crate::geom::Rect;
use crate::pixel::{BitDepth, ByDepth, Channel, Pixel};
use crate::raster::{Mask, Raster};
use crate::tile::{Tile, TileCoord};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Largest canvas edge, in pixels.
pub const MAX_DIMENSION: u32 = 300_000;
/// Largest layer offset in either direction, in pixels.
pub const MAX_OFFSET: i32 = 1 << 24;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// Placeholder for an item not yet added to a document. The
            /// document assigns a real id when the item is inserted.
            pub const UNASSIGNED: $name = $name(0);

            pub const fn from_raw(raw: u64) -> Self {
                Self(raw)
            }

            pub const fn raw(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a layer or group. Unique within a document and stable
    /// for the item's lifetime, including across undo and save.
    LayerId
);
id_type!(
    /// Identifies an alpha channel.
    ChannelId
);
id_type!(
    /// Identifies a path.
    PathId
);

/// How colour is represented. Only RGB exists so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorMode {
    Rgb,
}

/// Print resolution in pixels per inch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolution {
    pub ppi: f64,
}

impl Default for Resolution {
    fn default() -> Self {
        Self { ppi: 72.0 }
    }
}

/// An embedded ICC colour profile. No profile means sRGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorProfile {
    pub name: String,
    pub icc: Vec<u8>,
}

/// Descriptive information about the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub author: String,
    pub description: String,
    pub copyright: String,
    /// Seconds since the Unix epoch. The engine never reads the clock;
    /// the application supplies these.
    pub created: Option<i64>,
    pub modified: Option<i64>,
    /// Any other key/value pairs.
    pub custom: BTreeMap<String, String>,
}

/// Layer pixels at the document's bit depth.
pub type PixelData = ByDepth<Raster<u8>, Raster<u16>, Raster<f32>>;
/// Single-channel data at the document's bit depth.
pub type MaskData = ByDepth<Mask<u8>, Mask<u16>, Mask<f32>>;
/// One composited tile at the document's bit depth.
pub type AnyTile = ByDepth<Tile<Pixel<u8>>, Tile<Pixel<u16>>, Tile<Pixel<f32>>>;
/// Row-major premultiplied pixels at the document's bit depth.
pub type AnyPixels = ByDepth<Vec<Pixel<u8>>, Vec<Pixel<u16>>, Vec<Pixel<f32>>>;

impl PixelData {
    pub fn empty(depth: BitDepth) -> Self {
        match depth {
            BitDepth::U8 => ByDepth::U8(Raster::new()),
            BitDepth::U16 => ByDepth::U16(Raster::new()),
            BitDepth::F32 => ByDepth::F32(Raster::new()),
        }
    }

    pub fn tile_coords(&self) -> Vec<TileCoord> {
        by_depth!(self, r => r.tile_coords().collect())
    }
}

impl MaskData {
    pub fn empty(depth: BitDepth) -> Self {
        match depth {
            BitDepth::U8 => ByDepth::U8(Mask::new()),
            BitDepth::U16 => ByDepth::U16(Mask::new()),
            BitDepth::F32 => ByDepth::F32(Mask::new()),
        }
    }
}

/// A flattened image.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: AnyPixels,
}

/// What a layer is.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    Raster {
        pixels: PixelData,
        /// Where the pixels' origin sits on the canvas. Moving a layer
        /// changes this and copies nothing.
        offset: (i32, i32),
    },
    Group {
        /// Bottom first.
        children: Vec<Layer>,
        /// If true the group is only a folder and `blend` is not used.
        pass_through: bool,
    },
}

/// A layer or group.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    /// `0.0..=1.0`.
    pub opacity: f32,
    pub blend: BlendMode,
    /// Protects the pixels and position from change.
    pub locked: bool,
    pub kind: LayerKind,
}

impl Layer {
    /// A new raster layer, not yet in a document.
    pub fn raster(name: impl Into<String>, pixels: PixelData) -> Self {
        Self::with_kind(
            name,
            LayerKind::Raster {
                pixels,
                offset: (0, 0),
            },
        )
    }

    /// A new empty group, not yet in a document. Groups start as
    /// pass-through, as in Photoshop.
    pub fn group(name: impl Into<String>) -> Self {
        Self::with_kind(
            name,
            LayerKind::Group {
                children: Vec::new(),
                pass_through: true,
            },
        )
    }

    fn with_kind(name: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            id: LayerId::UNASSIGNED,
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            locked: false,
            kind,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group { .. })
    }

    /// Child layers, bottom first. Empty for a raster layer.
    pub fn children(&self) -> &[Layer] {
        match &self.kind {
            LayerKind::Group { children, .. } => children,
            LayerKind::Raster { .. } => &[],
        }
    }

    /// This layer and all its descendants, depth first.
    pub fn walk(&self) -> Vec<&Layer> {
        let mut out = vec![self];
        for child in self.children() {
            out.extend(child.walk());
        }
        out
    }

    /// A copy with every id cleared, ready to be added as a new layer.
    pub fn duplicate(&self) -> Layer {
        let mut copy = self.clone();
        fn clear(layer: &mut Layer) {
            layer.id = LayerId::UNASSIGNED;
            if let LayerKind::Group { children, .. } = &mut layer.kind {
                children.iter_mut().for_each(clear);
            }
        }
        clear(&mut copy);
        copy
    }
}

/// A saved single-channel image, such as a stored selection.
#[derive(Clone, Debug, PartialEq)]
pub struct AlphaChannel {
    pub id: ChannelId,
    pub name: String,
    pub data: MaskData,
}

/// One point of a path, with its two Bézier handles, in canvas coordinates.
/// A corner point has both handles equal to the point itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub point: [f64; 2],
    pub handle_in: [f64; 2],
    pub handle_out: [f64; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub closed: bool,
    pub anchors: Vec<Anchor>,
}

/// A named vector path.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    pub id: PathId,
    pub name: String,
    pub subpaths: Vec<Subpath>,
}

/// Why a document could not be created or changed.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentError {
    /// Width or height is zero or above [`MAX_DIMENSION`].
    InvalidSize {
        width: u32,
        height: u32,
    },
    InvalidResolution,
    LayerNotFound(LayerId),
    ChannelNotFound(ChannelId),
    PathNotFound(PathId),
    /// The named parent is not a group.
    NotAGroup(LayerId),
    /// The operation needs a raster layer.
    NotARaster(LayerId),
    IndexOutOfRange {
        index: usize,
        len: usize,
    },
    /// Pixel data is at a different bit depth from the document.
    DepthMismatch {
        document: BitDepth,
        data: BitDepth,
    },
    /// An item being added carries an id that is already in use.
    DuplicateId(u64),
    /// A group cannot be moved inside itself.
    WouldCycle(LayerId),
    LayerLocked(LayerId),
    /// Opacity must be a number in `0.0..=1.0`.
    InvalidOpacity,
    /// Offsets are limited to ±[`MAX_OFFSET`].
    InvalidOffset,
    /// A path coordinate is not a finite number.
    InvalidPath,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize { width, height } => {
                write!(
                    f,
                    "canvas size {width} x {height} is outside 1..={MAX_DIMENSION}"
                )
            }
            Self::InvalidResolution => write!(f, "resolution must be a positive, finite number"),
            Self::LayerNotFound(id) => write!(f, "no layer with id {id}"),
            Self::ChannelNotFound(id) => write!(f, "no channel with id {id}"),
            Self::PathNotFound(id) => write!(f, "no path with id {id}"),
            Self::NotAGroup(id) => write!(f, "layer {id} is not a group"),
            Self::NotARaster(id) => write!(f, "layer {id} is not a raster layer"),
            Self::IndexOutOfRange { index, len } => {
                write!(f, "index {index} is past the end ({len})")
            }
            Self::DepthMismatch { document, data } => {
                write!(f, "pixel data is {data:?} but the document is {document:?}")
            }
            Self::DuplicateId(id) => write!(f, "id {id} is already in use"),
            Self::WouldCycle(id) => write!(f, "group {id} cannot be moved inside itself"),
            Self::LayerLocked(id) => write!(f, "layer {id} is locked"),
            Self::InvalidOpacity => write!(f, "opacity must be between 0 and 1"),
            Self::InvalidOffset => write!(f, "layer offset is out of range"),
            Self::InvalidPath => {
                write!(f, "path contains a coordinate that is not a finite number")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

/// Where a layer sits in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerLocation {
    /// `None` for a top-level layer.
    pub parent: Option<LayerId>,
    /// Position among its siblings, bottom first.
    pub index: usize,
}

/// An editable image document.
///
/// Two documents compare equal when their contents are the same. The id
/// counter is left out of the comparison: ids are never reused, so undoing
/// an "add layer" restores the contents but not the counter.
#[derive(Clone, Debug)]
pub struct Document {
    width: u32,
    height: u32,
    resolution: Resolution,
    color_mode: ColorMode,
    bit_depth: BitDepth,
    profile: Option<ColorProfile>,
    metadata: Metadata,
    /// Top-level layers, bottom first.
    layers: Vec<Layer>,
    channels: Vec<AlphaChannel>,
    paths: Vec<VectorPath>,
    /// The next id to hand out. Shared by layers, channels and paths.
    next_id: u64,
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.resolution == other.resolution
            && self.color_mode == other.color_mode
            && self.bit_depth == other.bit_depth
            && self.profile == other.profile
            && self.metadata == other.metadata
            && self.layers == other.layers
            && self.channels == other.channels
            && self.paths == other.paths
    }
}

fn check_size(width: u32, height: u32) -> Result<(), DocumentError> {
    let ok = |v: u32| (1..=MAX_DIMENSION).contains(&v);
    if ok(width) && ok(height) {
        Ok(())
    } else {
        Err(DocumentError::InvalidSize { width, height })
    }
}

pub(crate) fn check_opacity(opacity: f32) -> Result<(), DocumentError> {
    if (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(DocumentError::InvalidOpacity)
    }
}

pub(crate) fn check_offset(offset: (i32, i32)) -> Result<(), DocumentError> {
    let ok = |v: i32| (-MAX_OFFSET..=MAX_OFFSET).contains(&v);
    if ok(offset.0) && ok(offset.1) {
        Ok(())
    } else {
        Err(DocumentError::InvalidOffset)
    }
}

// ----- Read-only API -----------------------------------------------------

impl Document {
    /// A new, empty RGB document.
    pub fn new(width: u32, height: u32, bit_depth: BitDepth) -> Result<Self, DocumentError> {
        check_size(width, height)?;
        Ok(Self {
            width,
            height,
            resolution: Resolution::default(),
            color_mode: ColorMode::Rgb,
            bit_depth,
            profile: None,
            metadata: Metadata::default(),
            layers: Vec::new(),
            channels: Vec::new(),
            paths: Vec::new(),
            next_id: 1,
        })
    }

    /// A new document whose single layer, "Background", is filled with a
    /// straight (not premultiplied) RGBA colour across the canvas. Tiles
    /// that are wholly inside the canvas share one block of memory.
    pub fn with_background(
        width: u32,
        height: u32,
        bit_depth: BitDepth,
        rgba: [f32; 4],
    ) -> Result<Self, DocumentError> {
        fn fill<C: Channel>(width: u32, height: u32, rgba: [f32; 4]) -> Raster<C> {
            let pixel: Pixel<C> = crate::pixel::from_straight(rgba);
            let shared = Tile::filled(pixel);
            let size = crate::tile::TILE_SIZE;
            let mut raster = Raster::new();
            for ty in 0..height.div_ceil(size) {
                for tx in 0..width.div_ceil(size) {
                    let (x, y) = (tx * size, ty * size);
                    let (w, h) = ((width - x).min(size), (height - y).min(size));
                    if w == size && h == size {
                        raster.insert_tile(TileCoord::new(tx as i32, ty as i32), shared.clone());
                    } else {
                        let rect = Rect::new(x as i32, y as i32, w, h);
                        raster.write_rect(rect, &vec![pixel; rect.area()]);
                    }
                }
            }
            raster
        }
        let mut doc = Self::new(width, height, bit_depth)?;
        let pixels = match bit_depth {
            BitDepth::U8 => ByDepth::U8(fill::<u8>(width, height, rgba)),
            BitDepth::U16 => ByDepth::U16(fill::<u16>(width, height, rgba)),
            BitDepth::F32 => ByDepth::F32(fill::<f32>(width, height, rgba)),
        };
        doc.insert_layer(None, 0, Layer::raster("Background", pixels))?;
        Ok(doc)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The canvas rectangle: `(0, 0)` to `(width, height)`.
    pub fn canvas(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn resolution(&self) -> Resolution {
        self.resolution
    }

    pub fn color_mode(&self) -> ColorMode {
        self.color_mode
    }

    pub fn bit_depth(&self) -> BitDepth {
        self.bit_depth
    }

    pub fn profile(&self) -> Option<&ColorProfile> {
        self.profile.as_ref()
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Top-level layers, bottom first.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn channels(&self) -> &[AlphaChannel] {
        &self.channels
    }

    pub fn paths(&self) -> &[VectorPath] {
        &self.paths
    }

    /// Every layer and group in the document, depth first, bottom first.
    pub fn all_layers(&self) -> Vec<&Layer> {
        self.layers.iter().flat_map(Layer::walk).collect()
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.all_layers().into_iter().find(|l| l.id == id)
    }

    pub fn locate(&self, id: LayerId) -> Option<LayerLocation> {
        fn search(
            siblings: &[Layer],
            parent: Option<LayerId>,
            id: LayerId,
        ) -> Option<LayerLocation> {
            for (index, layer) in siblings.iter().enumerate() {
                if layer.id == id {
                    return Some(LayerLocation { parent, index });
                }
                if let Some(found) = search(layer.children(), Some(layer.id), id) {
                    return Some(found);
                }
            }
            None
        }
        search(&self.layers, None, id)
    }

    /// The id the next added item will receive. Saved with the document so
    /// ids are never reused.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Every tile the visible layers can touch, on or off the canvas.
    pub fn covered_tiles(&self) -> BTreeSet<TileCoord> {
        match self.bit_depth {
            BitDepth::U8 => compositor::covered_tiles(&build_nodes::<u8>(&self.layers)),
            BitDepth::U16 => compositor::covered_tiles(&build_nodes::<u16>(&self.layers)),
            BitDepth::F32 => compositor::covered_tiles(&build_nodes::<f32>(&self.layers)),
        }
    }

    /// Every tile the given layer (and its children) can touch, ignoring
    /// visibility. Used to work out what to redraw after a change.
    pub fn layer_tiles(&self, id: LayerId) -> BTreeSet<TileCoord> {
        let mut out = BTreeSet::new();
        let Some(layer) = self.layer(id) else {
            return out;
        };
        for node in layer.walk() {
            if let LayerKind::Raster { pixels, offset } = &node.kind {
                for coord in pixels.tile_coords() {
                    out.extend(shifted_tiles(coord, *offset));
                }
            }
        }
        out
    }

    /// The composited document for one tile, or `None` if it is empty there.
    pub fn composite_tile(&self, coord: TileCoord) -> Option<AnyTile> {
        Some(match self.bit_depth {
            BitDepth::U8 => ByDepth::U8(compositor::composite_tile(
                &build_nodes::<u8>(&self.layers),
                coord,
            )?),
            BitDepth::U16 => ByDepth::U16(compositor::composite_tile(
                &build_nodes::<u16>(&self.layers),
                coord,
            )?),
            BitDepth::F32 => ByDepth::F32(compositor::composite_tile(
                &build_nodes::<f32>(&self.layers),
                coord,
            )?),
        })
    }

    /// The composited document, cropped to the canvas.
    pub fn flatten(&self) -> Image {
        fn run<C: DepthChannel>(doc: &Document) -> Vec<Pixel<C>> {
            let nodes = build_nodes::<C>(&doc.layers);
            let canvas = doc.canvas();
            let on_canvas = compositor::covered_tiles(&nodes)
                .into_iter()
                .filter(|coord| {
                    let (x, y) = coord.origin();
                    Rect::new(x, y, crate::tile::TILE_SIZE, crate::tile::TILE_SIZE)
                        .intersect(&canvas)
                        .is_some()
                });
            compositor::composite_tiles(&nodes, on_canvas).read_rect(canvas)
        }
        let pixels = match self.bit_depth {
            BitDepth::U8 => ByDepth::U8(run::<u8>(self)),
            BitDepth::U16 => ByDepth::U16(run::<u16>(self)),
            BitDepth::F32 => ByDepth::F32(run::<f32>(self)),
        };
        Image {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

/// Destination tiles covered by source tile `coord` of a layer at `offset`.
pub(crate) fn shifted_tiles(coord: TileCoord, offset: (i32, i32)) -> Vec<TileCoord> {
    let size = crate::tile::TILE_SIZE as i32;
    let (x, y) = coord.origin();
    let (x0, y0) = (x.wrapping_add(offset.0), y.wrapping_add(offset.1));
    let (first, _) = TileCoord::of_pixel(x0, y0);
    let (last, _) = TileCoord::of_pixel(x0.wrapping_add(size - 1), y0.wrapping_add(size - 1));
    let mut out = Vec::with_capacity(4);
    for ty in first.ty..=last.ty {
        for tx in first.tx..=last.tx {
            out.push(TileCoord::new(tx, ty));
        }
    }
    out
}

/// A channel type that can be picked out of the run-time depth enums.
pub trait DepthChannel: Channel {
    fn raster(data: &PixelData) -> Option<&Raster<Self>>;
}

impl DepthChannel for u8 {
    fn raster(data: &PixelData) -> Option<&Raster<u8>> {
        match data {
            ByDepth::U8(r) => Some(r),
            _ => None,
        }
    }
}

impl DepthChannel for u16 {
    fn raster(data: &PixelData) -> Option<&Raster<u16>> {
        match data {
            ByDepth::U16(r) => Some(r),
            _ => None,
        }
    }
}

impl DepthChannel for f32 {
    fn raster(data: &PixelData) -> Option<&Raster<f32>> {
        match data {
            ByDepth::F32(r) => Some(r),
            _ => None,
        }
    }
}

/// Borrows the layer tree as compositor nodes.
fn build_nodes<C: DepthChannel>(layers: &[Layer]) -> Vec<Node<'_, C>> {
    layers
        .iter()
        .filter_map(|layer| match &layer.kind {
            LayerKind::Raster { pixels, offset } => C::raster(pixels).map(|raster| {
                Node::Layer(compositor::Layer {
                    raster,
                    offset: *offset,
                    opacity: layer.opacity,
                    blend: layer.blend,
                    visible: layer.visible,
                })
            }),
            LayerKind::Group {
                children,
                pass_through,
            } => Some(Node::Group(compositor::Group {
                children: build_nodes(children),
                opacity: layer.opacity,
                blend: if *pass_through {
                    GroupBlend::PassThrough
                } else {
                    GroupBlend::Isolated(layer.blend)
                },
                visible: layer.visible,
            })),
        })
        .collect()
}

// ----- Crate-private mutation, used only by commands and file loading ----

impl Document {
    fn id_in_use(&self, raw: u64) -> bool {
        self.all_layers().iter().any(|l| l.id.raw() == raw)
            || self.channels.iter().any(|c| c.id.raw() == raw)
            || self.paths.iter().any(|p| p.id.raw() == raw)
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Accepts an id that came with an item (undo, or a loaded file), or
    /// hands out a fresh one.
    fn adopt_id(&mut self, raw: u64) -> Result<u64, DocumentError> {
        if raw == 0 {
            return Ok(self.take_id());
        }
        if self.id_in_use(raw) {
            return Err(DocumentError::DuplicateId(raw));
        }
        self.next_id = self.next_id.max(raw + 1);
        Ok(raw)
    }

    fn siblings_mut(&mut self, parent: Option<LayerId>) -> Result<&mut Vec<Layer>, DocumentError> {
        let Some(parent) = parent else {
            return Ok(&mut self.layers);
        };
        match self.layer_mut(parent) {
            None => Err(DocumentError::LayerNotFound(parent)),
            Some(Layer {
                kind: LayerKind::Group { children, .. },
                ..
            }) => Ok(children),
            Some(_) => Err(DocumentError::NotAGroup(parent)),
        }
    }

    fn siblings(&self, parent: Option<LayerId>) -> Result<&[Layer], DocumentError> {
        let Some(parent) = parent else {
            return Ok(&self.layers);
        };
        match self.layer(parent) {
            None => Err(DocumentError::LayerNotFound(parent)),
            Some(Layer {
                kind: LayerKind::Group { children, .. },
                ..
            }) => Ok(children),
            Some(_) => Err(DocumentError::NotAGroup(parent)),
        }
    }

    pub(crate) fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        fn search(siblings: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
            for layer in siblings {
                if layer.id == id {
                    return Some(layer);
                }
                if let LayerKind::Group { children, .. } = &mut layer.kind {
                    if let Some(found) = search(children, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        search(&mut self.layers, id)
    }

    /// Checks a layer subtree can be added to this document.
    fn validate_layer(&self, layer: &Layer) -> Result<(), DocumentError> {
        let mut seen = BTreeSet::new();
        for node in layer.walk() {
            check_opacity(node.opacity)?;
            if let LayerKind::Raster { pixels, offset } = &node.kind {
                check_offset(*offset)?;
                if pixels.depth() != self.bit_depth {
                    return Err(DocumentError::DepthMismatch {
                        document: self.bit_depth,
                        data: pixels.depth(),
                    });
                }
            }
            let raw = node.id.raw();
            if raw != 0 && (self.id_in_use(raw) || !seen.insert(raw)) {
                return Err(DocumentError::DuplicateId(raw));
            }
        }
        Ok(())
    }

    /// Checks that [`Document::insert_layer`] would succeed, without
    /// changing anything.
    pub(crate) fn check_layer_insert(
        &self,
        parent: Option<LayerId>,
        index: usize,
        layer: &Layer,
    ) -> Result<(), DocumentError> {
        self.validate_layer(layer)?;
        let len = self.siblings(parent)?.len();
        if index > len {
            return Err(DocumentError::IndexOutOfRange { index, len });
        }
        Ok(())
    }

    /// Inserts `layer` (with any children) and returns its id. Layers whose
    /// id is [`LayerId::UNASSIGNED`] are given new ids.
    pub(crate) fn insert_layer(
        &mut self,
        parent: Option<LayerId>,
        index: usize,
        mut layer: Layer,
    ) -> Result<LayerId, DocumentError> {
        self.check_layer_insert(parent, index, &layer)?;
        fn assign(doc: &mut Document, layer: &mut Layer) {
            // Ids were checked above, so adopting cannot fail.
            layer.id = LayerId(
                doc.adopt_id(layer.id.raw())
                    .unwrap_or_else(|_| doc.take_id()),
            );
            if let LayerKind::Group { children, .. } = &mut layer.kind {
                for child in children {
                    assign(doc, child);
                }
            }
        }
        assign(self, &mut layer);
        let id = layer.id;
        self.siblings_mut(parent)?.insert(index, layer);
        Ok(id)
    }

    /// Removes a layer (with any children) and returns it and where it was.
    pub(crate) fn remove_layer(
        &mut self,
        id: LayerId,
    ) -> Result<(Layer, LayerLocation), DocumentError> {
        let location = self.locate(id).ok_or(DocumentError::LayerNotFound(id))?;
        let layer = self.siblings_mut(location.parent)?.remove(location.index);
        Ok((layer, location))
    }

    /// Moves a layer. `index` is its position among the new siblings after
    /// it has been taken out of its old place. Returns where it was.
    pub(crate) fn move_layer(
        &mut self,
        id: LayerId,
        parent: Option<LayerId>,
        index: usize,
    ) -> Result<LayerLocation, DocumentError> {
        let from = self.locate(id).ok_or(DocumentError::LayerNotFound(id))?;
        if let Some(parent) = parent {
            let target = self
                .layer(parent)
                .ok_or(DocumentError::LayerNotFound(parent))?;
            if !target.is_group() {
                return Err(DocumentError::NotAGroup(parent));
            }
            let moving = self.layer(id).ok_or(DocumentError::LayerNotFound(id))?;
            if moving.walk().iter().any(|l| l.id == parent) {
                return Err(DocumentError::WouldCycle(id));
            }
        }
        let same_parent = from.parent == parent;
        let len = self.siblings_mut(parent)?.len() - usize::from(same_parent);
        if index > len {
            return Err(DocumentError::IndexOutOfRange { index, len });
        }
        let layer = self.siblings_mut(from.parent)?.remove(from.index);
        self.siblings_mut(parent)?.insert(index, layer);
        Ok(from)
    }

    pub(crate) fn set_canvas_size(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32), DocumentError> {
        check_size(width, height)?;
        let old = (self.width, self.height);
        self.width = width;
        self.height = height;
        Ok(old)
    }

    pub(crate) fn set_resolution(
        &mut self,
        resolution: Resolution,
    ) -> Result<Resolution, DocumentError> {
        if !(resolution.ppi.is_finite() && resolution.ppi > 0.0) {
            return Err(DocumentError::InvalidResolution);
        }
        Ok(std::mem::replace(&mut self.resolution, resolution))
    }

    pub(crate) fn set_profile(&mut self, profile: Option<ColorProfile>) -> Option<ColorProfile> {
        std::mem::replace(&mut self.profile, profile)
    }

    pub(crate) fn set_metadata(&mut self, metadata: Metadata) -> Metadata {
        std::mem::replace(&mut self.metadata, metadata)
    }

    pub(crate) fn check_channel(
        &self,
        index: usize,
        channel: &AlphaChannel,
    ) -> Result<(), DocumentError> {
        if channel.data.depth() != self.bit_depth {
            return Err(DocumentError::DepthMismatch {
                document: self.bit_depth,
                data: channel.data.depth(),
            });
        }
        if index > self.channels.len() {
            return Err(DocumentError::IndexOutOfRange {
                index,
                len: self.channels.len(),
            });
        }
        let raw = channel.id.raw();
        if raw != 0 && self.id_in_use(raw) {
            return Err(DocumentError::DuplicateId(raw));
        }
        Ok(())
    }

    pub(crate) fn insert_channel(
        &mut self,
        index: usize,
        mut channel: AlphaChannel,
    ) -> Result<ChannelId, DocumentError> {
        self.check_channel(index, &channel)?;
        channel.id = ChannelId(self.adopt_id(channel.id.raw())?);
        let id = channel.id;
        self.channels.insert(index, channel);
        Ok(id)
    }

    pub(crate) fn remove_channel(
        &mut self,
        id: ChannelId,
    ) -> Result<(AlphaChannel, usize), DocumentError> {
        let index = self
            .channels
            .iter()
            .position(|c| c.id == id)
            .ok_or(DocumentError::ChannelNotFound(id))?;
        Ok((self.channels.remove(index), index))
    }

    pub(crate) fn check_path(&self, index: usize, path: &VectorPath) -> Result<(), DocumentError> {
        let finite = |p: &[f64; 2]| p[0].is_finite() && p[1].is_finite();
        let valid = path
            .subpaths
            .iter()
            .flat_map(|s| &s.anchors)
            .all(|a| finite(&a.point) && finite(&a.handle_in) && finite(&a.handle_out));
        if !valid {
            return Err(DocumentError::InvalidPath);
        }
        if index > self.paths.len() {
            return Err(DocumentError::IndexOutOfRange {
                index,
                len: self.paths.len(),
            });
        }
        let raw = path.id.raw();
        if raw != 0 && self.id_in_use(raw) {
            return Err(DocumentError::DuplicateId(raw));
        }
        Ok(())
    }

    pub(crate) fn insert_path(
        &mut self,
        index: usize,
        mut path: VectorPath,
    ) -> Result<PathId, DocumentError> {
        self.check_path(index, &path)?;
        path.id = PathId(self.adopt_id(path.id.raw())?);
        let id = path.id;
        self.paths.insert(index, path);
        Ok(id)
    }

    pub(crate) fn remove_path(&mut self, id: PathId) -> Result<(VectorPath, usize), DocumentError> {
        let index = self
            .paths
            .iter()
            .position(|p| p.id == id)
            .ok_or(DocumentError::PathNotFound(id))?;
        Ok((self.paths.remove(index), index))
    }

    /// Restores the id counter from a loaded file. Never lowers it below
    /// what the loaded items require.
    pub(crate) fn restore_next_id(&mut self, next_id: u64) {
        self.next_id = self.next_id.max(next_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_fills_exactly_the_canvas_and_shares_interior_tiles() {
        let doc = Document::with_background(600, 300, BitDepth::U8, [1.0, 1.0, 1.0, 1.0]).unwrap();
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.layers()[0].name, "Background");
        let ByDepth::U8(pixels) = doc.flatten().pixels else {
            panic!()
        };
        assert!(pixels.iter().all(|p| *p == [255, 255, 255, 255]));
        let LayerKind::Raster {
            pixels: ByDepth::U8(r),
            ..
        } = &doc.layers()[0].kind
        else {
            panic!()
        };
        assert_eq!(r.tile_count(), 6);
        assert_eq!(r.pixel(599, 299), [255, 255, 255, 255]);
        assert_eq!(r.pixel(600, 0), [0; 4], "nothing past the right edge");
        assert_eq!(r.pixel(0, 300), [0; 4], "nothing past the bottom edge");
        let a = r.tile(TileCoord::new(0, 0)).unwrap();
        let b = r.tile(TileCoord::new(1, 0)).unwrap();
        assert!(
            a.shares_data_with(b),
            "interior tiles should share one block"
        );
    }

    #[test]
    fn transparent_background_and_other_depths() {
        let doc = Document::with_background(10, 10, BitDepth::U16, [0.0, 0.0, 0.0, 0.0]).unwrap();
        let ByDepth::U16(pixels) = doc.flatten().pixels else {
            panic!()
        };
        assert!(pixels.iter().all(|p| *p == [0; 4]));
        let doc = Document::with_background(3, 2, BitDepth::F32, [0.5, 0.25, 1.0, 1.0]).unwrap();
        let ByDepth::F32(pixels) = doc.flatten().pixels else {
            panic!()
        };
        assert_eq!(pixels, vec![[0.5, 0.25, 1.0, 1.0]; 6]);
        assert!(Document::with_background(0, 10, BitDepth::U8, [1.0; 4]).is_err());
    }
}
