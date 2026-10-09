//! Every command the user can give from a menu, a shortcut or a button.
//!
//! One list drives the native menu bar, the in-window menu used on Linux,
//! and keyboard shortcuts, so they cannot drift apart. Only features that
//! work appear here (charter section 5).

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    New,
    Open,
    Close,
    Save,
    SaveAs,
    ExportAs,
    Revert,
    Quit,
    Undo,
    Redo,
    NewLayer,
    NewGroup,
    DuplicateLayer,
    DeleteLayer,
    GroupLayers,
    ZoomIn,
    ZoomOut,
    FitOnScreen,
    ActualPixels,
    ResetRotation,
    FlipHorizontal,
    ToolHand,
    ToolZoom,
    ToolRotate,
    ShowLayers,
    ShowHistory,
    ResetLayout,
}

/// A top-level menu and its items; `None` is a separator.
pub struct Menu {
    pub title: &'static str,
    pub items: Vec<Option<Action>>,
}

pub fn menus() -> Vec<Menu> {
    use Action::*;
    vec![
        Menu {
            title: "File",
            items: vec![
                Some(New),
                Some(Open),
                None,
                Some(Close),
                Some(Save),
                Some(SaveAs),
                Some(ExportAs),
                Some(Revert),
                None,
                Some(Quit),
            ],
        },
        Menu {
            title: "Edit",
            items: vec![Some(Undo), Some(Redo)],
        },
        Menu {
            title: "Layer",
            items: vec![
                Some(NewLayer),
                Some(NewGroup),
                Some(DuplicateLayer),
                Some(DeleteLayer),
                None,
                Some(GroupLayers),
            ],
        },
        Menu {
            title: "View",
            items: vec![
                Some(ZoomIn),
                Some(ZoomOut),
                Some(FitOnScreen),
                Some(ActualPixels),
                None,
                Some(ResetRotation),
                Some(FlipHorizontal),
            ],
        },
        Menu {
            title: "Window",
            items: vec![Some(ShowLayers), Some(ShowHistory), None, Some(ResetLayout)],
        },
    ]
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::COMMAND
};
const CMD_ALT_SHIFT: Modifiers = Modifiers {
    shift: true,
    alt: true,
    ..Modifiers::COMMAND
};

impl Action {
    pub const ALL: [Action; 27] = [
        Action::New,
        Action::Open,
        Action::Close,
        Action::Save,
        Action::SaveAs,
        Action::ExportAs,
        Action::Revert,
        Action::Quit,
        Action::Undo,
        Action::Redo,
        Action::NewLayer,
        Action::NewGroup,
        Action::DuplicateLayer,
        Action::DeleteLayer,
        Action::GroupLayers,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::FitOnScreen,
        Action::ActualPixels,
        Action::ResetRotation,
        Action::FlipHorizontal,
        Action::ToolHand,
        Action::ToolZoom,
        Action::ToolRotate,
        Action::ShowLayers,
        Action::ShowHistory,
        Action::ResetLayout,
    ];

    /// The menu text. Undo and Redo name the step they act on at run time.
    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New…",
            Action::Open => "Open…",
            Action::Close => "Close",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::ExportAs => "Export As…",
            Action::Revert => "Revert",
            Action::Quit => "Quit ImageWorks",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::NewLayer => "New Layer",
            Action::NewGroup => "New Group",
            Action::DuplicateLayer => "Duplicate Layer",
            Action::DeleteLayer => "Delete Layer",
            Action::GroupLayers => "Group Layers",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::FitOnScreen => "Fit on Screen",
            Action::ActualPixels => "100%",
            Action::ResetRotation => "Reset Rotation",
            Action::FlipHorizontal => "Flip View Horizontally",
            Action::ToolHand => "Hand Tool",
            Action::ToolZoom => "Zoom Tool",
            Action::ToolRotate => "Rotate View Tool",
            Action::ShowLayers => "Layers",
            Action::ShowHistory => "History",
            Action::ResetLayout => "Reset Panel Layout",
        }
    }

    /// The keyboard shortcut, using Cmd on macOS and Ctrl elsewhere.
    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let s = |modifiers, key| Some(KeyboardShortcut::new(modifiers, key));
        match self {
            Action::New => s(CMD, Key::N),
            Action::Open => s(CMD, Key::O),
            Action::Close => s(CMD, Key::W),
            Action::Save => s(CMD, Key::S),
            Action::SaveAs => s(CMD_SHIFT, Key::S),
            Action::ExportAs => s(CMD_ALT_SHIFT, Key::W),
            Action::Revert => s(Modifiers::NONE, Key::F12),
            Action::Quit => s(CMD, Key::Q),
            Action::Undo => s(CMD, Key::Z),
            Action::Redo => s(CMD_SHIFT, Key::Z),
            Action::NewLayer => s(CMD_SHIFT, Key::N),
            Action::DuplicateLayer => s(CMD, Key::J),
            Action::GroupLayers => s(CMD, Key::G),
            Action::ZoomIn => s(CMD, Key::Equals),
            Action::ZoomOut => s(CMD, Key::Minus),
            Action::FitOnScreen => s(CMD, Key::Num0),
            Action::ActualPixels => s(CMD, Key::Num1),
            Action::ToolHand => s(Modifiers::NONE, Key::H),
            Action::ToolZoom => s(Modifiers::NONE, Key::Z),
            Action::ToolRotate => s(Modifiers::NONE, Key::R),
            Action::NewGroup
            | Action::DeleteLayer
            | Action::ResetRotation
            | Action::FlipHorizontal
            | Action::ShowLayers
            | Action::ShowHistory
            | Action::ResetLayout => None,
        }
    }

    /// Single-key tool shortcuts must not fire while typing in a text field.
    pub fn is_single_key(self) -> bool {
        self.shortcut().is_some_and(|s| s.modifiers.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_action_is_in_a_menu_or_is_a_tool() {
        let in_menus: HashSet<Action> = menus()
            .into_iter()
            .flat_map(|m| m.items)
            .flatten()
            .collect();
        for action in Action::ALL {
            let tool = matches!(
                action,
                Action::ToolHand | Action::ToolZoom | Action::ToolRotate
            );
            assert!(in_menus.contains(&action) != tool, "{action:?}");
        }
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = HashSet::new();
        for action in Action::ALL {
            if let Some(s) = action.shortcut() {
                assert!(
                    seen.insert((
                        s.modifiers.command,
                        s.modifiers.shift,
                        s.modifiers.alt,
                        s.logical_key
                    )),
                    "{action:?}"
                );
            }
        }
    }
}
