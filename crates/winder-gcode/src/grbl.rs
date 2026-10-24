//! Post-processor for GRBL 1.1.

use crate::profile::{Homing, MachineProfile};
use thiserror::Error;
use winder_core::{Design, Direction, Step, Toolpath, estimate};

#[derive(Debug, Error, PartialEq)]
pub enum PostError {
    #[error("section {section} runs at {rpm:.0} RPM, above this machine's limit of {limit:.0}")]
    TooFast {
        section: usize,
        rpm: f64,
        limit: f64,
    },
    #[error(transparent)]
    Design(#[from] winder_core::DesignError),
}

/// Moves shorter than this many turns are folded into the next line.
/// Coordinates are absolute, so nothing is lost.
const MIN_TURNS: f64 = 1e-4;

pub fn post_grbl(
    design: &Design,
    toolpath: &Toolpath,
    profile: &MachineProfile,
) -> Result<String, PostError> {
    let limit = profile.max_rpm.min(profile.grbl_max_rpm());
    if let Some(m) = toolpath.moves().find(|m| m.rpm > limit) {
        return Err(PostError::TooFast {
            section: m.section + 1,
            rpm: m.rpm,
            limit,
        });
    }
    let wire = design.wire_spec()?;
    let est = estimate(
        design.total_turns(),
        &design.bobbin,
        &wire,
        &design.estimate,
    );
    let stats = toolpath.stats();
    let sign = match design.direction {
        Direction::Cw => 1.0,
        Direction::Ccw => -1.0,
    };
    let off = profile.flange_offset_mm;

    let mut g = String::new();
    let mut line = |s: &str| {
        g.push_str(s);
        g.push('\n');
    };
    line("; Pickup Winder program for GRBL 1.1");
    line(&format!("; design: {}", ascii(&design.name, 60)));
    line(&format!(
        "; {} turns, {} sections, {} passes, {} wire, {}",
        design.total_turns(),
        design.sections.len(),
        stats.passes,
        wire.name,
        design.direction.label()
    ));
    line(&format!(
        "; estimate: {:.2} kOhm at {:.0} C, {:.0} m wire, {} layers",
        est.dcr_kohm, design.estimate.temp_c, est.length_m, est.layers
    ));
    line(&format!("; run time about {:.1} min", stats.minutes));
    line(&format!(
        "; machine: X = spindle turns ($100={}), Y = traverse mm ($101={})",
        profile.spindle_steps_per_turn, profile.traverse_steps_per_mm
    ));
    line("G21 G90 G94");
    match &profile.homing {
        Homing::Probe {
            toward_mm,
            feed_mm_min,
        } => {
            line("; find the traverse switch");
            line(&format!(
                "G38.2 Y{} F{}",
                num(*toward_mm, 3),
                num(*feed_mm_min, 1)
            ));
        }
        Homing::Cycle => line("$H"),
        Homing::None => line("; no homing: guide must sit at the reference"),
    }
    line("G92 X0 Y0");
    line(&format!("G0 Y{}", num(off + toolpath.start_y_mm, 4)));
    line("G93");

    let mut base = 0.0; // turns at the last G92 X0
    let mut last_t = 0.0; // turns at the last emitted line
    for step in &toolpath.steps {
        match step {
            Step::SectionStart { section, turns } => {
                let s = &design.sections[*section];
                line(&format!(
                    "; section {}: {}, {} turns",
                    section + 1,
                    s.pattern.label(),
                    s.turns
                ));
                if *turns > 0.0 {
                    line("G92 X0");
                    base = *turns;
                }
            }
            Step::Move(m) => {
                let dt = m.turns - last_t;
                if dt < MIN_TURNS {
                    continue;
                }
                // Inverse time: F = 1 / minutes for this line.
                let f = m.rpm / dt;
                line(&format!(
                    "G1 X{} Y{} F{}",
                    num(sign * (m.turns - base), 4),
                    num(off + m.y_mm, 4),
                    num(f, 3)
                ));
                last_t = m.turns;
            }
            Step::Stop { message, .. } => {
                line(&format!("M0 ({})", ascii(message, 40)));
            }
        }
    }
    line("G94");
    line("M2");
    Ok(g)
}

/// A number with at most `places` decimals and no trailing zeros.
fn num(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// Text safe inside a G-code comment.
fn ascii(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || " .,-_+/#:".contains(*c))
        .take(max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(1.0, 4), "1");
        assert_eq!(num(2.22100, 4), "2.221");
        assert_eq!(num(-0.00001, 4), "0");
        assert_eq!(num(420.0 / 0.5, 3), "840");
    }

    #[test]
    fn comment_text_is_sanitized() {
        assert_eq!(ascii("Solder (tap) ; now!", 40), "Solder tap  now");
    }

    #[test]
    fn refuses_speeds_above_the_limit() {
        let mut d = Design::example("strat").unwrap();
        d.speed.rpm = 1500.0;
        let tp = winder_core::generate(&d).unwrap();
        let err = post_grbl(&d, &tp, &MachineProfile::default()).unwrap_err();
        assert!(matches!(err, PostError::TooFast { .. }));
    }

    #[test]
    fn program_opening_lines() {
        let mut d = Design::example("strat").unwrap();
        d.sections[0].stop_after = Some("tap".into());
        let tp = winder_core::generate(&d).unwrap();
        let g = post_grbl(&d, &tp, &MachineProfile::default()).unwrap();
        let code: Vec<&str> = g.lines().filter(|l| !l.starts_with(';')).collect();
        assert_eq!(
            &code[..6],
            &[
                "G21 G90 G94",
                "G38.2 Y-20 F120",
                "G92 X0 Y0",
                "G0 Y10.0355",
                "G93",
                "G1 X1 Y10.0355 F63.6"
            ]
        );
        assert!(g.contains("\nM0 (tap)\n; section 2: scatter, 7600 turns\nG92 X0\n"));
        assert!(g.ends_with("G94\nM2\n"));
    }

    #[test]
    fn ccw_runs_the_spindle_backwards() {
        let mut d = Design::example("paf").unwrap();
        d.direction = Direction::Ccw;
        let tp = winder_core::generate(&d).unwrap();
        let g = post_grbl(&d, &tp, &MachineProfile::default()).unwrap();
        assert!(g.contains("\nG1 X-1 Y"));
    }
}
