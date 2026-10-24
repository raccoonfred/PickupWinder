//! What the post-processor needs to know about the machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MachineProfile {
    pub name: String,
    /// Motor steps per spindle revolution (200 × microstep). Set GRBL `$100`
    /// to this so that X1 is one turn.
    pub spindle_steps_per_turn: f64,
    /// Motor steps per mm of traverse (200 × microstep / lead-screw lead).
    /// GRBL `$101`.
    pub traverse_steps_per_mm: f64,
    /// Fastest spindle speed allowed in a program.
    pub max_rpm: f64,
    /// Guide position, relative to the homing reference, at which the wire
    /// touches the inner face of the first flange (mm).
    pub flange_offset_mm: f64,
    pub homing: Homing,
}

impl Default for MachineProfile {
    /// The documented build: Uno + CNC Shield v3, A4988 at 1/8 step, NEMA 17,
    /// T8 lead screw with 8 mm lead, GRBL 1.1.
    fn default() -> Self {
        MachineProfile {
            name: "Uno + CNC Shield v3, GRBL 1.1".into(),
            spindle_steps_per_turn: 1600.0,
            traverse_steps_per_mm: 200.0,
            max_rpm: 1000.0,
            flange_offset_mm: 0.0,
            homing: Homing::Probe {
                toward_mm: -20.0,
                feed_mm_min: 120.0,
            },
        }
    }
}

impl MachineProfile {
    /// GRBL steps up to about 30 kHz on an Uno.
    pub fn grbl_max_rpm(&self) -> f64 {
        30_000.0 * 60.0 / self.spindle_steps_per_turn
    }

    /// GRBL settings this profile expects, as lines to send once.
    pub fn grbl_settings(&self) -> Vec<String> {
        vec![
            format!("$100={}", self.spindle_steps_per_turn),
            format!("$101={}", self.traverse_steps_per_mm),
            // Max rates (units/min): turns/min for X, mm/min for Y.
            format!("$110={}", self.max_rpm),
            "$111=600".into(),
            // Accelerations (units/s²) and junction deviation. GRBL's default
            // of 10 would slow the spindle to about 100 RPM at every step of
            // a perfect layer, where the guide starts and stops each turn;
            // these keep the junction speed above the winding speed.
            "$120=200".into(),
            "$121=200".into(),
            "$11=0.02".into(),
            // Keep drivers energized after moves so a feed hold keeps tension.
            "$1=255".into(),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Homing {
    /// No switch: the guide must already sit at the reference when the
    /// program starts.
    None,
    /// Move the traverse toward a switch on GRBL's probe input until it
    /// trips (`G38.2`). Works with stock GRBL, no recompile.
    Probe { toward_mm: f64, feed_mm_min: f64 },
    /// GRBL's own homing cycle (`$H`), for a build configured to home Y only.
    Cycle,
}
