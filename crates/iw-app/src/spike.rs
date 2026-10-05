//! The M0 toolkit spike UI: three dockable diagnostic panels.
//!
//! Each panel answers one of the spike's questions. Nothing here is an
//! editor feature; the real shell replaces this module in M3.

use crate::native_menu::{Command, NativeMenu};
use eframe::egui::{self, Color32, Event, Pos2, Sense, Stroke, TouchPhase};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};

const LOG_CAPACITY: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    PenProbe,
    EventLog,
    Display,
}

/// One sampled point of a probe stroke. `pressure` is `None` when the
/// platform reported no force for it (a mouse, or a pen the windowing
/// layer does not understand).
#[derive(Clone, Copy)]
struct Sample {
    pos: Pos2,
    pressure: Option<f32>,
}

struct State {
    strokes: Vec<Vec<Sample>>,
    touch_down: bool,
    max_pressure: Option<f32>,
    log: Vec<String>,
    adapter: String,
    menu_status: String,
}

impl State {
    fn log(&mut self, line: impl Into<String>) {
        if self.log.len() == LOG_CAPACITY {
            self.log.remove(0);
        }
        self.log.push(line.into());
    }
}

pub struct SpikeApp {
    dock: DockState<Tab>,
    menu: Option<NativeMenu>,
    state: State,
}

fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::PenProbe]);
    let surface = dock.main_surface_mut();
    let [_, right] = surface.split_right(NodeIndex::root(), 0.72, vec![Tab::Display]);
    surface.split_below(right, 0.5, vec![Tab::EventLog]);
    dock
}

impl SpikeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let adapter = match &cc.wgpu_render_state {
            Some(rs) => {
                let info = rs.adapter.get_info();
                format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
            }
            None => "none: not rendering through wgpu".to_owned(),
        };
        let (menu, menu_status) = match NativeMenu::install(cc) {
            Ok(m) => (Some(m), "native menu bar installed".to_owned()),
            Err(e) => (None, format!("in-window fallback: {e}")),
        };

        let mut state = State {
            strokes: Vec::new(),
            touch_down: false,
            max_pressure: None,
            log: Vec::new(),
            adapter,
            menu_status,
        };
        state.log(format!("started, engine {}", iw_engine::VERSION));
        Self {
            dock: default_layout(),
            menu,
            state,
        }
    }

    fn run(&mut self, cmd: Command, ctx: &egui::Context) {
        self.state.log(format!("command: {cmd:?}"));
        match cmd {
            Command::TestOpenDialog => {
                let picked = rfd::FileDialog::new()
                    .set_title("Toolkit spike: open")
                    .pick_file();
                self.state.log(match picked {
                    Some(p) => format!("open dialog returned {}", p.display()),
                    None => "open dialog cancelled".to_owned(),
                });
            }
            Command::TestSaveDialog => {
                let picked = rfd::FileDialog::new()
                    .set_title("Toolkit spike: save (nothing is written)")
                    .set_file_name("untitled.png")
                    .save_file();
                self.state.log(match picked {
                    Some(p) => format!("save dialog returned {} (not written)", p.display()),
                    None => "save dialog cancelled".to_owned(),
                });
            }
            Command::ResetLayout => self.dock = default_layout(),
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

impl eframe::App for SpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut commands = match &self.menu {
            Some(menu) => menu.poll(),
            None => Vec::new(),
        };

        if self.menu.is_none() {
            egui::Panel::top("fallback_menu").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for cmd in Command::ALL {
                        if ui.button(cmd.label()).clicked() {
                            commands.push(cmd);
                        }
                    }
                });
            });
        }
        for cmd in commands {
            self.run(cmd, &ctx);
        }

        DockArea::new(&mut self.dock)
            .style(Style::from_egui(ui.style().as_ref()))
            .show_leaf_close_all_buttons(false)
            .show_inside(
                ui,
                &mut Viewer {
                    state: &mut self.state,
                },
            );
    }
}

struct Viewer<'a> {
    state: &'a mut State,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::PenProbe => "Pen probe",
            Tab::EventLog => "Event log",
            Tab::Display => "Display",
        }
        .into()
    }

    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::PenProbe => pen_probe(ui, self.state),
            Tab::EventLog => event_log(ui, self.state),
            Tab::Display => display(ui, self.state),
        }
    }
}

/// Draw with a mouse or pen. Stroke width follows reported pressure, so a
/// pen that works draws a visibly tapered line and a pen that does not
/// draws a hairline.
fn pen_probe(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
            state.strokes.clear();
            state.max_pressure = None;
        }
        ui.label(match state.max_pressure {
            Some(p) => format!("pressure received, peak {p:.3}"),
            None => "no pressure data received yet".to_owned(),
        });
    });

    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::drag());
    let rect = response.rect;
    painter.rect_filled(rect, 0.0, Color32::from_gray(24));

    // Pens and touch arrive as `Event::Touch` with a force; egui also turns
    // them into pointer events, so pointer input is ignored while one is down.
    let touches: Vec<(TouchPhase, Pos2, Option<f32>)> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                Event::Touch {
                    phase, pos, force, ..
                } => Some((*phase, *pos, *force)),
                _ => None,
            })
            .collect()
    });
    for (phase, pos, force) in touches {
        if !rect.contains(pos) && phase == TouchPhase::Start {
            continue;
        }
        let recording = match phase {
            TouchPhase::Start => {
                state.touch_down = true;
                state.strokes.push(Vec::new());
                state.log(format!("touch/pen down, force {force:?}"));
                true
            }
            TouchPhase::Move => state.touch_down,
            TouchPhase::End => std::mem::take(&mut state.touch_down),
            TouchPhase::Cancel => {
                state.touch_down = false;
                false
            }
        };
        if !recording {
            continue;
        }
        if let Some(f) = force {
            state.max_pressure = Some(state.max_pressure.map_or(f, |m| m.max(f)));
        }
        if let Some(stroke) = state.strokes.last_mut() {
            stroke.push(Sample {
                pos,
                pressure: force,
            });
        }
    }

    if !state.touch_down {
        if response.drag_started() {
            state.strokes.push(Vec::new());
        }
        if response.dragged() {
            if let (Some(pos), Some(stroke)) =
                (response.interact_pointer_pos(), state.strokes.last_mut())
            {
                stroke.push(Sample {
                    pos,
                    pressure: None,
                });
            }
        }
    }

    for stroke in &state.strokes {
        for pair in stroke.windows(2) {
            let width = match pair[1].pressure {
                Some(p) => 1.0 + p * 14.0,
                None => 1.0,
            };
            painter.line_segment(
                [pair[0].pos, pair[1].pos],
                Stroke::new(width, Color32::from_gray(230)),
            );
        }
    }
}

fn event_log(ui: &mut egui::Ui, state: &mut State) {
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &state.log {
                ui.monospace(line);
            }
        });
}

fn display(ui: &mut egui::Ui, state: &mut State) {
    let ctx = ui.ctx().clone();
    let (native_ppp, monitor, inner) = ctx.input(|i| {
        let v = i.viewport();
        (v.native_pixels_per_point, v.monitor_size, v.inner_rect)
    });
    let row = |ui: &mut egui::Ui, k: &str, v: String| {
        ui.label(k);
        ui.add(egui::Label::new(egui::RichText::new(v).monospace()).wrap());
        ui.end_row();
    };
    egui::Grid::new("display_grid")
        .num_columns(2)
        .show(ui, |ui| {
            row(
                ui,
                "OS",
                format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            );
            row(ui, "GPU adapter", state.adapter.clone());
            row(ui, "Menu bar", state.menu_status.clone());
            row(ui, "Scale factor (OS)", format!("{native_ppp:?}"));
            row(
                ui,
                "Scale factor (in use)",
                format!("{:.2}", ctx.pixels_per_point()),
            );
            row(ui, "Monitor size (points)", format!("{monitor:?}"));
            row(ui, "Window inner rect", format!("{inner:?}"));
        });
}
