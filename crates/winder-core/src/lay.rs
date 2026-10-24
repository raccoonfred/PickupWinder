//! Where each turn ends up: a cross-section of the coil built from the
//! toolpath.
//!
//! The cut is taken where the spindle passes angle zero, so every turn
//! shows once, at the guide position it had at the end of that turn. Each
//! wire drops onto the coil and comes to rest on the highest wires beneath
//! it (or on the core), like round wire really nests. A perfect layer
//! pattern therefore stacks at OD·√3/2 per layer, while scatter and dwell
//! pile up wherever the toolpath sends them.
//!
//! This turns the lay pattern into numbers: real build depth, the length of
//! every turn at its own height, and so a DCR that depends on the pattern.

use crate::{design::Design, toolpath::Toolpath, wire::WireSpec};

/// One turn in the cross-section.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// 1-based turn number.
    pub n: u32,
    /// Across the channel, from the first flange's inner face (mm).
    pub y_mm: f64,
    /// Height of the wire's centre above the core (mm).
    pub z_mm: f64,
    pub section: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrossSection {
    pub turns: Vec<Turn>,
    pub od_mm: f64,
    pub channel_mm: f64,
    /// Top of the winding across the channel, `BINS_PER_OD` bins per wire.
    pub profile: Vec<f64>,
    core_perimeter_mm: f64,
    ohm_per_m: f64,
}

/// Resolution of the height map, in bins per wire diameter.
pub const BINS_PER_OD: usize = 4;

impl CrossSection {
    pub fn build(design: &Design, toolpath: &Toolpath, wire: &WireSpec) -> CrossSection {
        let od = wire.od_mm;
        let channel = design.bobbin.channel_height_mm;
        let bin_w = od / BINS_PER_OD as f64;
        let bins = (channel / bin_w).ceil() as usize + 1;
        // Index of the topmost wire covering each bin.
        let mut top: Vec<Option<usize>> = vec![None; bins];
        let mut turns: Vec<Turn> = Vec::with_capacity(design.total_turns() as usize);

        let mut t0 = 0.0;
        let mut y0 = toolpath.start_y_mm;
        let mut next = 1u32;
        for m in toolpath.moves() {
            while (next as f64) <= m.turns + 1e-9 {
                let span = m.turns - t0;
                let y = if span > 0.0 {
                    y0 + (m.y_mm - y0) * (next as f64 - t0) / span
                } else {
                    m.y_mm
                };
                let (y, z) = settle(&turns, &top, y, od, bin_w, channel);
                let idx = turns.len();
                turns.push(Turn {
                    n: next,
                    y_mm: y,
                    z_mm: z,
                    section: m.section,
                });
                let (lo, hi) = bin_range(y, od / 2.0, bin_w, bins);
                for slot in &mut top[lo..=hi] {
                    if slot.is_none_or(|i| turns[i].z_mm <= z) {
                        *slot = Some(idx);
                    }
                }
                next += 1;
            }
            t0 = m.turns;
            y0 = m.y_mm;
        }

        let profile = (0..bins)
            .map(|b| {
                let yb = (b as f64 + 0.5) * bin_w;
                top[b].map_or(0.0, |i| {
                    let w = turns[i];
                    let dy = (yb - w.y_mm).abs().min(od / 2.0);
                    w.z_mm + (od * od / 4.0 - dy * dy).sqrt()
                })
            })
            .collect();
        CrossSection {
            turns,
            od_mm: od,
            channel_mm: channel,
            profile,
            core_perimeter_mm: design.bobbin.core_perimeter_mm(),
            ohm_per_m: wire.ohm_per_m,
        }
    }

    /// Deepest point of the winding above the core (mm).
    pub fn build_mm(&self) -> f64 {
        self.build_mm_upto(self.turns.len())
    }

    /// Build after the first `n` turns.
    pub fn build_mm_upto(&self, n: usize) -> f64 {
        self.turns[..n.min(self.turns.len())]
            .iter()
            .map(|t| t.z_mm + self.od_mm / 2.0)
            .fold(0.0, f64::max)
    }

    /// Mean build across the channel (mm): the coil's average thickness.
    pub fn mean_build_mm(&self) -> f64 {
        if self.profile.is_empty() {
            return 0.0;
        }
        self.profile.iter().sum::<f64>() / self.profile.len() as f64
    }

    /// Total wire length, each turn at its own height (m).
    pub fn length_m(&self) -> f64 {
        self.turns
            .iter()
            .map(|t| self.core_perimeter_mm + 2.0 * std::f64::consts::PI * t.z_mm)
            .sum::<f64>()
            / 1000.0
    }

    /// DCR from the laid wire at `temp_c` (kΩ).
    pub fn dcr_kohm(&self, temp_c: f64) -> f64 {
        self.length_m() * self.ohm_per_m / 1000.0
            * (1.0 + crate::physics::COPPER_ALPHA_PER_C * (temp_c - 20.0))
    }
}

fn bin_range(y: f64, half: f64, bin_w: f64, bins: usize) -> (usize, usize) {
    let lo = ((y - half) / bin_w).floor().max(0.0) as usize;
    let hi = (((y + half) / bin_w).ceil() as usize).min(bins - 1);
    (lo.min(bins - 1), hi)
}

/// Where a wire laid at `y` comes to rest. Tension pulls it into the lowest
/// groove within half a wire of the guide, which is what makes a layer
/// pattern nest (orthocyclic winding); it never leaves the channel.
fn settle(
    turns: &[Turn],
    top: &[Option<usize>],
    y: f64,
    od: f64,
    bin_w: f64,
    channel: f64,
) -> (f64, f64) {
    let (min_y, max_y) = (od / 2.0, channel - od / 2.0);
    let steps = 2 * BINS_PER_OD as i32;
    let mut best = (y.clamp(min_y, max_y), f64::INFINITY);
    for k in -steps / 2..=steps / 2 {
        let yc = (y + k as f64 * od / steps as f64).clamp(min_y, max_y);
        let z = rest_height(turns, top, yc, od, bin_w);
        let closer = (yc - y).abs() < (best.0 - y).abs();
        if z < best.1 - 1e-9 || ((z - best.1).abs() <= 1e-9 && closer) {
            best = (yc, z);
        }
    }
    // Refine to the bottom of the groove (the rest height is V-shaped there).
    let step = od / steps as f64;
    let (mut a, mut b) = ((best.0 - step).max(min_y), (best.0 + step).min(max_y));
    for _ in 0..40 {
        let m1 = a + (b - a) / 3.0;
        let m2 = b - (b - a) / 3.0;
        if rest_height(turns, top, m1, od, bin_w) <= rest_height(turns, top, m2, od, bin_w) {
            b = m2;
        } else {
            a = m1;
        }
    }
    let yr = (a + b) / 2.0;
    let zr = rest_height(turns, top, yr, od, bin_w);
    if zr < best.1 { (yr, zr) } else { best }
}

/// Height at which a wire dropped at `y` comes to rest.
fn rest_height(turns: &[Turn], top: &[Option<usize>], y: f64, od: f64, bin_w: f64) -> f64 {
    let (lo, hi) = bin_range(y, od, bin_w, top.len());
    let mut z = od / 2.0; // on the core
    let mut seen = usize::MAX;
    for i in top[lo..=hi].iter().flatten() {
        if *i == seen {
            continue;
        }
        seen = *i;
        let w = turns[*i];
        let dy = (y - w.y_mm).abs();
        if dy < od {
            z = z.max(w.z_mm + (od * od - dy * dy).sqrt());
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        design::Section,
        physics::{EstimateInput, estimate},
        toolpath::generate,
    };

    fn strat(turns: u32, pitch_od: f64) -> Design {
        let mut d = Design::example("strat").unwrap();
        d.sections = vec![Section::layer(turns, pitch_od)];
        d
    }

    fn section(d: &Design) -> CrossSection {
        CrossSection::build(d, &generate(d).unwrap(), &d.wire_spec().unwrap())
    }

    #[test]
    fn one_circle_per_turn() {
        let d = Design::example("paf").unwrap();
        let cs = section(&d);
        assert_eq!(cs.turns.len(), 5000);
        assert_eq!(cs.turns.last().unwrap().n, 5000);
    }

    #[test]
    fn layer_pattern_nests_into_the_layer_below() {
        // The second layer drops into the grooves of the first: about
        // OD·√3/2 = 0.866 OD higher, not a full OD. It sits a little above
        // ideal because the flange shifts it off the groove centres.
        let cs = section(&strat(300, 1.0));
        let od = 0.071;
        let first = cs.turns[50].z_mm;
        let second = cs.turns[220].z_mm;
        assert!((first - od / 2.0).abs() < 1e-9);
        let step = (second - first) / od;
        assert!((0.85..0.92).contains(&step), "{step}");
    }

    #[test]
    fn layer_pattern_agrees_with_the_analytic_model() {
        let d = strat(8000, 1.0);
        let cs = section(&d);
        let e = estimate(
            8000,
            &d.bobbin,
            &d.wire_spec().unwrap(),
            &EstimateInput::default(),
        );
        let rel = |a: f64, b: f64| (a - b).abs() / b;
        assert!(
            rel(cs.dcr_kohm(20.0), e.dcr_kohm) < 0.02,
            "{} vs {}",
            cs.dcr_kohm(20.0),
            e.dcr_kohm
        );
        assert!(
            rel(cs.build_mm(), e.build_mm) < 0.05,
            "{} vs {}",
            cs.build_mm(),
            e.build_mm
        );
    }

    #[test]
    fn looser_pitch_builds_deeper() {
        let tight = section(&strat(8000, 1.0));
        let loose = section(&strat(8000, 1.6));
        assert!(loose.build_mm() > tight.build_mm() * 1.1);
        assert!(loose.dcr_kohm(20.0) > tight.dcr_kohm(20.0));
    }

    #[test]
    fn dwell_piles_wire_at_the_flanges() {
        let edge = |cs: &CrossSection| {
            let n = cs.profile.len();
            cs.profile[1].max(cs.profile[n - 2])
        };
        let plain = section(&strat(2000, 1.0));
        let mut d = strat(2000, 1.0);
        d.sections[0].dwell_turns = 3.0;
        let dwell = section(&d);
        assert!(
            edge(&dwell) > edge(&plain) * 1.1,
            "{} vs {}",
            edge(&dwell),
            edge(&plain)
        );
    }
}
