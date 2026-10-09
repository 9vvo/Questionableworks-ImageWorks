//! The Layers panel.
//!
//! It never changes the document itself: every gesture becomes an
//! [`Intent`] carrying a command, which the app executes through the
//! session (architecture rule 2). The logic behind each gesture lives in
//! [`crate::layers_model`], where it is tested.

use crate::icons::{self, Icon};
use crate::layers_model::{self as model, BlendChoice, Drop, Row};
use crate::theme;
use eframe::egui::{self, pos2, vec2, Color32, Key, Rect, Sense, Stroke, Ui};
use iw_engine::blend::BlendMode;
use iw_engine::command::{Command, LayerProps};
use iw_engine::document::{Document, LayerId};
use std::collections::{BTreeSet, HashSet};

/// What the panel wants done.
pub enum Intent {
    Execute(Command),
    /// Part of a continuous gesture (dragging the opacity slider).
    Coalesce(Command, &'static str),
    EndGesture,
    NewLayer,
    NewGroup,
    Delete,
}

/// Per-document panel state: selection, collapsed groups, editing.
#[derive(Default)]
pub struct PanelState {
    pub selected: BTreeSet<LayerId>,
    /// The layer new layers go above, and whose settings the controls show.
    pub active: Option<LayerId>,
    anchor: Option<LayerId>,
    pub collapsed: HashSet<LayerId>,
    renaming: Option<(LayerId, String)>,
    dragging: Option<BTreeSet<LayerId>>,
}

impl PanelState {
    pub fn select_only(&mut self, id: LayerId) {
        self.selected = [id].into();
        self.active = Some(id);
        self.anchor = Some(id);
    }

    pub fn prune(&mut self, doc: &Document) {
        model::prune(doc, &mut self.selected, &mut self.active);
        if self.anchor.is_some_and(|id| doc.layer(id).is_none()) {
            self.anchor = self.active;
        }
        self.collapsed.retain(|id| doc.layer(*id).is_some());
        if self
            .renaming
            .as_ref()
            .is_some_and(|(id, _)| doc.layer(*id).is_none())
        {
            self.renaming = None;
        }
    }
}

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;

pub fn show(ui: &mut Ui, doc: &Document, state: &mut PanelState) -> Vec<Intent> {
    let mut intents = Vec::new();
    controls(ui, doc, state, &mut intents);
    ui.separator();

    let rows = model::rows(doc, &state.collapsed);
    let footer = 30.0;
    let list_height = (ui.available_height() - footer).max(ROW_HEIGHT);
    let mut row_rects: Vec<(Row, Rect)> = Vec::with_capacity(rows.len());
    egui::ScrollArea::vertical()
        .max_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.weak("No layers");
            }
            for row in &rows {
                let rect = row_ui(ui, doc, row, &rows, state, &mut intents);
                row_rects.push((row.clone(), rect));
            }
            drag_and_drop(ui, doc, state, &row_rects, &mut intents);
        });

    ui.separator();
    ui.horizontal(|ui| {
        if icons::button(ui, Icon::NewLayer, 22.0, false, "New Layer").clicked() {
            intents.push(Intent::NewLayer);
        }
        if icons::button(ui, Icon::NewGroup, 22.0, false, "New Group").clicked() {
            intents.push(Intent::NewGroup);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_delete = !state.selected.is_empty();
            if ui
                .add_enabled_ui(can_delete, |ui| {
                    icons::button(ui, Icon::Trash, 22.0, false, "Delete Layer")
                })
                .inner
                .clicked()
            {
                intents.push(Intent::Delete);
            }
        });
    });
    intents
}

fn controls(ui: &mut Ui, doc: &Document, state: &mut PanelState, intents: &mut Vec<Intent>) {
    let active = state.active.and_then(|id| doc.layer(id));
    let all_groups = !state.selected.is_empty()
        && state
            .selected
            .iter()
            .all(|id| doc.layer(*id).is_some_and(|l| l.is_group()));
    ui.add_enabled_ui(active.is_some(), |ui| {
        ui.horizontal(|ui| {
            let current = active
                .map(model::blend_choice)
                .unwrap_or(BlendChoice::Mode(BlendMode::Normal));
            let text = match current {
                BlendChoice::PassThrough => "Pass Through",
                BlendChoice::Mode(m) => m.name(),
            };
            let mut chosen = None;
            ui.label("Blend");
            egui::ComboBox::from_id_salt("blend-mode")
                .selected_text(text)
                .width((ui.available_width() - 4.0).max(80.0))
                .height(520.0)
                .show_ui(ui, |ui| {
                    if all_groups
                        && ui
                            .selectable_label(current == BlendChoice::PassThrough, "Pass Through")
                            .clicked()
                    {
                        chosen = Some(BlendChoice::PassThrough);
                    }
                    for (i, mode) in BlendMode::ALL.into_iter().enumerate() {
                        // Separators between the families, as menus usually show them.
                        if [2, 7, 12, 19, 23].contains(&i) {
                            ui.separator();
                        }
                        if ui
                            .selectable_label(current == BlendChoice::Mode(mode), mode.name())
                            .clicked()
                        {
                            chosen = Some(BlendChoice::Mode(mode));
                        }
                    }
                });
            if let Some(choice) = chosen {
                let props = match choice {
                    BlendChoice::PassThrough => LayerProps {
                        pass_through: Some(true),
                        ..Default::default()
                    },
                    BlendChoice::Mode(mode) => {
                        // Choosing a mode for a group makes it isolated.
                        let groups = all_groups.then_some(false);
                        LayerProps {
                            blend: Some(mode),
                            pass_through: groups,
                            ..Default::default()
                        }
                    }
                };
                if let Some(command) = model::props_command(&state.selected, props) {
                    intents.push(Intent::Execute(command));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Opacity");
            let mut percent = active.map_or(100.0, |l| (l.opacity * 100.0).round());
            // Leave room for the number box beside the slider.
            ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(40.0);
            let response = ui.add(
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .suffix("%")
                    .integer(),
            );
            if response.changed() {
                let props = LayerProps {
                    opacity: Some((percent / 100.0).clamp(0.0, 1.0)),
                    ..Default::default()
                };
                if let Some(command) = model::props_command(&state.selected, props) {
                    intents.push(Intent::Coalesce(command, "layer-opacity"));
                }
            }
            if response.drag_stopped() || response.lost_focus() {
                intents.push(Intent::EndGesture);
            }
        });
    });
}

fn row_ui(
    ui: &mut Ui,
    doc: &Document,
    row: &Row,
    rows: &[Row],
    state: &mut PanelState,
    intents: &mut Vec<Intent>,
) -> Rect {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click_and_drag());
    let selected = state.selected.contains(&row.id);
    // Rows are drawn by hand, so describe them for screen readers (and
    // the UI tests, which find widgets the same way).
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &row.name)
    });
    let painter = ui.painter().clone();
    if selected {
        let fill = if state.active == Some(row.id) {
            theme::ACCENT_DIM
        } else {
            Color32::from_rgb(0x45, 0x3A, 0x2A)
        };
        painter.rect_filled(rect, 2.0, fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let dim = if row.effectively_visible {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };

    // Visibility, in a fixed column.
    let eye = Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(20.0, 20.0));
    let eye_response = ui.interact(eye, ui.id().with(("eye", row.id)), Sense::click());
    let eye_label = format!("{} {}", if row.visible { "Hide" } else { "Show" }, row.name);
    eye_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &eye_label));
    icons::paint(
        &painter,
        eye.shrink(3.0),
        if row.visible { Icon::Eye } else { Icon::EyeOff },
        dim,
    );
    if eye_response.clicked() {
        let props = LayerProps {
            visible: Some(!row.visible),
            ..Default::default()
        };
        intents.push(Intent::Execute(Command::SetLayerProps {
            id: row.id,
            props,
        }));
    }
    eye_response.on_hover_text(if row.visible {
        "Hide layer"
    } else {
        "Show layer"
    });

    let mut x = eye.right() + 4.0 + row.depth as f32 * INDENT;
    // Expand or collapse a group.
    if row.is_group {
        let chevron = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(16.0, 16.0));
        let r = ui.interact(chevron, ui.id().with(("chevron", row.id)), Sense::click());
        icons::paint(
            &painter,
            chevron.shrink(2.0),
            if row.expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            dim,
        );
        if r.clicked() {
            if row.expanded {
                state.collapsed.insert(row.id);
            } else {
                state.collapsed.remove(&row.id);
            }
        }
        x = chevron.right() + 2.0;
        icons::paint(
            &painter,
            Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(16.0, 16.0)),
            Icon::Folder,
            dim,
        );
        x += 20.0;
    } else {
        // A small swatch stands in for a thumbnail.
        let swatch = Rect::from_min_size(pos2(x + 2.0, rect.top() + 5.0), vec2(14.0, 14.0));
        painter.rect_stroke(swatch, 1.0, Stroke::new(1.0, dim), egui::StrokeKind::Inside);
        x = swatch.right() + 6.0;
    }

    // Lock, at the right.
    let lock = Rect::from_min_size(
        pos2(rect.right() - 22.0, rect.top() + 2.0),
        vec2(20.0, 20.0),
    );
    let lock_response = ui.interact(lock, ui.id().with(("lock", row.id)), Sense::click());
    let lock_label = format!(
        "{} {}",
        if row.locked { "Unlock" } else { "Lock" },
        row.name
    );
    lock_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &lock_label));
    if row.locked || lock_response.hovered() {
        icons::paint(
            &painter,
            lock.shrink(4.0),
            if row.locked { Icon::Lock } else { Icon::Unlock },
            dim,
        );
    }
    if lock_response.clicked() {
        let props = LayerProps {
            locked: Some(!row.locked),
            ..Default::default()
        };
        intents.push(Intent::Execute(Command::SetLayerProps {
            id: row.id,
            props,
        }));
    }
    lock_response.on_hover_text(if row.locked {
        "Unlock layer"
    } else {
        "Lock layer"
    });

    // Name, or the rename field.
    let name_rect = Rect::from_min_max(
        pos2(x, rect.top() + 2.0),
        pos2(lock.left() - 4.0, rect.bottom() - 2.0),
    );
    let mut finished = None;
    if let Some((id, text)) = state.renaming.as_mut().filter(|(id, _)| *id == row.id) {
        let edit = ui.put(
            name_rect,
            egui::TextEdit::singleline(text).desired_width(name_rect.width()),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel {
            finished = Some(None);
        } else if edit.lost_focus() {
            finished = Some(Some((*id, text.trim().to_string())));
        }
    } else {
        painter.text(
            name_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            &row.name,
            egui::FontId::proportional(13.0),
            dim,
        );
    }
    if let Some(result) = finished {
        state.renaming = None;
        if let Some((id, name)) = result {
            let unchanged = doc.layer(id).is_none_or(|l| l.name == name);
            if !name.is_empty() && !unchanged {
                let props = LayerProps {
                    name: Some(name),
                    ..Default::default()
                };
                intents.push(Intent::Execute(Command::SetLayerProps { id, props }));
            }
        }
    }

    // Selection.
    if response.double_clicked()
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| name_rect.contains(p))
    {
        state.renaming = Some((row.id, row.name.clone()));
    } else if response.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.command {
            if !state.selected.remove(&row.id) {
                state.selected.insert(row.id);
                state.active = Some(row.id);
            } else if state.active == Some(row.id) {
                state.active = state.selected.iter().next().copied();
            }
            state.anchor = Some(row.id);
        } else if modifiers.shift {
            let anchor = state.anchor.or(state.active).unwrap_or(row.id);
            let a = rows.iter().position(|r| r.id == anchor);
            let b = rows.iter().position(|r| r.id == row.id);
            if let (Some(a), Some(b)) = (a, b) {
                state.selected = rows[a.min(b)..=a.max(b)].iter().map(|r| r.id).collect();
                state.active = Some(row.id);
            }
        } else {
            state.select_only(row.id);
        }
    }
    if response.drag_started() {
        if !state.selected.contains(&row.id) {
            state.select_only(row.id);
        }
        state.dragging = Some(state.selected.clone());
    }
    rect
}

fn drag_and_drop(
    ui: &mut Ui,
    doc: &Document,
    state: &mut PanelState,
    rows: &[(Row, Rect)],
    intents: &mut Vec<Intent>,
) {
    let Some(moving) = state.dragging.clone() else {
        return;
    };
    let (pointer, released) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_released()));
    let drop = pointer.and_then(|p| drop_target(p, rows));
    if let Some(drop) = drop {
        let valid = model::move_command(doc, &moving, drop).is_some();
        let color = if valid {
            theme::ACCENT
        } else {
            theme::TEXT_DIM
        };
        let rect_of = |id: LayerId| rows.iter().find(|(r, _)| r.id == id).map(|(_, rect)| *rect);
        match drop {
            Drop::Above(id) | Drop::Below(id) => {
                if let Some(rect) = rect_of(id) {
                    let y = if matches!(drop, Drop::Above(_)) {
                        rect.top()
                    } else {
                        rect.bottom()
                    };
                    ui.painter()
                        .hline(rect.x_range(), y, Stroke::new(2.0, color));
                }
            }
            Drop::Into(id) => {
                if let Some(rect) = rect_of(id) {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        Stroke::new(2.0, color),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        ui.ctx().set_cursor_icon(if valid {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::NoDrop
        });
    }
    if released {
        state.dragging = None;
        if let Some(command) = drop.and_then(|d| model::move_command(doc, &moving, d)) {
            intents.push(Intent::Execute(command));
        }
    }
}

/// Which drop a pointer position over the list means. The top and bottom
/// quarters of a row put the layers beside it; the middle of a group's
/// row puts them inside the group.
fn drop_target(pointer: egui::Pos2, rows: &[(Row, Rect)]) -> Option<Drop> {
    let (row, rect) = rows.iter().find(|(_, r)| r.y_range().contains(pointer.y))?;
    let t = (pointer.y - rect.top()) / rect.height();
    Some(if row.is_group && (0.25..0.75).contains(&t) {
        Drop::Into(row.id)
    } else if t < 0.5 {
        Drop::Above(row.id)
    } else {
        Drop::Below(row.id)
    })
}
