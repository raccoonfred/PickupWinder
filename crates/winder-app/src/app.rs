//! Window layout, file handling and the scrub/play state. Every parameter
//! sits in the panel on the left and changes the coil as you edit it (undo
//! with Ctrl+Z); what the coil measures sits on the right; the views fill
//! the middle, with the timeline of the wind along the bottom.

use crate::{
    chrome::{self, Glyph, Icon},
    editor::{self, Kind},
    model::{self, Computed},
    view3d,
    viewcube::BLUE,
    views::{self, Coloring, View},
};
use eframe::egui::{
    self, Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use winder_core::{Bobbin, Design, Pattern, auto, recipe};
use winder_gcode::MachineProfile;

const WARN: Color32 = Color32::from_rgb(200, 90, 50);

pub struct WinderApp {
    design: Design,
    path: Option<PathBuf>,
    /// The design as last opened or saved, to tell unsaved edits.
    saved: Design,
    profile: MachineProfile,
    profile_path: Option<PathBuf>,
    /// Last design that computed cleanly, and what it computed to.
    computed: Option<(Design, Computed)>,
    error: Option<String>,
    status: String,
    view: View,
    coloring: Coloring,
    camera: view3d::Camera,
    /// The 3D view's GPU side; None without OpenGL 3.
    renderer: Option<Arc<Mutex<view3d::Renderer>>>,
    /// Cut through the long side (true) or the short end.
    long_side: bool,
    /// Turns shown; follows the end of the coil unless scrubbed or playing.
    scrub: f64,
    follow_end: bool,
    playing: bool,
    /// How far round the spindle is in the turn under way, as drawn. At
    /// fast playback it turns at a speed you can still follow.
    spin_phase: f64,
    /// How far the spindle turned in the last frame (rad).
    spin_sweep: f64,
    /// Playback speed as a multiple of the real winding speed.
    speedup: f64,
    imported: Vec<recipe::Imported>,
    /// Designs to go back to (Ctrl+Z) and forward to again, and the design
    /// as of the last step taken.
    undo: Vec<Design>,
    redo: Vec<Design>,
    committed: Design,
    /// Section picked in the panel or on the timeline.
    selected: Option<usize>,
    /// Section a timeline context menu was opened on.
    menu_section: Option<usize>,
    /// The design as it was last frame, to see what an edit changed; None
    /// after opening a different design.
    prev: Option<Design>,
}

impl WinderApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> WinderApp {
        chrome::fusion_style(&cc.egui_ctx);
        let mut app = WinderApp {
            design: Design::simple("strat").expect("preset exists"),
            path: None,
            saved: Design::simple("strat").expect("preset exists"),
            profile: MachineProfile::default(),
            profile_path: None,
            computed: None,
            error: None,
            status: "New design from the Strat preset.".into(),
            view: View::Coil3d,
            coloring: Coloring::Order,
            camera: view3d::Camera::default(),
            renderer: cc
                .gl
                .as_ref()
                .and_then(|gl| view3d::Renderer::new(gl).ok())
                .map(|r| Arc::new(Mutex::new(r))),
            long_side: true,
            scrub: 0.0,
            follow_end: true,
            playing: false,
            spin_phase: 0.0,
            spin_sweep: 0.0,
            speedup: 60.0,
            imported: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            committed: Design::simple("strat").expect("preset exists"),
            selected: None,
            prev: None,
            menu_section: None,
        };
        if let Some(p) = path {
            app.open_path(p);
        }
        app
    }

    fn dirty(&self) -> bool {
        self.design != self.saved
    }

    /// Let settings that follow other settings catch up with an edit.
    fn follow(&mut self) {
        if let Some(prev) = &self.prev
            && *prev != self.design
        {
            let changed = auto::follow(prev, &mut self.design);
            if !changed.is_empty() {
                let what: Vec<String> = changed
                    .iter()
                    .map(|f| format!("section {} {}", f.section + 1, f.what))
                    .collect();
                self.status = format!("Kept the lay: {}", what.join("; "));
            }
        }
        self.prev = Some(self.design.clone());
    }

    fn recompute(&mut self, settled: bool) {
        if self
            .computed
            .as_ref()
            .is_some_and(|(d, _)| *d == self.design)
        {
            return;
        }
        // Hitting a target DCR lays the coil a few times, so it waits until
        // a drag is let go.
        if settled && self.design.auto.target_dcr_kohm.is_some() && self.design.validate().is_ok() {
            let mut d = self.design.clone();
            match auto::solve(&mut d) {
                Ok(Some(s)) if d.sections[s.section].turns != s.turns_before => {
                    self.status = format!(
                        "Target {:.2} kΩ: section {} set to {} turns (was {}).",
                        d.auto.target_dcr_kohm.unwrap_or_default(),
                        s.section + 1,
                        d.sections[s.section].turns,
                        s.turns_before
                    );
                    self.design = d;
                    self.prev = Some(self.design.clone());
                }
                Ok(_) => {}
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        match Computed::new(&self.design) {
            Ok(c) => {
                self.error = None;
                if let Some(r) = &self.renderer {
                    let g = view3d::Geometry::build(&self.design, &c);
                    r.lock().expect("renderer lock").set(g);
                }
                self.computed = Some((self.design.clone(), c));
            }
            // Keep showing the last good design while this one is invalid.
            Err(e) => self.error = Some(e),
        }
    }

    fn total_turns(&self) -> f64 {
        self.computed.as_ref().map_or(0.0, |(_, c)| c.stats.turns)
    }

    /// Add a section of `kind` after the others.
    fn create(&mut self, kind: Kind) {
        let s = editor::new_section(&self.design, kind);
        self.design.sections.push(s);
        self.selected = Some(self.design.sections.len() - 1);
        self.follow_end = true;
    }

    /// Take a step for undo once an edit has settled (a drag let go).
    fn commit(&mut self, settled: bool) {
        if settled && self.design != self.committed {
            let before = std::mem::replace(&mut self.committed, self.design.clone());
            self.undo.push(before);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.design, d.clone()));
            self.committed = d;
            self.status = "Undone.".into();
        }
    }

    fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.design, d.clone()));
            self.committed = d;
            self.status = "Redone.".into();
        }
    }

    fn duplicate(&mut self, i: usize) {
        if let Some(s) = self.design.sections.get(i).cloned() {
            self.design.sections.insert(i + 1, s);
            self.selected = Some(i + 1);
        }
    }

    fn delete(&mut self, i: usize) {
        if self.design.sections.len() > 1 && i < self.design.sections.len() {
            self.design.sections.remove(i);
            self.selected = None;
        } else {
            self.status = "A design needs at least one section.".into();
        }
    }

    fn move_section(&mut self, i: usize, later: bool) {
        let n = self.design.sections.len();
        let j = if later { i + 1 } else { i.wrapping_sub(1) };
        if i < n && j < n {
            self.design.sections.swap(i, j);
            self.selected = Some(j);
        }
    }

    // ---------- files ----------

    fn new_from(&mut self, preset: &str) {
        if let Some(d) = Design::simple(preset) {
            self.design = d.clone();
            self.saved = d;
            self.path = None;
            self.prev = None;
            self.follow_end = true;
            self.status = format!("New design from the {preset} preset.");
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Coil design", &["toml"])
            .pick_file()
        {
            self.open_path(p);
        }
    }

    fn open_path(&mut self, p: PathBuf) {
        match model::load_design(&p) {
            Ok(d) => {
                self.design = d.clone();
                self.saved = d;
                self.prev = None;
                self.status = format!("Opened {}", p.display());
                self.path = Some(p);
                self.follow_end = true;
            }
            Err(e) => self.status = e,
        }
    }

    fn save(&mut self, choose: bool) {
        let path = match (&self.path, choose) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("Coil design", &["toml"])
                .set_file_name(format!("{}.toml", file_stem(&self.design.name)))
                .save_file(),
        };
        let Some(p) = path else { return };
        match model::save_design(&self.design, &p) {
            Ok(()) => {
                self.saved = self.design.clone();
                self.status = format!("Saved {}", p.display());
                self.path = Some(p);
            }
            Err(e) => self.status = e,
        }
    }

    fn import_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Python app recipes", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| recipe::import_recipes(&t).map_err(|e| e.to_string()))
        {
            Ok(list) => self.imported = list,
            Err(e) => self.status = format!("{}: {e}", p.display()),
        }
    }

    fn export_dialog(&mut self) {
        if self.error.is_some() {
            self.status = "Fix the design first: G-code is only made from a valid design.".into();
            return;
        }
        let Some(p) = rfd::FileDialog::new()
            .add_filter("G-code", &["nc", "gcode"])
            .set_file_name(format!("{}.nc", file_stem(&self.design.name)))
            .save_file()
        else {
            return;
        };
        self.status = model::export_gcode(&self.design, &self.profile, &p).unwrap_or_else(|e| e);
    }

    fn machine_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Machine profile", &["toml"])
            .pick_file()
        {
            match model::load_profile(&p) {
                Ok(m) => {
                    self.profile = m;
                    self.status = format!("Machine profile {}", p.display());
                    self.profile_path = Some(p);
                }
                Err(e) => self.status = e,
            }
        }
    }

    // ---------- top ----------

    fn app_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.menu_button("New from preset", |ui| {
                    for p in Bobbin::preset_names() {
                        if ui.button(p).clicked() {
                            self.new_from(p);
                        }
                    }
                });
                if ui.button("Open…").clicked() {
                    self.open_dialog();
                }
                if ui.button("Save").clicked() {
                    self.save(false);
                }
                if ui.button("Save as…").clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.button("Import Python recipes…").clicked() {
                    self.import_dialog();
                }
                if ui.button("Export G-code…").clicked() {
                    self.export_dialog();
                }
                ui.separator();
                if ui.button("Load machine profile…").clicked() {
                    self.machine_dialog();
                }
            });
            if ui.button("Save").on_hover_text("Save (Ctrl+S)").clicked() {
                self.save(false);
            }
            ui.add_space(12.0);
            // The document tab.
            let name = self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
                || format!("{} (unsaved)", self.design.name),
                |n| n.to_string_lossy().into_owned(),
            );
            let text = format!("{name}{}", if self.dirty() { "  •" } else { "" });
            let galley = ui
                .painter()
                .layout_no_wrap(text, FontId::proportional(12.5), chrome::INK);
            let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + 28.0, 24.0), Sense::hover());
            ui.painter().rect(
                r,
                egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                },
                Color32::WHITE,
                Stroke::new(1.0, Color32::from_gray(205)),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0.0, BLUE);
            ui.painter().galley(
                pos2(r.left() + 14.0, r.center().y - galley.size().y / 2.0),
                galley,
                chrome::INK,
            );
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            chrome::group(ui, "EDIT", |ui| {
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .on_hover_text("Ctrl+Z")
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .on_hover_text("Ctrl+Y or Ctrl+Shift+Z")
                        .clicked()
                    {
                        self.redo();
                    }
                });
            });
            ui.separator();
            chrome::group(ui, "VIEW", |ui| {
                for (icon, label, view, tip) in [
                    (
                        Icon::Coil3d,
                        "3D",
                        View::Coil3d,
                        "The coil in 3D, every turn",
                    ),
                    (
                        Icon::Section,
                        "Section",
                        View::CrossSection,
                        "Cut through the coil",
                    ),
                    (
                        Icon::Lay,
                        "Lay",
                        View::Lay,
                        "Where the guide puts the wire, turn by turn",
                    ),
                    (
                        Icon::Build,
                        "Build",
                        View::Build,
                        "How deep the coil builds across the window",
                    ),
                ] {
                    if chrome::tool_button(ui, icon, label, self.view == view)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.view = view;
                    }
                }
            });
            ui.separator();
            chrome::group(ui, "MAKE", |ui| {
                if chrome::tool_button(ui, Icon::Export, "G-code", false)
                    .on_hover_text("Bake the program, read it back to check it, then save it")
                    .clicked()
                {
                    self.export_dialog();
                }
                if chrome::tool_button(ui, Icon::Machine, "Machine", false)
                    .on_hover_text(format!(
                        "{}\nClick to load another machine profile",
                        self.profile.name
                    ))
                    .clicked()
                {
                    self.machine_dialog();
                }
            });
        });
    }

    // ---------- side panels ----------

    /// Every parameter, grouped, changing the coil as it is edited.
    fn parameters(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Name").color(chrome::MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.design.name)
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(4.0);
                let total = self.design.total_turns();
                let pattern = match self.design.sections.as_slice() {
                    [one] => title_case(one.pattern.label()),
                    many => format!("{} sections", many.len()),
                };
                card(
                    ui,
                    "Winding",
                    &format!("{pattern} · {total} turns"),
                    true,
                    |ui| {
                        editor::winding(ui, &mut self.design);
                        if self.design.sections.len() > 1 {
                            ui.add_space(4.0);
                            self.sections_list(ui);
                        }
                    },
                );
                let b = &self.design.bobbin;
                let size = format!(
                    "{} · {:.1} × {:.1} mm",
                    b.style.label(),
                    b.core_length_mm,
                    b.core_width_mm
                );
                card(ui, "Bobbin", &size, true, |ui| {
                    editor::bobbin(ui, &mut self.design.bobbin)
                });
                let wire = self.design.wire_spec().map(|w| w.name).unwrap_or_default();
                card(ui, "Wire", &wire, true, |ui| {
                    editor::wire(ui, &mut self.design)
                });
                let rpm = format!("{:.0} RPM", self.design.speed.rpm);
                card(ui, "Speed", &rpm, false, |ui| {
                    editor::speed(ui, &mut self.design)
                });
                let rules = if self.design.auto.keep_lay {
                    "keep the lay"
                } else {
                    "off"
                };
                card(ui, "Rules", rules, false, |ui| {
                    editor::auto(ui, &mut self.design)
                });
                let span = format!("{:.0} mm span", self.design.lay.free_span_mm);
                card(ui, "Wire path", &span, false, |ui| {
                    editor::wire_path(ui, &mut self.design)
                });
                card(ui, "Estimate inputs", "", false, |ui| {
                    editor::estimate(ui, &mut self.design)
                });
                card(ui, "Notes", "", false, |ui| {
                    editor::document(ui, &mut self.design)
                });
                if let Some(err) = &self.error {
                    ui.add_space(4.0);
                    ui.colored_label(WARN, err);
                }
                ui.add_space(8.0);
            });
    }

    /// The sections of a winding wound in parts, each opening its settings.
    fn sections_list(&mut self, ui: &mut Ui) {
        let mut action = None;
        let fill = self
            .design
            .auto
            .target_dcr_kohm
            .map(|_| auto::fill_index(&self.design));
        for i in 0..self.design.sections.len() {
            let s = &self.design.sections[i];
            let title = format!("{}  {}", i + 1, title_case(s.pattern.label()));
            let detail = if fill == Some(i) {
                format!("{} turns · auto", s.turns)
            } else {
                format!("{} turns", s.turns)
            };
            let open = self.selected == Some(i);
            let id = ui.make_persistent_id(("section-card", i));
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                open,
            );
            if open {
                state.set_open(true);
            }
            let header = state.show_header(ui, |ui| {
                ui.label(RichText::new(title).strong());
                ui.label(RichText::new(detail).color(chrome::MUTED).size(11.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Delete").clicked() {
                        action = Some((i, 4));
                    }
                    if ui.small_button("⧉").on_hover_text("Duplicate").clicked() {
                        action = Some((i, 1));
                    }
                    if ui.small_button("▼").on_hover_text("Wind later").clicked() {
                        action = Some((i, 3));
                    }
                    if ui.small_button("▲").on_hover_text("Wind earlier").clicked() {
                        action = Some((i, 2));
                    }
                });
            });
            header.body(|ui| {
                editor::section_form(ui, &mut self.design, i);
            });
            if open {
                self.selected = None;
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Add").color(chrome::MUTED));
            for (label, kind) in [
                ("Layer", Kind::Layer),
                ("Helix", Kind::Helix),
                ("Scatter", Kind::Scatter),
            ] {
                if ui.small_button(label).clicked() {
                    self.create(kind);
                }
            }
        });
        ui.add_space(4.0);
        if let Some((i, a)) = action {
            self.section_action(i, a);
        }
    }

    fn section_action(&mut self, i: usize, a: u8) {
        match a {
            0 => self.selected = Some(i),
            1 => self.duplicate(i),
            2 => self.move_section(i, false),
            3 => self.move_section(i, true),
            _ => self.delete(i),
        }
    }

    fn results(&mut self, ui: &mut Ui) {
        let Some((_, c)) = &self.computed else {
            ui.label("No valid design yet.");
            return;
        };
        let d = &self.design;
        let e = &c.estimate;
        let temp = d.estimate.temp_c;
        egui::Grid::new("results")
            .num_columns(2)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                let mut kv = |k: &str, v: String| {
                    ui.label(RichText::new(k).color(chrome::MUTED));
                    ui.label(RichText::new(v).strong());
                    ui.end_row();
                };
                let l = &c.laid;
                kv("DCR", format!("{:.2} kΩ", l.dcr_kohm));
                kv(
                    "Wire",
                    format!("{:.0} m  ({:.0} ft)", l.length_m, l.length_m * 3.281),
                );
                kv(
                    "Build, long side",
                    format!(
                        "{:.2} of {:.1} mm",
                        l.side_build_mm, d.bobbin.build_depth_mm
                    ),
                );
                kv("Build, short end", format!("{:.2} mm", l.end_build_mm));
                kv("Fill factor", format!("{:.0} %", l.fill_factor * 100.0));
                kv("Inductance", format!("{:.2} H", l.inductance_h));
                kv("Self-capacitance", format!("{:.0} pF", l.cap_pf));
                kv("Resonant peak", format!("{:.1} kHz", l.resonance_khz));
                kv("Q", format!("{:.1}", l.q_factor));
                kv("DCR, layer model", format!("{:.2} kΩ", e.dcr_kohm));
                kv("Passes", format!("{}", c.stats.passes));
                kv("Run time", format!("{:.1} min", c.stats.minutes));
            });
        if c.laid.build_mm > d.bobbin.build_depth_mm {
            ui.colored_label(WARN, "The coil is deeper than the bobbin allows.");
        }
        if e.break_warning() {
            ui.colored_label(
                WARN,
                format!("Tension is {:.0}% of breaking force.", e.break_risk * 100.0),
            );
        }
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!(
                "From where every turn lands. DCR at {temp:.0} °C; L, C and the peak are \
                 estimates until you calibrate with measured coils."
            ))
            .size(11.0)
            .color(chrome::MUTED),
        );
    }

    // ---------- canvas ----------

    fn canvas(&mut self, ui: &mut Ui) {
        let Some((shown, c)) = &self.computed else {
            return;
        };
        let upto = self.scrub.round() as usize;
        let station = if self.long_side {
            c.cross.side_station()
        } else {
            c.cross.end_station()
        };
        if self.view == View::Coil3d {
            view3d::coil_3d(
                ui,
                shown,
                c,
                self.scrub,
                self.spin_phase as f32,
                self.spin_sweep as f32,
                &mut self.camera,
                self.renderer.as_ref(),
            );
            return;
        }
        // The flat views get a white sheet with their options along the top.
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
        let inner = rect.shrink2(vec2(16.0, 8.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(views_title(self.view)).strong());
                if matches!(self.view, View::CrossSection | View::Build) {
                    ui.separator();
                    ui.label("Cut through");
                    ui.selectable_value(&mut self.long_side, true, "long side")
                        .on_hover_text("Where the wire spans straight: the coil bulges here");
                    ui.selectable_value(&mut self.long_side, false, "short end")
                        .on_hover_text("Where tension presses the wire into its grooves");
                }
                if self.view == View::CrossSection {
                    ui.separator();
                    ui.label("Color by");
                    ui.selectable_value(&mut self.coloring, Coloring::Order, "turn order");
                    ui.selectable_value(&mut self.coloring, Coloring::Section, "section");
                }
            });
            ui.add_space(4.0);
            let body = ui.available_rect_before_wrap();
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                match self.view {
                    View::CrossSection => {
                        views::cross_section(ui, shown, c, station, upto, self.coloring)
                    }
                    View::Lay => views::lay_plot(ui, shown, c, upto),
                    View::Build => views::build_plot(ui, shown, c, station, upto),
                    View::Coil3d => {}
                }
            });
        });
    }

    // ---------- timeline ----------

    fn timeline(&mut self, ui: &mut Ui) {
        let total = self.total_turns();
        let bounds: Vec<f64> = self
            .computed
            .as_ref()
            .map(|(_, c)| c.section_bounds.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if chrome::glyph_button(ui, Glyph::ToStart)
                .on_hover_text("Back to the first turn")
                .clicked()
            {
                self.scrub = 0.0;
                self.playing = false;
                self.follow_end = false;
            }
            if chrome::glyph_button(ui, Glyph::Back)
                .on_hover_text("Previous section")
                .clicked()
            {
                let now = self.scrub - 0.5;
                self.scrub = bounds
                    .iter()
                    .rev()
                    .copied()
                    .find(|&b| b < now)
                    .unwrap_or(0.0);
                self.playing = false;
                self.follow_end = false;
            }
            let g = if self.playing {
                Glyph::Pause
            } else {
                Glyph::Play
            };
            if chrome::glyph_button(ui, g)
                .on_hover_text(if self.playing { "Pause" } else { "Wind it" })
                .clicked()
            {
                if !self.playing && self.scrub >= total {
                    self.scrub = 0.0;
                }
                self.playing = !self.playing;
                self.follow_end = false;
            }
            if chrome::glyph_button(ui, Glyph::Forward)
                .on_hover_text("Next section")
                .clicked()
            {
                let now = self.scrub + 0.5;
                self.scrub = bounds.iter().copied().find(|&b| b > now).unwrap_or(total);
                self.playing = false;
                self.follow_end = self.scrub >= total;
            }
            if chrome::glyph_button(ui, Glyph::ToEnd)
                .on_hover_text("The finished coil")
                .clicked()
            {
                self.playing = false;
                self.follow_end = true;
            }
            ui.add_space(6.0);
            egui::ComboBox::from_id_salt("speed")
                .selected_text(format!("{}×", self.speedup))
                .width(62.0)
                .show_ui(ui, |ui| {
                    for s in [0.1, 0.25, 1.0, 10.0, 60.0, 300.0, 1000.0] {
                        ui.selectable_value(&mut self.speedup, s, format!("{s}×"));
                    }
                })
                .response
                .on_hover_text(format!(
                    "Playback speed against the real wind: the spindle turns at \
                     {:.0} RPM on screen ({:.1} turns a second). Above about 20 \
                     turns a second it is drawn blurred, as a camera would see it.",
                    self.design.speed.rpm * self.speedup,
                    self.design.speed.rpm / 60.0 * self.speedup
                ));
            ui.add_space(8.0);
            let readout = 150.0;
            let w = (ui.available_width() - readout).max(120.0);
            self.feature_strip(ui, w, total);
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{:.0} / {:.0} turns", self.scrub.round(), total))
                    .monospace()
                    .color(chrome::INK),
            );
        });
    }

    /// Sections as features in a row, sized by their turns, with the marker
    /// at the turn shown. Drag to scrub; click a section to select it,
    /// double-click to edit it, right-click for more.
    fn feature_strip(&mut self, ui: &mut Ui, width: f32, total: f64) {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click_and_drag());
        let Some((shown, _)) = &self.computed else {
            return;
        };
        let sections = shown.sections.clone();
        let sum: f64 = sections
            .iter()
            .map(|s| s.turns as f64)
            .sum::<f64>()
            .max(1.0);
        let x_of = |t: f64| rect.left() + (t / sum) as f32 * rect.width();
        let t_of = |x: f32| ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * sum;
        let p = ui.painter_at(rect.expand(6.0));
        p.rect_filled(rect, 3.0, Color32::from_gray(232));

        let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
        let mut start = 0.0;
        let mut blocks = Vec::new();
        for s in &sections {
            let end = start + s.turns as f64;
            blocks.push((x_of(start), x_of(end)));
            start = end;
        }
        let under = |x: f32| blocks.iter().position(|&(a, b)| x >= a && x < b);
        let hovered = pointer.and_then(|p| under(p.x));
        let done_x = x_of(self.scrub.min(sum));
        for (i, (s, &(a, b))) in sections.iter().zip(&blocks).enumerate() {
            let r =
                Rect::from_x_y_ranges(a + 1.0..=(b - 1.0).max(a + 2.0), rect.y_range().shrink(3.0));
            let col = pattern_color(&s.pattern);
            // Turns not wound yet at the marker are rolled back, as features
            // after Fusion's marker are.
            p.rect_filled(r, 2.0, col.gamma_multiply(0.3));
            if done_x > r.left() {
                let wound = Rect::from_min_max(r.min, pos2(done_x.min(r.right()), r.max.y));
                p.rect_filled(wound, 2.0, col);
            }
            let stroke = if self.selected == Some(i) {
                Stroke::new(2.0, BLUE)
            } else if hovered == Some(i) {
                Stroke::new(1.0, Color32::from_rgb(110, 180, 225))
            } else {
                Stroke::new(1.0, Color32::from_gray(170))
            };
            p.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            if r.width() > 36.0 {
                let icon =
                    Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(16.0, 13.0));
                chrome::draw_icon(&p, pattern_icon(&s.pattern), icon);
            }
            let label = format!("{} {}", title_case(s.pattern.label()), s.turns);
            let galley = p.layout_no_wrap(label, FontId::proportional(11.0), chrome::INK);
            if galley.size().x + 34.0 < r.width() {
                p.galley(
                    pos2(r.left() + 26.0, r.center().y - galley.size().y / 2.0),
                    galley,
                    chrome::INK,
                );
            }
        }
        // The marker.
        let mx = done_x.clamp(rect.left(), rect.right());
        p.line_segment(
            [pos2(mx, rect.top() - 3.0), pos2(mx, rect.bottom() + 3.0)],
            Stroke::new(2.0, Color32::from_gray(60)),
        );
        p.add(egui::Shape::convex_polygon(
            vec![
                pos2(mx - 5.0, rect.top() - 5.0),
                pos2(mx + 5.0, rect.top() - 5.0),
                pos2(mx, rect.top() + 1.0),
            ],
            Color32::from_gray(60),
            Stroke::NONE,
        ));

        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let t = t_of(p.x).round().min(total);
            self.scrub = t;
            self.playing = false;
            self.follow_end = t >= total;
        }
        if resp.clicked() {
            self.selected = pointer.and_then(|p| under(p.x));
        }
        if resp.double_clicked()
            && let Some(i) = pointer.and_then(|p| under(p.x))
        {
            self.selected = Some(i);
        }
        if resp.secondary_clicked() {
            self.menu_section = pointer.and_then(|p| under(p.x));
        }
        let resp = match hovered {
            Some(i) => resp.on_hover_text_at_pointer(section_summary(i, &sections[i])),
            None => resp,
        };
        let mut action = None;
        let menu_section = self.menu_section;
        resp.context_menu(|ui| {
            if let Some(i) = menu_section {
                ui.label(RichText::new(format!("Section {}", i + 1)).strong());
                if let Some(a) = section_menu(ui) {
                    action = Some((i, a));
                }
            } else {
                ui.label("Right-click a section");
            }
        });
        if let Some((i, a)) = action {
            self.section_action(i, a);
        }
    }

    fn advance_playback(&mut self, ui: &Ui) {
        let total = self.total_turns();
        if self.follow_end {
            self.scrub = total;
        }
        if !self.playing {
            // Standing still: the spindle is where the scrubbed turn has it.
            self.spin_phase = if self.scrub >= total {
                0.0
            } else {
                self.scrub.fract()
            };
            self.spin_sweep = 0.0;
            return;
        }
        let rpm = self.design.speed.rpm;
        let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
        let turns_per_s = rpm / 60.0 * self.speedup;
        let step = (turns_per_s * dt).min(total - self.scrub).max(0.0);
        self.scrub += step;
        // The spindle is always where the turn count has it. Faster than
        // the screen can show, the 3D view blurs it over the angle it swept.
        self.spin_phase = self.scrub.fract();
        self.spin_sweep = step * std::f64::consts::TAU;
        if self.scrub >= total {
            self.playing = false;
            self.follow_end = true;
        }
        ui.ctx().request_repaint();
    }
}

/// Section menu shared by the browser and the timeline: returns 0 edit,
/// 1 duplicate, 2 earlier, 3 later, 4 delete.
fn section_menu(ui: &mut Ui) -> Option<u8> {
    let mut a = None;
    for (k, label) in ["Edit…", "Duplicate", "Move earlier", "Move later", "Delete"]
        .iter()
        .enumerate()
    {
        if k == 4 {
            ui.separator();
        }
        if ui.button(*label).clicked() {
            a = Some(k as u8);
            ui.close();
        }
    }
    a
}

fn pattern_icon(p: &Pattern) -> Icon {
    match p {
        Pattern::Layer { .. } => Icon::Layer,
        Pattern::Helix { .. } => Icon::Helix,
        Pattern::Scatter { .. } => Icon::Scatter,
    }
}

fn pattern_color(p: &Pattern) -> Color32 {
    match p {
        Pattern::Layer { .. } => Color32::from_rgb(160, 205, 236),
        Pattern::Helix { .. } => Color32::from_rgb(240, 196, 150),
        Pattern::Scatter { .. } => Color32::from_rgb(178, 218, 164),
    }
}

fn section_summary(i: usize, s: &winder_core::Section) -> String {
    let how = match &s.pattern {
        Pattern::Layer { .. } => "perfect layers".to_string(),
        Pattern::Helix { tpl } => format!("{tpl:.0} turns per pass"),
        Pattern::Scatter {
            tpl_min, tpl_max, ..
        } => {
            format!("{tpl_min:.0}–{tpl_max:.0} turns per pass")
        }
    };
    format!(
        "{}. {} · {} turns · {how}\nClick to open it in the panel; right-click for more",
        i + 1,
        title_case(s.pattern.label()),
        s.turns
    )
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn views_title(v: View) -> &'static str {
    match v {
        View::Coil3d => "3D",
        View::CrossSection => "Cross-section",
        View::Lay => "Lay plot",
        View::Build => "Build profile",
    }
}

impl eframe::App for WinderApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save(false);
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing {
            let shift_cmd = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
            if ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
                    || i.consume_key(shift_cmd, egui::Key::Z)
            }) {
                self.redo();
            } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.undo();
            }
        }
        let title = format!(
            "Pickup Winder · {}{}",
            self.design.name,
            if self.dirty() { " •" } else { "" }
        );
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(title));

        let bar = |fill: Color32, margin: (i8, i8)| {
            egui::Frame::NONE
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(margin.0, margin.1))
        };
        egui::Panel::top("appbar")
            .frame(bar(Color32::from_rgb(226, 229, 233), (8, 3)))
            .show(ui, |ui| self.app_bar(ui));
        egui::Panel::top("toolbar")
            .frame(bar(Color32::WHITE, (8, 4)).stroke(Stroke::new(1.0, Color32::from_gray(214))))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(bar(chrome::PANEL, (10, 3)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(e) = &self.error {
                        ui.colored_label(WARN, format!("Design problem: {e}"));
                    } else {
                        ui.label(RichText::new(&self.status).size(11.5).color(chrome::MUTED));
                    }
                });
            });
        egui::Panel::bottom("timeline")
            .frame(
                bar(Color32::from_rgb(250, 250, 251), (8, 6))
                    .stroke(Stroke::new(1.0, Color32::from_gray(214))),
            )
            .show(ui, |ui| self.timeline(ui));
        egui::Panel::left("parameters")
            .frame(bar(chrome::PANEL, (10, 4)).stroke(Stroke::new(1.0, Color32::from_gray(214))))
            .resizable(true)
            .default_size(340.0)
            .min_size(280.0)
            .show(ui, |ui| self.parameters(ui));
        egui::Panel::right("results")
            .frame(bar(chrome::PANEL, (10, 8)).stroke(Stroke::new(1.0, Color32::from_gray(214))))
            .resizable(false)
            .exact_size(250.0)
            .show(ui, |ui| {
                ui.label(RichText::new("THE COIL").size(11.5).color(chrome::MUTED));
                ui.add_space(4.0);
                self.results(ui);
            });
        self.follow();
        let settled = !ui.input(|i| i.pointer.any_down());
        self.recompute(settled);
        self.commit(settled);
        if !settled {
            ui.ctx().request_repaint();
        }
        self.advance_playback(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.canvas(ui));

        let ctx = ui.ctx().clone();

        if !self.imported.is_empty() {
            let mut pick = None;
            let mut open = true;
            egui::Window::new("Imported recipes")
                .open(&mut open)
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.label("Pick one to load as a new, unsaved design.");
                    for (i, r) in self.imported.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.button(&r.design.name).clicked() {
                                pick = Some(i);
                            }
                            ui.weak(format!("{} turns", r.design.total_turns()));
                        });
                        for n in &r.notes {
                            ui.weak(format!("   {n}"));
                        }
                    }
                });
            if let Some(i) = pick {
                let r = self.imported.swap_remove(i);
                self.design = r.design;
                self.path = None;
                self.prev = None;
                self.follow_end = true;
                self.status = format!("Imported \"{}\". Save it to keep it.", self.design.name);
                self.imported.clear();
            } else if !open {
                self.imported.clear();
            }
        }
    }
}

fn file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() { "coil".into() } else { s }
}

/// A titled group of parameters that folds away, with a summary beside the
/// title so it says what it holds while folded.
fn card(ui: &mut Ui, title: &str, summary: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, Color32::from_gray(222)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let id = ui.make_persistent_id(("card", title));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open)
                .show_header(ui, |ui| {
                    ui.label(RichText::new(title).strong().size(13.5));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(summary).color(chrome::MUTED).size(11.5));
                    });
                })
                .body(|ui| {
                    ui.add_space(2.0);
                    add(ui);
                });
        });
    ui.add_space(6.0);
}
