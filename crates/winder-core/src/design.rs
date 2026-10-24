//! A coil design: bobbin, wire, and an ordered stack of winding sections.
//!
//! Saved as TOML. See `designs/` in the repository for examples.

use crate::{
    auto::Auto,
    bobbin::Bobbin,
    lay::LaySettings,
    physics::EstimateInput,
    wire::{WireRef, WireSpec},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current design file version.
pub const DESIGN_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// A built-in name such as `"42 AWG"`, or a full wire table.
    pub wire: WireRef,
    #[serde(default)]
    pub direction: Direction,
    pub bobbin: Bobbin,
    #[serde(default)]
    pub speed: Speed,
    /// Inputs to the DCR/tone estimate that the toolpath does not decide.
    #[serde(default)]
    pub estimate: EstimateInput,
    /// How the wire behaves between the guide and the coil.
    #[serde(default)]
    pub lay: LaySettings,
    /// Settings that follow other settings.
    #[serde(default, skip_serializing_if = "Auto::is_default")]
    pub auto: Auto,
    pub sections: Vec<Section>,
}

/// Spindle direction seen from the front of the bobbin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Cw,
    Ccw,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Cw => "cw",
            Direction::Ccw => "ccw",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Speed {
    /// Spindle speed for every section that doesn't set its own.
    pub rpm: f64,
    /// Turns spent ramping up at the start and down at the end.
    pub ramp_turns: f64,
    /// Spindle speed at the very start and end of the ramps.
    pub ramp_start_rpm: f64,
}

impl Default for Speed {
    fn default() -> Self {
        Speed {
            rpm: 420.0,
            ramp_turns: 50.0,
            ramp_start_rpm: 60.0,
        }
    }
}

/// A run of turns laid one way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub turns: u32,
    pub pattern: Pattern,
    /// Overrides `speed.rpm` for this section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm: Option<f64>,
    /// Keep the wire this far from the first flange (mm).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inset_start_mm: f64,
    /// Keep the wire this far from the second flange (mm).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inset_end_mm: f64,
    /// Turns laid in place at each reversal, before the guide heads back.
    /// Not used by the `layer` pattern, which climbs instead.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dwell_turns: f64,
    /// Pause the machine after this section with this message, e.g. to
    /// solder a tap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_after: Option<String>,
}

/// Scatter's usual speed wander: the pitch swings about a third either side
/// of the pass's own, as a hand-guided traverse does.
pub const SPEED_WANDER: f64 = 0.35;

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// How the guide moves while the spindle turns.
///
/// Real winders are set by turns per pass ("turns per layer", TPL): how many
/// turns the spindle makes while the guide crosses from one edge of the
/// window to the other. On a geared or cam machine that is the gear ratio.
/// The pitch follows from it: window / TPL. Pickups are usually wound at
/// 50-200 TPL, which puts the pitch above one wire diameter, so each pass
/// crosses the one below instead of lying in its grooves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pattern {
    /// Perfect layers: turns side by side, each layer in the grooves of the
    /// one below. The guide holds still for most of the turn and steps one
    /// wire diameter in a short step zone, then climbs half a wire at each
    /// flange (orthocyclic winding).
    Layer {
        /// Share of each turn the step takes, starting where the wire runs
        /// onto the first short end. Default: the short end itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_fraction: Option<f64>,
    },
    /// A fixed traverse ratio, like a geared or cam-driven machine: the guide
    /// moves steadily and reverses at the window edges.
    Helix { tpl: f64 },
    /// Hand-guided character: each pass draws its own turns per pass and its
    /// own reversal points from a seeded random stream, so it repeats.
    Scatter {
        tpl_min: f64,
        tpl_max: f64,
        /// Each reversal lands up to this far inside the window edge (mm).
        #[serde(default)]
        reversal_jitter_mm: f64,
        /// How unevenly the guide moves within a pass, as a hand does: the
        /// pitch swings by up to this share either side of the pass's own
        /// (0 = a steady sweep, like a helix).
        #[serde(default, skip_serializing_if = "is_zero")]
        speed_wander: f64,
        seed: u64,
    },
}

impl Pattern {
    pub fn label(&self) -> &'static str {
        match self {
            Pattern::Layer { .. } => "layer",
            Pattern::Helix { .. } => "helix",
            Pattern::Scatter { .. } => "scatter",
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum DesignError {
    #[error("design file version {0} is not supported (expected {DESIGN_VERSION})")]
    Version(u32),
    #[error(
        "unknown wire \"{0}\" (built in: 42 AWG, 43 AWG, 44 AWG; or give od_mm, bare_mm, ohm_per_m)"
    )]
    UnknownWire(String),
    #[error("{0}")]
    Invalid(String),
    #[error("could not read design: {0}")]
    Parse(String),
}

impl Design {
    /// Reads a design. Version 1 files (pitch in wire diameters) are
    /// converted to turns per pass on the way in.
    pub fn from_toml(text: &str) -> Result<Design, DesignError> {
        let parse = |e: toml::de::Error| DesignError::Parse(e.to_string());
        let mut table: toml::Table = toml::from_str(text).map_err(parse)?;
        if table.get("version").and_then(toml::Value::as_integer) == Some(1) {
            migrate_v1(&mut table)?;
        }
        let d: Design = table.try_into().map_err(parse)?;
        d.validate()?;
        Ok(d)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a design always serializes")
    }

    pub fn wire_spec(&self) -> Result<WireSpec, DesignError> {
        self.wire.resolve().ok_or_else(|| match &self.wire {
            WireRef::Named(n) => DesignError::UnknownWire(n.clone()),
            WireRef::Custom(w) => DesignError::UnknownWire(w.name.clone()),
        })
    }

    pub fn total_turns(&self) -> u32 {
        self.sections.iter().map(|s| s.turns).sum()
    }

    /// Checks everything the toolpath generator relies on.
    // `!(x > 0.0)` on purpose: it also rejects NaN.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn validate(&self) -> Result<(), DesignError> {
        if self.version != DESIGN_VERSION {
            return Err(DesignError::Version(self.version));
        }
        let wire = self.wire_spec()?;
        let bad = |m: String| Err(DesignError::Invalid(m));
        if !(wire.od_mm > 0.0 && wire.bare_mm > 0.0 && wire.ohm_per_m > 0.0) {
            return bad(format!(
                "wire {}: od_mm, bare_mm and ohm_per_m must be positive",
                wire.name
            ));
        }
        let b = &self.bobbin;
        for (name, v) in [
            ("channel_height_mm", b.channel_height_mm),
            ("core_width_mm", b.core_width_mm),
            ("core_length_mm", b.core_length_mm),
            ("build_depth_mm", b.build_depth_mm),
        ] {
            if !(v > 0.0 && v.is_finite()) {
                return bad(format!("bobbin.{name} must be a positive number"));
            }
        }
        let dt = &b.detail;
        for (name, v) in [
            ("flange_length_mm", dt.flange_length_mm),
            ("flange_width_mm", dt.flange_width_mm),
            ("top_thickness_mm", dt.top_thickness_mm),
            ("bottom_thickness_mm", dt.bottom_thickness_mm),
            ("pole_diameter_mm", dt.pole_diameter_mm),
        ] {
            if v.is_some_and(|v| !(v > 0.0 && v.is_finite())) {
                return bad(format!("bobbin.detail.{name} must be a positive number"));
            }
        }
        for (name, v) in [
            ("flange_corner_mm", dt.flange_corner_mm),
            ("pole_spread_mm", dt.pole_spread_mm),
            ("pole_height_mm", dt.pole_height_mm),
        ] {
            if v.is_some_and(|v| !(v >= 0.0 && v.is_finite())) {
                return bad(format!("bobbin.detail.{name} can't be negative"));
            }
        }
        if dt.pole_count.is_some_and(|n| n > 12) {
            return bad("bobbin.detail.pole_count: at most 12".into());
        }
        if b.corner_radius_mm < 0.0 {
            return bad("bobbin.corner_radius_mm can't be negative".into());
        }
        let sp = &self.speed;
        if !(sp.rpm > 0.0 && sp.ramp_start_rpm > 0.0 && sp.ramp_turns >= 0.0) {
            return bad(
                "speed: rpm and ramp_start_rpm must be positive, ramp_turns not negative".into(),
            );
        }
        if !(self.lay.free_span_mm >= 0.0 && self.lay.free_span_mm.is_finite()) {
            return bad("lay.free_span_mm can't be negative".into());
        }
        if self.sections.is_empty() {
            return bad("a design needs at least one section".into());
        }
        for (i, s) in self.sections.iter().enumerate() {
            let n = i + 1;
            if s.turns == 0 {
                return bad(format!("section {n}: turns must be at least 1"));
            }
            if s.rpm.is_some_and(|r| !(r > 0.0)) {
                return bad(format!("section {n}: rpm must be positive"));
            }
            if s.inset_start_mm < 0.0 || s.inset_end_mm < 0.0 || s.dwell_turns < 0.0 {
                return bad(format!(
                    "section {n}: insets and dwell_turns can't be negative"
                ));
            }
            let extra = match &s.pattern {
                Pattern::Layer { step_fraction } => {
                    if step_fraction.is_some_and(|f| !(f > 0.0 && f <= 1.0)) {
                        return bad(format!("section {n}: step_fraction must be in (0, 1]"));
                    }
                    0.0
                }
                Pattern::Helix { tpl } => {
                    if !(*tpl >= 1.0 && tpl.is_finite()) {
                        return bad(format!("section {n}: tpl must be at least 1"));
                    }
                    0.0
                }
                Pattern::Scatter {
                    tpl_min,
                    tpl_max,
                    reversal_jitter_mm,
                    speed_wander,
                    ..
                } => {
                    if !(*tpl_min >= 1.0 && tpl_max >= tpl_min && tpl_max.is_finite()) {
                        return bad(format!("section {n}: need 1 <= tpl_min <= tpl_max"));
                    }
                    if !(0.0..=0.8).contains(speed_wander) {
                        return bad(format!("section {n}: speed_wander must be 0 to 0.8"));
                    }
                    if *reversal_jitter_mm < 0.0 {
                        return bad(format!("section {n}: reversal_jitter_mm can't be negative"));
                    }
                    2.0 * reversal_jitter_mm
                }
            };
            // The window must still hold a couple of wires at its narrowest.
            let room = b.channel_height_mm - s.inset_start_mm - s.inset_end_mm - extra - wire.od_mm;
            if room < 2.0 * wire.od_mm {
                return bad(format!(
                    "section {n}: the insets leave no room to lay wire in a {} mm channel",
                    b.channel_height_mm
                ));
            }
        }
        Ok(())
    }

    /// A starting point for a new design on a built-in bobbin.
    pub fn example(preset: &str) -> Option<Design> {
        let bobbin = Bobbin::preset(preset)?;
        let turns = if bobbin.channel_height_mm < 9.0 {
            5000
        } else {
            8000
        };
        // Side-by-side turns per pass for 42 AWG; the scatter body runs at
        // 55-85 % of that, about 85-130 TPL on a Strat.
        let full = bobbin.channel_height_mm / 0.071;
        let cover = 200;
        Some(Design {
            version: DESIGN_VERSION,
            name: format!("{preset} example"),
            notes: "Perfect-layer base, scattered body, perfect-layer cover.".into(),
            wire: WireRef::Named("42 AWG".into()),
            direction: Direction::Cw,
            bobbin,
            speed: Speed::default(),
            estimate: EstimateInput {
                scatter: 12.0,
                ..Default::default()
            },
            lay: LaySettings::default(),
            auto: Auto::default(),
            sections: vec![
                Section::layer(cover),
                Section {
                    pattern: Pattern::Scatter {
                        tpl_min: (full * 0.55).round(),
                        tpl_max: (full * 0.85).round(),
                        reversal_jitter_mm: 0.3,
                        speed_wander: SPEED_WANDER,
                        seed: 1234,
                    },
                    ..Section::layer(turns - 2 * cover)
                },
                Section::layer(cover),
            ],
        })
    }

    /// A complete starting point for a kind of pickup: its real bobbin, the
    /// wire and turns it is usually wound with, as one scatter winding.
    /// Names as `Bobbin::preset_names`.
    pub fn pickup(preset: &str) -> Option<Design> {
        // (wire, turns, name). Vintage counts: Strat ~8k of 42 (5.8-6.3k);
        // Tele bridge ~9.2k of 42 (7.0-7.8k); Tele neck ~8k of 43 (7.3-8.1k);
        // PAF ~5k of 42 per coil (about 4k each, 8k the pair); P-90 ~10k
        // of 42.
        let (wire, turns, name) = match preset {
            "strat" => ("42 AWG", 8000, "Strat single coil"),
            "tele-bridge" => ("42 AWG", 9200, "Tele bridge"),
            "tele-neck" => ("43 AWG", 8000, "Tele neck"),
            "paf" => ("42 AWG", 5000, "Humbucker (PAF), one coil"),
            "p90" => ("42 AWG", 10000, "P-90"),
            _ => return None,
        };
        let mut d = Design::simple(preset)?;
        let full = d.bobbin.channel_height_mm / WireSpec::named(wire)?.od_mm;
        d.name = name.into();
        d.wire = WireRef::Named(wire.into());
        d.sections[0].turns = turns;
        if let Pattern::Scatter {
            tpl_min, tpl_max, ..
        } = &mut d.sections[0].pattern
        {
            *tpl_min = (full * 0.55).round();
            *tpl_max = (full * 0.85).round();
        }
        Some(d)
    }

    /// The plain version of `example`: one scattered winding, the way most
    /// pickups are wound. Sections can be added later for a perfect-layer
    /// base or cover.
    pub fn simple(preset: &str) -> Option<Design> {
        let mut d = Design::example(preset)?;
        let turns = d.total_turns();
        let body = d.sections.remove(1);
        d.sections = vec![Section { turns, ..body }];
        d.notes = "Scatter wound.".into();
        Some(d)
    }

    /// Width the guide sweeps for a section, between wire centres (mm).
    pub fn window_mm(&self, s: &Section, od_mm: f64) -> f64 {
        self.bobbin.channel_height_mm - s.inset_start_mm - s.inset_end_mm - od_mm
    }
}

impl Section {
    /// A perfect-layer section with the default step zone.
    pub fn layer(turns: u32) -> Section {
        Section::with(
            turns,
            Pattern::Layer {
                step_fraction: None,
            },
        )
    }

    pub fn helix(turns: u32, tpl: f64) -> Section {
        Section::with(turns, Pattern::Helix { tpl })
    }

    pub fn with(turns: u32, pattern: Pattern) -> Section {
        Section {
            turns,
            pattern,
            rpm: None,
            inset_start_mm: 0.0,
            inset_end_mm: 0.0,
            dwell_turns: 0.0,
            stop_after: None,
        }
    }
}

/// Version 1 gave pitch in wire diameters per turn. The same guide motion
/// is a turns-per-pass figure over that section's window, so convert it.
fn migrate_v1(table: &mut toml::Table) -> Result<(), DesignError> {
    let parse = |e: toml::de::Error| DesignError::Parse(e.to_string());
    let wire: WireRef = table
        .get("wire")
        .cloned()
        .ok_or_else(|| DesignError::Parse("missing wire".into()))?
        .try_into()
        .map_err(parse)?;
    let od = wire
        .resolve()
        .map(|w| w.od_mm)
        .ok_or_else(|| DesignError::UnknownWire(format!("{wire:?}")))?;
    let channel = table
        .get("bobbin")
        .and_then(|b| b.get("channel_height_mm"))
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        .ok_or_else(|| DesignError::Parse("missing bobbin.channel_height_mm".into()))?;
    let f = |t: &toml::Table, k: &str| {
        t.get(k)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
    };
    if let Some(sections) = table.get_mut("sections").and_then(|v| v.as_array_mut()) {
        for s in sections.iter_mut().filter_map(|s| s.as_table_mut()) {
            let window = channel
                - f(s, "inset_start_mm").unwrap_or(0.0)
                - f(s, "inset_end_mm").unwrap_or(0.0)
                - od;
            let tpl = |pitch_od: f64| (window / (pitch_od * od)).max(1.0);
            let Some(p) = s.get_mut("pattern").and_then(|p| p.as_table_mut()) else {
                continue;
            };
            let mut out = toml::Table::new();
            match p.get("kind").and_then(toml::Value::as_str) {
                Some("layer") => {
                    out.insert("kind".into(), "helix".into());
                    out.insert("tpl".into(), tpl(f(p, "pitch_od").unwrap_or(1.0)).into());
                }
                Some("scatter") => {
                    let lo = f(p, "pitch_min_od").unwrap_or(1.0);
                    let hi = f(p, "pitch_max_od").unwrap_or(lo);
                    out.insert("kind".into(), "scatter".into());
                    out.insert("tpl_min".into(), tpl(hi).into());
                    out.insert("tpl_max".into(), tpl(lo).into());
                    out.insert(
                        "reversal_jitter_mm".into(),
                        f(p, "inset_max_mm").unwrap_or(0.0).into(),
                    );
                    if let Some(seed) = p.get("seed") {
                        out.insert("seed".into(), seed.clone());
                    }
                }
                _ => continue,
            }
            *p = out;
        }
    }
    table.insert("version".into(), (DESIGN_VERSION as i64).into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pickup_preset_is_valid() {
        for p in Bobbin::preset_names() {
            let d = Design::pickup(p).unwrap();
            d.validate().unwrap();
            assert_eq!(d.sections.len(), 1);
        }
        assert_eq!(
            Design::pickup("tele-neck").unwrap().wire,
            WireRef::Named("43 AWG".into())
        );
    }

    #[test]
    fn simple_is_one_winding_of_the_same_turns() {
        for p in Bobbin::preset_names() {
            let (a, b) = (Design::example(p).unwrap(), Design::simple(p).unwrap());
            assert_eq!(b.sections.len(), 1);
            assert_eq!(a.total_turns(), b.total_turns());
            assert!(matches!(b.sections[0].pattern, Pattern::Scatter { .. }));
            b.validate().unwrap();
        }
    }

    #[test]
    fn example_round_trips_through_toml() {
        let d = Design::example("strat").unwrap();
        d.validate().unwrap();
        let text = d.to_toml();
        assert_eq!(Design::from_toml(&text).unwrap(), d);
    }

    #[test]
    fn shipped_designs_load() {
        for text in [
            include_str!("../../../designs/strat-vintage.toml"),
            include_str!("../../../designs/paf-tapped.toml"),
        ] {
            let d = Design::from_toml(text).unwrap();
            crate::toolpath::generate(&d).unwrap();
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut text = Design::example("paf").unwrap().to_toml();
        text = text.replace("[bobbin]", "[bobbin]\nwidth = 3.0");
        assert!(matches!(
            Design::from_toml(&text),
            Err(DesignError::Parse(_))
        ));
    }

    #[test]
    fn rejects_window_too_narrow() {
        let mut d = Design::example("paf").unwrap();
        d.sections[0].inset_start_mm = 3.2;
        d.sections[0].inset_end_mm = 3.2;
        assert!(matches!(d.validate(), Err(DesignError::Invalid(_))));
    }

    #[test]
    fn minimal_design_with_custom_wire() {
        let text = r#"
version = 2
name = "heavy formvar test"

[wire]
name = "42 heavy formvar"
od_mm = 0.074
bare_mm = 0.0635
ohm_per_m = 5.44

[bobbin]
channel_height_mm = 11.0
core_width_mm = 16.0
core_length_mm = 46.0
build_depth_mm = 6.0

[[sections]]
turns = 8000
pattern = { kind = "helix", tpl = 120 }
"#;
        let d = Design::from_toml(text).unwrap();
        assert_eq!(d.wire_spec().unwrap().od_mm, 0.074);
        assert_eq!(d.speed, Speed::default());
        assert_eq!(d.direction, Direction::Cw);
    }

    #[test]
    fn reads_version_1_files_as_turns_per_pass() {
        let text = r#"
version = 1
name = "old"
wire = "42 AWG"

[bobbin]
channel_height_mm = 11.0
core_width_mm = 16.0
core_length_mm = 46.0
build_depth_mm = 6.0

[[sections]]
turns = 200
pattern = { kind = "layer", pitch_od = 1.0 }

[[sections]]
turns = 7600
pattern = { kind = "scatter", pitch_min_od = 1.2, pitch_max_od = 1.8, inset_max_mm = 0.3, seed = 9 }
"#;
        let d = Design::from_toml(text).unwrap();
        assert_eq!(d.version, DESIGN_VERSION);
        let w = 11.0 - 0.071;
        let Pattern::Helix { tpl } = d.sections[0].pattern else {
            panic!("{:?}", d.sections[0].pattern)
        };
        assert!((tpl - w / 0.071).abs() < 1e-9);
        let Pattern::Scatter {
            tpl_min,
            tpl_max,
            reversal_jitter_mm,
            seed,
            ..
        } = d.sections[1].pattern
        else {
            panic!()
        };
        assert!((tpl_min - w / (1.8 * 0.071)).abs() < 1e-9);
        assert!((tpl_max - w / (1.2 * 0.071)).abs() < 1e-9);
        assert_eq!((reversal_jitter_mm, seed), (0.3, 9));
    }
}
