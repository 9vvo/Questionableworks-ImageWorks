//! Editing sessions and undo history.
//!
//! A [`Session`] owns a document and is the only thing that can change it.
//! Every change is a [`Command`]; the session keeps the inverse of each one
//! so it can be undone.

use crate::command::{apply, Command, Effects};
use crate::document::{Document, DocumentError};

/// Default number of undo steps kept.
pub const DEFAULT_HISTORY_LIMIT: usize = 50;

#[derive(Debug)]
struct Entry {
    label: String,
    /// Applying this moves the document across the entry: backwards if the
    /// entry is on the undo stack, forwards if it is on the redo stack.
    step: Command,
    /// Document state ids on either side of the entry.
    before: u64,
    after: u64,
}

/// One row of the History panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryState {
    pub label: String,
    /// False for states that have been undone and can be redone.
    pub applied: bool,
}

/// A document being edited, with its undo history.
#[derive(Debug)]
pub struct Session {
    document: Document,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    limit: usize,
    /// Identifies the document's current contents. Every executed command
    /// produces a new id; undo and redo return to earlier ones.
    state: u64,
    next_state: u64,
    saved_state: Option<u64>,
}

impl Session {
    /// Starts editing `document`. A document that has never been saved
    /// counts as modified.
    pub fn new(document: Document) -> Self {
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            limit: DEFAULT_HISTORY_LIMIT,
            state: 0,
            next_state: 1,
            saved_state: None,
        }
    }

    /// Starts editing a document that was just loaded from, or saved to, a
    /// file, so it does not count as modified.
    pub fn opened(document: Document) -> Self {
        let mut session = Self::new(document);
        session.mark_saved();
        session
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn into_document(self) -> Document {
        self.document
    }

    /// Applies a command. On error the document and history are unchanged.
    pub fn execute(&mut self, command: Command) -> Result<Effects, DocumentError> {
        let label = command.label();
        let applied = apply(&mut self.document, command).map_err(|rejected| rejected.error)?;
        let before = self.state;
        self.state = self.next_state;
        self.next_state += 1;
        self.undo.push(Entry {
            label,
            step: applied.inverse,
            before,
            after: self.state,
        });
        self.redo.clear();
        self.trim();
        Ok(applied.effects)
    }

    /// Undoes the latest command. Returns `None` if there is nothing to undo.
    pub fn undo(&mut self) -> Option<Effects> {
        let entry = self.undo.pop()?;
        match apply(&mut self.document, entry.step) {
            Ok(applied) => {
                self.state = entry.before;
                self.redo.push(Entry {
                    step: applied.inverse,
                    ..entry
                });
                Some(applied.effects)
            }
            Err(rejected) => {
                // An inverse always applies to the state it was made for;
                // reaching here is an engine bug. Keep the entry so no
                // history is lost, and report nothing undone.
                debug_assert!(false, "undo step was rejected: {}", rejected.error);
                self.undo.push(Entry {
                    step: rejected.command,
                    ..entry
                });
                None
            }
        }
    }

    /// Redoes the most recently undone command.
    pub fn redo(&mut self) -> Option<Effects> {
        let entry = self.redo.pop()?;
        match apply(&mut self.document, entry.step) {
            Ok(applied) => {
                self.state = entry.after;
                self.undo.push(Entry {
                    step: applied.inverse,
                    ..entry
                });
                Some(applied.effects)
            }
            Err(rejected) => {
                debug_assert!(false, "redo step was rejected: {}", rejected.error);
                self.redo.push(Entry {
                    step: rejected.command,
                    ..entry
                });
                None
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// History states, oldest first: applied ones, then undone ones.
    pub fn states(&self) -> Vec<HistoryState> {
        let applied = self.undo.iter().map(|e| HistoryState {
            label: e.label.clone(),
            applied: true,
        });
        let undone = self.redo.iter().rev().map(|e| HistoryState {
            label: e.label.clone(),
            applied: false,
        });
        applied.chain(undone).collect()
    }

    /// Number of applied states, i.e. how many times undo can run.
    pub fn position(&self) -> usize {
        self.undo.len()
    }

    /// Undoes or redoes until exactly `position` states are applied, as when
    /// a row of the History panel is clicked. `0` is the oldest state the
    /// history can still reach. Returns the combined effects.
    pub fn jump_to(&mut self, position: usize) -> Vec<Effects> {
        let mut effects = Vec::new();
        while self.undo.len() > position {
            match self.undo() {
                Some(e) => effects.push(e),
                None => break,
            }
        }
        while self.undo.len() < position {
            match self.redo() {
                Some(e) => effects.push(e),
                None => break,
            }
        }
        effects
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Sets how many undo steps are kept (at least 1), dropping the oldest
    /// if there are now too many.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        self.trim();
    }

    fn trim(&mut self) {
        if self.undo.len() > self.limit {
            let excess = self.undo.len() - self.limit;
            self.undo.drain(..excess);
        }
    }

    /// Records that the current contents are what is on disk.
    pub fn mark_saved(&mut self) {
        self.saved_state = Some(self.state);
    }

    /// True if the document differs from what was last saved.
    pub fn is_modified(&self) -> bool {
        self.saved_state != Some(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::BlendMode;
    use crate::command::{Damage, LayerProps, TilePatch};
    use crate::document::{
        AlphaChannel, Anchor, ChannelId, ColorProfile, Layer, LayerId, LayerKind, MaskData,
        Metadata, PathId, PixelData, Resolution, Subpath, VectorPath,
    };
    use crate::geom::Rect;
    use crate::pixel::{BitDepth, ByDepth};
    use crate::raster::Raster;
    use crate::tile::{Tile, TileCoord};

    /// `assert_eq!` for documents, without printing megabytes of pixels.
    macro_rules! assert_doc {
        ($a:expr, $b:expr) => {
            assert!($a == $b, "documents differ")
        };
        ($a:expr, $b:expr, $($msg:tt)+) => {
            assert!($a == $b, $($msg)+)
        };
    }

    fn painted(seed: u8, tiles: &[(i32, i32)]) -> PixelData {
        let mut r: Raster<u8> = Raster::new();
        for (i, (tx, ty)) in tiles.iter().enumerate() {
            let v = seed.wrapping_add(i as u8 * 17);
            r.insert_tile(
                TileCoord::new(*tx, *ty),
                Tile::filled([v, v / 2, v / 3, 255]),
            );
        }
        ByDepth::U8(r)
    }

    fn add(session: &mut Session, parent: Option<LayerId>, index: usize, layer: Layer) -> LayerId {
        let effects = session
            .execute(Command::AddLayer {
                parent,
                index,
                layer: Box::new(layer),
            })
            .unwrap();
        effects.added_layers[0]
    }

    /// A session with a small tree:
    /// `[bg, group[inner_a, inner_b], top]`.
    fn sample() -> (Session, [LayerId; 5]) {
        let mut s = Session::new(Document::new(600, 400, BitDepth::U8).unwrap());
        let bg = add(
            &mut s,
            None,
            0,
            Layer::raster("Background", painted(10, &[(0, 0), (1, 0), (0, 1), (1, 1)])),
        );
        let group = add(&mut s, None, 1, Layer::group("Group"));
        let a = add(
            &mut s,
            Some(group),
            0,
            Layer::raster("A", painted(60, &[(0, 0)])),
        );
        let b = add(
            &mut s,
            Some(group),
            1,
            Layer::raster("B", painted(90, &[(1, 1)])),
        );
        let top = add(
            &mut s,
            None,
            2,
            Layer::raster("Top", painted(120, &[(2, 0)])),
        );
        (s, [bg, group, a, b, top])
    }

    /// The contract from the charter: apply, undo, expect the starting
    /// state; redo, expect the applied state.
    fn assert_round_trips(session: &mut Session, command: Command) {
        let start = session.document().clone();
        session.execute(command).expect("command should apply");
        let applied = session.document().clone();
        assert!(applied != start, "command changed nothing");
        assert!(session.undo().is_some());
        assert_doc!(
            *session.document(),
            start,
            "undo did not restore the starting state"
        );
        assert!(session.redo().is_some());
        assert_doc!(
            *session.document(),
            applied,
            "redo did not restore the applied state"
        );
        // And once more, to catch inverses that only work the first time.
        session.undo();
        assert_doc!(*session.document(), start);
        session.redo();
        assert_doc!(*session.document(), applied);
    }

    fn props(f: impl FnOnce(&mut LayerProps)) -> LayerProps {
        let mut p = LayerProps::default();
        f(&mut p);
        p
    }

    #[test]
    fn every_command_undoes_and_redoes_exactly() {
        let (mut s, [bg, group, a, b, top]) = sample();

        let commands = vec![
            Command::AddLayer {
                parent: Some(group),
                index: 1,
                layer: Box::new(Layer::raster("New", painted(200, &[(3, 3)]))),
            },
            Command::AddLayer {
                parent: None,
                index: 0,
                layer: Box::new(Layer::group("Empty group")),
            },
            Command::RemoveLayer { id: a },
            Command::RemoveLayer { id: group },
            Command::MoveLayer {
                id: top,
                parent: None,
                index: 0,
            },
            Command::MoveLayer {
                id: bg,
                parent: Some(group),
                index: 2,
            },
            Command::MoveLayer {
                id: b,
                parent: None,
                index: 3,
            },
            Command::SetLayerProps {
                id: bg,
                props: props(|p| p.name = Some("Renamed".into())),
            },
            Command::SetLayerProps {
                id: bg,
                props: props(|p| p.visible = Some(false)),
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.opacity = Some(0.25)),
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.blend = Some(BlendMode::Screen)),
            },
            Command::SetLayerProps {
                id: top,
                props: props(|p| p.locked = Some(true)),
            },
            Command::SetLayerProps {
                id: top,
                props: props(|p| p.offset = Some((-37, 512))),
            },
            Command::SetLayerProps {
                id: group,
                props: props(|p| p.pass_through = Some(false)),
            },
            Command::SetLayerProps {
                id: group,
                props: props(|p| {
                    p.name = Some("Both".into());
                    p.opacity = Some(0.5);
                    p.blend = Some(BlendMode::Multiply);
                }),
            },
            Command::ReplaceTiles {
                id: bg,
                tiles: ByDepth::U8(vec![
                    (TileCoord::new(0, 0), Some(Tile::filled([1, 2, 3, 255]))), // replace
                    (TileCoord::new(1, 1), None),                               // remove
                    (TileCoord::new(5, 5), Some(Tile::filled([9, 9, 9, 9]))),   // add
                ]),
            },
            Command::SetCanvasSize {
                width: 1000,
                height: 50,
            },
            Command::SetResolution(Resolution { ppi: 300.0 }),
            Command::SetProfile(Some(ColorProfile {
                name: "Display P3".into(),
                icc: vec![1, 2, 3],
            })),
            Command::SetMetadata(Box::new(Metadata {
                title: "Hello".into(),
                created: Some(1_700_000_000),
                ..Metadata::default()
            })),
            Command::AddChannel {
                index: 0,
                channel: Box::new(AlphaChannel {
                    id: ChannelId::UNASSIGNED,
                    name: "Alpha 1".into(),
                    data: MaskData::empty(BitDepth::U8),
                }),
            },
            Command::AddPath {
                index: 0,
                path: Box::new(VectorPath {
                    id: PathId::UNASSIGNED,
                    name: "Path 1".into(),
                    subpaths: vec![Subpath {
                        closed: true,
                        anchors: vec![Anchor {
                            point: [1.0, 2.0],
                            handle_in: [0.0, 2.0],
                            handle_out: [2.0, 2.0],
                        }],
                    }],
                }),
            },
            Command::Batch {
                label: "Brush Tool".into(),
                commands: vec![
                    Command::ReplaceTiles {
                        id: a,
                        tiles: ByDepth::U8(vec![(
                            TileCoord::new(0, 0),
                            Some(Tile::filled([7, 7, 7, 7])),
                        )]),
                    },
                    Command::SetLayerProps {
                        id: a,
                        props: props(|p| p.name = Some("Painted".into())),
                    },
                ],
            },
        ];
        for command in commands {
            let what = format!("{command:?}");
            let what = &what[..what.len().min(60)];
            // Each command starts from the same sample document.
            let before = s.document().clone();
            assert_round_trips(&mut s, command);
            s.undo();
            assert_doc!(*s.document(), before, "left a mess after {what}");
        }

        // Removing a channel and a path needs them to exist first.
        s.execute(Command::AddChannel {
            index: 0,
            channel: Box::new(AlphaChannel {
                id: ChannelId::UNASSIGNED,
                name: "C".into(),
                data: MaskData::empty(BitDepth::U8),
            }),
        })
        .unwrap();
        let channel = s.document().channels()[0].id;
        assert_round_trips(&mut s, Command::RemoveChannel { id: channel });
        s.execute(Command::AddPath {
            index: 0,
            path: Box::new(VectorPath {
                id: PathId::UNASSIGNED,
                name: "P".into(),
                subpaths: vec![],
            }),
        })
        .unwrap();
        let path = s.document().paths()[0].id;
        assert_round_trips(&mut s, Command::RemovePath { id: path });
    }

    #[test]
    fn undoing_everything_returns_to_the_empty_document_and_redo_rebuilds_it() {
        let empty = Document::new(600, 400, BitDepth::U8).unwrap();
        let (mut s, [bg, group, a, _b, top]) = sample();
        s.execute(Command::MoveLayer {
            id: top,
            parent: Some(group),
            index: 0,
        })
        .unwrap();
        s.execute(Command::SetLayerProps {
            id: bg,
            props: props(|p| p.opacity = Some(0.3)),
        })
        .unwrap();
        s.execute(Command::RemoveLayer { id: a }).unwrap();
        let built = s.document().clone();

        let steps = s.position();
        assert_eq!(steps, 8);
        while s.undo().is_some() {}
        // Ids are never reused, so the counter is the one thing undo keeps.
        assert_eq!(s.document().layers(), empty.layers());
        assert_eq!(s.document().all_layers().len(), 0);
        while s.redo().is_some() {}
        assert_doc!(*s.document(), built);
        assert_eq!(s.position(), steps);
    }

    #[test]
    fn layer_ids_survive_undo_and_are_never_reused() {
        let (mut s, [_, _, a, _, _]) = sample();
        s.execute(Command::RemoveLayer { id: a }).unwrap();
        s.undo();
        assert_eq!(s.document().layer(a).unwrap().name, "A");
        s.redo();
        let fresh = add(
            &mut s,
            None,
            0,
            Layer::raster("Fresh", PixelData::empty(BitDepth::U8)),
        );
        assert!(fresh.raw() > a.raw());
        assert!(s.document().all_layers().iter().all(|l| l.id != a));
    }

    #[test]
    fn duplicate_gets_new_ids_for_the_whole_subtree() {
        let (mut s, [_, group, a, b, _]) = sample();
        let copy = s.document().layer(group).unwrap().duplicate();
        let new_group = add(&mut s, None, 3, copy);
        let ids: Vec<LayerId> = s
            .document()
            .layer(new_group)
            .unwrap()
            .walk()
            .iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&group) && !ids.contains(&a) && !ids.contains(&b));
        let all: std::collections::HashSet<_> =
            s.document().all_layers().iter().map(|l| l.id).collect();
        assert_eq!(all.len(), 8, "ids must be unique");
    }

    /// Architecture rule 4: history holds deltas, not document copies.
    #[test]
    fn a_pixel_edit_keeps_only_the_tiles_it_replaced() {
        let (mut s, [bg, ..]) = sample();
        let before = s.document().clone();
        let edited = TileCoord::new(1, 0);
        s.execute(Command::ReplaceTiles {
            id: bg,
            tiles: ByDepth::U8(vec![(edited, Some(Tile::filled([255, 0, 0, 255])))]),
        })
        .unwrap();

        // The history entry holds exactly one tile: the old version.
        let Command::ReplaceTiles {
            tiles: TilePatch::U8(kept),
            ..
        } = &s.undo.last().unwrap().step
        else {
            panic!("expected a tile patch");
        };
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, edited);

        // And the other three tiles of the layer were not copied: the
        // document still shares them with the pre-edit clone.
        let raster = |d: &Document| match &d.layer(bg).unwrap().kind {
            LayerKind::Raster {
                pixels: PixelData::U8(r),
                ..
            } => r.clone(),
            _ => unreachable!(),
        };
        let (old, new) = (raster(&before), raster(s.document()));
        for coord in old.tile_coords() {
            let shared = old
                .tile(coord)
                .unwrap()
                .shares_data_with(new.tile(coord).unwrap());
            assert_eq!(shared, coord != edited, "tile {coord:?}");
        }
    }

    #[test]
    fn rejected_commands_change_nothing() {
        let (mut s, [bg, group, a, _, top]) = sample();
        let before = s.document().clone();
        let history = s.states();
        let missing = LayerId::from_raw(9999);
        let u16_layer = Layer::raster("16-bit", PixelData::empty(BitDepth::U16));
        s.execute(Command::SetLayerProps {
            id: top,
            props: props(|p| p.locked = Some(true)),
        })
        .unwrap();
        let locked_doc = s.document().clone();

        let bad = vec![
            Command::RemoveLayer { id: missing },
            Command::AddLayer {
                parent: Some(bg),
                index: 0,
                layer: Box::new(Layer::group("In a raster")),
            },
            Command::AddLayer {
                parent: None,
                index: 99,
                layer: Box::new(Layer::group("Past the end")),
            },
            Command::AddLayer {
                parent: None,
                index: 0,
                layer: Box::new(u16_layer),
            },
            Command::MoveLayer {
                id: group,
                parent: Some(group),
                index: 0,
            },
            Command::MoveLayer {
                id: bg,
                parent: Some(a),
                index: 0,
            },
            Command::MoveLayer {
                id: bg,
                parent: None,
                index: 3,
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.opacity = Some(1.5)),
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.opacity = Some(f32::NAN)),
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.pass_through = Some(true)),
            },
            Command::SetLayerProps {
                id: group,
                props: props(|p| p.offset = Some((1, 1))),
            },
            Command::SetLayerProps {
                id: a,
                props: props(|p| p.offset = Some((i32::MAX, 0))),
            },
            Command::SetLayerProps {
                id: top,
                props: props(|p| p.offset = Some((5, 5))),
            },
            Command::ReplaceTiles {
                id: top,
                tiles: ByDepth::U8(vec![(TileCoord::new(0, 0), None)]),
            },
            Command::ReplaceTiles {
                id: group,
                tiles: ByDepth::U8(vec![]),
            },
            Command::ReplaceTiles {
                id: bg,
                tiles: ByDepth::U16(vec![]),
            },
            Command::SetCanvasSize {
                width: 0,
                height: 10,
            },
            Command::SetResolution(Resolution { ppi: 0.0 }),
            Command::RemoveChannel {
                id: ChannelId::from_raw(1),
            },
            Command::RemovePath {
                id: PathId::from_raw(1),
            },
        ];
        for command in bad {
            let what = format!("{command:?}");
            assert!(
                s.execute(command).is_err(),
                "should have been rejected: {what}"
            );
            assert_doc!(
                *s.document(),
                locked_doc,
                "rejected command changed the document: {what}"
            );
            assert_eq!(s.states().len(), history.len() + 1);
        }
        s.undo();
        assert_doc!(*s.document(), before);
    }

    #[test]
    fn a_failing_batch_rolls_back_what_it_had_done() {
        let (mut s, [bg, _, a, _, _]) = sample();
        let before = s.document().clone();
        let steps = s.position();
        let result = s.execute(Command::Batch {
            label: "Doomed".into(),
            commands: vec![
                Command::SetLayerProps {
                    id: bg,
                    props: props(|p| p.name = Some("changed".into())),
                },
                Command::RemoveLayer { id: a },
                Command::RemoveLayer {
                    id: LayerId::from_raw(9999),
                },
                Command::SetCanvasSize {
                    width: 1,
                    height: 1,
                },
            ],
        });
        assert_eq!(
            result,
            Err(DocumentError::LayerNotFound(LayerId::from_raw(9999)))
        );
        assert_doc!(*s.document(), before);
        assert_eq!(s.position(), steps);
    }

    #[test]
    fn unlocking_and_moving_in_one_command_is_allowed() {
        let (mut s, [.., top]) = sample();
        s.execute(Command::SetLayerProps {
            id: top,
            props: props(|p| p.locked = Some(true)),
        })
        .unwrap();
        s.execute(Command::SetLayerProps {
            id: top,
            props: props(|p| {
                p.locked = Some(false);
                p.offset = Some((3, 4));
            }),
        })
        .unwrap();
        // A locked layer can still be renamed, hidden, reordered and deleted.
        s.execute(Command::SetLayerProps {
            id: top,
            props: props(|p| p.locked = Some(true)),
        })
        .unwrap();
        s.execute(Command::SetLayerProps {
            id: top,
            props: props(|p| p.visible = Some(false)),
        })
        .unwrap();
        s.execute(Command::MoveLayer {
            id: top,
            parent: None,
            index: 0,
        })
        .unwrap();
        s.execute(Command::RemoveLayer { id: top }).unwrap();
    }

    #[test]
    fn a_new_command_discards_the_redo_states() {
        let (mut s, [bg, ..]) = sample();
        s.undo();
        s.undo();
        assert!(s.can_redo());
        s.execute(Command::SetLayerProps {
            id: bg,
            props: props(|p| p.visible = Some(false)),
        })
        .unwrap();
        assert!(!s.can_redo());
        assert!(s.redo().is_none());
    }

    #[test]
    fn history_states_and_jumping() {
        let (mut s, [bg, ..]) = sample();
        s.execute(Command::SetLayerProps {
            id: bg,
            props: props(|p| p.opacity = Some(0.5)),
        })
        .unwrap();
        let labels: Vec<String> = s.states().into_iter().map(|h| h.label).collect();
        assert_eq!(
            labels,
            [
                "New Layer",
                "New Group",
                "New Layer",
                "New Layer",
                "New Layer",
                "Opacity Change"
            ]
        );

        let full = s.document().clone();
        s.jump_to(2);
        assert_eq!(s.position(), 2);
        assert_eq!(s.document().all_layers().len(), 2);
        let applied: Vec<bool> = s.states().into_iter().map(|h| h.applied).collect();
        assert_eq!(applied, [true, true, false, false, false, false]);
        // Undone states keep their order.
        assert_eq!(s.states()[5].label, "Opacity Change");

        s.jump_to(6);
        assert_doc!(*s.document(), full);
        s.jump_to(100);
        assert_eq!(s.position(), 6);
    }

    #[test]
    fn the_limit_drops_the_oldest_states() {
        let (mut s, [bg, ..]) = sample();
        s.set_limit(3);
        assert_eq!(s.position(), 3);
        for i in 0..10 {
            s.execute(Command::SetLayerProps {
                id: bg,
                props: props(|p| p.opacity = Some(i as f32 / 10.0)),
            })
            .unwrap();
        }
        assert_eq!(s.position(), 3);
        while s.undo().is_some() {}
        // Three steps back from opacity 0.9 is 0.6.
        assert_eq!(s.document().layer(bg).unwrap().opacity, 0.6);
    }

    #[test]
    fn modified_tracking_follows_undo_and_redo() {
        let (mut s, [bg, ..]) = sample();
        assert!(s.is_modified(), "a new document has never been saved");
        s.mark_saved();
        assert!(!s.is_modified());
        s.execute(Command::SetLayerProps {
            id: bg,
            props: props(|p| p.visible = Some(false)),
        })
        .unwrap();
        assert!(s.is_modified());
        s.undo();
        assert!(
            !s.is_modified(),
            "undoing back to the saved state is not a modification"
        );
        s.redo();
        assert!(s.is_modified());
        s.undo();
        s.undo();
        assert!(
            s.is_modified(),
            "before the saved state is also different from it"
        );

        let opened = Session::opened(Document::new(1, 1, BitDepth::U8).unwrap());
        assert!(!opened.is_modified());
    }

    #[test]
    fn damage_reports_what_to_redraw() {
        let (mut s, [bg, _, a, _, top]) = sample();
        let tiles = |d: Damage| match d {
            Damage::Tiles(t) => t.into_iter().collect::<Vec<_>>(),
            other => panic!("expected tiles, got {other:?}"),
        };

        // A rename repaints nothing.
        let e = s
            .execute(Command::SetLayerProps {
                id: a,
                props: props(|p| p.name = Some("x".into())),
            })
            .unwrap();
        assert_eq!(e.damage, Damage::None);

        // An opacity change repaints the layer's tiles.
        let e = s
            .execute(Command::SetLayerProps {
                id: a,
                props: props(|p| p.opacity = Some(0.5)),
            })
            .unwrap();
        assert_eq!(tiles(e.damage), [TileCoord::new(0, 0)]);

        // A move repaints where it was and where it is now.
        let e = s
            .execute(Command::SetLayerProps {
                id: top,
                props: props(|p| p.offset = Some((-256, 256))),
            })
            .unwrap();
        assert_eq!(
            tiles(e.damage),
            [TileCoord::new(2, 0), TileCoord::new(1, 1)]
        );

        // A pixel edit repaints the edited tiles, shifted by the layer offset.
        let e = s
            .execute(Command::ReplaceTiles {
                id: top,
                tiles: ByDepth::U8(vec![(
                    TileCoord::new(2, 0),
                    Some(Tile::filled([1, 1, 1, 1])),
                )]),
            })
            .unwrap();
        assert_eq!(tiles(e.damage), [TileCoord::new(1, 1)]);

        // Undo reports the same area.
        assert_eq!(tiles(s.undo().unwrap().damage), [TileCoord::new(1, 1)]);

        let e = s
            .execute(Command::SetCanvasSize {
                width: 10,
                height: 10,
            })
            .unwrap();
        assert_eq!(e.damage, Damage::All);
        let _ = bg;
    }

    #[test]
    fn document_composites_its_layer_tree() {
        let mut s = Session::new(Document::new(4, 4, BitDepth::U8).unwrap());
        let grey = |v: u8| {
            let mut r: Raster<u8> = Raster::new();
            r.write_rect(Rect::new(0, 0, 4, 4), &[[v, v, v, 255]; 16]);
            ByDepth::U8(r)
        };
        add(&mut s, None, 0, Layer::raster("bottom", grey(102))); // 0.4
        let group = add(&mut s, None, 1, Layer::group("group"));
        let mut top = Layer::raster("top", grey(153)); // 0.6
        top.blend = BlendMode::Multiply;
        add(&mut s, Some(group), 0, top);

        let flat = |s: &Session| match s.document().flatten().pixels {
            ByDepth::U8(p) => p,
            _ => unreachable!(),
        };
        // Pass-through group: multiply reaches the bottom layer (0.24).
        assert_eq!(flat(&s), vec![[61, 61, 61, 255]; 16]);
        // Isolated: the layer has nothing to multiply with.
        s.execute(Command::SetLayerProps {
            id: group,
            props: props(|p| p.pass_through = Some(false)),
        })
        .unwrap();
        assert_eq!(flat(&s), vec![[153, 153, 153, 255]; 16]);
        // Flatten crops to the canvas.
        s.execute(Command::SetCanvasSize {
            width: 2,
            height: 6,
        })
        .unwrap();
        let image = s.document().flatten();
        assert_eq!((image.width, image.height), (2, 6));
        let pixels = flat(&s);
        assert_eq!(pixels[0], [153, 153, 153, 255]);
        assert_eq!(
            pixels[2 * 4],
            [0; 4],
            "below the painted area is transparent"
        );
    }

    /// Throws a long random mix of commands (many of them invalid) at a
    /// session, then checks every state on the way back and forward again.
    #[test]
    fn random_command_sequences_undo_and_redo_exactly() {
        struct Rng(u64);
        impl Rng {
            fn next(&mut self, bound: usize) -> usize {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((self.0 >> 33) as usize) % bound.max(1)
            }
        }

        for seed in 1..=4u64 {
            let mut rng = Rng(seed);
            let (mut s, _) = sample();
            s.set_limit(10_000);
            let mut snapshots = vec![s.document().clone()];
            let base = s.position();

            for _ in 0..400 {
                let ids: Vec<LayerId> = s.document().all_layers().iter().map(|l| l.id).collect();
                let pick = |rng: &mut Rng| {
                    ids.get(rng.next(ids.len()))
                        .copied()
                        .unwrap_or(LayerId::from_raw(1))
                };
                let parent = |rng: &mut Rng| {
                    if rng.next(2) == 0 {
                        None
                    } else {
                        Some(pick(rng))
                    }
                };
                let id = pick(&mut rng);
                let command = match rng.next(9) {
                    0 => Command::AddLayer {
                        parent: parent(&mut rng),
                        index: rng.next(4),
                        layer: Box::new(Layer::raster(
                            "r",
                            painted(rng.next(255) as u8, &[(rng.next(3) as i32, 0)]),
                        )),
                    },
                    1 => Command::AddLayer {
                        parent: parent(&mut rng),
                        index: rng.next(4),
                        layer: Box::new(Layer::group("g")),
                    },
                    2 if ids.len() > 3 => Command::RemoveLayer { id },
                    3 | 4 => Command::MoveLayer {
                        id,
                        parent: parent(&mut rng),
                        index: rng.next(4),
                    },
                    5 => Command::SetLayerProps {
                        id,
                        props: props(|p| {
                            p.opacity = Some(rng.next(11) as f32 / 10.0);
                            p.visible = Some(rng.next(2) == 0);
                        }),
                    },
                    6 => Command::SetLayerProps {
                        id,
                        props: props(|p| {
                            p.offset =
                                Some((rng.next(600) as i32 - 300, rng.next(600) as i32 - 300))
                        }),
                    },
                    7 => Command::ReplaceTiles {
                        id,
                        tiles: ByDepth::U8(vec![(
                            TileCoord::new(rng.next(3) as i32, rng.next(2) as i32),
                            (rng.next(3) > 0).then(|| Tile::filled([rng.next(255) as u8; 4])),
                        )]),
                    },
                    _ => Command::Batch {
                        label: "batch".into(),
                        commands: vec![
                            Command::SetLayerProps {
                                id,
                                props: props(|p| p.name = Some(format!("n{}", rng.next(99)))),
                            },
                            Command::MoveLayer {
                                id: pick(&mut rng),
                                parent: parent(&mut rng),
                                index: rng.next(3),
                            },
                        ],
                    },
                };
                let before = s.document().clone();
                match s.execute(command) {
                    Ok(_) => snapshots.push(s.document().clone()),
                    Err(_) => assert_doc!(
                        *s.document(),
                        before,
                        "a rejected command changed the document"
                    ),
                }
            }
            assert!(
                snapshots.len() > 100,
                "too few commands applied to be a useful test"
            );

            for expected in snapshots.iter().rev().skip(1) {
                assert!(s.undo().is_some());
                assert_doc!(*s.document(), *expected, "undo diverged (seed {seed})");
            }
            assert_eq!(s.position(), base);
            for expected in snapshots.iter().skip(1) {
                assert!(s.redo().is_some());
                assert_doc!(*s.document(), *expected, "redo diverged (seed {seed})");
            }
            assert!(!s.can_redo());
        }
    }
}
