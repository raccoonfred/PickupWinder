//! The design's edit forms: bobbin, wire, speed, wire path, estimate inputs
//! and each section. Each is the body of one browser node's edit dialog.
//! They edit the design in place; the app notices the change and
//! recomputes, so the coil updates while you type.

use crate::{formula, machine};
use eframe::egui::{self, DragValue, Ui};
use winder_core::{Bobbin, Design, Direction, Pattern, Section, Style, WireSpec, wire::WireRef};

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
            "Spindle speed",
            num(&mut d.speed.rpm)
                .range(10.0..=2000.0)
                .speed(5.0)
                .suffix(" RPM"),
        )
        .on_hover_text("How fast the bobbin turns while winding");
        row(
            ui,
            "Speed up over",
            num(&mut d.speed.ramp_turns)
                .range(0.0..=2000.0)
                .speed(1.0)
                .suffix(" turns"),
        )
        .on_hover_text(
            "The spindle starts slowly and reaches full speed after this many \
             turns (and slows down over the same at the end), so the wire doesn't snap",
        );
        row(
            ui,
            "Starting speed",
            num(&mut d.speed.ramp_start_rpm)
                .range(10.0..=2000.0)
                .speed(1.0)
                .suffix(" RPM"),
        )
        .on_hover_text("Speed for the first and last turns");
    });
}

pub fn wire_path(ui: &mut Ui, d: &mut Design) {
    grid(ui, "lay", |ui| {
        row(
            ui,
            "Guide to coil",
            mm(&mut d.lay.free_span_mm, 500.0).speed(0.5),
        )
        .on_hover_text(
            "How far the wire runs in the air, from the guide's eyelet to \
             where it touches the coil. Measure it on your winder.",
        );
    });
    ui.label(
        egui::RichText::new(
            "The wire doesn't land exactly where the guide is: it trails \
             behind, more the further the guide is from the coil. So when the \
             guide turns around at a flange, the wire turns around a little \
             later and rounder. 20 to 40 mm is usual; the 3D view draws the \
             guide this far away.",
        )
        .size(11.0)
        .weak(),
    );
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
                num(t)
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
    ui.weak("These only tune the estimates on the right; they don't change how the coil is wound.");
    grid(ui, "estimate", |ui| {
        let e = &mut d.estimate;
        let mut use_tension = e.tension_g.is_some();
        ui.label("Wire tension");
        ui.horizontal(|ui| {
            if ui.checkbox(&mut use_tension, "").changed() {
                e.tension_g = use_tension.then_some(35.0);
            }
            if let Some(t) = &mut e.tension_g {
                ui.add(num(t).range(1.0..=120.0).suffix(" g"));
            } else {
                ui.weak("off: set how tight below");
            }
        });
        ui.end_row();
        if e.tension_g.is_none() {
            row(
                ui,
                "How tight it packs",
                num(&mut e.pack).range(0.5..=1.0).speed(0.005),
            )
            .on_hover_text("Share of the coil that is wire rather than air: 0.85 to 0.95");
        }
        row(
            ui,
            "How scattered",
            num(&mut e.scatter).range(0.0..=100.0).suffix(" %"),
        )
        .on_hover_text("0 % neat layers to 100 % fully random, for the capacitance estimate");
        row(
            ui,
            "Room temperature",
            num(&mut e.temp_c).range(-10.0..=80.0).suffix(" °C"),
        )
        .on_hover_text("Copper's resistance rises with heat: DCR is given at this temperature");
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
    ui.add_space(4.0);
    heading(ui, "Winding window");
    grid(ui, "bobbin", |ui| {
        ui.label("Kind");
        egui::ComboBox::from_id_salt("bobbin-style")
            .selected_text(b.style.label())
            .show_ui(ui, |ui| {
                for s in Style::ALL {
                    ui.selectable_value(&mut b.style, s, s.label());
                }
            })
            .response
            .on_hover_text("How it is built: fibre flatwork on rod magnets, or moulded");
        ui.end_row();
        row(ui, "Winding width", mm(&mut b.channel_height_mm, 40.0))
            .on_hover_text("Space between the two flanges, where the wire goes");
        row(ui, "Core width", mm(&mut b.core_width_mm, 60.0)).on_hover_text(
            "The narrow side of what the wire wraps around (the magnets on a single coil)",
        );
        row(ui, "Core length", mm(&mut b.core_length_mm, 120.0))
            .on_hover_text("The long side of what the wire wraps around");
        ui.label("Corner radius");
        ui.horizontal(|ui| {
            let half = b.core_width_mm.min(b.core_length_mm) / 2.0;
            ui.add(mm(&mut b.corner_radius_mm, half));
            if ui
                .small_button("round ends")
                .on_hover_text("Fully round ends, as wound on rod magnets")
                .clicked()
            {
                b.corner_radius_mm = half;
            }
        });
        ui.end_row();
        row(ui, "Max coil thickness", mm(&mut b.build_depth_mm, 40.0))
            .on_hover_text("How thick the coil can get before it reaches the flange edge");
    });

    // Looks only: the lay doesn't depend on these. Blank fields follow the
    // kind and the core.
    let u = machine::usual(b, b.build_depth_mm as f32);
    let t = &mut b.detail;
    ui.add_space(4.0);
    heading(ui, "Flanges");
    grid(ui, "bobbin-flanges", |ui| {
        over(
            ui,
            "Length",
            &mut t.flange_length_mm,
            u.flange_length,
            150.0,
        );
        over(ui, "Width", &mut t.flange_width_mm, u.flange_width, 80.0);
        over(
            ui,
            "Corner radius",
            &mut t.flange_corner_mm,
            u.flange_corner,
            40.0,
        );
        over(
            ui,
            "Top thickness",
            &mut t.top_thickness_mm,
            u.top_thickness,
            10.0,
        );
        over(
            ui,
            "Bottom thickness",
            &mut t.bottom_thickness_mm,
            u.bottom_thickness,
            10.0,
        );
    });
    ui.add_space(4.0);
    heading(ui, "Poles");
    grid(ui, "bobbin-poles", |ui| {
        ui.label("Count");
        ui.horizontal(|ui| {
            let mut n = t.pole_count.unwrap_or(u.pole_count);
            if ui.add(num(&mut n).range(0..=12)).changed() {
                t.pole_count = Some(n);
            }
            if t.pole_count.is_none() {
                ui.weak("auto");
            }
            reset(ui, &mut t.pole_count);
        });
        ui.end_row();
        over(ui, "Spread", &mut t.pole_spread_mm, u.pole_spread, 120.0);
        over(
            ui,
            "Diameter",
            &mut t.pole_diameter_mm,
            u.pole_diameter,
            20.0,
        );
        over(
            ui,
            "Height above top",
            &mut t.pole_height_mm,
            u.pole_height,
            20.0,
        );
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !b.detail.is_default(),
                egui::Button::new("Reset to kind defaults"),
            )
            .on_hover_text("Flanges and poles follow the kind and the core again")
            .clicked()
        {
            b.detail = Default::default();
        }
    });
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().strong().weak());
}

/// A size that follows the kind until it is set. Shows the value in use;
/// dragging or typing sets it, the arrow puts it back.
fn over(ui: &mut Ui, label: &str, v: &mut Option<f64>, usual: f32, max: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        let mut x = v.unwrap_or(usual as f64);
        let r = ui.add(mm(&mut x, max));
        if r.changed() {
            *v = Some(x);
        }
        if v.is_none() {
            ui.weak("auto");
        }
        reset(ui, v);
    });
    ui.end_row();
}

fn reset<T>(ui: &mut Ui, v: &mut Option<T>) {
    if v.is_some()
        && ui
            .small_button("↺")
            .on_hover_text("Back to the kind's size")
            .clicked()
    {
        *v = None;
    }
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
                "Diameter",
                num(&mut w.od_mm)
                    .range(0.02..=1.0)
                    .speed(0.0005)
                    .suffix(" mm"),
            );
            row(
                ui,
                "Copper only",
                num(&mut w.bare_mm)
                    .range(0.02..=1.0)
                    .speed(0.0005)
                    .suffix(" mm"),
            );
            row(
                ui,
                "Resistance",
                num(&mut w.ohm_per_m)
                    .range(0.01..=50.0)
                    .speed(0.01)
                    .suffix(" Ω/m"),
            );
        }
        ui.label("Direction");
        ui.horizontal(|ui| {
            ui.radio_value(&mut d.direction, Direction::Cw, "Clockwise");
            ui.radio_value(&mut d.direction, Direction::Ccw, "Counter-clockwise");
        })
        .response
        .on_hover_text(
            "Which way the bobbin turns, seen from the top. Winding the other way \
             swaps the coil's start and finish, as for a reverse-wound middle pickup \
             (RWRP). The 3D view turns the other way and mirrors the coil.",
        );
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
    section(ui, i, s, window, od, true);
}

/// The winding as one choice: how the guide lays the wire, and how much to
/// wind. A design built from several sections shows them as a list instead.
pub fn winding(ui: &mut Ui, d: &mut Design) {
    let od = d.wire_spec().map_or(0.071, |w| w.od_mm);
    let channel = d.bobbin.channel_height_mm;
    let mut target = d.auto.target_dcr_kohm;
    let total = d.total_turns();
    grid(ui, "winding-amount", |ui| {
        ui.label("Wind to");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(target.is_none(), "Turns")
                .on_hover_text("Wind a set number of turns")
                .clicked()
            {
                target = None;
            }
            if ui
                .selectable_label(target.is_some(), "Resistance")
                .on_hover_text(
                    "Wind until the coil reaches a DCR: the turns are worked out \
                     on the laid coil and follow every change",
                )
                .clicked()
                && target.is_none()
            {
                target = Some(6.0);
            }
        });
        ui.end_row();
        match &mut target {
            Some(t) => {
                row(
                    ui,
                    "DCR",
                    num(t)
                        .range(0.1..=50.0)
                        .speed(0.01)
                        .fixed_decimals(2)
                        .suffix(" kΩ"),
                );
                ui.label("Turns");
                ui.weak(format!("{total} (worked out)"));
                ui.end_row();
            }
            None if d.sections.len() == 1 => {
                row(
                    ui,
                    "Turns",
                    num(&mut d.sections[0].turns).range(1..=100_000).speed(10.0),
                );
            }
            None => {
                ui.label("Turns");
                ui.weak(format!("{total} in all"));
                ui.end_row();
            }
        }
    });
    d.auto.target_dcr_kohm = target;
    ui.add_space(6.0);

    if d.sections.len() == 1 {
        let s = &mut d.sections[0];
        let window = channel - s.inset_start_mm - s.inset_end_mm - od;
        section(ui, 0, s, window, od, false);
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Advanced: wind in sections")
            .id_salt("winding-advanced")
            .show(ui, |ui| {
                ui.label(
                    "Some winders lay a few perfect layers under and over the \
                     scatter, to seat the wire on the bobbin and to finish it \
                     neatly. Each part is then a section, wound one after another.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Add a layered base").clicked() {
                        add_layers(d, true);
                    }
                    if ui.button("Add a layered cover").clicked() {
                        add_layers(d, false);
                    }
                });
            });
    } else {
        ui.label(format!(
            "Wound in {} sections, one after another.",
            d.sections.len()
        ));
        ui.add_space(4.0);
        if ui
            .button("Make it one winding")
            .on_hover_text("Keep the biggest section's pattern for all the turns")
            .clicked()
        {
            let i = winder_core::auto::fill_index(d);
            let mut one = d.sections[i].clone();
            one.turns = total;
            d.sections = vec![one];
            d.auto.fill_section = None;
        }
    }
}

/// Put 200 turns of perfect layers under (or over) the winding.
fn add_layers(d: &mut Design, base: bool) {
    let main = winder_core::auto::fill_index(d);
    let take = 200.min(d.sections[main].turns.saturating_sub(1));
    if take == 0 {
        return;
    }
    d.sections[main].turns -= take;
    if base {
        d.sections.insert(0, Section::layer(take));
    } else {
        d.sections.push(Section::layer(take));
    }
}

fn scatter_default(seed: u64, side_by_side: f64) -> Pattern {
    Pattern::Scatter {
        tpl_min: (side_by_side * 0.55).round(),
        tpl_max: (side_by_side * 0.85).round(),
        reversal_jitter_mm: 0.3,
        speed_wander: winder_core::design::SPEED_WANDER,
        seed: 1000 + seed,
    }
}

/// Turns per pass, with the pitch it gives shown alongside.
fn tpl_row(ui: &mut Ui, label: &str, tpl: &mut f64, min: f64, window: f64, od: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(num(tpl).range(min.max(1.0)..=2000.0).speed(0.5))
            .on_hover_text(
                "How many turns the bobbin makes while the guide travels once \
                 from flange to flange. Fewer turns per sweep spread the wire \
                 further apart. Pickups are usually wound at 50 to 200.",
            );
        let pitch = window / tpl.max(1.0) / od;
        let text = format!("{pitch:.1} wires apart");
        if pitch < 1.0 {
            ui.colored_label(egui::Color32::from_rgb(224, 122, 90), text)
                .on_hover_text("Closer than one wire: turns will push each other and pile up");
        } else {
            ui.weak(text);
        }
    });
    ui.end_row();
}

fn section(ui: &mut Ui, i: usize, s: &mut Section, window: f64, od: f64, turns: bool) {
    let side_by_side = (window / od).max(1.0).round();
    grid(ui, ("section-grid", i), |ui| {
        if turns {
            row(
                ui,
                "Turns",
                num(&mut s.turns).range(1..=100_000).speed(10.0),
            );
        }
        ui.label("How it's laid");
        ui.horizontal(|ui| {
            let kind = s.pattern.label();
            if ui
                .selectable_label(kind == "layer", "Neat layers")
                .on_hover_text(
                    "Neat layers: every turn right beside the last, like thread on a spool",
                )
                .clicked()
                && kind != "layer"
            {
                s.pattern = Pattern::Layer {
                    step_fraction: None,
                };
            }
            if ui
                .selectable_label(kind == "helix", "Machine")
                .on_hover_text(
                    "Machine-even: the guide sweeps at a steady speed, every pass the same",
                )
                .clicked()
                && kind != "helix"
            {
                s.pattern = Pattern::Helix {
                    tpl: (side_by_side * 0.7).round(),
                };
            }
            if ui
                .selectable_label(kind == "scatter", "Scatter")
                .on_hover_text("Like winding by hand: every pass a little different")
                .clicked()
                && kind != "scatter"
            {
                s.pattern = scatter_default(i as u64, side_by_side);
            }
        });
        ui.end_row();
        ui.label("");
        let text = egui::RichText::new(match s.pattern {
            Pattern::Layer { .. } => {
                "Each turn right beside the last, row on row: tidy, \
                     lowest DCR for the turns, highest capacitance."
            }
            Pattern::Helix { .. } => {
                "The guide sweeps steadily, so turns cross the row below \
                     at the same angle every pass."
            }
            Pattern::Scatter { .. } => {
                "Every sweep different, like winding by hand: turns cross \
                     at random, for lower capacitance and a brighter tone."
            }
        })
        .size(11.0)
        .weak();
        ui.scope(|ui| {
            ui.set_max_width(190.0);
            ui.add(egui::Label::new(text).wrap());
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
                        ui.add(num(f).range(0.02..=1.0).speed(0.005).suffix(" of a turn"));
                    } else {
                        ui.weak("first short end");
                    }
                });
                ui.end_row();
            }
            Pattern::Helix { tpl } => tpl_row(ui, "Turns per sweep", tpl, 1.0, window, od),
            Pattern::Scatter {
                tpl_min,
                tpl_max,
                reversal_jitter_mm,
                speed_wander,
                seed,
            } => {
                tpl_row(ui, "Turns per sweep from", tpl_min, 1.0, window, od);
                if *tpl_max < *tpl_min {
                    *tpl_max = *tpl_min;
                }
                let lo = *tpl_min;
                tpl_row(ui, "to", tpl_max, lo, window, od);
                row(
                    ui,
                    "Turnaround wobble",
                    num(reversal_jitter_mm)
                        .range(0.0..=3.0)
                        .speed(0.01)
                        .suffix(" mm"),
                )
                .on_hover_text(
                    "The guide turns around up to this far short of the flange, \
                     a different amount each time, so the edges don't build up",
                );
                ui.label("Hand unevenness");
                ui.horizontal(|ui| {
                    let mut pct = *speed_wander * 100.0;
                    if ui
                        .add(
                            num(&mut pct)
                                .range(0.0..=80.0)
                                .speed(0.5)
                                .max_decimals(0)
                                .suffix(" %"),
                        )
                        .on_hover_text(
                            "How unevenly the guide moves within a pass, as a hand does: \
                             turns bunch up in places and spread out in others. \
                             At 0 % each pass is a steady sweep, like a helix.",
                        )
                        .changed()
                    {
                        *speed_wander = pct / 100.0;
                    }
                    if *speed_wander == 0.0 {
                        ui.weak("steady, like a helix");
                    }
                });
                ui.end_row();
                ui.label("Pattern number");
                ui.horizontal(|ui| {
                    ui.add(num(seed));
                    if ui
                        .small_button("New")
                        .on_hover_text(
                            "Another random scatter. The same number always gives \
                             the same coil, so a good one can be wound again",
                        )
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
        ui.label("Own speed");
        ui.horizontal(|ui| {
            if ui.checkbox(&mut own_rpm, "").changed() {
                s.rpm = own_rpm.then_some(420.0);
            }
            if let Some(r) = &mut s.rpm {
                ui.add(num(r).range(10.0..=2000.0).speed(5.0));
            }
        });
        ui.end_row();
        row(
            ui,
            "Gap at bottom flange",
            mm(&mut s.inset_start_mm, 10.0).speed(0.01),
        )
        .on_hover_text("Keep the wire this far from the bottom flange");
        row(
            ui,
            "Gap at top flange",
            mm(&mut s.inset_end_mm, 10.0).speed(0.01),
        )
        .on_hover_text("Keep the wire this far from the top flange");
        if !matches!(s.pattern, Pattern::Layer { .. }) {
            row(
                ui,
                "Linger at each end",
                num(&mut s.dwell_turns)
                    .range(0.0..=20.0)
                    .speed(0.1)
                    .suffix(" turns"),
            );
        }
        let mut stop = s.stop_after.is_some();
        ui.label("Stop when done");
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
    num(v)
        .range(0.0..=max)
        .speed(0.05)
        .max_decimals(2)
        .suffix(" mm")
}

/// A number field that also takes a formula (see `formula`).
fn num<Num: egui::emath::Numeric>(v: &mut Num) -> DragValue<'_> {
    DragValue::new(v).custom_parser(formula::parse)
}
