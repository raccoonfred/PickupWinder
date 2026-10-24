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
}

struct Lay<'a> {
    section: &'a Section,
    od: f64,
    channel: f64,
    rng: Rng,
}

impl Lay<'_> {
    fn next_pass(&mut self) -> Pass {
        let s = self.section;
        let lo = s.inset_start_mm + self.od / 2.0;
        let hi = self.channel - s.inset_end_mm - self.od / 2.0;
        match s.pattern {
            Pattern::Layer { pitch_od } => Pass {
                lo,
                hi,
                pitch_mm: pitch_od * self.od,
            },
            Pattern::Scatter {
                pitch_min_od,
                pitch_max_od,
                inset_max_mm,
                ..
            } => Pass {
                lo: lo + self.rng.range(0.0, inset_max_mm),
                hi: hi - self.rng.range(0.0, inset_max_mm),
                pitch_mm: self.rng.range(pitch_min_od, pitch_max_od) * self.od,
            },
        }
    }
}

const EPS: f64 = 1e-9;

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

    let mut steps = Vec::new();
    let mut start_y = None;
    let mut y = 0.0;
    let mut dir = 1.0;
    let mut pass_no: u32 = 0;
    let mut section_start = 0.0;

    for (si, section) in design.sections.iter().enumerate() {
        let seed = match section.pattern {
            Pattern::Scatter { seed, .. } => seed,
            Pattern::Layer { .. } => 0,
        };
        let mut lay = Lay {
            section,
            od: wire.od_mm,
            channel: design.bobbin.channel_height_mm,
            rng: Rng::new(seed),
        };
        let mut pass = lay.next_pass();
        if start_y.is_none() {
            y = pass.lo;
            start_y = Some(y);
        } else if si > 0 {
            // A new section starts a fresh pass from wherever the guide is.
            pass_no += 1;
        }
        steps.push(Step::SectionStart {
            section: si,
            turns: section_start,
        });
        let rpm = section.rpm.unwrap_or(speed.rpm);
        let mut dwell_left = 0.0;
        let mut t = section_start;

        for k in 0..section.turns {
            // One turn per move, split where the guide reverses.
            let mut f = 1.0;
            while f > EPS {
                let t0 = t;
                if dwell_left > EPS {
                    let d = dwell_left.min(f);
                    t += d;
                    f -= d;
                    dwell_left -= d;
                } else {
                    let edge = if dir > 0.0 { pass.hi } else { pass.lo };
                    let room = (edge - y) * dir;
                    let need = pass.pitch_mm * f;
                    if need <= room + EPS {
                        y += dir * need;
                        t += f;
                        f = 0.0;
                    } else {
                        let used = (room.max(0.0) / pass.pitch_mm).min(f);
                        if used > EPS {
                            y = edge;
                            t += used;
                            f -= used;
                        }
                        // Reverse and start the next pass.
                        if t - t0 > EPS {
                            steps.push(Step::Move(Move {
                                turns: t,
                                y_mm: y,
                                rpm: rpm_at(rpm, (t0 + t) / 2.0),
                                section: si,
                                pass: pass_no,
                            }));
                        }
                        dir = -dir;
                        pass_no += 1;
                        pass = lay.next_pass();
                        dwell_left = section.dwell_turns;
                        continue;
                    }
                }
                if f <= EPS {
                    // Snap the end of the turn so rounding never accumulates.
                    t = section_start + (k + 1) as f64;
                }
                steps.push(Step::Move(Move {
                    turns: t,
                    y_mm: y,
                    rpm: rpm_at(rpm, (t0 + t) / 2.0),
                    section: si,
                    pass: pass_no,
                }));
            }
            t = section_start + (k + 1) as f64;
        }
        section_start += section.turns as f64;
        if let Some(msg) = &section.stop_after {
            steps.push(Step::Stop {
                section: si,
                turns: section_start,
                message: msg.clone(),
            });
        }
    }

    Ok(Toolpath {
        start_y_mm: start_y.unwrap_or(0.0),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Section;

    fn strat_layer(turns: u32) -> Design {
        let mut d = Design::example("strat").unwrap();
        d.sections = vec![Section::layer(turns, 1.0)];
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
    fn layer_pattern_lays_one_wire_per_turn() {
        let d = strat_layer(1000);
        let tp = generate(&d).unwrap();
        let od: f64 = 0.071;
        let (lo, hi) = (od / 2.0, 11.0 - od / 2.0);
        // Window / pitch = turns per pass.
        let per_pass = (hi - lo) / od;
        let st = tp.stats();
        assert_eq!(st.passes, (1000.0 / per_pass).ceil() as u32);
        assert!((st.min_y_mm - lo).abs() < 1e-9 && (st.max_y_mm - hi).abs() < 1e-9);
        let mut t = 0.0;
        let mut y = tp.start_y_mm;
        for m in tp.moves() {
            let dy = (m.y_mm - y).abs();
            let dt = m.turns - t;
            assert!((dy - od * dt).abs() < 1e-9, "pitch drift at {}", m.turns);
            t = m.turns;
            y = m.y_mm;
        }
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
        let d = strat_layer(1000);
        let tp = generate(&d).unwrap();
        let moves: Vec<_> = tp.moves().collect();
        assert!(moves[0].rpm < 70.0);
        assert!(moves.last().unwrap().rpm < 70.0);
        assert_eq!(moves[500].rpm, 420.0);
    }

    #[test]
    fn dwell_holds_the_guide_at_reversals() {
        let mut d = strat_layer(400);
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
