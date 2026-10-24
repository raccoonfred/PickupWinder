//! Bobbin geometry.

use serde::{Deserialize, Serialize};

/// The three measurable numbers that define a coil's shape, plus how deep it
/// may build. A single-coil and a humbucker are different shapes, not sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bobbin {
    /// Distance between the flanges, along the traverse (mm). The guide sweeps
    /// this; it sets turns per layer. Strat about 11, PAF about 6.5.
    pub channel_height_mm: f64,
    /// Inner footprint the wire wraps around (mm), short side.
    pub core_width_mm: f64,
    /// Inner footprint the wire wraps around (mm), long side.
    pub core_length_mm: f64,
    /// Radial depth available before the bobbin is full (mm).
    pub build_depth_mm: f64,
}

impl Bobbin {
    /// Perimeter of the bare core (mm).
    pub fn core_perimeter_mm(&self) -> f64 {
        2.0 * self.core_width_mm + 2.0 * self.core_length_mm
    }

    /// Built-in shapes, validated against published pickups.
    pub fn preset(name: &str) -> Option<Bobbin> {
        PRESETS
            .iter()
            .find(|(n, ..)| n.eq_ignore_ascii_case(name.trim()))
            .map(|&(_, h, d, w, l)| Bobbin {
                channel_height_mm: h,
                core_width_mm: w,
                core_length_mm: l,
                build_depth_mm: d,
            })
    }

    pub fn preset_names() -> impl Iterator<Item = &'static str> {
        PRESETS.iter().map(|(n, ..)| *n)
    }
}

/// (name, channel height, build depth, core width, core length), all mm.
const PRESETS: &[(&str, f64, f64, f64, f64)] = &[
    ("strat", 11.0, 6.0, 16.0, 46.0),
    ("tele-bridge", 11.0, 6.0, 16.0, 48.0),
    ("tele-neck", 10.0, 6.0, 14.0, 42.0),
    ("paf", 6.5, 11.0, 16.0, 52.0),
    ("p90", 8.0, 13.0, 18.0, 62.0),
];
