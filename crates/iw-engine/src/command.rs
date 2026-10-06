//! Commands: the only way to change a document.
//!
//! A [`Command`] is plain data describing one change. Applying it yields
//! the command that reverses it, and that inverse is what the history
//! keeps. Inverses hold only what changed: the previous value of a
//! property, the tiles an edit replaced, or the layer that was removed.
//! Nothing ever stores a copy of the whole document (architecture rule 4).
//!
//! Every command checks its inputs before touching the document, so a
//! failed command leaves the document exactly as it was.

use crate::blend::BlendMode;
use crate::document::{
    check_offset, check_opacity, shifted_tiles, AlphaChannel, ChannelId, ColorProfile, Document,
    DocumentError, Layer, LayerId, LayerKind, Metadata, PathId, PixelData, Resolution, VectorPath,
};
use crate::pixel::{ByDepth, Pixel, Texel};
use crate::raster::Grid;
use crate::tile::{Tile, TileCoord};
use std::collections::BTreeSet;

/// New contents for some tiles of a layer. `None` removes the tile.
pub type TileEdits<P> = Vec<(TileCoord, Option<Tile<P>>)>;
/// [`TileEdits`] at the document's bit depth.
pub type TilePatch = ByDepth<TileEdits<Pixel<u8>>, TileEdits<Pixel<u16>>, TileEdits<Pixel<f32>>>;

/// Changes to a layer's properties. Fields left as `None` are untouched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerProps {
    pub name: Option<String>,
    pub visible: Option<bool>,
    pub opacity: Option<f32>,
    pub blend: Option<BlendMode>,
    pub locked: Option<bool>,
    /// Raster layers only.
    pub offset: Option<(i32, i32)>,
    /// Groups only.
    pub pass_through: Option<bool>,
}

/// One change to a document.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Adds a layer (with any children) under `parent`, or at the top level
    /// for `None`. `index` counts from the bottom.
    AddLayer {
        parent: Option<LayerId>,
        index: usize,
        layer: Box<Layer>,
    },
    RemoveLayer {
        id: LayerId,
    },
    /// Moves a layer. `index` is its position among the new siblings once
    /// it has been taken out of its old place.
    MoveLayer {
        id: LayerId,
        parent: Option<LayerId>,
        index: usize,
    },
    SetLayerProps {
        id: LayerId,
        props: LayerProps,
    },
    /// Replaces or removes tiles of a raster layer. This is how every
    /// pixel edit reaches the document.
    ReplaceTiles {
        id: LayerId,
        tiles: TilePatch,
    },
    SetCanvasSize {
        width: u32,
        height: u32,
    },
    SetResolution(Resolution),
    SetProfile(Option<ColorProfile>),
    SetMetadata(Box<Metadata>),
    AddChannel {
        index: usize,
        channel: Box<AlphaChannel>,
    },
    RemoveChannel {
        id: ChannelId,
    },
    AddPath {
        index: usize,
        path: Box<VectorPath>,
    },
    RemovePath {
        id: PathId,
    },
    /// Several commands applied as one step: all succeed or none do, and
    /// they undo together.
    Batch {
        label: String,
        commands: Vec<Command>,
    },
}

impl Command {
    /// The name shown in the History panel.
    pub fn label(&self) -> String {
        match self {
            Command::AddLayer { layer, .. } if layer.is_group() => "New Group".into(),
            Command::AddLayer { .. } => "New Layer".into(),
            Command::RemoveLayer { .. } => "Delete Layer".into(),
            Command::MoveLayer { .. } => "Layer Order".into(),
            Command::SetLayerProps { props, .. } => props.label().into(),
            Command::ReplaceTiles { .. } => "Edit Pixels".into(),
            Command::SetCanvasSize { .. } => "Canvas Size".into(),
            Command::SetResolution(_) => "Resolution".into(),
            Command::SetProfile(_) => "Assign Profile".into(),
            Command::SetMetadata(_) => "Document Info".into(),
            Command::AddChannel { .. } => "New Channel".into(),
            Command::RemoveChannel { .. } => "Delete Channel".into(),
            Command::AddPath { .. } => "New Path".into(),
            Command::RemovePath { .. } => "Delete Path".into(),
            Command::Batch { label, .. } => label.clone(),
        }
    }
}

impl LayerProps {
    fn label(&self) -> &'static str {
        let set = [
            self.name.is_some(),
            self.visible.is_some(),
            self.opacity.is_some(),
            self.blend.is_some(),
            self.locked.is_some(),
            self.offset.is_some(),
            self.pass_through.is_some(),
        ];
        if set.iter().filter(|s| **s).count() != 1 {
            return "Layer Properties";
        }
        match set.iter().position(|s| *s) {
            Some(0) => "Name Change",
            Some(1) => "Layer Visibility",
            Some(2) => "Opacity Change",
            Some(3) => "Blending Change",
            Some(4) if self.locked == Some(true) => "Lock Layer",
            Some(4) => "Unlock Layer",
            Some(5) => "Move",
            _ => "Group Blending",
        }
    }
}

/// What part of the composited image a command changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Damage {
    /// Nothing visible changed.
    #[default]
    None,
    /// Only these tiles need recompositing.
    Tiles(BTreeSet<TileCoord>),
    /// Recomposite everything (the canvas itself changed).
    All,
}

impl Damage {
    fn add(&mut self, other: Damage) {
        *self = match (std::mem::take(self), other) {
            (Damage::All, _) | (_, Damage::All) => Damage::All,
            (Damage::None, d) | (d, Damage::None) => d,
            (Damage::Tiles(mut a), Damage::Tiles(b)) => {
                a.extend(b);
                Damage::Tiles(a)
            }
        };
    }

    fn tiles(tiles: BTreeSet<TileCoord>) -> Damage {
        if tiles.is_empty() {
            Damage::None
        } else {
            Damage::Tiles(tiles)
        }
    }
}

/// What applying a command did, besides changing the document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    pub damage: Damage,
    /// Ids of layers the command added (top of each added subtree).
    pub added_layers: Vec<LayerId>,
    pub added_channels: Vec<ChannelId>,
    pub added_paths: Vec<PathId>,
}

impl Effects {
    fn damage(damage: Damage) -> Self {
        Self {
            damage,
            ..Self::default()
        }
    }

    fn merge(&mut self, other: Effects) {
        self.damage.add(other.damage);
        self.added_layers.extend(other.added_layers);
        self.added_channels.extend(other.added_channels);
        self.added_paths.extend(other.added_paths);
    }
}

/// A successfully applied command.
pub(crate) struct Applied {
    /// The command that undoes it.
    pub inverse: Command,
    pub effects: Effects,
}

/// A command that could not be applied. The document is unchanged and the
/// command is handed back.
pub(crate) struct Rejected {
    pub error: DocumentError,
    pub command: Command,
}

fn swap_tiles<P: Texel>(grid: &mut Grid<P>, edits: TileEdits<P>) -> TileEdits<P> {
    let mut previous: TileEdits<P> = edits
        .into_iter()
        .map(|(coord, tile)| {
            let old = match tile {
                Some(tile) => grid.insert_tile(coord, tile),
                None => grid.remove_tile(coord),
            };
            (coord, old)
        })
        .collect();
    // If a coordinate appears twice, undoing must restore in reverse order.
    previous.reverse();
    previous
}

/// Applies `command` to `doc`.
pub(crate) fn apply(doc: &mut Document, command: Command) -> Result<Applied, Rejected> {
    // Each arm validates first and returns `reject` before any mutation.
    macro_rules! reject {
        ($error:expr, $command:expr) => {
            return Err(Rejected {
                error: $error,
                command: $command,
            })
        };
    }

    match command {
        Command::AddLayer {
            parent,
            index,
            layer,
        } => {
            if let Err(error) = doc.check_layer_insert(parent, index, &layer) {
                reject!(
                    error,
                    Command::AddLayer {
                        parent,
                        index,
                        layer
                    }
                );
            }
            let id = doc
                .insert_layer(parent, index, *layer)
                .expect("checked above");
            Ok(Applied {
                inverse: Command::RemoveLayer { id },
                effects: Effects {
                    damage: Damage::tiles(doc.layer_tiles(id)),
                    added_layers: vec![id],
                    ..Effects::default()
                },
            })
        }

        Command::RemoveLayer { id } => {
            let tiles = doc.layer_tiles(id);
            match doc.remove_layer(id) {
                Ok((layer, at)) => Ok(Applied {
                    inverse: Command::AddLayer {
                        parent: at.parent,
                        index: at.index,
                        layer: Box::new(layer),
                    },
                    effects: Effects::damage(Damage::tiles(tiles)),
                }),
                Err(error) => reject!(error, Command::RemoveLayer { id }),
            }
        }

        Command::MoveLayer { id, parent, index } => match doc.move_layer(id, parent, index) {
            Ok(from) => Ok(Applied {
                inverse: Command::MoveLayer {
                    id,
                    parent: from.parent,
                    index: from.index,
                },
                effects: Effects::damage(Damage::tiles(doc.layer_tiles(id))),
            }),
            Err(error) => reject!(error, Command::MoveLayer { id, parent, index }),
        },

        Command::SetLayerProps { id, props } => {
            let check = (|| {
                let layer = doc.layer(id).ok_or(DocumentError::LayerNotFound(id))?;
                if let Some(opacity) = props.opacity {
                    check_opacity(opacity)?;
                }
                if let Some(offset) = props.offset {
                    check_offset(offset)?;
                    if layer.is_group() {
                        return Err(DocumentError::NotARaster(id));
                    }
                    // A lock holds unless this same command lifts it.
                    if layer.locked && props.locked != Some(false) {
                        return Err(DocumentError::LayerLocked(id));
                    }
                }
                if props.pass_through.is_some() && !layer.is_group() {
                    return Err(DocumentError::NotAGroup(id));
                }
                Ok(())
            })();
            if let Err(error) = check {
                reject!(error, Command::SetLayerProps { id, props });
            }

            let mut tiles = doc.layer_tiles(id);
            let layer = doc.layer_mut(id).expect("checked above");
            let mut old = LayerProps::default();
            if let Some(name) = props.name {
                old.name = Some(std::mem::replace(&mut layer.name, name));
            }
            if let Some(visible) = props.visible {
                old.visible = Some(std::mem::replace(&mut layer.visible, visible));
            }
            if let Some(opacity) = props.opacity {
                old.opacity = Some(std::mem::replace(&mut layer.opacity, opacity));
            }
            if let Some(blend) = props.blend {
                old.blend = Some(std::mem::replace(&mut layer.blend, blend));
            }
            if let Some(locked) = props.locked {
                old.locked = Some(std::mem::replace(&mut layer.locked, locked));
            }
            if let (Some(new), LayerKind::Raster { offset, .. }) = (props.offset, &mut layer.kind) {
                old.offset = Some(std::mem::replace(offset, new));
            }
            if let (Some(new), LayerKind::Group { pass_through, .. }) =
                (props.pass_through, &mut layer.kind)
            {
                old.pass_through = Some(std::mem::replace(pass_through, new));
            }
            // A name or lock change repaints nothing; a move repaints both
            // the old and the new position.
            let visual = old.visible.is_some()
                || old.opacity.is_some()
                || old.blend.is_some()
                || old.offset.is_some()
                || old.pass_through.is_some();
            tiles.extend(doc.layer_tiles(id));
            Ok(Applied {
                inverse: Command::SetLayerProps { id, props: old },
                effects: Effects::damage(if visual {
                    Damage::tiles(tiles)
                } else {
                    Damage::None
                }),
            })
        }

        Command::ReplaceTiles { id, tiles } => {
            let check = (|| {
                let layer = doc.layer(id).ok_or(DocumentError::LayerNotFound(id))?;
                let LayerKind::Raster { pixels, .. } = &layer.kind else {
                    return Err(DocumentError::NotARaster(id));
                };
                if layer.locked {
                    return Err(DocumentError::LayerLocked(id));
                }
                if pixels.depth() != tiles.depth() {
                    return Err(DocumentError::DepthMismatch {
                        document: pixels.depth(),
                        data: tiles.depth(),
                    });
                }
                Ok(())
            })();
            if let Err(error) = check {
                reject!(error, Command::ReplaceTiles { id, tiles });
            }

            let layer = doc.layer_mut(id).expect("checked above");
            let LayerKind::Raster { pixels, offset } = &mut layer.kind else {
                unreachable!("checked above")
            };
            let mut touched = BTreeSet::new();
            let mut note = |coord: TileCoord| touched.extend(shifted_tiles(coord, *offset));
            let previous: TilePatch = match (pixels, tiles) {
                (PixelData::U8(r), TilePatch::U8(t)) => {
                    t.iter().for_each(|(c, _)| note(*c));
                    ByDepth::U8(swap_tiles(r, t))
                }
                (PixelData::U16(r), TilePatch::U16(t)) => {
                    t.iter().for_each(|(c, _)| note(*c));
                    ByDepth::U16(swap_tiles(r, t))
                }
                (PixelData::F32(r), TilePatch::F32(t)) => {
                    t.iter().for_each(|(c, _)| note(*c));
                    ByDepth::F32(swap_tiles(r, t))
                }
                _ => unreachable!("depths checked above"),
            };
            Ok(Applied {
                inverse: Command::ReplaceTiles {
                    id,
                    tiles: previous,
                },
                effects: Effects::damage(Damage::tiles(touched)),
            })
        }

        Command::SetCanvasSize { width, height } => match doc.set_canvas_size(width, height) {
            Ok((w, h)) => Ok(Applied {
                inverse: Command::SetCanvasSize {
                    width: w,
                    height: h,
                },
                effects: Effects::damage(Damage::All),
            }),
            Err(error) => reject!(error, Command::SetCanvasSize { width, height }),
        },

        Command::SetResolution(resolution) => match doc.set_resolution(resolution) {
            Ok(old) => Ok(Applied {
                inverse: Command::SetResolution(old),
                effects: Effects::default(),
            }),
            Err(error) => reject!(error, Command::SetResolution(resolution)),
        },

        Command::SetProfile(profile) => {
            let old = doc.set_profile(profile);
            // Reinterpreting the colours changes how everything looks.
            Ok(Applied {
                inverse: Command::SetProfile(old),
                effects: Effects::damage(Damage::All),
            })
        }

        Command::SetMetadata(metadata) => {
            let old = doc.set_metadata(*metadata);
            Ok(Applied {
                inverse: Command::SetMetadata(Box::new(old)),
                effects: Effects::default(),
            })
        }

        Command::AddChannel { index, channel } => {
            let check = doc.check_channel(index, &channel);
            if let Err(error) = check {
                reject!(error, Command::AddChannel { index, channel });
            }
            let id = doc.insert_channel(index, *channel).expect("checked above");
            Ok(Applied {
                inverse: Command::RemoveChannel { id },
                effects: Effects {
                    added_channels: vec![id],
                    ..Effects::default()
                },
            })
        }

        Command::RemoveChannel { id } => match doc.remove_channel(id) {
            Ok((channel, index)) => Ok(Applied {
                inverse: Command::AddChannel {
                    index,
                    channel: Box::new(channel),
                },
                effects: Effects::default(),
            }),
            Err(error) => reject!(error, Command::RemoveChannel { id }),
        },

        Command::AddPath { index, path } => {
            if let Err(error) = doc.check_path(index, &path) {
                reject!(error, Command::AddPath { index, path });
            }
            let id = doc.insert_path(index, *path).expect("checked above");
            Ok(Applied {
                inverse: Command::RemovePath { id },
                effects: Effects {
                    added_paths: vec![id],
                    ..Effects::default()
                },
            })
        }

        Command::RemovePath { id } => match doc.remove_path(id) {
            Ok((path, index)) => Ok(Applied {
                inverse: Command::AddPath {
                    index,
                    path: Box::new(path),
                },
                effects: Effects::default(),
            }),
            Err(error) => reject!(error, Command::RemovePath { id }),
        },

        Command::Batch { label, commands } => {
            let mut inverses = Vec::with_capacity(commands.len());
            let mut effects = Effects::default();
            let mut pending = commands.into_iter();
            while let Some(command) = pending.next() {
                match apply(doc, command) {
                    Ok(applied) => {
                        inverses.push(applied.inverse);
                        effects.merge(applied.effects);
                    }
                    Err(rejected) => {
                        // Roll back what was done. Undoing each inverse
                        // gives back a command equivalent to the original.
                        let mut restored = Vec::with_capacity(inverses.len() + 1);
                        while let Some(inverse) = inverses.pop() {
                            let undone = apply(doc, inverse).unwrap_or_else(|_| {
                                unreachable!("the inverse of an applied command applies")
                            });
                            restored.push(undone.inverse);
                        }
                        restored.reverse();
                        restored.push(rejected.command);
                        restored.extend(pending);
                        reject!(
                            rejected.error,
                            Command::Batch {
                                label,
                                commands: restored
                            }
                        );
                    }
                }
            }
            inverses.reverse();
            Ok(Applied {
                inverse: Command::Batch {
                    label,
                    commands: inverses,
                },
                effects,
            })
        }
    }
}
