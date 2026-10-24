//! Turns a design into the exact moves the machine makes.
//!
//! Coordinates: `turns` counts spindle revolutions from the start of the
//! wind; `y_mm` is the guide position measured from the inner face of the
//! first flange. The post-processor maps both onto machine axes.
//!
//! Every move ends exactly where the guide reverses, so a reversal never
//! falls in the middle of a G-code line.

use crate::{
    design::{Design, DesignError, Pattern, Section},
    rng::Rng,
    wire::WireSpec,
};

/// The spindle reaches `turns` and the guide reaches `y_mm` together, at a
/// steady `rpm`.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub turns: f64,
    pub y_mm: f64,
    pub rpm: f64,
    /// Index into `Design::sections`.
    pub section: usize,
    /// Guide pass counted from 0; a pass ends at each reversal.
    pub pass: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// A section begins (used to rebase the spindle axis and label output).
    SectionStart {
        section: usize,
        turns: f64,
    },
    Move(Move),
    /// Pause for the operator.
    Stop {
        section: usize,
        turns: f64,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toolpath {
    /// Where the guide sits before the first move.
    pub start_y_mm: f64,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub turns: f64,
    pub moves: usize,
    pub passes: u32,
    pub stops: usize,
    pub min_y_mm: f64,
    pub max_y_mm: f64,
    pub minutes: f64,
}

impl Toolpath {
    pub fn moves(&self) -> impl Iterator<Item = &Move> {
        self.steps.iter().filter_map(|s| match s {
            Step::Move(m) => Some(m),
            _ => None,
        })
    }

    pub fn stats(&self) -> Stats {
        let mut st = Stats {
            turns: 0.0,
            moves: 0,
            passes: 0,
            stops: 0,
            min_y_mm: self.start_y_mm,
            max_y_mm: self.start_y_mm,
            minutes: 0.0,
        };
        let mut t = 0.0;
        for s in &self.steps {
            match s {
                Step::Move(m) => {
                    st.moves += 1;
                    st.minutes += (m.turns - t) / m.rpm;
                    t = m.turns;
                    st.passes = st.passes.max(m.pass + 1);
                    st.min_y_mm = st.min_y_mm.min(m.y_mm);
                    st.max_y_mm = st.max_y_mm.max(m.y_mm);
                }
                Step::Stop { .. } => st.stops += 1,
                Step::SectionStart { .. } => {}
            }
        }
        st.turns = t;
        st
    }
}

/// Lay window and pitch for one guide pass.
struct Pass {
    lo: f64,
    hi: f64,
    pitch_mm: f64,
    /// Scatter's uneven hand: (share, cycles across the pass, phase) for two
    /// slow swings of the pitch.
    wander: [(f64, f64, f64); 2],
    /// Wander never slows the guide below one wire per turn, where turns
    /// would ride up on each other instead of bunching.
    min_pitch_mm: f64,
}

impl Pass {
    /// Pitch with the guide at `y`. Wander slows and hurries the guide
    /// along the pass, so turns bunch up in some places and spread out in
    /// others, as by hand; a steady pass has the same pitch everywhere.
    fn pitch_at(&self, y: f64) -> f64 {
        let x = ((y - self.lo) / (self.hi - self.lo).max(EPS)).clamp(0.0, 1.0);
        let swing: f64 = self
            .wander
            .iter()
            .map(|&(a, k, ph)| a * (std::f64::consts::TAU * k * x + ph).sin())
            .sum();
        if swing == 0.0 {
            return self.pitch_mm;
        }
        (self.pitch_mm * (1.0 + swing)).max(self.min_pitch_mm.min(self.pitch_mm))
    }
}

/// Window edges for a section, as guide positions (wire centres).
fn window(section: &Section, od: f64, channel: f64) -> (f64, f64) {
    (
        section.inset_start_mm + od / 2.0,
        channel - section.inset_end_mm - od / 2.0,
    )
}

/// Draws the next pass of a steady (helix or scatter) section.
struct Passes<'a> {
    section: &'a Section,
    od: f64,
    channel: f64,
    rng: Rng,
}

impl Passes<'_> {
    fn next(&mut self) -> Pass {
        let (lo, hi) = window(self.section, self.od, self.channel);
        let mut wander = [(0.0, 0.0, 0.0); 2];
        let (lo, hi, tpl) = match self.section.pattern {
            Pattern::Helix { tpl } => (lo, hi, tpl),
            Pattern::Scatter {
                tpl_min,
                tpl_max,
                reversal_jitter_mm,
                speed_wander,
                ..
            } => {
                let pass = (
                    lo + self.rng.range(0.0, reversal_jitter_mm),
                    hi - self.rng.range(0.0, reversal_jitter_mm),
                    self.rng.range(tpl_min, tpl_max),
                );
                if speed_wander > 0.0 {
                    // One slow swing and one quicker, each pass its own.
                    let tau = std::f64::consts::TAU;
                    wander = [
                        (
                            speed_wander * 0.65,
                            self.rng.range(0.5, 1.5),
                            self.rng.range(0.0, tau),
                        ),
                        (
                            speed_wander * 0.35,
                            self.rng.range(2.0, 4.0),
                            self.rng.range(0.0, tau),
                        ),
                    ];
                }
                pass
            }
            Pattern::Layer { .. } => unreachable!("layer sections step, they don't sweep"),
        };
        Pass {
            lo,
            hi,
            // Turns per pass over this pass's own window, edge to edge.
            pitch_mm: (hi - lo) / tpl,
            wander,
            min_pitch_mm: self.od,
        }
    }
}

const EPS: f64 = 1e-9;

/// Where the guide is between sections, and what it has emitted so far.
struct Cursor<'a> {
    steps: Vec<Step>,
    y: f64,
    dir: f64,
    pass: u32,
    rpm_at: &'a dyn Fn(f64, f64) -> f64,
}

impl Cursor<'_> {
    fn emit(&mut self, t0: f64, t: f64, rpm: f64, section: usize) {
        self.steps.push(Step::Move(Move {
            turns: t,
            y_mm: self.y,
            rpm: (self.rpm_at)(rpm, (t0 + t) / 2.0),
            section,
            pass: self.pass,
        }));
    }
}

pub fn generate(design: &Design) -> Result<Toolpath, DesignError> {
    design.validate()?;
    let wire: WireSpec = design.wire_spec()?;
    let total = design.total_turns() as f64;
    let speed = &design.speed;
    let rpm_at = |section_rpm: f64, t: f64| {
        let ramp = speed.ramp_turns;
        let mut r = section_rpm;
        if ramp > 0.0 {
            let start = speed.ramp_start_rpm.min(section_rpm);
            let edge = t.min(total - t).max(0.0);
            if edge < ramp {
                r = start + (section_rpm - start) * edge / ramp;
            }
        }
        r
    };

    let od = wire.od_mm;
    let channel = design.bobbin.channel_height_mm;
    let start_y = window(&design.sections[0], od, channel).0;
    let mut cur = Cursor {
        steps: Vec::new(),
        y: start_y,
        dir: 1.0,
        pass: 0,
        rpm_at: &rpm_at,
    };
    let mut section_start = 0.0;

    for (si, section) in design.sections.iter().enumerate() {
        if si > 0 {
            // A new section starts a fresh pass from wherever the guide is.
            cur.pass += 1;
        }
        cur.steps.push(Step::SectionStart {
            section: si,
            turns: section_start,
        });
        let rpm = section.rpm.unwrap_or(speed.rpm);
        match section.pattern {
            Pattern::Layer { step_fraction } => {
                let f = step_fraction
                    .unwrap_or(design.bobbin.end_mm() / design.bobbin.core_perimeter_mm());
                layered(&mut cur, si, section, od, channel, f, rpm, section_start);
            }
            Pattern::Helix { .. } | Pattern::Scatter { .. } => {
                let seed = match section.pattern {
                    Pattern::Scatter { seed, .. } => seed,
                    _ => 0,
                };
                let passes = Passes {
                    section,
                    od,
                    channel,
                    rng: Rng::new(seed),
                };
                steady(&mut cur, si, section, passes, rpm, section_start);
            }
        }
        section_start += section.turns as f64;
        if let Some(msg) = &section.stop_after {
            cur.steps.push(Step::Stop {
                section: si,
                turns: section_start,
                message: msg.clone(),
            });
        }
    }

    Ok(Toolpath {
        start_y_mm: start_y,
        steps: cur.steps,
    })
}

/// Perfect layers. Each turn the guide steps to the next wire position in
/// the first `f` of the turn, then holds still for the rest, so the wire
/// lies flat and square to the axis everywhere but the step zone. After
/// the last turn of a layer it climbs half a wire, into the groove beside
/// the flange, and the next layer comes back in the grooves of this one.
#[allow(clippy::too_many_arguments)]
fn layered(
    cur: &mut Cursor,
    si: usize,
    section: &Section,
    od: f64,
    channel: f64,
    f: f64,
    rpm: f64,
    section_start: f64,
) {
    let (lo, hi) = window(section, od, channel);
    // Turns per layer: the window holds T wires plus the half-wire offset
    // of the layer above.
    let per_layer = (((hi - lo) / od + 0.5).floor() as u32).max(1);
    // Start from the nearer edge, heading inward.
    let (mut base, mut dir) = if (cur.y - lo).abs() <= (hi - cur.y).abs() {
        (lo, 1.0)
    } else {
        (hi, -1.0)
    };
    let mut k = 0;
    for i in 0..section.turns {
        let t0 = section_start + i as f64;
        let target = base + dir * k as f64 * od;
        if (target - cur.y).abs() > EPS && f < 1.0 {
            cur.y = target;
            cur.emit(t0, t0 + f, rpm, si);
        } else {
            cur.y = target;
        }
        cur.emit(t0, t0 + 1.0, rpm, si);
        k += 1;
        if k == per_layer {
            base += dir * (per_layer as f64 - 0.5) * od;
            dir = -dir;
            k = 0;
            cur.pass += 1;
        }
    }
    cur.dir = dir;
}

/// A guide sweeping steadily between the window edges, at a pitch each pass
/// draws for itself. Moves are one turn each, split exactly at reversals.
fn steady(
    cur: &mut Cursor,
    si: usize,
    section: &Section,
    mut passes: Passes,
    rpm: f64,
    section_start: f64,
) {
    let mut pass = passes.next();
    let mut dwell_left = 0.0;
    let mut t = section_start;
    for k in 0..section.turns {
        let mut f = 1.0;
        while f > EPS {
            let t0 = t;
            if dwell_left > EPS {
                let d = dwell_left.min(f);
                t += d;
                f -= d;
                dwell_left -= d;
            } else {
                let edge = if cur.dir > 0.0 { pass.hi } else { pass.lo };
                let room = (edge - cur.y) * cur.dir;
                let pitch = pass.pitch_at(cur.y);
                let need = pitch * f;
                if need <= room + EPS {
                    cur.y += cur.dir * need;
                    t += f;
                    f = 0.0;
                } else {
                    let used = (room.max(0.0) / pitch).min(f);
                    if used > EPS {
                        cur.y = edge;
                        t += used;
                        f -= used;
                    }
                    // Reverse and start the next pass.
                    if t - t0 > EPS {
                        cur.emit(t0, t, rpm, si);
                    }
                    cur.dir = -cur.dir;
                    cur.pass += 1;
                    pass = passes.next();
                    dwell_left = section.dwell_turns;
                    continue;
                }
            }
            if f <= EPS {
                // Snap the end of the turn so rounding never accumulates.
                t = section_start + (k + 1) as f64;
            }
            cur.emit(t0, t, rpm, si);
        }
        t = section_start + (k + 1) as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Section;

    fn strat_helix(turns: u32, tpl: f64) -> Design {
        let mut d = Design::example("strat").unwrap();
        d.sections = vec![Section::helix(turns, tpl)];
        d
    }

    fn strat_layer(turns: u32) -> Design {
        let mut d = Design::example("strat").unwrap();
        d.sections = vec![Section::layer(turns)];
        d
    }

    #[test]
    fn turn_count_is_exact() {
        for preset in Bobbin::preset_names() {
            let d = Design::example(preset).unwrap();
            let tp = generate(&d).unwrap();
            assert_eq!(tp.stats().turns, d.total_turns() as f64, "{preset}");
        }
    }

    #[test]
    fn helix_crosses_the_window_in_tpl_turns() {
        let tpl = 100.0;
        let d = strat_helix(1000, tpl);
        let tp = generate(&d).unwrap();
        let od: f64 = 0.071;
        let (lo, hi) = (od / 2.0, 11.0 - od / 2.0);
        let pitch = (hi - lo) / tpl;
        let st = tp.stats();
        assert_eq!(st.passes, 10);
        assert!((st.min_y_mm - lo).abs() < 1e-9 && (st.max_y_mm - hi).abs() < 1e-9);
        let mut t = 0.0;
        let mut y = tp.start_y_mm;
        for m in tp.moves() {
            let dy = (m.y_mm - y).abs();
            let dt = m.turns - t;
            assert!((dy - pitch * dt).abs() < 1e-9, "pitch drift at {}", m.turns);
            t = m.turns;
            y = m.y_mm;
        }
    }

    #[test]
    fn layer_steps_one_wire_then_holds() {
        let d = strat_layer(400);
        let tp = generate(&d).unwrap();
        let od: f64 = 0.071;
        let per_layer = ((11.0 - od) / od + 0.5).floor();
        let f = d.bobbin.end_mm() / d.bobbin.core_perimeter_mm();
        let moves: Vec<_> = tp.moves().collect();
        // Turn 1 sits at the edge; every later turn is a step then a hold.
        assert_eq!(moves[0].turns, 1.0);
        let (step, hold) = (moves[1], moves[2]);
        assert!((step.turns - (1.0 + f)).abs() < 1e-9);
        assert!((step.y_mm - (od / 2.0 + od)).abs() < 1e-9);
        assert_eq!((hold.turns, hold.y_mm), (2.0, step.y_mm));
        // The first layer tops out one wire short of a full window, the climb
        // adds half a wire, and the second layer comes back half a wire in.
        let top = tp.stats().max_y_mm;
        assert!((top - (od / 2.0 + (per_layer - 0.5) * od)).abs() < 1e-9);
        assert!(top <= 11.0 - od / 2.0 + 1e-9);
        assert_eq!(tp.stats().passes, (400.0 / per_layer).ceil() as u32);
        assert_eq!(tp.stats().turns, 400.0);
    }

    #[test]
    fn guide_never_leaves_the_channel() {
        for seed in 0..50 {
            let mut d = Design::example("paf").unwrap();
            if let Pattern::Scatter { seed: s, .. } = &mut d.sections[1].pattern {
                *s = seed;
            }
            d.sections[1].dwell_turns = 0.5;
            let tp = generate(&d).unwrap();
            let st = tp.stats();
            assert!(st.min_y_mm >= 0.071 / 2.0 - 1e-9, "seed {seed}");
            assert!(st.max_y_mm <= 6.5 - 0.071 / 2.0 + 1e-9, "seed {seed}");
            assert_eq!(st.turns, 5000.0);
        }
    }

    /// Spread of the guide's speed within passes, as a share of its mean.
    fn speed_spread(d: &Design) -> f64 {
        let tp = generate(d).unwrap();
        let moves: Vec<_> = tp.moves().collect();
        let mut spread = Vec::new();
        let mut speeds: Vec<f64> = Vec::new();
        for w in moves.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a.pass != b.pass {
                if speeds.len() > 20 {
                    let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
                    let var = speeds.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / speeds.len() as f64;
                    spread.push(var.sqrt() / mean);
                }
                speeds.clear();
                continue;
            }
            let dt = b.turns - a.turns;
            if dt > 0.5 {
                speeds.push((b.y_mm - a.y_mm).abs() / dt);
            }
        }
        spread.iter().sum::<f64>() / spread.len().max(1) as f64
    }

    #[test]
    fn scatter_wanders_within_a_pass_and_helix_does_not() {
        let helix = strat_helix(3000, 100.0);
        assert!(speed_spread(&helix) < 0.01, "{}", speed_spread(&helix));
        let mut scatter = helix.clone();
        scatter.sections[0].pattern = Pattern::Scatter {
            tpl_min: 100.0,
            tpl_max: 100.0,
            reversal_jitter_mm: 0.0,
            speed_wander: crate::design::SPEED_WANDER,
            seed: 3,
        };
        let s = speed_spread(&scatter);
        assert!(s > 0.12, "{s}");
        // Without wander, scatter's passes are steady sweeps again.
        if let Pattern::Scatter { speed_wander, .. } = &mut scatter.sections[0].pattern {
            *speed_wander = 0.0;
        }
        assert!(speed_spread(&scatter) < 0.01);
    }

    #[test]
    fn same_seed_same_path_other_seed_differs() {
        let d = Design::example("strat").unwrap();
        assert_eq!(generate(&d).unwrap(), generate(&d).unwrap());
        let mut other = d.clone();
        if let Pattern::Scatter { seed, .. } = &mut other.sections[1].pattern {
            *seed += 1;
        }
        assert_ne!(generate(&d).unwrap(), generate(&other).unwrap());
    }

    #[test]
    fn ramps_up_and_down() {
        let d = strat_helix(1000, 120.0);
        let tp = generate(&d).unwrap();
        let moves: Vec<_> = tp.moves().collect();
        assert!(moves[0].rpm < 70.0);
        assert!(moves.last().unwrap().rpm < 70.0);
        assert_eq!(moves[500].rpm, 420.0);
    }

    #[test]
    fn dwell_holds_the_guide_at_reversals() {
        let mut d = strat_helix(400, 120.0);
        d.sections[0].dwell_turns = 2.0;
        let tp = generate(&d).unwrap();
        let held = tp
            .moves()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0].y_mm == w[1].y_mm)
            .count();
        assert!(held >= 4, "{held}");
        assert_eq!(tp.stats().turns, 400.0);
    }

    #[test]
    fn stop_points_are_emitted() {
        let mut d = Design::example("strat").unwrap();
        d.sections[0].stop_after = Some("solder tap".into());
        let tp = generate(&d).unwrap();
        assert!(
            tp.steps
                .iter()
                .any(|s| matches!(s, Step::Stop { turns, .. } if *turns == 200.0))
        );
    }

    use crate::Bobbin;
}
