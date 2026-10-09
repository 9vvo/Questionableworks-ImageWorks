//! The Layers panel's view of the layer tree, and the commands its
//! gestures produce. No UI here, so the logic is tested directly.
//!
//! The engine keeps siblings bottom first; the panel lists them top first,
//! as image editors do.

use iw_engine::blend::BlendMode;
use iw_engine::command::{Command, LayerProps};
use iw_engine::document::{Document, Layer, LayerId, LayerKind, PixelData};
use iw_engine::history::Session;
use std::collections::{BTreeSet, HashSet};

/// One visible row of the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: LayerId,
    pub depth: usize,
    pub name: String,
    pub visible: bool,
    /// False if a parent group is hidden, so the layer does not show even
    /// though its own eye is on.
    pub effectively_visible: bool,
    pub locked: bool,
    pub is_group: bool,
    pub expanded: bool,
    pub child_count: usize,
}

/// Rows in display order. Children of collapsed groups are left out.
pub fn rows(doc: &Document, collapsed: &HashSet<LayerId>) -> Vec<Row> {
    fn walk(
        layers: &[Layer],
        depth: usize,
        parent_visible: bool,
        collapsed: &HashSet<LayerId>,
        out: &mut Vec<Row>,
    ) {
        for layer in layers.iter().rev() {
            let expanded = !collapsed.contains(&layer.id);
            out.push(Row {
                id: layer.id,
                depth,
                name: layer.name.clone(),
                visible: layer.visible,
                effectively_visible: parent_visible && layer.visible,
                locked: layer.locked,
                is_group: layer.is_group(),
                expanded,
                child_count: layer.children().len(),
            });
            if layer.is_group() && expanded {
                walk(
                    layer.children(),
                    depth + 1,
                    parent_visible && layer.visible,
                    collapsed,
                    out,
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(doc.layers(), 0, true, collapsed, &mut out);
    out
}

/// Every layer in display order, ignoring collapsed groups.
pub fn display_order(doc: &Document) -> Vec<LayerId> {
    rows(doc, &HashSet::new())
        .into_iter()
        .map(|r| r.id)
        .collect()
}

/// The selected layers that are not inside another selected group, in
/// display order. Acting on these acts on everything selected exactly once.
pub fn selection_roots(doc: &Document, selected: &BTreeSet<LayerId>) -> Vec<LayerId> {
    display_order(doc)
        .into_iter()
        .filter(|id| selected.contains(id))
        .filter(|id| !ancestors(doc, *id).iter().any(|a| selected.contains(a)))
        .collect()
}

fn ancestors(doc: &Document, id: LayerId) -> Vec<LayerId> {
    let mut out = Vec::new();
    let mut current = doc.locate(id).and_then(|l| l.parent);
    while let Some(parent) = current {
        out.push(parent);
        current = doc.locate(parent).and_then(|l| l.parent);
    }
    out
}

/// Where a dragged row was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// Just above this row, as a sibling.
    Above(LayerId),
    /// Just below this row, as a sibling.
    Below(LayerId),
    /// At the top of this group.
    Into(LayerId),
}

/// The command for dropping `moving` at `drop`, keeping the dragged
/// layers' order. `None` if the drop would change nothing or is not
/// allowed (such as dropping a group inside itself).
pub fn move_command(doc: &Document, moving: &BTreeSet<LayerId>, drop: Drop) -> Option<Command> {
    let roots = selection_roots(doc, moving);
    let target = match drop {
        Drop::Above(t) | Drop::Below(t) | Drop::Into(t) => t,
    };
    if roots.is_empty() || doc.layer(target).is_none() {
        return None;
    }
    // Dropping onto or inside something being moved is meaningless.
    if roots.contains(&target) && !matches!(drop, Drop::Above(_) | Drop::Below(_))
        || ancestors(doc, target).iter().any(|a| roots.contains(a))
    {
        return None;
    }
    if let Drop::Into(group) = drop {
        if !doc.layer(group)?.is_group() {
            return None;
        }
    }

    // Work the moves out on a scratch copy, one layer at a time.
    let mut scratch = Session::new(doc.clone());
    let mut commands = Vec::new();
    let mut anchor = drop;
    for id in roots {
        if anchor == Drop::Above(id) || anchor == Drop::Below(id) {
            // Dropping a layer next to itself: it stays, and the next one
            // goes below it.
            anchor = Drop::Below(id);
            continue;
        }
        let d = scratch.document();
        let (parent, mut index) = match anchor {
            Drop::Above(t) => {
                let at = d.locate(t)?;
                (at.parent, at.index + 1)
            }
            Drop::Below(t) => {
                let at = d.locate(t)?;
                (at.parent, at.index)
            }
            Drop::Into(g) => (Some(g), d.layer(g)?.children().len()),
        };
        let from = d.locate(id)?;
        if from.parent == parent && from.index < index {
            index -= 1;
        }
        if !(from.parent == parent && from.index == index) {
            let command = Command::MoveLayer { id, parent, index };
            scratch.execute(command.clone()).ok()?;
            commands.push(command);
        }
        anchor = Drop::Below(id);
    }
    (!commands.is_empty()).then(|| Command::Batch {
        label: "Layer Order".into(),
        commands,
    })
}

/// Where a new layer goes: directly above the active layer, beside it in
/// its group. With no active layer, at the top.
pub fn insertion_point(doc: &Document, active: Option<LayerId>) -> (Option<LayerId>, usize) {
    match active.and_then(|id| doc.locate(id)) {
        Some(at) => (at.parent, at.index + 1),
        None => (None, doc.layers().len()),
    }
}

/// A name like "Layer 3" that no layer has yet.
pub fn fresh_name(doc: &Document, base: &str) -> String {
    let taken: HashSet<&str> = doc.all_layers().iter().map(|l| l.name.as_str()).collect();
    (1..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken.contains(name.as_str()))
        .expect("unbounded")
}

pub fn new_layer_command(doc: &Document, active: Option<LayerId>) -> Command {
    let (parent, index) = insertion_point(doc, active);
    let layer = Layer::raster(fresh_name(doc, "Layer"), PixelData::empty(doc.bit_depth()));
    Command::AddLayer {
        parent,
        index,
        layer: Box::new(layer),
    }
}

pub fn new_group_command(doc: &Document, active: Option<LayerId>) -> Command {
    let (parent, index) = insertion_point(doc, active);
    Command::AddLayer {
        parent,
        index,
        layer: Box::new(Layer::group(fresh_name(doc, "Group"))),
    }
}

/// Duplicates each selected layer directly above itself.
pub fn duplicate_command(doc: &Document, selected: &BTreeSet<LayerId>) -> Option<Command> {
    let roots = selection_roots(doc, selected);
    let mut scratch = Session::new(doc.clone());
    let mut commands = Vec::new();
    for id in roots {
        let d = scratch.document();
        let at = d.locate(id)?;
        let mut copy = d.layer(id)?.duplicate();
        copy.name = format!("{} copy", copy.name);
        let command = Command::AddLayer {
            parent: at.parent,
            index: at.index + 1,
            layer: Box::new(copy),
        };
        scratch.execute(command.clone()).ok()?;
        commands.push(command);
    }
    let label = if commands.len() == 1 {
        "Duplicate Layer"
    } else {
        "Duplicate Layers"
    };
    (!commands.is_empty()).then(|| Command::Batch {
        label: label.into(),
        commands,
    })
}

pub fn delete_command(doc: &Document, selected: &BTreeSet<LayerId>) -> Option<Command> {
    let commands: Vec<Command> = selection_roots(doc, selected)
        .into_iter()
        .map(|id| Command::RemoveLayer { id })
        .collect();
    let label = if commands.len() == 1 {
        "Delete Layer"
    } else {
        "Delete Layers"
    };
    (!commands.is_empty()).then(|| Command::Batch {
        label: label.into(),
        commands,
    })
}

/// Puts the selected layers into a new group, placed where the topmost of
/// them was. Returns the command and the new group's id.
pub fn group_command(doc: &Document, selected: &BTreeSet<LayerId>) -> Option<(Command, LayerId)> {
    let roots = selection_roots(doc, selected);
    let top = *roots.first()?;
    let at = doc.locate(top)?;
    let mut group = Layer::group(fresh_name(doc, "Group"));
    // Choose the id up front so the moves in the same batch can name it.
    let group_id = LayerId::from_raw(doc.next_id());
    group.id = group_id;
    let add = Command::AddLayer {
        parent: at.parent,
        index: at.index + 1,
        layer: Box::new(group),
    };

    let mut scratch = Session::new(doc.clone());
    scratch.execute(add.clone()).ok()?;
    let ids: BTreeSet<LayerId> = roots.iter().copied().collect();
    let mut commands = vec![add];
    if let Some(Command::Batch {
        commands: moves, ..
    }) = move_command(scratch.document(), &ids, Drop::Into(group_id))
    {
        commands.extend(moves);
    }
    Some((
        Command::Batch {
            label: "Group Layers".into(),
            commands,
        },
        group_id,
    ))
}

/// Sets one property on every selected layer, as one step.
pub fn props_command(selected: &BTreeSet<LayerId>, props: LayerProps) -> Option<Command> {
    let commands: Vec<Command> = selected
        .iter()
        .map(|id| Command::SetLayerProps {
            id: *id,
            props: props.clone(),
        })
        .collect();
    match commands.len() {
        0 => None,
        1 => commands.into_iter().next(),
        _ => Some(Command::Batch {
            label: props_label(&props).into(),
            commands,
        }),
    }
}

fn props_label(props: &LayerProps) -> &'static str {
    if props.opacity.is_some() {
        "Opacity Change"
    } else if props.blend.is_some() || props.pass_through.is_some() {
        "Blending Change"
    } else if props.visible.is_some() {
        "Layer Visibility"
    } else {
        "Layer Properties"
    }
}

/// What the blend control shows for the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendChoice {
    PassThrough,
    Mode(BlendMode),
}

pub fn blend_choice(layer: &Layer) -> BlendChoice {
    match &layer.kind {
        LayerKind::Group {
            pass_through: true, ..
        } => BlendChoice::PassThrough,
        _ => BlendChoice::Mode(layer.blend),
    }
}

/// Removes ids that no longer exist after a command or undo.
pub fn prune(doc: &Document, selected: &mut BTreeSet<LayerId>, active: &mut Option<LayerId>) {
    selected.retain(|id| doc.layer(*id).is_some());
    if active.is_some_and(|id| doc.layer(id).is_none()) {
        *active = None;
    }
    if active.is_none() {
        *active = selected.iter().next().copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iw_engine::pixel::BitDepth;

    /// Builds `[bg, group[a, b], top]` (bottom first) and returns the ids.
    fn sample() -> (Session, [LayerId; 5]) {
        let mut s = Session::new(Document::new(100, 100, BitDepth::U8).unwrap());
        let mut add = |parent, index, layer: Layer| {
            s.execute(Command::AddLayer {
                parent,
                index,
                layer: Box::new(layer),
            })
            .unwrap()
            .added_layers[0]
        };
        let empty = || PixelData::empty(BitDepth::U8);
        let bg = add(None, 0, Layer::raster("bg", empty()));
        let group = add(None, 1, Layer::group("group"));
        let a = add(Some(group), 0, Layer::raster("a", empty()));
        let b = add(Some(group), 1, Layer::raster("b", empty()));
        let top = add(None, 2, Layer::raster("top", empty()));
        (s, [bg, group, a, b, top])
    }

    fn names(doc: &Document) -> Vec<String> {
        rows(doc, &HashSet::new())
            .into_iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    fn set(ids: &[LayerId]) -> BTreeSet<LayerId> {
        ids.iter().copied().collect()
    }

    fn apply(s: &mut Session, command: Option<Command>) {
        s.execute(command.expect("expected a command")).unwrap();
    }

    #[test]
    fn rows_are_top_first_with_depth_and_collapse() {
        let (s, [_, group, ..]) = sample();
        assert_eq!(names(s.document()), ["top", "group", "  b", "  a", "bg"]);
        let collapsed: HashSet<LayerId> = [group].into();
        let r = rows(s.document(), &collapsed);
        assert_eq!(r.len(), 3);
        assert!(!r[1].expanded);
        assert_eq!(r[1].child_count, 2);
    }

    #[test]
    fn hidden_groups_hide_their_children() {
        let (mut s, [_, group, a, ..]) = sample();
        let c = props_command(
            &set(&[group]),
            LayerProps {
                visible: Some(false),
                ..Default::default()
            },
        );
        apply(&mut s, c);
        let r = rows(s.document(), &HashSet::new());
        let row = r.iter().find(|r| r.id == a).unwrap();
        assert!(row.visible && !row.effectively_visible);
    }

    #[test]
    fn dragging_reorders_and_regroups() {
        let (mut s, [bg, group, a, b, top]) = sample();
        // Top layer dropped below the background.
        let c = move_command(s.document(), &set(&[top]), Drop::Below(bg));
        apply(&mut s, c);
        assert_eq!(names(s.document()), ["group", "  b", "  a", "bg", "top"]);
        // Background dropped into the group goes to the top of it.
        let c = move_command(s.document(), &set(&[bg]), Drop::Into(group));
        apply(&mut s, c);
        assert_eq!(names(s.document()), ["group", "  bg", "  b", "  a", "top"]);
        // Layer a dragged out of the group, above it.
        let c = move_command(s.document(), &set(&[a]), Drop::Above(group));
        apply(&mut s, c);
        assert_eq!(names(s.document()), ["a", "group", "  bg", "  b", "top"]);
        // Two layers dragged together keep their order.
        let c = move_command(s.document(), &set(&[a, top]), Drop::Above(b));
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["group", "  bg", "  a", "  top", "  b"]
        );
        // Moving within the same parent, downwards and upwards.
        let c = move_command(s.document(), &set(&[bg]), Drop::Below(b));
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["group", "  a", "  top", "  b", "  bg"]
        );
        let c = move_command(s.document(), &set(&[bg]), Drop::Above(a));
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["group", "  bg", "  a", "  top", "  b"]
        );
    }

    #[test]
    fn invalid_or_empty_drops_do_nothing() {
        let (s, [bg, group, a, b, top]) = sample();
        let d = s.document();
        assert_eq!(
            move_command(d, &set(&[group]), Drop::Into(group)),
            None,
            "into itself"
        );
        assert_eq!(
            move_command(d, &set(&[group]), Drop::Above(a)),
            None,
            "inside itself"
        );
        assert_eq!(
            move_command(d, &set(&[top]), Drop::Into(bg)),
            None,
            "into a raster layer"
        );
        assert_eq!(
            move_command(d, &set(&[top]), Drop::Above(top)),
            None,
            "onto itself"
        );
        assert_eq!(
            move_command(d, &set(&[b]), Drop::Above(a)),
            None,
            "already there"
        );
        assert_eq!(
            move_command(d, &set(&[]), Drop::Above(a)),
            None,
            "nothing dragged"
        );
    }

    #[test]
    fn a_group_and_its_child_selected_together_move_once() {
        let (mut s, [bg, group, a, ..]) = sample();
        assert_eq!(selection_roots(s.document(), &set(&[group, a])), [group]);
        let c = move_command(s.document(), &set(&[group, a]), Drop::Below(bg));
        apply(&mut s, c);
        assert_eq!(names(s.document()), ["top", "bg", "group", "  b", "  a"]);
    }

    #[test]
    fn every_panel_command_undoes_cleanly() {
        let (mut s, [bg, group, a, b, top]) = sample();
        let start = s.document().clone();
        let selection = set(&[a, top]);
        let commands = vec![
            Some(new_layer_command(s.document(), Some(a))),
            Some(new_group_command(s.document(), None)),
            duplicate_command(s.document(), &selection),
            delete_command(s.document(), &selection),
            group_command(s.document(), &set(&[bg, b])).map(|(c, _)| c),
            props_command(
                &selection,
                LayerProps {
                    opacity: Some(0.5),
                    ..Default::default()
                },
            ),
            move_command(s.document(), &selection, Drop::Into(group)),
        ];
        for command in commands {
            let command = command.expect("expected a command");
            s.execute(command).unwrap();
            assert!(s.undo().is_some());
            assert!(*s.document() == start);
        }
    }

    #[test]
    fn new_layers_go_above_the_active_layer_with_fresh_names() {
        let (mut s, [_, _, a, ..]) = sample();
        let c = Some(new_layer_command(s.document(), Some(a)));
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["top", "group", "  b", "  Layer 1", "  a", "bg"]
        );
        let c = Some(new_layer_command(s.document(), None));
        apply(&mut s, c);
        assert_eq!(names(s.document())[0], "Layer 2");
        let c = Some(new_group_command(s.document(), None));
        apply(&mut s, c);
        assert_eq!(names(s.document())[0], "Group 1");
    }

    #[test]
    fn duplicating_and_deleting_a_selection() {
        let (mut s, [_, group, a, _, top]) = sample();
        let c = duplicate_command(s.document(), &set(&[a, top]));
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["top copy", "top", "group", "  b", "  a copy", "  a", "bg"]
        );
        let c = delete_command(s.document(), &set(&[group, a]));
        apply(&mut s, c);
        assert_eq!(names(s.document()), ["top copy", "top", "bg"]);
    }

    #[test]
    fn grouping_a_selection_keeps_its_order_and_place() {
        let (mut s, [bg, _, a, _, top]) = sample();
        let (command, id) = group_command(s.document(), &set(&[top, a, bg])).unwrap();
        let c = Some(command);
        apply(&mut s, c);
        assert_eq!(
            names(s.document()),
            ["Group 1", "  top", "  a", "  bg", "group", "  b"]
        );
        assert!(s.document().layer(id).unwrap().is_group());
        // One undo step takes it all back.
        assert_eq!(s.undo_label(), Some("Group Layers"));
    }

    #[test]
    fn pruning_drops_deleted_layers_from_the_selection() {
        let (mut s, [bg, _, a, ..]) = sample();
        let c = delete_command(s.document(), &set(&[a]));
        apply(&mut s, c);
        let mut selected = set(&[a, bg]);
        let mut active = Some(a);
        prune(s.document(), &mut selected, &mut active);
        assert_eq!(selected, set(&[bg]));
        assert_eq!(active, Some(bg));
    }
}
