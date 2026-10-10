//! The menu bar: native on macOS (global menu) and Windows (window menu),
//! drawn in the window on Linux. Both are built from [`crate::actions`].

use crate::actions::{menus, Action};
use eframe::egui;

/// How each menu item should look this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemState {
    pub enabled: bool,
    /// Replaces the item's label when set (Undo and Redo name their step).
    pub label: Option<String>,
}

/// Draws the in-window menu bar and returns the chosen action. Used where
/// there is no native menu bar.
pub fn egui_menu_bar(ui: &mut egui::Ui, state: &dyn Fn(Action) -> ItemState) -> Option<Action> {
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in menus() {
            ui.menu_button(menu.title, |ui| {
                for item in menu.items {
                    let Some(action) = item else {
                        ui.separator();
                        continue;
                    };
                    let st = state(action);
                    let label = st.label.unwrap_or_else(|| action.label().to_string());
                    let mut button = egui::Button::new(label);
                    if let Some(shortcut) = action.shortcut() {
                        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                    }
                    if ui.add_enabled(st.enabled, button).clicked() {
                        chosen = Some(action);
                        ui.close();
                    }
                }
            });
        }
    });
    chosen
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use native::NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl NativeMenu {
    pub fn install(_cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        Err("no native menu bar on this platform".into())
    }

    pub fn poll(&self) -> Vec<Action> {
        Vec::new()
    }

    pub fn update(&mut self, _state: &dyn Fn(Action) -> ItemState) {}

    /// Whether menu shortcuts reach the app through the menu itself.
    pub fn handles_shortcuts(&self) -> bool {
        false
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use super::*;
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    use std::collections::HashMap;

    pub struct NativeMenu {
        // Dropping the menu removes it, so it lives as long as the app.
        _menu: Menu,
        items: Vec<(Action, MenuItem, ItemState)>,
        by_id: HashMap<muda::MenuId, Action>,
    }

    fn code(key: egui::Key) -> Option<Code> {
        use egui::Key as K;
        Some(match key {
            K::N => Code::KeyN,
            K::O => Code::KeyO,
            K::W => Code::KeyW,
            K::S => Code::KeyS,
            K::Q => Code::KeyQ,
            K::Z => Code::KeyZ,
            K::J => Code::KeyJ,
            K::G => Code::KeyG,
            K::Equals => Code::Equal,
            K::Minus => Code::Minus,
            K::Num0 => Code::Digit0,
            K::Num1 => Code::Digit1,
            K::F12 => Code::F12,
            _ => return None,
        })
    }

    /// Only shortcuts with Cmd/Ctrl go on menu items. A bare key such as
    /// F12 would be taken by the menu even while typing in a text field.
    fn accelerator(action: Action) -> Option<Accelerator> {
        let s = action.shortcut()?;
        if !s.modifiers.command {
            return None;
        }
        let mut m = muda::accelerator::CMD_OR_CTRL;
        if s.modifiers.shift {
            m |= Modifiers::SHIFT;
        }
        if s.modifiers.alt {
            m |= Modifiers::ALT;
        }
        Some(Accelerator::new(m, code(s.logical_key)?))
    }

    impl NativeMenu {
        pub fn install(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
            let err = |e: muda::Error| e.to_string();
            let menu = Menu::new();
            let mut items = Vec::new();
            let mut by_id = HashMap::new();
            let mut item = |action: Action| {
                let label = if cfg!(target_os = "windows") && action == Action::Quit {
                    "Exit"
                } else {
                    action.label()
                };
                let it = MenuItem::new(label, true, accelerator(action));
                by_id.insert(it.id().clone(), action);
                items.push((
                    action,
                    it.clone(),
                    ItemState {
                        enabled: true,
                        label: None,
                    },
                ));
                it
            };

            // macOS puts the first submenu under the application name, and
            // Quit belongs there. It is our own item, not the system one,
            // so quitting asks about unsaved documents first.
            #[cfg(target_os = "macos")]
            {
                let about = PredefinedMenuItem::about(
                    Some("About ImageWorks"),
                    Some(muda::AboutMetadata {
                        name: Some("ImageWorks".into()),
                        version: Some(env!("CARGO_PKG_VERSION").into()),
                        copyright: Some("Questionableworks".into()),
                        ..Default::default()
                    }),
                );
                let quit = item(Action::Quit);
                let app = Submenu::with_items(
                    "ImageWorks",
                    true,
                    &[
                        &about,
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::hide(None),
                        &PredefinedMenuItem::hide_others(None),
                        &PredefinedMenuItem::show_all(None),
                        &PredefinedMenuItem::separator(),
                        &quit,
                    ],
                )
                .map_err(err)?;
                menu.append(&app).map_err(err)?;
            }

            for m in menus() {
                let submenu = Submenu::new(m.title, true);
                for entry in m.items {
                    match entry {
                        None => submenu
                            .append(&PredefinedMenuItem::separator())
                            .map_err(err)?,
                        Some(Action::Quit) if cfg!(target_os = "macos") => {}
                        Some(action) => submenu.append(&item(action)).map_err(err)?,
                    }
                }
                menu.append(&submenu).map_err(err)?;
            }

            #[cfg(target_os = "macos")]
            {
                let _ = cc;
                menu.init_for_nsapp();
            }
            #[cfg(target_os = "windows")]
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                let handle = cc.window_handle().map_err(|e| e.to_string())?;
                let RawWindowHandle::Win32(h) = handle.as_raw() else {
                    return Err("window is not a Win32 window".into());
                };
                // SAFETY: the HWND comes from the live eframe window, and the
                // menu is kept alive in `self` for the window's lifetime.
                unsafe { menu.init_for_hwnd(h.hwnd.get()) }.map_err(err)?;
            }

            Ok(Self {
                _menu: menu,
                items,
                by_id,
            })
        }

        /// Actions chosen from the menu since the last call.
        pub fn poll(&self) -> Vec<Action> {
            let mut out = Vec::new();
            while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
                if let Some(action) = self.by_id.get(&event.id) {
                    out.push(*action);
                }
            }
            out
        }

        /// Brings enabled states and labels up to date, touching only
        /// items that changed.
        pub fn update(&mut self, state: &dyn Fn(Action) -> ItemState) {
            for (action, item, last) in &mut self.items {
                let now = state(*action);
                if now == *last {
                    continue;
                }
                if now.enabled != last.enabled {
                    item.set_enabled(now.enabled);
                }
                if now.label != last.label {
                    item.set_text(now.label.as_deref().unwrap_or(action.label()));
                }
                *last = now;
            }
        }

        /// On macOS menu key equivalents fire the menu item themselves, so
        /// the app must not also handle those keys. Windows menus need a
        /// translation step in the message loop that winit does not run,
        /// so there the app handles the keys.
        pub fn handles_shortcuts(&self) -> bool {
            cfg!(target_os = "macos")
        }
    }
}
