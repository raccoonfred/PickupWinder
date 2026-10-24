//! Bobbin geometry.

use serde::{Deserialize, Serialize};

/// The measurable numbers that define a coil's shape, plus how deep it may
/// build. A single-coil and a humbucker are different shapes, not sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bobbin {
    /// Distance between the flanges, along the traverse (mm). The guide sweeps
    /// this; it sets turns per layer. Strat about 11, PAF about 6.3.
    pub channel_height_mm: f64,
    /// Inner footprint the wire wraps around (mm), short side. On a Strat or
    /// Tele that is the row of pole magnets: one magnet across.
    pub core_width_mm: f64,
    /// Inner footprint the wire wraps around (mm), long side.
    pub core_length_mm: f64,
    /// Radial depth available before the bobbin is full (mm).
    pub build_depth_mm: f64,
    /// Radius of the core's corners (mm). A row of rod magnets or a moulded
    /// humbucker core has fully round ends: half the width. 0 is square.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub corner_radius_mm: f64,
    /// What kind of bobbin this is, for drawing it.
    #[serde(default, skip_serializing_if = "Style::is_generic")]
    pub style: Style,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// How a bobbin is built, which decides how it is drawn: fibre flatwork
/// held apart by the pole magnets, or a moulded plastic bobbin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    /// Plain flanges around the core.
    #[default]
    Generic,
    /// Fibre flatwork, six rod magnets wound directly.
    Strat,
    TeleBridge,
    TeleNeck,
    /// Moulded humbucker bobbin (one coil), six pole screws.
    Humbucker,
    /// Moulded P-90 bobbin with mounting ears.
    P90,
}

impl Style {
    pub fn is_generic(&self) -> bool {
        *self == Style::Generic
    }
}

impl Bobbin {
    /// Corner radius, kept within the core.
    pub fn corner_r(&self) -> f64 {
        self.corner_radius_mm
            .clamp(0.0, self.core_width_mm.min(self.core_length_mm) / 2.0)
    }

    /// Perimeter of the bare core (mm).
    pub fn core_perimeter_mm(&self) -> f64 {
        2.0 * self.core_width_mm + 2.0 * self.core_length_mm
            - (8.0 - 2.0 * std::f64::consts::PI) * self.corner_r()
    }

    /// Length of one end, where tension presses the wire down: the short
    /// side and half of each corner beside it (mm).
    pub fn end_mm(&self) -> f64 {
        let r = self.corner_r();
        self.core_width_mm - 2.0 * r + std::f64::consts::PI * r
    }

    /// Straight run of one long side (mm), where the wire spans free.
    pub fn side_mm(&self) -> f64 {
        self.core_length_mm - 2.0 * self.corner_r()
    }

    /// Built-in shapes, measured from real parts (see `PRESETS`).
    pub fn preset(name: &str) -> Option<Bobbin> {
        PRESETS
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
            .map(|p| Bobbin {
                channel_height_mm: p.channel,
                core_width_mm: p.width,
                core_length_mm: p.length,
                build_depth_mm: p.depth,
                corner_radius_mm: p.width / 2.0,
                style: p.style,
            })
    }

    pub fn preset_names() -> impl Iterator<Item = &'static str> {
        PRESETS.iter().map(|p| p.name)
    }
}

struct Preset {
    name: &'static str,
    style: Style,
    channel: f64,
    depth: f64,
    width: f64,
    length: f64,
}

/// Real bobbins, all mm.
///
/// - Single coils wind straight onto six rod magnets, 0.187-0.195" (4.75-
///   4.95 mm) across, so the core is one magnet wide with round ends, and
///   as long as the pole spread plus one magnet: Strat 52 mm spread (vintage
///   flatwork), Tele bridge 55 mm, Tele neck 50 mm. Flatwork is about 17.5 mm
///   wide on a Strat, leaving about 6 mm to build. The '54 Strat measures
///   0.718" overall with 0.064" and 0.097" flatwork, so about 11-12 mm
///   between the plates.
/// - A PAF coil's core is 2.20-2.25" by 0.255" (56-57 by 6.5 mm), 0.248"
///   (6.3 mm) between the flanges, in a 0.69" (17.6 mm) wide bobbin: about
///   5.5 mm to build (Music Electronics Forum, "PAF bobbin dimensions").
/// - A P-90 bobbin is 3.19" by 1.2" by 0.375" over its ears; it winds wide
///   and shallow round a core about 59 by 12.7 mm.
const PRESETS: &[Preset] = &[
    Preset {
        name: "strat",
        style: Style::Strat,
        channel: 11.0,
        depth: 6.0,
        width: 4.9,
        length: 56.9,
    },
    Preset {
        name: "tele-bridge",
        style: Style::TeleBridge,
        channel: 11.5,
        depth: 7.0,
        width: 4.75,
        length: 59.75,
    },
    Preset {
        name: "tele-neck",
        style: Style::TeleNeck,
        channel: 12.0,
        depth: 4.5,
        width: 4.75,
        length: 54.75,
    },
    Preset {
        name: "paf",
        style: Style::Humbucker,
        channel: 6.3,
        depth: 5.5,
        width: 6.5,
        length: 56.5,
    },
    Preset {
        name: "p90",
        style: Style::P90,
        channel: 7.0,
        depth: 7.5,
        width: 12.7,
        length: 59.0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_ended_core_perimeter() {
        let b = Bobbin::preset("strat").unwrap();
        let r = 4.9 / 2.0;
        let stadium = 2.0 * (56.9 - 4.9) + 2.0 * std::f64::consts::PI * r;
        assert!((b.core_perimeter_mm() - stadium).abs() < 1e-9);
        assert!((2.0 * b.end_mm() + 2.0 * b.side_mm() - stadium).abs() < 1e-9);
        // Square corners keep the old rectangle.
        let sq = Bobbin {
            corner_radius_mm: 0.0,
            ..b
        };
        assert!((sq.core_perimeter_mm() - 2.0 * (4.9 + 56.9)).abs() < 1e-9);
    }
}
