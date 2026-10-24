//! The design's edit forms: bobbin, wire, speed, wire path, estimate inputs
//! and each section. Each is the body of one browser node's edit dialog.
//! They edit the design in place; the app notices the change and
//! recomputes, so the coil updates while you type.

use eframe::egui::{self, DragValue, Ui};
use winder_core::{Bobbin, Design, Direction, Pattern, Section, WireSpec, wire::WireRef};

/// Name and notes.
pub fn document(ui: &mut Ui, d: &mut Design) {
    grid(ui, "document", |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut d.name);
        ui.end_row();
    });
    ui.label("Notes");
    ui.add(
        egui::TextEdit::multiline(&mut d.notes)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
}

pub fn speed(ui: &mut Ui, d: &mut Design) {
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
}

pub fn wire_path(ui: &mut Ui, d: &mut Design) {
    grid(ui, "lay", |ui| {
        row(
            ui,
            "Free span",
            mm(&mut d.lay.free_span_mm, 500.0).speed(0.5),
        )
        .on_hover_text(
            "Free wire from the guide's eye to where it meets the coil. \
             The longer it is, the more the wire lags the guide and the \
             rounder the reversals.",
        );
    });
}

/// Settings that follow other settings: keep the lay, and a target DCR.
pub fn auto(ui: &mut Ui, d: &mut Design) {
    ui.checkbox(
        &mut d.auto.keep_lay,
        "Keep the lay when the wire or bobbin changes",
    )
    .on_hover_text(
        "Change the gauge, the channel or a section's insets and each section \
         keeps its pitch in wire diameters (its turns per pass are scaled), \
         and each perfect-layer section keeps its number of layers.",
    );
    ui.add_space(6.0);
    let mut on = d.auto.target_dcr_kohm.is_some();
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut on, "Target DCR")
            .on_hover_text(
                "Set the resistance you want: the fill section's turns are worked out \
                 on the laid coil to reach it, and redone whenever anything changes.",
            )
            .changed()
        {
            d.auto.target_dcr_kohm = on.then_some(6.0);
        }
        if let Some(t) = &mut d.auto.target_dcr_kohm {
            ui.add(
                DragValue::new(t)
                    .range(0.1..=50.0)
                    .speed(0.01)
                    .fixed_decimals(2)
                    .suffix(" kΩ"),
            );
        }
    });
    if d.auto.target_dcr_kohm.is_some() {
        let fill = match d.auto.fill_section {
            Some(n) if n <= d.sections.len() => format!("section {n}"),
            _ => format!(
                "the biggest section ({})",
                winder_core::auto::fill_index(d) + 1
            ),
        };
        ui.horizontal(|ui| {
            ui.label("Turns go into");
            egui::ComboBox::from_id_salt("fill-section")
                .selected_text(fill)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(d.auto.fill_section.is_none(), "the biggest section")
                        .clicked()
                    {
                        d.auto.fill_section = None;
                    }
                    for (i, s) in d.sections.iter().enumerate() {
                        let label = format!("section {} ({})", i + 1, s.pattern.label());
                        if ui
                            .selectable_label(d.auto.fill_section == Some(i + 1), label)
                            .clicked()
                        {
                            d.auto.fill_section = Some(i + 1);
                        }
                    }
                });
        });
        ui.weak("The other sections keep their turns.");
    }
}

pub fn estimate(ui: &mut Ui, d: &mut Design) {
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
}

pub fn bobbin(ui: &mut Ui, b: &mut Bobbin) {
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

pub fn wire(ui: &mut Ui, d: &mut Design) {
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

/// The kinds of section the CREATE menu makes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Layer,
    Helix,
    Scatter,
}

/// A new section of `kind` sized for this design's window.
pub fn new_section(d: &Design, kind: Kind) -> Section {
    let od = d.wire_spec().map_or(0.071, |w| w.od_mm);
    let side_by_side = ((d.bobbin.channel_height_mm - od) / od).max(1.0).round();
    match kind {
        Kind::Layer => Section::layer(200),
        Kind::Helix => Section::helix(1000, (side_by_side * 0.7).round()),
        Kind::Scatter => {
            Section::with(1000, scatter_default(d.sections.len() as u64, side_by_side))
        }
    }
}

/// Section `i`'s settings.
pub fn section_form(ui: &mut Ui, d: &mut Design, i: usize) {
    let od = d.wire_spec().map_or(0.071, |w| w.od_mm);
    let channel = d.bobbin.channel_height_mm;
    let Some(s) = d.sections.get_mut(i) else {
        return;
    };
    let window = channel - s.inset_start_mm - s.inset_end_mm - od;
    section(ui, i, s, window, od);
}

fn scatter_default(seed: u64, side_by_side: f64) -> Pattern {
    Pattern::Scatter {
        tpl_min: (side_by_side * 0.55).round(),
        tpl_max: (side_by_side * 0.85).round(),
        reversal_jitter_mm: 0.3,
        seed: 1000 + seed,
    }
}

/// Turns per pass, with the pitch it gives shown alongside.
fn tpl_row(ui: &mut Ui, label: &str, tpl: &mut f64, min: f64, window: f64, od: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(DragValue::new(tpl).range(min.max(1.0)..=2000.0).speed(0.5))
            .on_hover_text(
                "Turns the spindle makes while the guide crosses the window once. \
                 Pickups are usually wound at 50-200.",
            );
        let pitch = window / tpl.max(1.0) / od;
        let text = format!("pitch {pitch:.2} × OD");
        if pitch < 1.0 {
            ui.colored_label(egui::Color32::from_rgb(224, 122, 90), text)
                .on_hover_text("Closer than one wire: turns will push and pile up");
        } else {
            ui.weak(text);
        }
    });
    ui.end_row();
}

fn section(ui: &mut Ui, i: usize, s: &mut Section, window: f64, od: f64) {
    let side_by_side = (window / od).max(1.0).round();
    grid(ui, ("section-grid", i), |ui| {
        row(
            ui,
            "Turns",
            DragValue::new(&mut s.turns).range(1..=100_000).speed(10.0),
        );
        ui.label("Pattern");
        ui.horizontal(|ui| {
            let kind = s.pattern.label();
            if ui
                .selectable_label(kind == "layer", "Layer")
                .on_hover_text("Perfect layers: side by side, stepping one wire per turn")
                .clicked()
                && kind != "layer"
            {
                s.pattern = Pattern::Layer {
                    step_fraction: None,
                };
            }
            if ui
                .selectable_label(kind == "helix", "Helix")
                .on_hover_text("A fixed traverse ratio, like a geared or cam machine")
                .clicked()
                && kind != "helix"
            {
                s.pattern = Pattern::Helix {
                    tpl: (side_by_side * 0.7).round(),
                };
            }
            if ui
                .selectable_label(kind == "scatter", "Scatter")
                .on_hover_text("Hand-guided character: every pass different")
                .clicked()
                && kind != "scatter"
            {
                s.pattern = scatter_default(i as u64, side_by_side);
            }
        });
        ui.end_row();
        match &mut s.pattern {
            Pattern::Layer { step_fraction } => {
                ui.label("Turns per layer");
                ui.weak(format!(
                    "{:.0}, then a half-wire climb",
                    (window / od + 0.5).floor()
                ));
                ui.end_row();
                let mut own = step_fraction.is_some();
                ui.label("Step zone");
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut own, "")
                        .on_hover_text(
                            "Off: the guide steps while the wire wraps the first short end",
                        )
                        .changed()
                    {
                        *step_fraction = own.then_some(0.15);
                    }
                    if let Some(f) = step_fraction {
                        ui.add(
                            DragValue::new(f)
                                .range(0.02..=1.0)
                                .speed(0.005)
                                .suffix(" of a turn"),
                        );
                    } else {
                        ui.weak("first short end");
                    }
                });
                ui.end_row();
            }
            Pattern::Helix { tpl } => tpl_row(ui, "Turns per pass", tpl, 1.0, window, od),
            Pattern::Scatter {
                tpl_min,
                tpl_max,
                reversal_jitter_mm,
                seed,
            } => {
                tpl_row(ui, "Turns per pass from", tpl_min, 1.0, window, od);
                if *tpl_max < *tpl_min {
                    *tpl_max = *tpl_min;
                }
                let lo = *tpl_min;
                tpl_row(ui, "to", tpl_max, lo, window, od);
                row(
                    ui,
                    "Reversal wander",
                    DragValue::new(reversal_jitter_mm)
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
        if !matches!(s.pattern, Pattern::Layer { .. }) {
            row(
                ui,
                "Dwell at reversal",
                DragValue::new(&mut s.dwell_turns)
                    .range(0.0..=20.0)
                    .speed(0.1)
                    .suffix(" turns"),
            );
        }
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
