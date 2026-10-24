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
    /// Flange and pole sizes. Anything left out takes the kind's usual size.
    #[serde(default, skip_serializing_if = "Detail::is_default")]
    pub detail: Detail,
}

/// The parts of a bobbin that don't change how the wire lays, only how it
/// looks: flanges (flatwork) and poles. `None` takes the kind's usual size.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flange_length_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flange_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flange_corner_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_thickness_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_thickness_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pole_count: Option<u32>,
    /// Centre to centre, first pole to last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pole_spread_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pole_diameter_mm: Option<f64>,
    /// How far the poles stand above the top flange.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pole_height_mm: Option<f64>,
}

impl Detail {
    pub fn is_default(&self) -> bool {
        *self == Detail::default()
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// How a bobbin is built, which decides how it is drawn: fibre flatwork
/// held apart by the pole magnets, or a moulded plastic bobbin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    /// Plain flanges around a solid core.
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

    pub const ALL: [Style; 6] = [
        Style::Strat,
        Style::TeleBridge,
        Style::TeleNeck,
        Style::Humbucker,
        Style::P90,
        Style::Generic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Style::Generic => "Plain bobbin",
            Style::Strat => "Strat flatwork",
            Style::TeleBridge => "Tele bridge flatwork",
            Style::TeleNeck => "Tele neck flatwork",
            Style::Humbucker => "Humbucker bobbin",
            Style::P90 => "P-90 bobbin",
        }
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
                corner_radius_mm: p.corner,
                style: p.style,
                detail: Detail::default(),
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
    /// Round-ended (half the width) where the wire wraps rod magnets; a
    /// moulded core is a rectangle with slightly eased corners.
    corner: f64,
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
/// - A PAF bobbin is 2.625" by 0.6875" by 0.375" (66.7 by 17.5 by 9.5
///   mm) round a rectangular core of 2.1875" by 0.25" (55.6 by 6.35 mm),
///   0.25" (6.35 mm) between the flanges, with 0.219" (5.55 mm) to build
///   (Music Electronics Forum, "Mojotone butyrate humbucker bobbins").
/// - A P-90 bobbin is 3.19" by 1.2" by 0.375" (81 by 30.5 by 9.5 mm) over
///   its ears. Its core is narrow, just round the pole screws (about 56 by
///   6.4 mm), and the coil winds wide and flat out to the 30.5 mm edge:
///   about 11.5 mm to build and 7 mm between the flanges. 10,000 turns of
///   42 then come to about 8.5 kΩ, as vintage P-90s measure.
const PRESETS: &[Preset] = &[
    Preset {
        name: "strat",
        style: Style::Strat,
        channel: 11.0,
        depth: 6.0,
        width: 4.9,
        length: 56.9,
        corner: 2.45,
    },
    Preset {
        name: "tele-bridge",
        style: Style::TeleBridge,
        channel: 11.5,
        depth: 7.0,
        width: 4.75,
        length: 59.75,
        corner: 2.375,
    },
    Preset {
        name: "tele-neck",
        style: Style::TeleNeck,
        channel: 12.0,
        depth: 4.5,
        width: 4.75,
        length: 54.75,
        corner: 2.375,
    },
    Preset {
        name: "paf",
        style: Style::Humbucker,
        channel: 6.35,
        depth: 5.55,
        width: 6.35,
        length: 55.6,
        corner: 0.8,
    },
    Preset {
        name: "p90",
        style: Style::P90,
        channel: 7.0,
        depth: 11.5,
        width: 6.35,
        length: 55.9,
        corner: 0.8,
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
