//! Magnet wire properties.

use serde::{Deserialize, Serialize};

/// One magnet wire.
///
/// `ohm_per_m` is the DC resistance of the bare copper at 20 °C, from copper
/// resistivity (1.724e-8 Ω·m) and the nominal bare diameter. `od_mm` is the
/// nominal insulated outer diameter, which sets how the coil builds up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSpec {
    pub name: String,
    /// Insulated outer diameter (mm).
    pub od_mm: f64,
    /// Bare copper diameter (mm), used for breaking strength.
    pub bare_mm: f64,
    /// DC resistance at 20 °C (Ω per metre).
    pub ohm_per_m: f64,
}

/// Built-in table: (name, OD, bare, Ω/m). Everything matches the Python app
/// so its validated numbers carry over unchanged; bare = OD × 0.86 as there
/// (enamel is about 14 % of OD on fine magnet wire).
const TABLE: &[(&str, f64, f64, f64)] = &[
    ("42 AWG", 0.071, 0.071 * 0.86, 5.44),
    ("43 AWG", 0.064, 0.064 * 0.86, 6.85),
    ("44 AWG", 0.058, 0.058 * 0.86, 8.61),
];

impl WireSpec {
    /// Look up a built-in wire by name, e.g. `"42 AWG"`.
    pub fn named(name: &str) -> Option<WireSpec> {
        TABLE
            .iter()
            .find(|(n, ..)| n.eq_ignore_ascii_case(name.trim()))
            .map(|&(n, od, bare, ohm)| WireSpec {
                name: n.to_string(),
                od_mm: od,
                bare_mm: bare,
                ohm_per_m: ohm,
            })
    }

    /// Names of the built-in wires.
    pub fn builtin_names() -> impl Iterator<Item = &'static str> {
        TABLE.iter().map(|(n, ..)| *n)
    }

    /// Force at which the wire breaks, in grams-force. Soft copper breaks at
    /// about 220 MPa, which puts 42 AWG near 70 g (real practice: wind it at
    /// 30-50 g).
    pub fn break_force_g(&self) -> f64 {
        let r_m = self.bare_mm / 2.0 * 1e-3;
        220e6 * std::f64::consts::PI * r_m * r_m / 9.81 * 1000.0
    }
}

/// A wire in a design file: either a built-in name or a full spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireRef {
    Named(String),
    Custom(WireSpec),
}

impl WireRef {
    pub fn resolve(&self) -> Option<WireSpec> {
        match self {
            WireRef::Named(n) => WireSpec::named(n),
            WireRef::Custom(w) => Some(w.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(WireSpec::named("42 awg").unwrap().od_mm, 0.071);
        assert!(WireSpec::named("41 AWG").is_none());
    }

    #[test]
    fn break_force_42_awg_is_about_70_g() {
        let f = WireSpec::named("42 AWG").unwrap().break_force_g();
        assert!((62.0..72.0).contains(&f), "{f}");
    }
}
