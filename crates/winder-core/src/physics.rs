//! Layer-by-layer coil physics: DCR, wire length, build, inductance,
//! self-capacitance, resonance and Q.
//!
//! Ported from `simulate_coil` in the Python app and locked to the same
//! numbers by the golden tests below.
//!
//! Base model (validated against published pickups): turns per layer =
//! channel height / wire OD; layers stack radially at OD·√3/2 (hexagonal
//! close packing); each layer sits on a larger perimeter, so its turns are
//! longer; DCR = total length × Ω/m.
//!
//! The pack, scatter and tension terms and the inductance and capacitance
//! factors are fitted estimates, not first-principles physics. They are
//! meant to be refined from measured coils.

use crate::{bobbin::Bobbin, wire::WireSpec};
use serde::{Deserialize, Serialize};

/// Copper resistance rises about 0.393 % per °C (annealed copper).
const COPPER_ALPHA_PER_C: f64 = 0.00393;
/// Wheeler assumes an air core; a single-coil's open magnetic path reads
/// lower. 0.62 lands a Strat-geometry 8000-turn coil at about 2.2 H.
const INDUCTANCE_CALIBRATION: f64 = 0.62;
/// Lumped self-capacitance per (turn × mean perimeter mm), in pF.
const CAP_PF_PER_TURN_MM: f64 = 2.2e-4;

/// Winding character inputs for the estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EstimateInput {
    /// How tightly turns pack (0.5-1.0); 1.0 is ideal hex packing. Ignored
    /// when `tension_g` is set.
    pub pack: f64,
    /// Scatter amount 0-100: loosens the lay slightly and lowers
    /// self-capacitance (brighter tone).
    pub scatter: f64,
    /// Wire tension in grams. When set, it drives the pack and the
    /// break-risk warning.
    pub tension_g: Option<f64>,
    /// Coil temperature when measured (°C).
    pub temp_c: f64,
}

impl Default for EstimateInput {
    fn default() -> Self {
        EstimateInput {
            pack: 1.0,
            scatter: 0.0,
            tension_g: None,
            temp_c: 20.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    pub dcr_kohm: f64,
    pub length_m: f64,
    pub layers: u32,
    pub build_mm: f64,
    pub turns_per_layer: f64,
    pub cap_pf: f64,
    pub resonance_khz: f64,
    /// How much looser than ideal the lay is (1.0 = ideal).
    pub pitch_factor: f64,
    pub inductance_h: f64,
    pub q_factor: f64,
    /// Tension as a fraction of breaking force (0-1).
    pub break_risk: f64,
    pub eff_pack: f64,
    pub temp_c: f64,
}

impl Estimate {
    /// The tension is close enough to breaking to warn about.
    pub fn break_warning(&self) -> bool {
        self.break_risk >= 0.85
    }
}

pub fn estimate(turns: u32, bobbin: &Bobbin, wire: &WireSpec, input: &EstimateInput) -> Estimate {
    let od = wire.od_mm;
    let win_h = bobbin.channel_height_mm;
    let mut out = Estimate {
        dcr_kohm: 0.0,
        length_m: 0.0,
        layers: 0,
        build_mm: 0.0,
        turns_per_layer: 0.0,
        cap_pf: 0.0,
        resonance_khz: 0.0,
        pitch_factor: 1.0,
        inductance_h: 0.0,
        q_factor: 0.0,
        break_risk: 0.0,
        eff_pack: input.pack,
        temp_c: input.temp_c,
    };
    if turns == 0 || win_h <= 0.0 || od <= 0.0 {
        return out;
    }
    let s = input.scatter.clamp(0.0, 100.0) / 100.0;

    // Wire tension -> effective pack + break risk.
    let eff_pack = match input.tension_g {
        Some(t) if t > 0.0 => {
            let break_g = wire.break_force_g().max(1e-6);
            out.break_risk = (t / break_g).clamp(0.0, 1.0);
            // 0 g -> loose (0.80), near break -> tight (~0.98).
            (0.80 + 0.18 * (t / (0.6 * break_g)).min(1.0)).clamp(0.6, 0.99)
        }
        _ => input.pack.clamp(0.5, 1.0),
    };

    // Geometry: layers, length, build.
    let loosen = (1.0 / eff_pack) * (1.0 + 0.12 * s);
    let tpl = ((win_h / od) / loosen).max(1.0);
    let pitch = od * 3f64.sqrt() / 2.0 * loosen;

    let base_perim = bobbin.core_perimeter_mm();
    let mut remaining = turns as f64;
    let mut layer: u32 = 0;
    let mut length_mm = 0.0;
    while remaining > 0.0 && layer < 500_000 {
        let n = tpl.min(remaining);
        let b = layer as f64 * pitch;
        let perim = base_perim + 2.0 * std::f64::consts::PI * (b + pitch / 2.0);
        length_mm += n * perim;
        remaining -= n;
        layer += 1;
    }
    let length_m = length_mm / 1000.0;

    // DCR with copper's temperature coefficient.
    let temp_factor = 1.0 + COPPER_ALPHA_PER_C * (input.temp_c - 20.0);
    let dcr_kohm = length_m * wire.ohm_per_m / 1000.0 * temp_factor;
    let build_mm = layer as f64 * pitch;

    // Inductance: Wheeler multilayer, L(µH) = 0.8 a² N² / (6a + 9b + 10c),
    // a = mean radius, b = coil length (axial), c = winding depth, inches.
    let mean_perim_mm = base_perim + std::f64::consts::PI * build_mm;
    let a_in = mean_perim_mm / (2.0 * std::f64::consts::PI) / 25.4;
    let b_in = win_h / 25.4;
    let c_in = build_mm / 25.4;
    let denom = 6.0 * a_in + 9.0 * b_in + 10.0 * c_in;
    let n = turns as f64;
    let induct_uh = if denom > 0.0 {
        0.8 * a_in * a_in * n * n / denom
    } else {
        0.0
    };
    let inductance_h = induct_uh * INDUCTANCE_CALIBRATION * 1e-6;

    // Self-capacitance, lowered by scatter.
    let cap_pf = (n * mean_perim_mm * CAP_PF_PER_TURN_MM * (1.0 - 0.35 * s)).max(1.0);

    // RLC resonant peak and Q = (1/R)·√(L/C).
    let (res_hz, q_factor) = if inductance_h > 0.0 {
        let c_f = cap_pf * 1e-12;
        let r = (dcr_kohm * 1000.0).max(1.0);
        (
            1.0 / (2.0 * std::f64::consts::PI * (inductance_h * c_f).sqrt()),
            (inductance_h / c_f).sqrt() / r,
        )
    } else {
        (0.0, 0.0)
    };

    out.dcr_kohm = dcr_kohm;
    out.length_m = length_m;
    out.layers = layer;
    out.build_mm = build_mm;
    out.turns_per_layer = tpl;
    out.cap_pf = cap_pf;
    out.resonance_khz = res_hz / 1000.0;
    out.pitch_factor = loosen;
    out.inductance_h = inductance_h;
    out.q_factor = q_factor;
    out.eff_pack = eff_pack;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w42() -> WireSpec {
        WireSpec::named("42 AWG").unwrap()
    }

    fn coil(h: f64, w: f64, l: f64) -> Bobbin {
        Bobbin {
            channel_height_mm: h,
            core_width_mm: w,
            core_length_mm: l,
            build_depth_mm: 6.0,
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Golden values from the Python `simulate_coil` (pack 1, scatter 0,
    /// 20 °C), which match published pickups.
    #[test]
    fn matches_python_and_published_pickups() {
        let w = w42();
        let cases = [
            // turns, h, w, l, dcr k, layers, wire ft, L H, res kHz
            (7500, 11.0, 16.0, 46.0, 5.44, 49, 3281.0, 1.93, 7.7),
            (8000, 11.0, 16.0, 46.0, 5.83, 52, 3517.0, 2.20, 7.0),
            (10866, 11.0, 16.0, 46.0, 8.13, 71, 4904.0, 4.04, 4.4),
            (5250, 6.5, 16.0, 52.0, 4.20, 58, 2533.0, 1.26, 10.9),
        ];
        for (turns, h, cw, cl, dcr, layers, ft, l, res) in cases {
            let e = estimate(turns, &coil(h, cw, cl), &w, &EstimateInput::default());
            assert!(close(e.dcr_kohm, dcr, 0.005), "{turns}: dcr {}", e.dcr_kohm);
            assert_eq!(e.layers, layers, "{turns}: layers");
            assert!(
                close(e.length_m * 3.281, ft, 1.0),
                "{turns}: ft {}",
                e.length_m * 3.281
            );
            assert!(
                close(e.inductance_h, l, 0.005),
                "{turns}: L {}",
                e.inductance_h
            );
            assert!(
                close(e.resonance_khz, res, 0.05),
                "{turns}: res {}",
                e.resonance_khz
            );
        }
    }

    /// The Python app's default UI state: pack 0.92, scatter 12, 35 g.
    #[test]
    fn matches_python_with_tension_and_scatter() {
        let input = EstimateInput {
            pack: 0.92,
            scatter: 12.0,
            tension_g: Some(35.0),
            temp_c: 20.0,
        };
        let e = estimate(8000, &coil(11.0, 16.0, 46.0), &w42(), &input);
        assert!(close(e.dcr_kohm, 5.88, 0.01), "{}", e.dcr_kohm);
        assert_eq!(e.layers, 55);
        assert!(close(e.cap_pf, 228.0, 1.0), "{}", e.cap_pf);
        assert!(close(e.resonance_khz, 7.12, 0.02), "{}", e.resonance_khz);
        assert!(close(e.q_factor, 16.7, 0.1), "{}", e.q_factor);
        assert!(close(e.eff_pack, 0.96, 0.01), "{}", e.eff_pack);
    }

    #[test]
    fn warm_coil_reads_higher() {
        let b = coil(11.0, 16.0, 46.0);
        let cold = estimate(8000, &b, &w42(), &EstimateInput::default());
        let warm = estimate(
            8000,
            &b,
            &w42(),
            &EstimateInput {
                temp_c: 30.0,
                ..Default::default()
            },
        );
        assert!(close(warm.dcr_kohm / cold.dcr_kohm, 1.0393, 1e-9));
    }

    #[test]
    fn zero_turns_is_empty() {
        let e = estimate(
            0,
            &coil(11.0, 16.0, 46.0),
            &w42(),
            &EstimateInput::default(),
        );
        assert_eq!(e.layers, 0);
        assert_eq!(e.dcr_kohm, 0.0);
    }
}
