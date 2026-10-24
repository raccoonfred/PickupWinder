//! Where each turn ends up: the coil built turn by turn from the toolpath.
//!
//! A pickup bobbin is not round. The wire runs straight along the two long
//! sides and is pressed onto the coil only where it wraps around the short
//! ends, so the model follows each turn through stations spaced evenly
//! around the core and keeps a separate cross-section at every station:
//!
//! * **Wire lag.** The wire leaves the guide's eye and meets the coil a free
//!   span later, so the lay point trails the guide: for every millimetre of
//!   wire wound it closes 1/span of the gap. Reversals round off and the
//!   step of a perfect layer lands late.
//! * **Settling.** On the short ends tension presses the wire down hard
//!   enough to roll it into the lowest groove within half a wire. Across a
//!   long side it runs straight from one end's contact to the next, so it
//!   can't seek grooves there and rides over whatever crosses beneath it.
//!   That is why real coils bulge on the long sides.
//! * **Crossing.** A pass that crosses the one below (any pitch above one
//!   wire) meets it at a shallow angle and sits on top of it for a long
//!   stretch, which is why helical and scatter coils build deeper than
//!   perfect layers.
//!
//! From the laid wire come the numbers the bench measures: build, the
//! length of every turn at its own height (and so DCR), fill factor and the
//! self-capacitance, from which turns ended up touching which.

use crate::{design::Design, toolpath::Toolpath, wire::WireSpec};
use serde::{Deserialize, Serialize};

/// How the wire behaves between the guide and the coil.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LaySettings {
    /// Free wire between the guide's eye and the point where it meets the
    /// coil (mm). 0 lays the wire exactly under the guide.
    pub free_span_mm: f64,
}

impl Default for LaySettings {
    fn default() -> Self {
        LaySettings { free_span_mm: 25.0 }
    }
}

/// Stations around each turn.
pub const STATIONS: usize = 16;
/// Resolution of the height maps, in bins per wire diameter.
pub const BINS_PER_OD: usize = 4;
/// How far a wire rolls sideways into a groove, in wire diameters. On a
/// short end tension presses it down and it finds the groove; on a long
/// side it is only nudged. 0.25 lands helical coils at the 0.73-0.80 fill
/// factor published for crossed winding, and perfect layers near 0.86.
const SLIDE_END_OD: f64 = 0.5;
const SLIDE_SIDE_OD: f64 = 0.25;
/// A neighbour further off to the side than this (in wire diameters) only
/// touches the wire's flank. Without it a pitch a hair under one wire
/// would climb the previous turn forever.
const SIDE_CONTACT_OD: f64 = 0.95;
/// Turn-to-turn capacitance: enamel film and the air gap around it, as one
/// effective permittivity. Wires further apart than `CAP_REACH_OD` centre
/// to centre are screened by the turns between them.
const EPS_EFFECTIVE: f64 = 8.854e-12 * 2.5;
const CAP_REACH_OD: f64 = 1.2;

/// One turn where it crosses one station.
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

/// A place around the core where the coil is cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Station {
    /// Share of the turn from where the wire runs onto the first short end.
    pub u: f64,
    /// On a short end, where the wire is pressed onto the coil.
    pub on_end: bool,
}

impl Station {
    pub fn label(&self) -> &'static str {
        if self.on_end {
            "short end"
        } else {
            "long side"
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrossSection {
    pub stations: Vec<Station>,
    /// Every turn at every station: `cuts[station][turn - 1]`.
    pub cuts: Vec<Vec<Turn>>,
    pub od_mm: f64,
    pub channel_mm: f64,
    /// Top of the winding across the channel at each station,
    /// `BINS_PER_OD` bins per wire.
    pub profiles: Vec<Vec<f64>>,
    core_perimeter_mm: f64,
    core_width_mm: f64,
    core_length_mm: f64,
    ohm_per_m: f64,
    bare_mm: f64,
}

/// What the laid coil measures.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub dcr_kohm: f64,
    pub length_m: f64,
    /// Deepest point anywhere around the coil.
    pub build_mm: f64,
    /// Deepest point mid long side, where the cover goes.
    pub side_build_mm: f64,
    /// Deepest point mid short end.
    pub end_build_mm: f64,
    pub mean_build_mm: f64,
    pub fill_factor: f64,
    pub cap_pf: f64,
    pub inductance_h: f64,
    pub resonance_khz: f64,
    pub q_factor: f64,
}

/// One station's height map while the coil is being built.
struct Cut {
    turns: Vec<Turn>,
    /// Index of the topmost wire covering each bin.
    top: Vec<Option<usize>>,
}

impl CrossSection {
    pub fn build(design: &Design, toolpath: &Toolpath, wire: &WireSpec) -> CrossSection {
        let od = wire.od_mm;
        let channel = design.bobbin.channel_height_mm;
        let perimeter = design.bobbin.core_perimeter_mm();
        let end = design.bobbin.core_width_mm / perimeter;
        let length = design.bobbin.core_length_mm / perimeter;
        let stations: Vec<Station> = (0..STATIONS)
            .map(|j| {
                let u = (j as f64 + 0.5) / STATIONS as f64;
                Station {
                    u,
                    on_end: u < end || (end + length..2.0 * end + length).contains(&u),
                }
            })
            .collect();
        let bin_w = od / BINS_PER_OD as f64;
        let bins = (channel / bin_w).ceil() as usize + 1;
        let total = design.total_turns() as usize;
        let mut cuts: Vec<Cut> = (0..STATIONS)
            .map(|_| Cut {
                turns: Vec::with_capacity(total),
                top: vec![None; bins],
            })
            .collect();

        let span = design.lay.free_span_mm;
        let ds_base = perimeter / STATIONS as f64;
        // A bobbin too narrow for any station on its ends is treated as round.
        let any_end = stations.iter().any(|s| s.on_end);
        let mut guide = Guide::new(toolpath);
        // The lay point: where the wire last touched the coil.
        let mut lay_y = toolpath.start_y_mm;
        let mut z_prev = od / 2.0;
        // Last settled contact on an end: (time in turns, y).
        let mut contact = (0.0, toolpath.start_y_mm);
        // Long-side stations waiting for the next end contact: the wire runs
        // straight across a long side, from one end's contact to the next.
        let mut pending: Vec<(usize, f64, u32, usize, f64)> = Vec::new();
        let place = |cut: &mut Cut, n: u32, section: usize, y: f64, z: f64| {
            let idx = cut.turns.len();
            cut.turns.push(Turn {
                n,
                y_mm: y,
                z_mm: z,
                section,
            });
            let (lo, hi) = bin_range(y, od / 2.0, bin_w, bins);
            for b in lo..=hi {
                if cut.top[b].is_none_or(|i| cut.turns[i].z_mm <= z) {
                    cut.top[b] = Some(idx);
                }
            }
        };
        for n in 0..total {
            for (j, st) in stations.iter().enumerate() {
                let t = n as f64 + st.u;
                let (gy, section) = guide.at(t);
                // Wire wound since the last station, at the current height.
                let ds = ds_base + 2.0 * std::f64::consts::PI * z_prev / STATIONS as f64;
                let pull = if span > 0.0 {
                    1.0 - (-ds / span).exp()
                } else {
                    1.0
                };
                let aim = lay_y + (gy - lay_y) * pull;
                if any_end && !st.on_end {
                    pending.push((j, t, n as u32 + 1, section, aim));
                    lay_y = aim;
                    continue;
                }
                let cut = &mut cuts[j];
                let (y, z) = settle(
                    &cut.turns,
                    &cut.top,
                    aim,
                    SLIDE_END_OD * od,
                    od,
                    bin_w,
                    channel,
                );
                place(cut, n as u32 + 1, section, y, z);
                // The long side just crossed spans the two contacts.
                for (k, tk, nk, sk, _) in pending.drain(..) {
                    let f = (tk - contact.0) / (t - contact.0);
                    let yk = contact.1 + (y - contact.1) * f;
                    let c = &mut cuts[k];
                    let (yk, zk) =
                        settle(&c.turns, &c.top, yk, SLIDE_SIDE_OD * od, od, bin_w, channel);
                    place(c, nk, sk, yk, zk);
                }
                contact = (t, y);
                lay_y = y;
                z_prev = z;
            }
        }
        // The last long side ends where the wire is cut off.
        for (k, _, nk, sk, aim) in pending.drain(..) {
            let c = &mut cuts[k];
            let (yk, zk) = settle(
                &c.turns,
                &c.top,
                aim,
                SLIDE_SIDE_OD * od,
                od,
                bin_w,
                channel,
            );
            place(c, nk, sk, yk, zk);
        }

        let profiles = cuts
            .iter()
            .map(|c| {
                (0..bins)
                    .map(|b| {
                        let yb = (b as f64 + 0.5) * bin_w;
                        c.top[b].map_or(0.0, |i| {
                            let w = c.turns[i];
                            let dy = (yb - w.y_mm).abs().min(od / 2.0);
                            w.z_mm + (od * od / 4.0 - dy * dy).sqrt()
                        })
                    })
                    .collect()
            })
            .collect();
        CrossSection {
            stations,
            cuts: cuts.into_iter().map(|c| c.turns).collect(),
            od_mm: od,
            channel_mm: channel,
            profiles,
            core_perimeter_mm: perimeter,
            core_width_mm: design.bobbin.core_width_mm,
            core_length_mm: design.bobbin.core_length_mm,
            ohm_per_m: wire.ohm_per_m,
            bare_mm: wire.bare_mm,
        }
    }

    /// Everything the bench would measure, from the laid wire. Inductance
    /// uses Wheeler's formula on the coil's mean build.
    pub fn laid(&self, design: &Design) -> Laid {
        let dcr = self.dcr_kohm(design.estimate.temp_c);
        let mean = self.mean_build_mm();
        let cap = self.cap_pf();
        let l = crate::physics::inductance_h(self.turn_count() as u32, &design.bobbin, mean);
        let (res, q) = crate::physics::resonance(l, cap, dcr);
        let n = self.turn_count();
        Laid {
            dcr_kohm: dcr,
            length_m: self.length_m(),
            build_mm: self.build_mm(),
            side_build_mm: self.build_at(self.side_station(), n),
            end_build_mm: self.build_at(self.end_station(), n),
            mean_build_mm: mean,
            fill_factor: self.fill_factor(),
            cap_pf: cap,
            inductance_h: l,
            resonance_khz: res,
            q_factor: q,
        }
    }

    pub fn turn_count(&self) -> usize {
        self.cuts.first().map_or(0, Vec::len)
    }

    /// The station in the middle of a long side: the classic cut through a
    /// pickup, and where the coil must fit under its cover.
    pub fn side_station(&self) -> usize {
        self.nearest((self.core_width_mm + self.core_length_mm / 2.0) / self.core_perimeter_mm)
    }

    /// The station in the middle of the first short end.
    pub fn end_station(&self) -> usize {
        self.nearest(self.core_width_mm / 2.0 / self.core_perimeter_mm)
    }

    fn nearest(&self, u: f64) -> usize {
        (0..self.stations.len())
            .min_by(|&a, &b| {
                let d = |i: usize| (self.stations[i].u - u).abs();
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(0)
    }

    /// Deepest point of the winding above the core, anywhere around it (mm).
    pub fn build_mm(&self) -> f64 {
        self.build_mm_upto(self.turn_count())
    }

    /// Deepest point after the first `n` turns.
    pub fn build_mm_upto(&self, n: usize) -> f64 {
        self.cuts
            .iter()
            .flat_map(|c| c[..n.min(c.len())].iter())
            .map(|t| t.z_mm + self.od_mm / 2.0)
            .fold(0.0, f64::max)
    }

    /// Deepest point at one station after the first `n` turns.
    pub fn build_at(&self, station: usize, n: usize) -> f64 {
        let c = &self.cuts[station];
        c[..n.min(c.len())]
            .iter()
            .map(|t| t.z_mm + self.od_mm / 2.0)
            .fold(0.0, f64::max)
    }

    /// Mean build across the channel and around the core (mm).
    pub fn mean_build_mm(&self) -> f64 {
        let all: Vec<f64> = self.profiles.iter().flatten().copied().collect();
        if all.is_empty() {
            return 0.0;
        }
        all.iter().sum::<f64>() / all.len() as f64
    }

    /// Share of the winding's cross-section that is wire (insulated
    /// diameter). Perfect hexagonal packing is 0.907, square stacking 0.785.
    pub fn fill_factor(&self) -> f64 {
        let area = self.mean_build_mm() * self.channel_mm;
        if area <= 0.0 {
            return 0.0;
        }
        let wire = std::f64::consts::PI / 4.0 * self.od_mm * self.od_mm;
        self.turn_count() as f64 * wire / area
    }

    /// Total wire length, each turn at its own height all the way round (m).
    pub fn length_m(&self) -> f64 {
        let s = self.stations.len().max(1) as f64;
        let rise: f64 =
            self.cuts.iter().flatten().map(|t| t.z_mm).sum::<f64>() * 2.0 * std::f64::consts::PI
                / s;
        (self.turn_count() as f64 * self.core_perimeter_mm + rise) / 1000.0
    }

    /// DCR from the laid wire at `temp_c` (kΩ).
    pub fn dcr_kohm(&self, temp_c: f64) -> f64 {
        self.length_m() * self.ohm_per_m / 1000.0
            * (1.0 + crate::physics::COPPER_ALPHA_PER_C * (temp_c - 20.0))
    }

    /// Self-capacitance from which turns touch which (pF).
    ///
    /// With the coil's voltage rising evenly from turn 1 to turn N, two
    /// neighbouring wires `Δn` turns apart hold `Δn/N` of the full voltage
    /// across their own capacitance. Summing that stored energy over every
    /// pair of neighbours at every station gives one equivalent capacitance:
    /// `C = Σ c·ℓ·(Δn/N)²`, with `c` the capacitance per length of two
    /// parallel wires. Turns far apart in number lying side by side are
    /// what make it large: high turns per pass, and layers that return over
    /// the start of the one below.
    pub fn cap_pf(&self) -> f64 {
        let n_total = self.turn_count() as f64;
        if n_total < 2.0 {
            return 0.0;
        }
        let od = self.od_mm;
        let reach = CAP_REACH_OD * od;
        let seg = |z: f64| {
            (self.core_perimeter_mm + 2.0 * std::f64::consts::PI * z)
                / self.stations.len() as f64
                / 1000.0
        };
        let mut c = 0.0;
        for cut in &self.cuts {
            let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
            let cell = |v: f64| (v / reach).floor() as i64;
            for (i, t) in cut.iter().enumerate() {
                grid.entry((cell(t.y_mm), cell(t.z_mm)))
                    .or_default()
                    .push(i);
            }
            for (i, a) in cut.iter().enumerate() {
                let (cy, cz) = (cell(a.y_mm), cell(a.z_mm));
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(list) = grid.get(&(cy + dy, cz + dz)) else {
                            continue;
                        };
                        for &k in list.iter().filter(|&&k| k > i) {
                            let b = cut[k];
                            let d = (a.y_mm - b.y_mm).hypot(a.z_mm - b.z_mm);
                            if d >= reach {
                                continue;
                            }
                            // Two parallel round conductors, centres d apart.
                            let x = (d / self.bare_mm).max(1.0 + 1e-6);
                            let per_m = std::f64::consts::PI * EPS_EFFECTIVE / x.acosh();
                            let dn = (b.n as f64 - a.n as f64) / n_total;
                            c += per_m * seg((a.z_mm + b.z_mm) / 2.0) * dn * dn;
                        }
                    }
                }
            }
        }
        c * 1e12
    }
}

/// The guide position at any moment, read off the toolpath in order.
struct Guide<'a> {
    moves: Vec<&'a crate::toolpath::Move>,
    i: usize,
    t0: f64,
    y0: f64,
}

impl<'a> Guide<'a> {
    fn new(tp: &'a Toolpath) -> Guide<'a> {
        Guide {
            moves: tp.moves().collect(),
            i: 0,
            t0: 0.0,
            y0: tp.start_y_mm,
        }
    }

    /// Guide position and section at `t` turns; `t` never goes backwards.
    fn at(&mut self, t: f64) -> (f64, usize) {
        while self.i + 1 < self.moves.len() && self.moves[self.i].turns < t {
            self.t0 = self.moves[self.i].turns;
            self.y0 = self.moves[self.i].y_mm;
            self.i += 1;
        }
        let Some(m) = self.moves.get(self.i) else {
            return (self.y0, 0);
        };
        let span = m.turns - self.t0;
        let f = if span > 0.0 {
            ((t - self.t0) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        (self.y0 + (m.y_mm - self.y0) * f, m.section)
    }
}

fn bin_range(y: f64, half: f64, bin_w: f64, bins: usize) -> (usize, usize) {
    let lo = ((y - half) / bin_w).floor().max(0.0) as usize;
    let hi = (((y + half) / bin_w).ceil() as usize).min(bins - 1);
    (lo.min(bins - 1), hi)
}

/// Where a wire aimed at `y` comes to rest: on the highest wires beneath
/// it, after rolling at most `slide` sideways into a lower groove. It never
/// leaves the channel.
fn settle(
    turns: &[Turn],
    top: &[Option<usize>],
    y: f64,
    slide: f64,
    od: f64,
    bin_w: f64,
    channel: f64,
) -> (f64, f64) {
    let (min_y, max_y) = (od / 2.0, channel - od / 2.0);
    let y = y.clamp(min_y, max_y);
    let here = rest_height(turns, top, y, od, bin_w);
    if slide <= 0.0 {
        return (y, here);
    }
    let steps = 2 * BINS_PER_OD as i32;
    let step = slide / (steps / 2) as f64;
    let mut best = (y, here);
    for k in -steps / 2..=steps / 2 {
        let yc = (y + k as f64 * step).clamp(min_y, max_y);
        let z = rest_height(turns, top, yc, od, bin_w);
        let closer = (yc - y).abs() < (best.0 - y).abs();
        if z < best.1 - 1e-9 || ((z - best.1).abs() <= 1e-9 && closer) {
            best = (yc, z);
        }
    }
    // Refine to the bottom of the groove (the rest height is V-shaped there),
    // staying within reach.
    let (lo, hi) = ((y - slide).max(min_y), (y + slide).min(max_y));
    let (mut a, mut b) = ((best.0 - step).max(lo), (best.0 + step).min(hi));
    for _ in 0..24 {
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
        if dy < SIDE_CONTACT_OD * od {
            z = z.max(w.z_mm + (od * od - dy * dy).sqrt());
        } else if dy < od {
            // Touching from the side: it pushes, it doesn't hold the wire up.
            z = z.max(w.z_mm);
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        design::{Pattern, Section},
        physics::{EstimateInput, estimate},
        toolpath::generate,
    };

    fn strat(sections: Vec<Section>) -> Design {
        let mut d = Design::example("strat").unwrap();
        d.sections = sections;
        d
    }

    /// Turns per pass as a share of side by side (155 on a Strat).
    fn tpl(share: f64) -> f64 {
        ((11.0 - 0.071) / 0.071 * share).round()
    }

    fn section(d: &Design) -> CrossSection {
        CrossSection::build(d, &generate(d).unwrap(), &d.wire_spec().unwrap())
    }

    #[test]
    fn every_turn_at_every_station() {
        let d = Design::example("paf").unwrap();
        let cs = section(&d);
        assert_eq!(cs.cuts.len(), STATIONS);
        for c in &cs.cuts {
            assert_eq!(c.len(), 5000);
            assert!(c.iter().enumerate().all(|(i, t)| t.n as usize == i + 1));
        }
        // Two stations on each short end of a 16 x 46 core.
        assert_eq!(cs.stations.iter().filter(|s| s.on_end).count(), 4);
        assert!(!cs.stations[cs.side_station()].on_end);
        assert!(cs.stations[cs.end_station()].on_end);
    }

    #[test]
    fn perfect_layers_pack_hexagonally() {
        let d = strat(vec![Section::layer(8000)]);
        let cs = section(&d);
        let f = cs.fill_factor();
        assert!((0.87..0.907).contains(&f), "{f}");
        // Second layer sits in the grooves of the first: OD·√3/2 higher.
        let side = &cs.cuts[cs.side_station()];
        let step = (side[200].z_mm - side[50].z_mm) / 0.071;
        assert!((0.84..0.9).contains(&step), "{step}");
        // And the layer model, which assumes exactly this, agrees.
        let e = estimate(
            8000,
            &d.bobbin,
            &d.wire_spec().unwrap(),
            &EstimateInput::default(),
        );
        let rel = |a: f64, b: f64| (a - b).abs() / b;
        assert!(rel(cs.dcr_kohm(20.0), e.dcr_kohm) < 0.02);
        assert!(
            rel(cs.mean_build_mm(), e.build_mm) < 0.05,
            "{} vs {}",
            cs.mean_build_mm(),
            e.build_mm
        );
    }

    #[test]
    fn crossed_helix_packs_like_real_machine_coils() {
        // Published fill for crossed (helical) winding is about 0.73-0.80.
        for share in [0.6, 0.8] {
            let cs = section(&strat(vec![Section::helix(8000, tpl(share))]));
            let f = cs.fill_factor();
            assert!((0.72..0.82).contains(&f), "{share}: {f}");
        }
    }

    #[test]
    fn long_sides_bulge() {
        let cs = section(&strat(vec![Section::helix(8000, tpl(0.6))]));
        let n = cs.turn_count();
        let side = cs.build_at(cs.side_station(), n);
        let end = cs.build_at(cs.end_station(), n);
        assert!(side > end * 1.05, "{side} vs {end}");
    }

    #[test]
    fn pitch_a_hair_under_one_wire_does_not_climb() {
        let cs = section(&strat(vec![Section::helix(8000, tpl(1.0) + 0.5)]));
        let f = cs.fill_factor();
        assert!(f > 0.85, "{f}");
        assert!(cs.build_mm() < 4.0, "{}", cs.build_mm());
    }

    #[test]
    fn wire_lag_rounds_the_reversals() {
        let reach = |span: f64| {
            let mut d = strat(vec![Section::helix(400, tpl(0.6))]);
            d.lay.free_span_mm = span;
            let cs = section(&d);
            let side = &cs.cuts[cs.side_station()];
            side[..93].iter().map(|t| t.y_mm).fold(0.0, f64::max)
        };
        let (tight, long) = (reach(0.0), reach(150.0));
        assert!(tight > long + 0.05, "{tight} vs {long}");
    }

    #[test]
    fn lower_turns_per_pass_lowers_capacitance() {
        let cap = |share| section(&strat(vec![Section::helix(8000, tpl(share))])).cap_pf();
        let (high, low) = (cap(1.0), cap(0.6));
        assert!(low < high * 0.8, "{low} vs {high}");
        // Strat-size coils measure roughly 80-150 pF.
        assert!((60.0..200.0).contains(&low) && (60.0..200.0).contains(&high));
    }

    #[test]
    fn dwell_piles_wire_at_the_flanges() {
        let edge = |cs: &CrossSection| {
            let p = &cs.profiles[cs.end_station()];
            p[1].max(p[p.len() - 2])
        };
        let plain = section(&strat(vec![Section::helix(2000, tpl(0.8))]));
        let mut d = strat(vec![Section::helix(2000, tpl(0.8))]);
        d.sections[0].dwell_turns = 3.0;
        let dwell = section(&d);
        assert!(
            edge(&dwell) > edge(&plain) * 1.1,
            "{} vs {}",
            edge(&dwell),
            edge(&plain)
        );
    }

    #[test]
    fn laid_summary_is_consistent() {
        let d = Design::example("strat").unwrap();
        let cs = section(&d);
        let l = cs.laid(&d);
        assert!(l.build_mm >= l.side_build_mm && l.build_mm >= l.end_build_mm);
        assert!((1.5..3.0).contains(&l.inductance_h), "{}", l.inductance_h);
        assert!(
            (5.0..15.0).contains(&l.resonance_khz),
            "{}",
            l.resonance_khz
        );
        assert!(matches!(d.sections[1].pattern, Pattern::Scatter { .. }));
    }
}
