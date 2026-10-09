//! The application shell: documents, panels, menus, tools and dialogs.
//!
//! Every change to a document goes through its [`Session`]; the shell only
//! decides which command to run (architecture rule 2). File work runs on
//! background threads and compositing on the renderer's workers, so the
//! window stays responsive (rule 6).

use crate::actions::Action;
use crate::canvas::{self, Canvas, Tool};
use crate::icons::{self, Icon};
use crate::layers_model as model;
use crate::native_menu::{self, ItemState, NativeMenu};
use crate::panels::{self, layers::Intent, layers::PanelState};
use crate::renderer::{DocKey, Renderer};
use crate::theme;
use eframe::egui::{self, Pos2, RichText, TextureHandle};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use iw_engine::command::{Command, Damage, Effects};
use iw_engine::document::Document;
use iw_engine::format::{self, ExportOptions, FileFormat};
use iw_engine::history::Session;
use iw_engine::pixel::BitDepth;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Largest canvas edge offered by the New dialog.
const MAX_NEW_DIMENSION: u32 = 30_000;

const PRESETS: [(&str, u32, u32); 6] = [
    ("HD, 1920 × 1080", 1920, 1080),
    ("4K UHD, 3840 × 2160", 3840, 2160),
    ("Square, 1080 × 1080", 1080, 1080),
    ("Texture, 1024 × 1024", 1024, 1024),
    ("Texture, 2048 × 2048", 2048, 2048),
    ("A4 at 300 ppi, 2480 × 3508", 2480, 3508),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// Shown in the document area when no document is open.
    Start,
    Document(DocKey),
    Layers,
    History,
}

pub struct OpenDoc {
    pub key: DocKey,
    pub session: Session,
    /// Where Save writes. Always a native document.
    pub path: Option<PathBuf>,
    /// The PNG or JPEG it was opened from, if any. Saving such a document
    /// asks for a native file name rather than flattening over the original.
    pub source: Option<PathBuf>,
    pub title: String,
    /// The first row of the History panel.
    pub origin: &'static str,
    pub canvas: Canvas,
    pub layers: PanelState,
    pub saving: bool,
}

impl OpenDoc {
    fn modified(&self) -> bool {
        self.session.is_modified()
    }
}

struct NewForm {
    width: u32,
    height: u32,
    ppi: f64,
    white: bool,
}

enum Dialog {
    New(NewForm),
    /// Save changes before closing?
    Unsaved(DocKey),
    ConfirmRevert(DocKey),
    Error(String),
}

/// Results of background file work.
enum Task {
    Opened {
        path: PathBuf,
        revert: Option<DocKey>,
        result: Result<Box<Document>, String>,
    },
    Saved {
        key: DocKey,
        path: PathBuf,
        native: bool,
        state: u64,
        result: Result<(), String>,
    },
}

pub struct App {
    docs: Vec<OpenDoc>,
    next_key: DocKey,
    untitled: u32,
    dock: DockState<Tab>,
    active: Option<DocKey>,
    tool: Tool,
    renderer: Renderer,
    menu: Option<NativeMenu>,
    checker: TextureHandle,
    tasks_tx: Sender<Task>,
    tasks_rx: Receiver<Task>,
    running_tasks: usize,
    dialog: Option<Dialog>,
    /// Documents waiting to be closed, in order; the first may be waiting
    /// for an answer about unsaved changes.
    close_queue: Vec<DocKey>,
    /// Quit once the close queue is empty.
    quitting: bool,
    allow_quit: bool,
    status: String,
    window_title: String,
    cursor: Option<Pos2>,
    last_new: (u32, u32, f64, bool),
}

fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Start]);
    let surface = dock.main_surface_mut();
    let [_, right] = surface.split_right(NodeIndex::root(), 0.76, vec![Tab::Layers]);
    surface.split_below(right, 0.62, vec![Tab::History]);
    dock
}

fn file_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

fn ensure_extension(mut path: PathBuf, format: FileFormat) -> PathBuf {
    if FileFormat::from_path(&path) != Some(format) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        path.set_file_name(format!("{name}.{}", format.extensions()[0]));
    }
    path
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (menu, status) = match NativeMenu::install(cc) {
            Ok(menu) => (Some(menu), String::new()),
            Err(_) => (None, String::new()),
        };
        let (tasks_tx, tasks_rx) = channel();
        let mut app = Self {
            docs: Vec::new(),
            next_key: 1,
            untitled: 0,
            dock: default_layout(),
            active: None,
            tool: Tool::Hand,
            renderer: Renderer::new(cc.egui_ctx.clone()),
            menu,
            checker: canvas::checker_texture(&cc.egui_ctx),
            tasks_tx,
            tasks_rx,
            running_tasks: 0,
            dialog: None,
            close_queue: Vec::new(),
            quitting: false,
            allow_quit: false,
            status,
            window_title: String::new(),
            cursor: None,
            last_new: (1920, 1080, 72.0, true),
        };
        for path in open {
            app.open_path(&cc.egui_ctx, path, None);
        }
        app
    }

    fn doc(&self, key: DocKey) -> Option<&OpenDoc> {
        self.docs.iter().find(|d| d.key == key)
    }

    fn doc_mut(&mut self, key: DocKey) -> Option<&mut OpenDoc> {
        self.docs.iter_mut().find(|d| d.key == key)
    }

    fn active_doc(&self) -> Option<&OpenDoc> {
        self.active.and_then(|k| self.doc(k))
    }

    // ----- Documents ------------------------------------------------------

    fn add_document(
        &mut self,
        session: Session,
        title: String,
        path: Option<PathBuf>,
        source: Option<PathBuf>,
        origin: &'static str,
    ) {
        let key = self.next_key;
        self.next_key += 1;
        let mut doc = OpenDoc {
            key,
            session,
            path,
            source,
            title,
            origin,
            canvas: Canvas::new(key),
            layers: PanelState::default(),
            saving: false,
        };
        if let Some(top) = doc.session.document().layers().last() {
            doc.layers.select_only(top.id);
        }
        self.docs.push(doc);
        self.show_document_tab(key);
        self.active = Some(key);
        self.request_damage(key, Damage::All);
    }

    /// Puts a document's tab beside the other documents, replacing the
    /// start tab if it is showing.
    fn show_document_tab(&mut self, key: DocKey) {
        let anchor = self
            .dock
            .iter_all_tabs()
            .find(|(_, t)| matches!(t, Tab::Document(_) | Tab::Start))
            .map(|(path, _)| path);
        match anchor {
            Some(path) => {
                self.dock.set_focused_node_and_surface(path.node_path());
                self.dock.push_to_focused_leaf(Tab::Document(key));
            }
            None => self.dock.push_to_first_leaf(Tab::Document(key)),
        }
        if let Some(start) = self.dock.find_tab(&Tab::Start) {
            self.dock.remove_tab(start);
        }
    }

    fn close_document(&mut self, key: DocKey) {
        self.renderer.forget(key);
        if let Some(path) = self.dock.find_tab(&Tab::Document(key)) {
            if self.docs.len() == 1 {
                // Keep the document area: put the start tab where it was.
                self.dock.set_focused_node_and_surface(path.node_path());
                self.dock.push_to_focused_leaf(Tab::Start);
            }
            if let Some(path) = self.dock.find_tab(&Tab::Document(key)) {
                self.dock.remove_tab(path);
            }
        }
        self.docs.retain(|d| d.key != key);
        if self.active == Some(key) {
            self.active = self.docs.last().map(|d| d.key);
        }
    }

    fn new_document(&mut self, form: &NewForm) {
        let rgba = if form.white {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.0; 4]
        };
        match Document::with_background(form.width, form.height, BitDepth::U8, rgba) {
            Ok(doc) => {
                let mut session = Session::new(doc);
                // Resolution is set before editing starts, so it is not an undo step.
                if (form.ppi - 72.0).abs() > f64::EPSILON {
                    let _ =
                        session.execute(Command::SetResolution(iw_engine::document::Resolution {
                            ppi: form.ppi,
                        }));
                    session = Session::new(session.into_document());
                }
                self.untitled += 1;
                self.add_document(
                    session,
                    format!("Untitled-{}", self.untitled),
                    None,
                    None,
                    "New",
                );
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(format!("Could not create the document: {e}")))
            }
        }
    }

    fn open_path(&mut self, ctx: &egui::Context, path: PathBuf, revert: Option<DocKey>) {
        if revert.is_none() {
            if let Some(existing) = self
                .docs
                .iter()
                .find(|d| d.path.as_deref() == Some(&path) || d.source.as_deref() == Some(&path))
            {
                // Already open: show it rather than opening a second copy.
                let key = existing.key;
                self.active = Some(key);
                if let Some(tab) = self.dock.find_tab(&Tab::Document(key)) {
                    let _ = self.dock.set_active_tab(tab);
                }
                return;
            }
        }
        let tx = self.tasks_tx.clone();
        let ctx = ctx.clone();
        self.running_tasks += 1;
        self.status = format!("Opening {}…", path.display());
        std::thread::spawn(move || {
            let result = format::open(&path).map(Box::new).map_err(|e| e.to_string());
            let _ = tx.send(Task::Opened {
                path,
                revert,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn start_save(&mut self, ctx: &egui::Context, key: DocKey, path: PathBuf, format: FileFormat) {
        let Some(doc) = self.doc_mut(key) else { return };
        let native = format == FileFormat::Native;
        let snapshot = doc.session.document().clone();
        let state = doc.session.state_id();
        if native {
            doc.saving = true;
        }
        let tx = self.tasks_tx.clone();
        let ctx = ctx.clone();
        self.running_tasks += 1;
        self.status = format!("Saving {}…", path.display());
        std::thread::spawn(move || {
            let result = format::save(&snapshot, &path, format, &ExportOptions::default())
                .map_err(|e| e.to_string());
            let _ = tx.send(Task::Saved {
                key,
                path,
                native,
                state,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn handle_tasks(&mut self, ctx: &egui::Context) {
        while let Ok(task) = self.tasks_rx.try_recv() {
            self.running_tasks = self.running_tasks.saturating_sub(1);
            match task {
                Task::Opened {
                    path,
                    revert: Some(key),
                    result,
                } => match result {
                    Ok(document) => {
                        if let Some(doc) = self.doc_mut(key) {
                            doc.session = Session::opened(*document);
                            doc.layers = PanelState::default();
                            if let Some(top) = doc.session.document().layers().last() {
                                doc.layers.select_only(top.id);
                            }
                            doc.origin = "Revert";
                        }
                        self.request_damage(key, Damage::All);
                        self.status = format!("Reverted to {}", path.display());
                    }
                    Err(e) => {
                        self.dialog = Some(Dialog::Error(format!(
                            "Could not revert to {}:\n{e}",
                            path.display()
                        )))
                    }
                },
                Task::Opened {
                    path,
                    revert: None,
                    result,
                } => match result {
                    Ok(document) => {
                        let native = FileFormat::from_path(&path) == Some(FileFormat::Native)
                            || std::fs::read(&path)
                                .ok()
                                .and_then(|b| FileFormat::sniff(&b))
                                == Some(FileFormat::Native);
                        let title = file_title(&path);
                        let (save_path, source) = if native {
                            (Some(path.clone()), None)
                        } else {
                            (None, Some(path.clone()))
                        };
                        self.add_document(
                            Session::opened(*document),
                            title,
                            save_path,
                            source,
                            "Open",
                        );
                        self.status = format!("Opened {}", path.display());
                    }
                    Err(e) => {
                        self.dialog = Some(Dialog::Error(format!(
                            "Could not open {}:\n{e}",
                            path.display()
                        )))
                    }
                },
                Task::Saved {
                    key,
                    path,
                    native,
                    state,
                    result,
                } => {
                    if let Some(doc) = self.doc_mut(key) {
                        if native {
                            doc.saving = false;
                        }
                        match &result {
                            Ok(()) if native => {
                                doc.session.mark_saved_at(state);
                                doc.title = file_title(&path);
                                doc.path = Some(path.clone());
                                doc.source = None;
                            }
                            _ => {}
                        }
                    }
                    match result {
                        Ok(()) => {
                            self.status = format!(
                                "{} {}",
                                if native { "Saved" } else { "Exported" },
                                path.display()
                            );
                            self.advance_close(ctx);
                        }
                        Err(e) => {
                            // A failed save stops any close or quit waiting on it.
                            self.close_queue.clear();
                            self.quitting = false;
                            self.dialog = Some(Dialog::Error(format!(
                                "Could not save {}:\n{e}",
                                path.display()
                            )));
                        }
                    }
                }
            }
        }
    }

    // ----- Commands -------------------------------------------------------

    fn run(&mut self, key: DocKey, command: Command, coalesce: Option<&'static str>) {
        let Some(doc) = self.doc_mut(key) else { return };
        let result = match coalesce {
            Some(k) => doc.session.execute_coalescing(command, k),
            None => doc.session.execute(command),
        };
        match result {
            Ok(effects) => self.after_change(key, effects),
            Err(e) => self.status = format!("Not possible: {e}"),
        }
    }

    fn after_change(&mut self, key: DocKey, effects: Effects) {
        if let Some(doc) = self.doc_mut(key) {
            if !effects.added_layers.is_empty() {
                doc.layers.selected = effects.added_layers.iter().copied().collect();
                doc.layers.active = effects.added_layers.last().copied();
            }
            doc.layers.prune(doc.session.document());
        }
        self.request_damage(key, effects.damage);
    }

    /// Asks the renderer to recomposite the damaged tiles.
    fn request_damage(&mut self, key: DocKey, damage: Damage) {
        let Some(doc) = self.docs.iter().find(|d| d.key == key) else {
            return;
        };
        let document = doc.session.document();
        let coords: Vec<_> = match damage {
            Damage::None => return,
            Damage::Tiles(tiles) => tiles.into_iter().collect(),
            // Everything that has content now, and everything showing now
            // (so tiles that became empty are cleared).
            Damage::All => {
                let mut all: BTreeSet<_> = document.covered_tiles();
                all.extend(doc.canvas.tile_coords());
                all.into_iter().collect()
            }
        };
        if coords.is_empty() {
            return;
        }
        let focus = doc.canvas.focus_tile();
        self.renderer
            .request(key, Arc::new(document.clone()), coords, focus);
    }

    fn undo_redo(&mut self, key: DocKey, redo: bool) {
        let Some(doc) = self.doc_mut(key) else { return };
        doc.session.end_coalescing();
        let effects = if redo {
            doc.session.redo()
        } else {
            doc.session.undo()
        };
        if let Some(effects) = effects {
            self.after_change(
                key,
                Effects {
                    added_layers: Vec::new(),
                    ..effects
                },
            );
        }
    }

    fn jump_history(&mut self, key: DocKey, position: usize) {
        let Some(doc) = self.doc_mut(key) else { return };
        doc.session.end_coalescing();
        let effects = doc.session.jump_to(position);
        for e in effects {
            self.after_change(
                key,
                Effects {
                    added_layers: Vec::new(),
                    ..e
                },
            );
        }
    }

    fn handle_intent(&mut self, key: DocKey, intent: Intent) {
        match intent {
            Intent::Execute(command) => {
                if let Some(doc) = self.doc_mut(key) {
                    doc.session.end_coalescing();
                }
                self.run(key, command, None);
            }
            Intent::Coalesce(command, k) => self.run(key, command, Some(k)),
            Intent::EndGesture => {
                if let Some(doc) = self.doc_mut(key) {
                    doc.session.end_coalescing();
                }
            }
            Intent::NewLayer => self.perform_on(key, Action::NewLayer),
            Intent::NewGroup => self.perform_on(key, Action::NewGroup),
            Intent::Delete => self.perform_on(key, Action::DeleteLayer),
        }
    }

    // ----- Actions --------------------------------------------------------

    fn item_state(&self, action: Action) -> ItemState {
        let doc = self.active_doc();
        let has_doc = doc.is_some();
        let has_selection = doc.is_some_and(|d| !d.layers.selected.is_empty());
        let mut label = None;
        let enabled = match action {
            Action::New
            | Action::Open
            | Action::Quit
            | Action::ResetLayout
            | Action::ShowLayers
            | Action::ShowHistory => true,
            Action::ToolHand | Action::ToolZoom | Action::ToolRotate => true,
            Action::Undo => {
                label = doc
                    .and_then(|d| d.session.undo_label())
                    .map(|l| format!("Undo {l}"));
                doc.is_some_and(|d| d.session.can_undo())
            }
            Action::Redo => {
                label = doc
                    .and_then(|d| d.session.redo_label())
                    .map(|l| format!("Redo {l}"));
                doc.is_some_and(|d| d.session.can_redo())
            }
            Action::Revert => doc.is_some_and(|d| d.path.is_some() || d.source.is_some()),
            Action::Save | Action::SaveAs | Action::ExportAs | Action::Close => {
                has_doc && !doc.is_some_and(|d| d.saving)
            }
            Action::DuplicateLayer | Action::DeleteLayer | Action::GroupLayers => has_selection,
            Action::NewLayer
            | Action::NewGroup
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::FitOnScreen
            | Action::ActualPixels
            | Action::ResetRotation
            | Action::FlipHorizontal => has_doc,
        };
        ItemState { enabled, label }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        if !self.item_state(action).enabled {
            return;
        }
        match action {
            Action::New => {
                let (width, height, ppi, white) = self.last_new;
                self.dialog = Some(Dialog::New(NewForm {
                    width,
                    height,
                    ppi,
                    white,
                }));
            }
            Action::Open => {
                let picked = rfd::FileDialog::new()
                    .set_title("Open")
                    .add_filter("Images", &["iwdoc", "png", "jpg", "jpeg", "jpe"])
                    .add_filter("ImageWorks Document", &["iwdoc"])
                    .add_filter("PNG", &["png"])
                    .add_filter("JPEG", &["jpg", "jpeg", "jpe"])
                    .pick_files();
                for path in picked.unwrap_or_default() {
                    self.open_path(ctx, path, None);
                }
            }
            Action::Close => {
                if let Some(key) = self.active {
                    self.request_close(ctx, vec![key], false);
                }
            }
            Action::Save => {
                if let Some(key) = self.active {
                    self.save(ctx, key, false);
                }
            }
            Action::SaveAs => {
                if let Some(key) = self.active {
                    self.save(ctx, key, true);
                }
            }
            Action::ExportAs => {
                if let Some(doc) = self.active_doc() {
                    let key = doc.key;
                    let picked = rfd::FileDialog::new()
                        .set_title("Export As")
                        .set_file_name(format!("{}.png", doc.title))
                        .add_filter("PNG", &["png"])
                        .add_filter("JPEG", &["jpg", "jpeg"])
                        .save_file();
                    if let Some(path) = picked {
                        let format = match FileFormat::from_path(&path) {
                            Some(FileFormat::Jpeg) => FileFormat::Jpeg,
                            _ => FileFormat::Png,
                        };
                        self.start_save(ctx, key, ensure_extension(path, format), format);
                    }
                }
            }
            Action::Revert => {
                if let Some(doc) = self.active_doc() {
                    let key = doc.key;
                    if doc.modified() {
                        self.dialog = Some(Dialog::ConfirmRevert(key));
                    } else {
                        self.revert(ctx, key);
                    }
                }
            }
            Action::Quit => {
                let keys = self.docs.iter().map(|d| d.key).collect();
                self.request_close(ctx, keys, true);
            }
            Action::Undo | Action::Redo => {
                if let Some(key) = self.active {
                    self.undo_redo(key, action == Action::Redo);
                }
            }
            Action::ToolHand => self.tool = Tool::Hand,
            Action::ToolZoom => self.tool = Tool::Zoom,
            Action::ToolRotate => self.tool = Tool::Rotate,
            Action::ShowLayers | Action::ShowHistory => {
                let tab = if action == Action::ShowLayers {
                    Tab::Layers
                } else {
                    Tab::History
                };
                match self.dock.find_tab(&tab) {
                    Some(path) => {
                        let _ = self.dock.set_active_tab(path);
                    }
                    None => {
                        self.dock.main_surface_mut().split_right(
                            NodeIndex::root(),
                            0.78,
                            vec![tab],
                        );
                    }
                }
            }
            Action::ResetLayout => {
                let mut dock = default_layout();
                if !self.docs.is_empty() {
                    if let Some(start) = dock.find_tab(&Tab::Start) {
                        dock.set_focused_node_and_surface(start.node_path());
                        for doc in &self.docs {
                            dock.push_to_focused_leaf(Tab::Document(doc.key));
                        }
                        if let Some(start) = dock.find_tab(&Tab::Start) {
                            dock.remove_tab(start);
                        }
                    }
                }
                self.dock = dock;
            }
            _ => {
                if let Some(key) = self.active {
                    self.perform_on(key, action);
                }
            }
        }
    }

    /// Actions that act on one document and need no dialogs.
    fn perform_on(&mut self, key: DocKey, action: Action) {
        let Some(doc) = self.doc_mut(key) else { return };
        let document = doc.session.document();
        let selected = doc.layers.selected.clone();
        let active = doc.layers.active;
        let command = match action {
            Action::NewLayer => Some(model::new_layer_command(document, active)),
            Action::NewGroup => Some(model::new_group_command(document, active)),
            Action::DuplicateLayer => model::duplicate_command(document, &selected),
            Action::DeleteLayer => model::delete_command(document, &selected),
            Action::GroupLayers => model::group_command(document, &selected).map(|(c, _)| c),
            _ => None,
        };
        if let Some(command) = command {
            let group = matches!(action, Action::GroupLayers);
            doc.session.end_coalescing();
            let before: BTreeSet<_> = doc
                .session
                .document()
                .all_layers()
                .iter()
                .map(|l| l.id)
                .collect();
            self.run(key, command, None);
            if group {
                // Select the new group rather than its contents.
                if let Some(doc) = self.doc_mut(key) {
                    let new: Vec<_> = doc
                        .session
                        .document()
                        .all_layers()
                        .iter()
                        .map(|l| l.id)
                        .filter(|id| !before.contains(id))
                        .collect();
                    if let Some(id) = new.first() {
                        doc.layers.select_only(*id);
                    }
                }
            }
            return;
        }
        let Some(viewport) = doc.canvas.viewport() else {
            return;
        };
        let (w, h) = (
            doc.session.document().width(),
            doc.session.document().height(),
        );
        let Some(view) = doc.canvas.view.as_mut() else {
            return;
        };
        match action {
            Action::ZoomIn => view.zoom_about(viewport.center(), view.step_in(), viewport),
            Action::ZoomOut => view.zoom_about(viewport.center(), view.step_out(), viewport),
            Action::FitOnScreen => view.fit_to(w, h, viewport, true),
            Action::ActualPixels => view.zoom_about(viewport.center(), 1.0, viewport),
            Action::ResetRotation => view.set_rotation(0.0),
            Action::FlipHorizontal => view.toggle_mirror(),
            _ => {}
        }
    }

    fn save(&mut self, ctx: &egui::Context, key: DocKey, ask: bool) -> bool {
        let Some(doc) = self.doc(key) else {
            return false;
        };
        let path = match (&doc.path, ask) {
            (Some(path), false) => Some(path.clone()),
            _ => {
                let suggested = format!("{}.{}", doc.title, format::native::EXTENSION);
                rfd::FileDialog::new()
                    .set_title("Save As")
                    .set_file_name(suggested)
                    .add_filter("ImageWorks Document", &[format::native::EXTENSION])
                    .save_file()
                    .map(|p| ensure_extension(p, FileFormat::Native))
            }
        };
        match path {
            Some(path) => {
                self.start_save(ctx, key, path, FileFormat::Native);
                true
            }
            None => false,
        }
    }

    fn revert(&mut self, ctx: &egui::Context, key: DocKey) {
        if let Some(path) = self
            .doc(key)
            .and_then(|d| d.path.clone().or_else(|| d.source.clone()))
        {
            self.open_path(ctx, path, Some(key));
        }
    }

    fn request_close(&mut self, ctx: &egui::Context, keys: Vec<DocKey>, quit: bool) {
        for key in keys {
            if !self.close_queue.contains(&key) {
                self.close_queue.push(key);
            }
        }
        self.quitting |= quit;
        self.advance_close(ctx);
    }

    /// Closes queued documents until one needs an answer or a save.
    fn advance_close(&mut self, ctx: &egui::Context) {
        while let Some(&key) = self.close_queue.first() {
            let Some(doc) = self.doc(key) else {
                self.close_queue.remove(0);
                continue;
            };
            if doc.saving {
                return;
            }
            if doc.modified() {
                if self.dialog.is_none() {
                    self.dialog = Some(Dialog::Unsaved(key));
                }
                return;
            }
            self.close_document(key);
            self.close_queue.remove(0);
        }
        if self.quitting {
            self.allow_quit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn cancel_close(&mut self) {
        self.close_queue.clear();
        self.quitting = false;
    }

    // ----- Frame ----------------------------------------------------------

    fn collect_shortcuts(&self, ctx: &egui::Context) -> Vec<Action> {
        let menu_handles = self.menu.as_ref().is_some_and(|m| m.handles_shortcuts());
        let typing = ctx.egui_wants_keyboard_input();
        let mut actions: Vec<Action> = Action::ALL.to_vec();
        // Check the most specific shortcuts first, so Cmd+Shift+Z is not
        // taken for Cmd+Z.
        actions.sort_by_key(|a| {
            std::cmp::Reverse(a.shortcut().map_or(0, |s| {
                s.modifiers.shift as u8 + s.modifiers.alt as u8 + s.modifiers.command as u8
            }))
        });
        let mut out = Vec::new();
        ctx.input_mut(|input| {
            for action in actions {
                let Some(shortcut) = action.shortcut() else {
                    continue;
                };
                if menu_handles && shortcut.modifiers.command {
                    continue;
                }
                if typing
                    && (action.is_single_key() || matches!(action, Action::Undo | Action::Redo))
                {
                    continue;
                }
                if input.consume_shortcut(&shortcut) {
                    out.push(action);
                }
            }
        });
        out
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.active_doc() {
            Some(doc) => format!(
                "{}{} — ImageWorks",
                doc.title,
                if doc.modified() { " •" } else { "" }
            ),
            None => "ImageWorks".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        for (tool, icon, action) in [
            (Tool::Hand, Icon::Hand, Action::ToolHand),
            (Tool::Zoom, Icon::Zoom, Action::ToolZoom),
            (Tool::Rotate, Icon::Rotate, Action::ToolRotate),
        ] {
            let tip = format!(
                "{} ({})",
                action.label(),
                action
                    .shortcut()
                    .map(|s| ui.ctx().format_shortcut(&s))
                    .unwrap_or_default()
            );
            if icons::button(ui, icon, 30.0, self.tool == tool, &tip).clicked() {
                self.tool = tool;
            }
        }
    }

    fn tool_options(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> Vec<Action> {
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.tool.name()).strong());
            ui.separator();
            let has_doc = self.active.is_some();
            ui.add_enabled_ui(has_doc, |ui| match self.tool {
                Tool::Hand | Tool::Zoom => {
                    if self.tool == Tool::Zoom {
                        ui.weak(if ctx.input(|i| i.modifiers.alt) {
                            "Click to zoom out"
                        } else {
                            "Click to zoom in · Alt-click to zoom out"
                        });
                        ui.separator();
                    }
                    if ui.button("Fit Screen").clicked() {
                        actions.push(Action::FitOnScreen);
                    }
                    if ui.button("100%").clicked() {
                        actions.push(Action::ActualPixels);
                    }
                }
                Tool::Rotate => {
                    if let Some(view) = self
                        .active
                        .and_then(|k| self.docs.iter_mut().find(|d| d.key == k))
                        .and_then(|d| d.canvas.view.as_mut())
                    {
                        let mut degrees = view.rotation_degrees();
                        ui.label("Angle");
                        if ui
                            .add(
                                egui::DragValue::new(&mut degrees)
                                    .range(-180.0..=180.0)
                                    .suffix("°")
                                    .speed(0.5),
                            )
                            .changed()
                        {
                            view.set_rotation(degrees.to_radians());
                        }
                    }
                    ui.weak("Drag to rotate · Shift snaps to 15°");
                    if ui.button("Reset View").clicked() {
                        actions.push(Action::ResetRotation);
                    }
                }
            });
        });
        actions
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let pending = self.renderer.pending();
            let left = if pending > 0 {
                format!("Rendering {pending} tiles…")
            } else if self.running_tasks > 0 || !self.status.is_empty() {
                self.status.clone()
            } else {
                String::new()
            };
            ui.label(RichText::new(left).color(theme::TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(doc) = self.active_doc() {
                    let d = doc.session.document();
                    let depth = match d.bit_depth() {
                        BitDepth::U8 => "8-bit",
                        BitDepth::U16 => "16-bit",
                        BitDepth::F32 => "32-bit float",
                    };
                    ui.label(format!(
                        "{} × {} px · {} ppi · RGB {depth}",
                        d.width(),
                        d.height(),
                        d.resolution().ppi
                    ));
                    if let Some(view) = doc.canvas.view {
                        ui.separator();
                        if view.rotation != 0.0 || view.mirror {
                            ui.label(format!(
                                "{:.1}°{}",
                                view.rotation_degrees(),
                                if view.mirror { " · flipped" } else { "" }
                            ));
                            ui.separator();
                        }
                        ui.label(format!("{:.1}%", view.zoom * 100.0));
                    }
                    if let Some(p) = self.cursor {
                        ui.separator();
                        ui.monospace(format!(
                            "X {:>6}  Y {:>6}",
                            p.x.floor() as i64,
                            p.y.floor() as i64
                        ));
                    }
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        match dialog {
            Dialog::New(mut form) => {
                let mut create = false;
                egui::Modal::new(egui::Id::new("new-document")).show(ctx, |ui| {
                    ui.heading("New Document");
                    ui.add_space(6.0);
                    egui::ComboBox::from_label("Preset")
                        .selected_text(
                            PRESETS
                                .iter()
                                .find(|(_, w, h)| (*w, *h) == (form.width, form.height))
                                .map_or("Custom", |p| p.0),
                        )
                        .show_ui(ui, |ui| {
                            for (name, w, h) in PRESETS {
                                if ui
                                    .selectable_label((w, h) == (form.width, form.height), name)
                                    .clicked()
                                {
                                    form.width = w;
                                    form.height = h;
                                }
                            }
                        });
                    egui::Grid::new("new-grid")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Width");
                            ui.add(
                                egui::DragValue::new(&mut form.width)
                                    .range(1..=MAX_NEW_DIMENSION)
                                    .suffix(" px"),
                            );
                            ui.end_row();
                            ui.label("Height");
                            ui.add(
                                egui::DragValue::new(&mut form.height)
                                    .range(1..=MAX_NEW_DIMENSION)
                                    .suffix(" px"),
                            );
                            ui.end_row();
                            ui.label("Resolution");
                            ui.add(
                                egui::DragValue::new(&mut form.ppi)
                                    .range(1.0..=10_000.0)
                                    .suffix(" ppi")
                                    .speed(1.0),
                            );
                            ui.end_row();
                            ui.label("Background");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut form.white, true, "White");
                                ui.radio_value(&mut form.white, false, "Transparent");
                            });
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            create = true;
                        }
                        if ui.button("Cancel").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            keep = false;
                        }
                    });
                });
                if create {
                    self.last_new = (form.width, form.height, form.ppi, form.white);
                    self.new_document(&form);
                    keep = false;
                }
                if keep {
                    self.dialog = Some(Dialog::New(form));
                }
            }
            Dialog::Unsaved(key) => {
                let title = self.doc(key).map(|d| d.title.clone()).unwrap_or_default();
                let mut choice = None;
                egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                    ui.set_max_width(360.0);
                    ui.heading(format!("Save changes to “{title}”?"));
                    ui.label("Your changes will be lost if you don't save them.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save…").clicked() {
                            choice = Some(0);
                        }
                        if ui.button("Don't Save").clicked() {
                            choice = Some(1);
                        }
                        if ui.button("Cancel").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            choice = Some(2);
                        }
                    });
                });
                match choice {
                    Some(0) => {
                        if !self.save(ctx, key, false) {
                            self.cancel_close();
                        }
                        keep = false;
                    }
                    Some(1) => {
                        self.close_document(key);
                        self.close_queue.retain(|k| *k != key);
                        keep = false;
                        self.advance_close(ctx);
                    }
                    Some(_) => {
                        self.cancel_close();
                        keep = false;
                    }
                    None => {}
                }
                if keep && self.dialog.is_none() {
                    self.dialog = Some(Dialog::Unsaved(key));
                }
            }
            Dialog::ConfirmRevert(key) => {
                let mut choice = None;
                egui::Modal::new(egui::Id::new("revert")).show(ctx, |ui| {
                    ui.heading("Revert to the saved version?");
                    ui.label("Your changes and the undo history will be discarded.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Revert").clicked() {
                            choice = Some(true);
                        }
                        if ui.button("Cancel").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            choice = Some(false);
                        }
                    });
                });
                if let Some(revert) = choice {
                    keep = false;
                    if revert {
                        self.revert(ctx, key);
                    }
                }
                if keep {
                    self.dialog = Some(Dialog::ConfirmRevert(key));
                }
            }
            Dialog::Error(message) => {
                let mut close = false;
                egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.heading("Something went wrong");
                    ui.label(&message);
                    ui.add_space(8.0);
                    if ui.button("OK").clicked()
                        || ui.input(|i| {
                            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
                        })
                    {
                        close = true;
                    }
                });
                if !close {
                    self.dialog = Some(Dialog::Error(message));
                }
            }
        }
    }
}

struct Viewer<'a> {
    docs: &'a mut Vec<OpenDoc>,
    active: Option<DocKey>,
    tool: Tool,
    checker: &'a TextureHandle,
    intents: Vec<(DocKey, Intent)>,
    jump: Option<(DocKey, usize)>,
    close: Vec<DocKey>,
    actions: Vec<Action>,
    cursor: Option<Pos2>,
    viewport_changed: Vec<DocKey>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Start => "Start".into(),
            Tab::Layers => "Layers".into(),
            Tab::History => "History".into(),
            Tab::Document(key) => match self.docs.iter().find(|d| d.key == *key) {
                Some(doc) => format!(
                    "{}{}",
                    doc.title,
                    if doc.session.is_modified() {
                        " •"
                    } else {
                        ""
                    }
                )
                .into(),
                None => "".into(),
            },
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        matches!(tab, Tab::Document(_))
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        // Closing goes through the app, which may ask about unsaved changes
        // and removes the tab itself.
        if let Tab::Document(key) = tab {
            self.close.push(*key);
        }
        OnCloseResponse::Ignore
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Document(_))
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Document(_) | Tab::Layers | Tab::History => [false, false],
            Tab::Start => [true, true],
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Start => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.heading("ImageWorks");
                    ui.weak("No document is open.");
                    ui.add_space(12.0);
                    if ui.button("New Document…").clicked() {
                        self.actions.push(Action::New);
                    }
                    if ui.button("Open…").clicked() {
                        self.actions.push(Action::Open);
                    }
                    ui.add_space(6.0);
                    ui.weak("You can also drop image files onto this window.");
                });
            }
            Tab::Document(key) => {
                if let Some(doc) = self.docs.iter_mut().find(|d| d.key == *key) {
                    let had_view = doc.canvas.view.is_some();
                    let events =
                        doc.canvas
                            .show(ui, doc.session.document(), self.tool, self.checker);
                    if events.cursor.is_some() {
                        self.cursor = events.cursor;
                    }
                    if events.viewport.is_some() && !had_view {
                        self.viewport_changed.push(*key);
                    }
                }
            }
            Tab::Layers => match self
                .active
                .and_then(|k| self.docs.iter().find(|d| d.key == k).map(|d| d.key))
            {
                Some(key) => {
                    let doc = self
                        .docs
                        .iter_mut()
                        .find(|d| d.key == key)
                        .expect("found above");
                    for intent in panels::layers::show(ui, doc.session.document(), &mut doc.layers)
                    {
                        self.intents.push((key, intent));
                    }
                }
                None => {
                    ui.weak("No document");
                }
            },
            Tab::History => match self
                .active
                .and_then(|k| self.docs.iter().find(|d| d.key == k))
            {
                Some(doc) => {
                    if let Some(position) = panels::history::show(ui, &doc.session, doc.origin) {
                        self.jump = Some((doc.key, position));
                    }
                }
                None => {
                    ui.weak("No document");
                }
            },
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_tasks(&ctx);
        for result in self.renderer.take_results() {
            if self.renderer.is_current(&result) {
                if let Some(doc) = self.docs.iter_mut().find(|d| d.key == result.doc) {
                    doc.canvas.apply(&ctx, result);
                }
            }
        }
        if let Some((_, Tab::Document(key))) = self.dock.find_active_focused() {
            self.active = Some(*key);
        }

        let mut actions = Vec::new();
        if let Some(menu) = &self.menu {
            actions.extend(menu.poll());
        }
        if self.dialog.is_none() {
            actions.extend(self.collect_shortcuts(&ctx));
        }
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            self.open_path(&ctx, file.path().to_path_buf(), None);
        }

        if self.menu.is_none() {
            egui::Panel::top("menu-bar").show(ui, |ui| {
                let state = |a: Action| self.item_state(a);
                if let Some(action) = native_menu::egui_menu_bar(ui, &state) {
                    actions.push(action);
                }
            });
        }
        egui::Panel::top("tool-options").show(ui, |ui| {
            actions.extend(self.tool_options(&ctx, ui));
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("toolbar")
            .resizable(false)
            .exact_size(40.0)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| self.toolbar(ui));
            });

        let mut viewer = Viewer {
            docs: &mut self.docs,
            active: self.active,
            tool: self.tool,
            checker: &self.checker,
            intents: Vec::new(),
            jump: None,
            close: Vec::new(),
            actions: Vec::new(),
            cursor: None,
            viewport_changed: Vec::new(),
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let mut style = Style::from_egui(ui.style().as_ref());
                style.tab_bar.bg_fill = theme::PANEL_DARK;
                DockArea::new(&mut self.dock)
                    .style(style)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
        let Viewer {
            intents,
            jump,
            close,
            actions: tab_actions,
            cursor,
            ..
        } = viewer;
        self.cursor = cursor;
        actions.extend(tab_actions);
        for (key, intent) in intents {
            self.handle_intent(key, intent);
        }
        if let Some((key, position)) = jump {
            self.jump_history(key, position);
        }
        if !close.is_empty() {
            self.request_close(&ctx, close, false);
        }

        self.dialogs(&ctx);
        for action in actions {
            self.perform(&ctx, action);
        }

        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_quit
            && self.docs.iter().any(|d| d.modified() || d.saving)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let keys = self.docs.iter().map(|d| d.key).collect();
            self.request_close(&ctx, keys, true);
        }

        self.update_title(&ctx);
        if let Some(mut menu) = self.menu.take() {
            menu.update(&|a| self.item_state(a));
            self.menu = Some(menu);
        }
        if self.renderer.pending() > 0 || self.running_tasks > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
