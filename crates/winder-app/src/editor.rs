//! The design form: bobbin, wire, speed, estimate inputs and the section
//! stack. Edits the design in place; the app notices the change and
//! recomputes.

use eframe::egui::{self, DragValue, RichText, Ui};
use winder_core::{Bobbin, Design, Direction, Pattern, Section, WireSpec, wire::WireRef};

pub fn design_form(ui: &mut Ui, d: &mut Design) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut d.name);
    });
    ui.label("Notes");
    ui.add(
        egui::TextEdit::multiline(&mut d.notes)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Bobbin").strong())
        .default_open(true)
        .show(ui, |ui| bobbin(ui, &mut d.bobbin));
    egui::CollapsingHeader::new(RichText::new("Wire and direction").strong())
        .default_open(true)
        .show(ui, |ui| wire(ui, d));
    egui::CollapsingHeader::new(RichText::new("Speed").strong())
        .default_open(true)
        .show(ui, |ui| {
            grid(ui, "speed", |ui| {
                row(
                    ui,
                    "Spindle RPM",
                    DragValue::new(&mut d.speed.rpm)
                        .range(10.0..=2000.0)
                        .speed(5.0),
                );
                row(
                    ui,
                    "Ramp (turns)",
                    DragValue::new(&mut d.speed.ramp_turns)
                        .range(0.0..=2000.0)
                        .speed(1.0),
                );
                row(
                    ui,
                    "Ramp start RPM",
                    DragValue::new(&mut d.speed.ramp_start_rpm)
                        .range(10.0..=2000.0)
                        .speed(1.0),
                );
            });
        });
    egui::CollapsingHeader::new(RichText::new("Estimate inputs").strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.weak("Used by the tone estimate; the toolpath doesn't set these.");
            grid(ui, "estimate", |ui| {
                let e = &mut d.estimate;
                let mut use_tension = e.tension_g.is_some();
                ui.label("Wire tension");
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut use_tension, "").changed() {
                        e.tension_g = use_tension.then_some(35.0);
                    }
                    if let Some(t) = &mut e.tension_g {
                        ui.add(DragValue::new(t).range(1.0..=120.0).suffix(" g"));
                    } else {
                        ui.weak("off: manual pack");
                    }
                });
                ui.end_row();
                if e.tension_g.is_none() {
                    row(
                        ui,
                        "Pack factor",
                        DragValue::new(&mut e.pack).range(0.5..=1.0).speed(0.005),
                    );
                }
                row(
                    ui,
                    "Scatter (0-100)",
                    DragValue::new(&mut e.scatter).range(0.0..=100.0),
                );
                row(
                    ui,
                    "Measure temp",
                    DragValue::new(&mut e.temp_c)
                        .range(-10.0..=80.0)
                        .suffix(" °C"),
                );
            });
        });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading("Sections");
        ui.weak(format!("{} turns", d.total_turns()));
    });
    sections(ui, d);
}

fn bobbin(ui: &mut Ui, b: &mut Bobbin) {
    ui.horizontal(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt("bobbin-preset")
            .selected_text("apply…")
            .show_ui(ui, |ui| {
                for name in Bobbin::preset_names() {
                    if ui.selectable_label(false, name).clicked() {
                        *b = Bobbin::preset(name).expect("listed preset exists");
                    }
                }
            });
    });
    grid(ui, "bobbin", |ui| {
        row(ui, "Channel height", mm(&mut b.channel_height_mm, 40.0))
            .on_hover_text("Flange to flange, along the traverse");
        row(ui, "Core width", mm(&mut b.core_width_mm, 60.0));
        row(ui, "Core length", mm(&mut b.core_length_mm, 120.0));
        row(ui, "Build depth", mm(&mut b.build_depth_mm, 40.0))
            .on_hover_text("How deep the coil may build before the bobbin is full");
    });
}

fn wire(ui: &mut Ui, d: &mut Design) {
    let current = match &d.wire {
        WireRef::Named(n) => n.clone(),
        WireRef::Custom(_) => "Custom".into(),
    };
    grid(ui, "wire", |ui| {
        ui.label("Wire");
        egui::ComboBox::from_id_salt("wire")
            .selected_text(&current)
            .show_ui(ui, |ui| {
                for name in WireSpec::builtin_names() {
                    if ui.selectable_label(current == name, name).clicked() {
                        d.wire = WireRef::Named(name.into());
                    }
                }
                if ui.selectable_label(current == "Custom", "Custom").clicked() {
                    let base = d
                        .wire
                        .resolve()
                        .unwrap_or_else(|| WireSpec::named("42 AWG").unwrap());
                    d.wire = WireRef::Custom(WireSpec {
                        name: format!("{} custom", base.name),
                        ..base
                    });
                }
            });
        ui.end_row();
        if let WireRef::Custom(w) = &mut d.wire {
            ui.label("Name");
            ui.text_edit_singleline(&mut w.name);
            ui.end_row();
            row(
                ui,
                "OD",
                DragValue::new(&mut w.od_mm)
                    .range(0.02..=1.0)
                    .speed(0.0005)
                    .suffix(" mm"),
            );
            row(
                ui,
                "Bare",
                DragValue::new(&mut w.bare_mm)
                    .range(0.02..=1.0)
                    .speed(0.0005)
                    .suffix(" mm"),
            );
            row(
                ui,
                "Resistance",
                DragValue::new(&mut w.ohm_per_m)
                    .range(0.01..=50.0)
                    .speed(0.01)
                    .suffix(" Ω/m"),
            );
        }
        ui.label("Direction");
        ui.horizontal(|ui| {
            ui.radio_value(&mut d.direction, Direction::Cw, "CW");
            ui.radio_value(&mut d.direction, Direction::Ccw, "CCW");
        });
        ui.end_row();
    });
}

enum Action {
    Up(usize),
    Down(usize),
    Duplicate(usize),
    Remove(usize),
}

fn sections(ui: &mut Ui, d: &mut Design) {
    let mut action = None;
    let n = d.sections.len();
    for (i, s) in d.sections.iter_mut().enumerate() {
        let title = format!("{}. {} · {} turns", i + 1, s.pattern.label(), s.turns);
        egui::CollapsingHeader::new(RichText::new(title).strong())
            .id_salt(("section", i))
            .default_open(i == 0)
            .show(ui, |ui| {
                section(ui, i, s);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(i > 0, egui::Button::new("Up").small())
                        .clicked()
                    {
                        action = Some(Action::Up(i));
                    }
                    if ui
                        .add_enabled(i + 1 < n, egui::Button::new("Down").small())
                        .clicked()
                    {
                        action = Some(Action::Down(i));
                    }
                    if ui.small_button("Duplicate").clicked() {
                        action = Some(Action::Duplicate(i));
                    }
                    if ui
                        .add_enabled(n > 1, egui::Button::new("Remove").small())
                        .clicked()
                    {
                        action = Some(Action::Remove(i));
                    }
                });
            });
    }
    match action {
        Some(Action::Up(i)) => d.sections.swap(i, i - 1),
        Some(Action::Down(i)) => d.sections.swap(i, i + 1),
        Some(Action::Duplicate(i)) => {
            let s = d.sections[i].clone();
            d.sections.insert(i + 1, s);
        }
        Some(Action::Remove(i)) => {
            d.sections.remove(i);
        }
        None => {}
    }
    ui.horizontal(|ui| {
        if ui.button("+ Layer section").clicked() {
            d.sections.push(Section::layer(200, 1.0));
        }
        if ui.button("+ Scatter section").clicked() {
            d.sections.push(Section {
                pattern: scatter_default(d.sections.len() as u64),
                ..Section::layer(1000, 1.0)
            });
        }
    });
}

fn scatter_default(seed: u64) -> Pattern {
    Pattern::Scatter {
        pitch_min_od: 1.2,
        pitch_max_od: 1.8,
        inset_max_mm: 0.3,
        seed: 1000 + seed,
    }
}

fn section(ui: &mut Ui, i: usize, s: &mut Section) {
    grid(ui, ("section-grid", i), |ui| {
        row(
            ui,
            "Turns",
            DragValue::new(&mut s.turns).range(1..=100_000).speed(10.0),
        );
        ui.label("Pattern");
        ui.horizontal(|ui| {
            let is_layer = matches!(s.pattern, Pattern::Layer { .. });
            if ui.selectable_label(is_layer, "Layer").clicked() && !is_layer {
                s.pattern = Pattern::Layer { pitch_od: 1.0 };
            }
            if ui.selectable_label(!is_layer, "Scatter").clicked() && is_layer {
                s.pattern = scatter_default(i as u64);
            }
        });
        ui.end_row();
        match &mut s.pattern {
            Pattern::Layer { pitch_od } => {
                row(
                    ui,
                    "Pitch",
                    DragValue::new(pitch_od)
                        .range(0.5..=20.0)
                        .speed(0.01)
                        .suffix(" × OD"),
                )
                .on_hover_text(
                    "Traverse per turn, in wire diameters. 1.0 lays each turn beside the last.",
                );
            }
            Pattern::Scatter {
                pitch_min_od,
                pitch_max_od,
                inset_max_mm,
                seed,
            } => {
                row(
                    ui,
                    "Pitch from",
                    DragValue::new(pitch_min_od)
                        .range(0.5..=20.0)
                        .speed(0.01)
                        .suffix(" × OD"),
                );
                if *pitch_max_od < *pitch_min_od {
                    *pitch_max_od = *pitch_min_od;
                }
                let lo = *pitch_min_od;
                row(
                    ui,
                    "Pitch to",
                    DragValue::new(pitch_max_od)
                        .range(lo..=20.0)
                        .speed(0.01)
                        .suffix(" × OD"),
                )
                .on_hover_text("Each pass picks its pitch at random in this range");
                row(
                    ui,
                    "Reversal wander",
                    DragValue::new(inset_max_mm)
                        .range(0.0..=3.0)
                        .speed(0.01)
                        .suffix(" mm"),
                )
                .on_hover_text("Each reversal lands up to this far inside the window edge");
                ui.label("Seed");
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(seed));
                    if ui
                        .small_button("New")
                        .on_hover_text("Pick a new random seed")
                        .clicked()
                    {
                        *seed = seed
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407)
                            >> 16;
                    }
                });
                ui.end_row();
            }
        }
        let mut own_rpm = s.rpm.is_some();
        ui.label("Own RPM");
        ui.horizontal(|ui| {
            if ui.checkbox(&mut own_rpm, "").changed() {
                s.rpm = own_rpm.then_some(420.0);
            }
            if let Some(r) = &mut s.rpm {
                ui.add(DragValue::new(r).range(10.0..=2000.0).speed(5.0));
            }
        });
        ui.end_row();
        row(
            ui,
            "Inset, flange 1",
            mm(&mut s.inset_start_mm, 10.0).speed(0.01),
        );
        row(
            ui,
            "Inset, flange 2",
            mm(&mut s.inset_end_mm, 10.0).speed(0.01),
        );
        row(
            ui,
            "Dwell at reversal",
            DragValue::new(&mut s.dwell_turns)
                .range(0.0..=20.0)
                .speed(0.1)
                .suffix(" turns"),
        );
        let mut stop = s.stop_after.is_some();
        ui.label("Pause after");
        ui.horizontal(|ui| {
            if ui.checkbox(&mut stop, "").changed() {
                s.stop_after = stop.then(|| "Solder tap".to_string());
            }
            if let Some(msg) = &mut s.stop_after {
                ui.text_edit_singleline(msg);
            }
        });
        ui.end_row();
    });
}

fn grid(ui: &mut Ui, id: impl egui::AsIdSalt, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, body);
}

fn row(ui: &mut Ui, label: &str, widget: impl egui::Widget) -> egui::Response {
    ui.label(label);
    let r = ui.add(widget);
    ui.end_row();
    r
}

fn mm(v: &mut f64, max: f64) -> DragValue<'_> {
    DragValue::new(v).range(0.0..=max).speed(0.05).suffix(" mm")
}
