//! Native menu bar (macOS global menu, Windows window menu) via `muda`.
//!
//! Linux is a development platform only, so it gets `None` here and the app
//! falls back to an in-window menu.

/// Things a menu item can ask the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    TestOpenDialog,
    TestSaveDialog,
    ResetLayout,
    Quit,
}

impl Command {
    pub const ALL: [Command; 4] = [
        Command::TestOpenDialog,
        Command::TestSaveDialog,
        Command::ResetLayout,
        Command::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::TestOpenDialog => "Test Open Dialog…",
            Command::TestSaveDialog => "Test Save Dialog…",
            Command::ResetLayout => "Reset Panel Layout",
            Command::Quit => "Quit",
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct NativeMenu {
    // Dropping the menu removes it, so it lives as long as the app.
    _menu: muda::Menu,
    ids: Vec<(muda::MenuId, Command)>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl NativeMenu {
    pub fn install(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

        let err = |e: muda::Error| e.to_string();
        let menu = Menu::new();
        let mut ids = Vec::new();
        let mut item = |cmd: Command| {
            let it = MenuItem::new(cmd.label(), true, None);
            ids.push((it.id().clone(), cmd));
            it
        };

        let open = item(Command::TestOpenDialog);
        let save = item(Command::TestSaveDialog);
        let reset = item(Command::ResetLayout);

        // macOS puts the first submenu under the application name.
        #[cfg(target_os = "macos")]
        {
            let app = Submenu::with_items(
                "ImageWorks",
                true,
                &[
                    &PredefinedMenuItem::hide(None),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::quit(None),
                ],
            )
            .map_err(err)?;
            menu.append(&app).map_err(err)?;
        }

        let spike = Submenu::with_items("Spike", true, &[&open, &save]).map_err(err)?;
        #[cfg(target_os = "windows")]
        {
            let quit = item(Command::Quit);
            spike
                .append(&PredefinedMenuItem::separator())
                .map_err(err)?;
            spike.append(&quit).map_err(err)?;
        }
        let window = Submenu::with_items("Window", true, &[&reset]).map_err(err)?;
        menu.append_items(&[&spike, &window]).map_err(err)?;

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

        Ok(Self { _menu: menu, ids })
    }

    /// Commands chosen from the native menu since the last call.
    pub fn poll(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(ev) = muda::MenuEvent::receiver().try_recv() {
            if let Some((_, cmd)) = self.ids.iter().find(|(id, _)| *id == ev.id) {
                out.push(*cmd);
            }
        }
        out
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl NativeMenu {
    pub fn install(_cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        Err("no native menu bar on this platform (development build)".into())
    }

    pub fn poll(&self) -> Vec<Command> {
        Vec::new()
    }
}
