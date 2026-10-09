//! End-to-end tests of the shell, driven through the UI the way a person
//! would: clicking menus and buttons found by their accessible labels, and
//! pressing shortcuts. No window or GPU is needed.

use crate::app::App;
use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn harness(open: Vec<PathBuf>) -> Harness<'static, App> {
    Harness::builder()
        .with_size([1400.0, 900.0])
        // Real frame timing, so two clicks a frame apart are a double click.
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |cc| App::with_options(cc, open, false))
}

/// Steps frames until `done` holds, for work that finishes on a
/// background thread (opening, saving, compositing).
fn wait(h: &mut Harness<'static, App>, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(3);
}

fn menu(h: &mut Harness<'static, App>, title: &str, item: &str) {
    h.get_by_label(title).click();
    h.run_steps(2);
    menu_item(h, item);
}

/// Menu items are labelled with their shortcut appended ("New… Ctrl+N").
/// A panel button can share a menu item's name, so take the last match:
/// the open menu is drawn on top, after everything else.
fn menu_item(h: &mut Harness<'static, App>, item: &str) {
    let prefix = format!("{item} ");
    let matches = h
        .query_all_by(move |n| {
            n.role() == eframe::egui::accesskit::Role::Button
                && n.label()
                    .is_some_and(|l| l == item || l.starts_with(&prefix))
        })
        .count();
    assert!(matches > 0, "no menu item {item}");
    let prefix = format!("{item} ");
    h.query_all_by(move |n| {
        n.role() == eframe::egui::accesskit::Role::Button
            && n.label()
                .is_some_and(|l| l == item || l.starts_with(&prefix))
    })
    .last()
    .expect("counted above")
    .click();
    h.run_steps(3);
}

fn new_document(h: &mut Harness<'static, App>) {
    menu(h, "File", "New…");
    assert!(h.state().has_dialog());
    h.get_by_label("Create").click();
    h.run_steps(3);
    assert!(!h.state().has_dialog());
}

fn layer_names(app: &App) -> Vec<String> {
    let doc = app.active_doc_for_test().expect("a document is open");
    crate::layers_model::rows(doc.session.document(), &Default::default())
        .into_iter()
        .map(|r| r.name)
        .collect()
}

#[test]
fn starts_with_the_start_screen() {
    let mut h = harness(vec![]);
    h.run_steps(3);
    assert!(h.state().docs().is_empty());
    h.get_by_label("New Document…");
    h.get_by_label("Open…");
}

#[test]
fn a_new_document_has_a_white_background_layer() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);
    let app = h.state();
    assert_eq!(app.docs().len(), 1);
    let doc = &app.docs()[0];
    assert_eq!(doc.title, "Untitled-1");
    assert_eq!(
        (
            doc.session.document().width(),
            doc.session.document().height()
        ),
        (1920, 1080)
    );
    assert_eq!(layer_names(app), ["Background"]);
    assert!(
        !doc.session.can_undo(),
        "creating the document is not an undo step"
    );
    // The history panel starts at "New".
    h.get_by_label("New");
}

#[test]
fn layer_menu_commands_undo_and_redo() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);

    menu(&mut h, "Layer", "New Layer");
    assert_eq!(layer_names(h.state()), ["Layer 1", "Background"]);
    menu(&mut h, "Layer", "New Group");
    assert_eq!(layer_names(h.state()), ["Group 1", "Layer 1", "Background"]);
    menu(&mut h, "Layer", "Duplicate Layer");
    assert_eq!(
        layer_names(h.state()),
        ["Group 1 copy", "Group 1", "Layer 1", "Background"]
    );
    menu(&mut h, "Layer", "Delete Layer");
    assert_eq!(layer_names(h.state()), ["Group 1", "Layer 1", "Background"]);

    // The Edit menu names the step it will undo.
    menu(&mut h, "Edit", "Undo Delete Layer");
    assert_eq!(
        layer_names(h.state()),
        ["Group 1 copy", "Group 1", "Layer 1", "Background"]
    );

    // Shortcuts work too.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(layer_names(h.state()), ["Group 1", "Layer 1", "Background"]);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    assert_eq!(
        layer_names(h.state()),
        ["Group 1 copy", "Group 1", "Layer 1", "Background"]
    );

    // Clicking a History row jumps there.
    // (The Layers panel also has a "New Layer" button, so pick the row.)
    h.get_all_by_label("New Layer")
        .last()
        .expect("history row")
        .click();
    h.run_steps(3);
    assert_eq!(layer_names(h.state()), ["Layer 1", "Background"]);
    assert!(h.state().active_doc_for_test().unwrap().session.can_redo());
}

#[test]
fn the_layers_panel_buttons_and_toggles_work() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);

    h.get_by_label("New Layer").click();
    h.run_steps(3);
    assert_eq!(layer_names(h.state()), ["Layer 1", "Background"]);

    h.get_by_label("Hide Background").click();
    h.run_steps(3);
    let doc = h.state().active_doc_for_test().unwrap();
    let background = doc.session.document().layers()[0].clone();
    assert!(!background.visible);
    assert_eq!(doc.session.undo_label(), Some("Layer Visibility"));

    h.get_by_label("Show Background").click();
    h.run_steps(3);
    h.get_by_label("Lock Background").click();
    h.run_steps(3);
    assert!(
        h.state()
            .active_doc_for_test()
            .unwrap()
            .session
            .document()
            .layers()[0]
            .locked
    );

    // Selecting a row, then deleting with the panel's button.
    h.get_by_label("Layer 1").click();
    h.run_steps(2);
    h.get_by_label("Delete Layer").click();
    h.run_steps(3);
    assert_eq!(layer_names(h.state()), ["Background"]);
}

#[test]
fn renaming_a_layer_by_double_clicking_its_name() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);
    h.get_by_label("Background").hover();
    h.run_steps(1);
    // Let the click on "Create" age, or the next two would count as a
    // triple click.
    h.run_steps(60);
    // A double click: two clicks one frame apart.
    h.get_by_label("Background").click();
    h.step();
    h.get_by_label("Background").click();
    h.run_steps(3);
    // The name field is focused; replace its text and press Enter.
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.get_by_role(eframe::egui::accesskit::Role::TextInput).type_text("Sky");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(layer_names(h.state()), ["Sky"]);
    assert_eq!(
        h.state()
            .active_doc_for_test()
            .unwrap()
            .session
            .undo_label(),
        Some("Name Change")
    );
}

#[test]
fn closing_a_modified_document_asks_first() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);
    menu(&mut h, "Layer", "New Layer");

    menu(&mut h, "File", "Close");
    assert!(h.state().has_dialog(), "should ask about unsaved changes");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().docs().len(), 1, "cancel keeps the document");

    menu(&mut h, "File", "Close");
    h.get_by_label("Don't Save").click();
    h.run_steps(3);
    assert!(h.state().docs().is_empty());
    h.get_by_label("New Document…");
}

#[test]
fn tool_shortcuts_switch_tools() {
    let mut h = harness(vec![]);
    h.run_steps(2);
    new_document(&mut h);
    h.key_press(Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().tool(), crate::canvas::Tool::Zoom);
    h.key_press(Key::R);
    h.run_steps(2);
    assert_eq!(h.state().tool(), crate::canvas::Tool::Rotate);
    h.get_by_label("Hand Tool (H)").click();
    h.run_steps(2);
    assert_eq!(h.state().tool(), crate::canvas::Tool::Hand);
}

#[test]
fn opening_editing_and_saving_a_native_document() {
    let dir = std::env::temp_dir().join(format!("iw-ui-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.iwdoc");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../iw-engine/tests/fixtures/v1.iwdoc"
        ),
        &path,
    )
    .unwrap();

    let mut h = harness(vec![path.clone()]);
    wait(&mut h, |app| !app.docs().is_empty());
    let doc = &h.state().docs()[0];
    assert_eq!(doc.title, "fixture");
    assert!(!doc.session.is_modified());
    assert_eq!(
        layer_names(h.state()),
        ["Hidden note", "Effects", "Glow", "Shade", "Background"]
    );

    menu(&mut h, "Layer", "New Layer");
    assert!(h.state().docs()[0].session.is_modified());
    // Save goes straight to the file it came from.
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    wait(&mut h, |app| {
        !app.docs()[0].session.is_modified() && !app.docs()[0].saving
    });
    let saved = iw_engine::format::open(&path).unwrap();
    assert_eq!(saved.all_layers().len(), 6);

    // Opening the same file again shows the open copy instead.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_a_damaged_file_reports_an_error() {
    let dir = std::env::temp_dir().join(format!("iw-ui-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.png");
    std::fs::write(&path, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let mut h = harness(vec![path]);
    wait(&mut h, |app| app.has_dialog());
    h.get_by_label("Something went wrong");
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert!(!h.state().has_dialog());
    assert!(h.state().docs().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
