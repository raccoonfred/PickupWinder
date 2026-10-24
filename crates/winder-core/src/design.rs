//! A coil design: bobbin, wire, and an ordered stack of winding sections.
//!
//! Saved as TOML. See `designs/` in the repository for examples.

use crate::{
    bobbin::Bobbin,
    physics::EstimateInput,
    wire::{WireRef, WireSpec},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current design file version.
pub const DESIGN_VERSION: u32 = 1;

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
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dwell_turns: f64,
    /// Pause the machine after this section with this message, e.g. to
    /// solder a tap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_after: Option<String>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// How the guide moves while the spindle turns. Pitches are in wire
/// diameters per turn: 1.0 lays each turn right beside the last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pattern {
    /// Even layers, reversing exactly at the window edges.
    Layer { pitch_od: f64 },
    /// Each pass draws its own pitch and its own reversal points from a
    /// seeded random stream: hand-wound character, but repeatable.
    Scatter {
        pitch_min_od: f64,
        pitch_max_od: f64,
        /// Each reversal lands up to this far inside the window edge (mm).
        #[serde(default)]
        inset_max_mm: f64,
        seed: u64,
    },
}

impl Pattern {
    pub fn label(&self) -> &'static str {
        match self {
            Pattern::Layer { .. } => "layer",
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
    pub fn from_toml(text: &str) -> Result<Design, DesignError> {
        let d: Design = toml::from_str(text).map_err(|e| DesignError::Parse(e.to_string()))?;
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
        let sp = &self.speed;
        if !(sp.rpm > 0.0 && sp.ramp_start_rpm > 0.0 && sp.ramp_turns >= 0.0) {
            return bad(
                "speed: rpm and ramp_start_rpm must be positive, ramp_turns not negative".into(),
            );
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
                Pattern::Layer { pitch_od } => {
                    if !(*pitch_od > 0.0) {
                        return bad(format!("section {n}: pitch_od must be positive"));
                    }
                    0.0
                }
                Pattern::Scatter {
                    pitch_min_od,
                    pitch_max_od,
                    inset_max_mm,
                    ..
                } => {
                    if !(*pitch_min_od > 0.0 && pitch_max_od >= pitch_min_od) {
                        return bad(format!(
                            "section {n}: need 0 < pitch_min_od <= pitch_max_od"
                        ));
                    }
                    if *inset_max_mm < 0.0 {
                        return bad(format!("section {n}: inset_max_mm can't be negative"));
                    }
                    2.0 * inset_max_mm
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
        let cover = 200;
        Some(Design {
            version: DESIGN_VERSION,
            name: format!("{preset} example"),
            notes: "Clean base layer, scattered body, clean cover layer.".into(),
            wire: WireRef::Named("42 AWG".into()),
            direction: Direction::Cw,
            bobbin,
            speed: Speed::default(),
            estimate: EstimateInput {
                scatter: 12.0,
                ..Default::default()
            },
            sections: vec![
                Section::layer(cover, 1.0),
                Section {
                    pattern: Pattern::Scatter {
                        pitch_min_od: 1.2,
                        pitch_max_od: 1.8,
                        inset_max_mm: 0.3,
                        seed: 1234,
                    },
                    ..Section::layer(turns - 2 * cover, 1.0)
                },
                Section::layer(cover, 1.0),
            ],
        })
    }
}

impl Section {
    pub fn layer(turns: u32, pitch_od: f64) -> Section {
        Section {
            turns,
            pattern: Pattern::Layer { pitch_od },
            rpm: None,
            inset_start_mm: 0.0,
            inset_end_mm: 0.0,
            dwell_turns: 0.0,
            stop_after: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
version = 1
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
pattern = { kind = "layer", pitch_od = 1.0 }
"#;
        let d = Design::from_toml(text).unwrap();
        assert_eq!(d.wire_spec().unwrap().od_mm, 0.074);
        assert_eq!(d.speed, Speed::default());
        assert_eq!(d.direction, Direction::Cw);
    }
}
