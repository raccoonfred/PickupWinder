//! Window layout, file handling and the scrub/play state.

use crate::{
    editor,
    model::{self, Computed},
    views::{self, Coloring, View},
};
use eframe::egui::{self, Color32, RichText, Ui};
use std::path::PathBuf;
use winder_core::{Bobbin, Design, recipe};
use winder_gcode::MachineProfile;

const WARN: Color32 = Color32::from_rgb(224, 122, 90);

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
    /// Cut through the long side (true) or the short end.
    long_side: bool,
    /// Turns shown; follows the end of the coil unless scrubbed or playing.
    scrub: f64,
    follow_end: bool,
    playing: bool,
    /// Playback speed as a multiple of the real winding speed.
    speedup: f64,
    imported: Vec<recipe::Imported>,
}

impl WinderApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> WinderApp {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = WinderApp {
            design: Design::example("strat").expect("preset exists"),
            path: None,
            saved: Design::example("strat").expect("preset exists"),
            profile: MachineProfile::default(),
            profile_path: None,
            computed: None,
            error: None,
            status: "New design from the Strat preset.".into(),
            view: View::CrossSection,
            coloring: Coloring::Order,
            long_side: true,
            scrub: 0.0,
            follow_end: true,
            playing: false,
            speedup: 60.0,
            imported: Vec::new(),
        };
        if let Some(p) = path {
            app.open_path(p);
        }
        app
    }

    fn dirty(&self) -> bool {
        self.design != self.saved
    }

    fn recompute(&mut self) {
        if self
            .computed
            .as_ref()
            .is_some_and(|(d, _)| *d == self.design)
        {
            return;
        }
        match Computed::new(&self.design) {
            Ok(c) => {
                self.error = None;
                self.computed = Some((self.design.clone(), c));
            }
            // Keep showing the last good design while this one is invalid.
            Err(e) => self.error = Some(e),
        }
    }

    fn total_turns(&self) -> f64 {
        self.computed.as_ref().map_or(0.0, |(_, c)| c.stats.turns)
    }

    // ---------- files ----------

    fn new_from(&mut self, preset: &str) {
        if let Some(d) = Design::example(preset) {
            self.design = d.clone();
            self.saved = d;
            self.path = None;
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

    // ---------- panels ----------

    fn menu_bar(&mut self, ui: &mut Ui) {
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
            ui.separator();
            let name = self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
                || "unsaved design".to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            ui.weak(format!("{name}{}", if self.dirty() { " •" } else { "" }));
        });
    }

    fn results_panel(&mut self, ui: &mut Ui) {
        ui.heading("Estimate");
        let Some((_, c)) = &self.computed else {
            ui.label("No valid design yet.");
            return;
        };
        let d = &self.design;
        let e = &c.estimate;
        let temp = d.estimate.temp_c;
        egui::Grid::new("results")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                let mut kv = |k: &str, v: String| {
                    ui.label(k);
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
        ui.weak(format!(
            "Worked out from where every turn lands. DCR at {temp:.0} °C. L, C and the peak \
             are estimates; measure finished coils to calibrate."
        ));

        ui.add_space(12.0);
        ui.heading("Machine");
        ui.label(&self.profile.name);
        ui.weak(format!(
            "{} steps/turn · {} steps/mm · max {:.0} RPM · flange at {:.2} mm",
            self.profile.spindle_steps_per_turn,
            self.profile.traverse_steps_per_mm,
            self.profile.max_rpm,
            self.profile.flange_offset_mm
        ));
        ui.add_space(6.0);
        let ok = self.error.is_none();
        if ui
            .add_enabled(
                ok,
                egui::Button::new(RichText::new("Export G-code…").strong()),
            )
            .on_hover_text("Bakes the program, reads it back to check it, then saves it")
            .clicked()
        {
            self.export_dialog();
        }
    }

    fn view_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::CrossSection, "Cross-section");
            ui.selectable_value(&mut self.view, View::Lay, "Lay plot");
            ui.selectable_value(&mut self.view, View::Build, "Build profile");
            if self.view != View::Lay {
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
        let total = self.total_turns();
        ui.horizontal(|ui| {
            let label = if self.playing { "Pause" } else { "Play" };
            if ui.button(label).clicked() {
                if !self.playing && self.scrub >= total {
                    self.scrub = 0.0;
                }
                self.playing = !self.playing;
                self.follow_end = false;
            }
            if ui
                .button("Start")
                .on_hover_text("Back to the first turn")
                .clicked()
            {
                self.scrub = 0.0;
                self.playing = false;
                self.follow_end = false;
            }
            if ui
                .button("End")
                .on_hover_text("The finished coil")
                .clicked()
            {
                self.playing = false;
                self.follow_end = true;
            }
            egui::ComboBox::from_id_salt("speed")
                .selected_text(format!("{}×", self.speedup))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for s in [1.0, 10.0, 60.0, 300.0, 1000.0] {
                        ui.selectable_value(&mut self.speedup, s, format!("{s}×"));
                    }
                });
            ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(100.0);
            // Playback keeps a fractional position; the slider shows whole
            // turns, so only a real drag or edit counts as scrubbing.
            let shown = self.scrub.round();
            let mut v = shown;
            ui.add(
                egui::Slider::new(&mut v, 0.0..=total.max(1.0))
                    .step_by(1.0)
                    .text("turns"),
            );
            if v != shown {
                self.scrub = v;
                self.playing = false;
                self.follow_end = v >= total;
            }
        });
    }

    fn advance_playback(&mut self, ui: &Ui) {
        let total = self.total_turns();
        if self.follow_end {
            self.scrub = total;
        }
        if !self.playing {
            return;
        }
        let rpm = self.design.speed.rpm;
        let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
        self.scrub = (self.scrub + rpm / 60.0 * self.speedup * dt).min(total);
        if self.scrub >= total {
            self.playing = false;
            self.follow_end = true;
        }
        ui.ctx().request_repaint();
    }
}

impl eframe::App for WinderApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save(false);
        }
        let title = format!(
            "Pickup Winder · {}{}",
            self.design.name,
            if self.dirty() { " •" } else { "" }
        );
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(WARN, format!("Design problem: {e}"));
                } else {
                    ui.weak(&self.status);
                }
            });
        });
        egui::Panel::left("design")
            .resizable(true)
            .default_size(370.0)
            .min_size(300.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .show(ui, |ui| editor::design_form(ui, &mut self.design));
            });
        self.recompute();
        egui::Panel::right("results")
            .resizable(true)
            .default_size(290.0)
            .min_size(240.0)
            .show(ui, |ui| self.results_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            self.view_toolbar(ui);
            self.advance_playback(ui);
            ui.separator();
            if let Some((shown, c)) = &self.computed {
                let upto = self.scrub.round() as usize;
                let station = if self.long_side {
                    c.cross.side_station()
                } else {
                    c.cross.end_station()
                };
                match self.view {
                    View::CrossSection => {
                        views::cross_section(ui, shown, c, station, upto, self.coloring)
                    }
                    View::Lay => views::lay_plot(ui, shown, c, upto),
                    View::Build => views::build_plot(ui, shown, c, station, upto),
                }
            }
        });

        if !self.imported.is_empty() {
            let mut pick = None;
            let mut open = true;
            egui::Window::new("Imported recipes")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
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
