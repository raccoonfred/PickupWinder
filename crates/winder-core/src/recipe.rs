//! Import recipes saved by the Python app (`winder_recipes.json`).
//!
//! The mapping is lossy on purpose, and every loss is reported:
//! - `jitter` was a ±% wobble of the traverse speed on the Uno. It becomes a
//!   Scatter section whose pitch ranges over 1 ± jitter/100 wire diameters,
//!   seeded with `scatter_seed`; it also stays the estimate's scatter amount.
//! - `pass` (firmware turns per sweep) and `lead` (preview-only) have no
//!   equivalent: the pitch now comes from the wire diameter.

use crate::{
    bobbin::Bobbin,
    design::{DESIGN_VERSION, Design, DesignError, Direction, Pattern, Section, Speed},
    physics::EstimateInput,
    wire::WireRef,
};
use serde_json::{Map, Value};

pub struct Imported {
    pub design: Design,
    /// What could not be carried over, in plain words.
    pub notes: Vec<String>,
}

pub fn import_recipes(json: &str) -> Result<Vec<Imported>, DesignError> {
    let root: Value = serde_json::from_str(json).map_err(|e| DesignError::Parse(e.to_string()))?;
    let map = root
        .as_object()
        .ok_or_else(|| DesignError::Parse("expected a JSON object of named recipes".into()))?;
    map.iter().map(|(name, r)| import_one(name, r)).collect()
}

fn import_one(name: &str, r: &Value) -> Result<Imported, DesignError> {
    let obj = r
        .as_object()
        .ok_or_else(|| DesignError::Parse(format!("recipe \"{name}\" is not an object")))?;
    let num = |keys: &[&str], default: f64| -> f64 {
        keys.iter()
            .find_map(|k| obj.get(*k).and_then(Value::as_f64))
            .unwrap_or(default)
    };
    let mut notes = Vec::new();

    let turns = num(&["turns"], 8000.0).round().max(1.0) as u32;
    let jitter = num(&["jitter"], 0.0).clamp(0.0, 100.0);
    let seed = num(&["scatter_seed"], 1234.0).max(0.0) as u64;
    let wire = obj.get("wire").and_then(Value::as_str).unwrap_or("42 AWG");
    let wire = if wire == "Other" {
        notes.push("wire \"Other\" imported as 42 AWG".into());
        "42 AWG"
    } else {
        wire
    };
    let direction = match obj.get("wind_dir").and_then(Value::as_str) {
        Some("CCW") => Direction::Ccw,
        _ => Direction::Cw,
    };
    let tension = obj.get("tension").and_then(Value::as_f64);
    let use_tension = obj
        .get("use_tension")
        .and_then(Value::as_bool)
        .unwrap_or(tension.is_some());

    let pattern = if jitter > 0.0 {
        Pattern::Scatter {
            pitch_min_od: (1.0 - jitter / 100.0).max(0.05),
            pitch_max_od: 1.0 + jitter / 100.0,
            inset_max_mm: 0.0,
            seed,
        }
    } else {
        Pattern::Layer { pitch_od: 1.0 }
    };
    if obj
        .get("pass")
        .and_then(Value::as_f64)
        .is_some_and(|p| p > 0.0)
    {
        notes.push("\"pass\" dropped: pitch now follows the wire diameter".into());
    }
    if obj.contains_key("lead") {
        notes.push("\"lead\" dropped: it only affected the old preview".into());
    }
    if num(&["scatter_range"], 0.0) > 0.0 {
        notes.push("\"scatter_range\" dropped: use a Scatter section's pitch range instead".into());
    }
    note_unknown(obj, &mut notes);

    let design = Design {
        version: DESIGN_VERSION,
        name: name.to_string(),
        notes: "Imported from the Python app's recipes.".into(),
        wire: WireRef::Named(wire.to_string()),
        direction,
        bobbin: Bobbin {
            channel_height_mm: num(&["win_h", "width", "win_w"], 11.0),
            core_width_mm: num(&["core_w"], 16.0),
            core_length_mm: num(&["core_l", "bobbin_len"], 46.0),
            build_depth_mm: num(&["win_d"], 6.0),
        },
        speed: Speed {
            rpm: num(&["rpm"], 420.0),
            ramp_turns: num(&["ramp"], 50.0),
            ..Speed::default()
        },
        estimate: EstimateInput {
            pack: num(&["pack"], 0.92),
            scatter: jitter,
            tension_g: if use_tension { tension } else { None },
            temp_c: num(&["temp_c"], 20.0),
        },
        sections: vec![Section {
            pattern,
            ..Section::layer(turns, 1.0)
        }],
    };
    design.validate()?;
    Ok(Imported { design, notes })
}

const KNOWN: &[&str] = &[
    "turns",
    "width",
    "win_h",
    "win_w",
    "win_d",
    "core_w",
    "core_l",
    "bobbin_len",
    "rpm",
    "ramp",
    "pass",
    "jitter",
    "wire",
    "lead",
    "pack",
    "tension",
    "use_tension",
    "temp_c",
    "scatter_range",
    "scatter_seed",
    "wind_dir",
];

fn note_unknown(obj: &Map<String, Value>, notes: &mut Vec<String>) {
    for k in obj.keys().filter(|k| !KNOWN.contains(&k.as_str())) {
        notes.push(format!("unknown field \"{k}\" ignored"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../app/winder_recipes.json");

    #[test]
    fn imports_the_shipped_recipes() {
        let all = import_recipes(SHIPPED).unwrap();
        assert_eq!(all.len(), 4);
        let paf = all
            .iter()
            .find(|i| i.design.name.starts_with("PAF"))
            .unwrap();
        assert_eq!(paf.design.bobbin.channel_height_mm, 6.5);
        assert_eq!(paf.design.bobbin.core_length_mm, 52.0);
        assert_eq!(paf.design.total_turns(), 5000);
        assert!(matches!(
            paf.design.sections[0].pattern,
            Pattern::Scatter { .. }
        ));
        assert!(paf.notes.iter().any(|n| n.contains("\"pass\"")));
        let hot = all
            .iter()
            .find(|i| i.design.name == "Hot Humbucker (per coil)")
            .unwrap();
        assert_eq!(hot.design.wire, WireRef::Named("43 AWG".into()));
    }

    #[test]
    fn imported_designs_generate() {
        for i in import_recipes(SHIPPED).unwrap() {
            let tp = crate::toolpath::generate(&i.design).unwrap();
            assert_eq!(tp.stats().turns, i.design.total_turns() as f64);
        }
    }
}
