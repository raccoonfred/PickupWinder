//! Reads a program back and checks it against the design that produced it.
//!
//! Only the G-code subset the post-processors write is understood; anything
//! else is an error, so a hand-edited file can't slip past unnoticed.

use crate::profile::{Homing, MachineProfile};
use thiserror::Error;
use winder_core::{Design, Toolpath};

/// GRBL's line buffer (excluding comments).
const MAX_LINE: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub line_no: usize,
    /// Machine coordinates at the end of the line.
    pub x: f64,
    pub y: f64,
    pub rapid: bool,
    /// Duration in minutes (inverse-time moves only).
    pub minutes: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub moves: Vec<Line>,
    pub stops: usize,
    pub homed: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum VerifyError {
    #[error("line {0}: {1}")]
    Syntax(usize, String),
    #[error("line {0} is {1} characters; GRBL accepts {MAX_LINE}")]
    TooLong(usize, usize),
    #[error("line {line}: guide at {y:.4} mm, outside the lay window {lo:.4} to {hi:.4} mm")]
    OutsideWindow {
        line: usize,
        y: f64,
        lo: f64,
        hi: f64,
    },
    #[error("line {0}: spindle reverses direction")]
    SpindleReverses(usize),
    #[error("program winds {got:.4} turns; the design asks for {want}")]
    TurnCount { got: f64, want: u32 },
    #[error("program has {got} stop points; the design has {want}")]
    Stops { got: usize, want: usize },
    #[error("program runs {got:.2} min; the toolpath says {want:.2} min")]
    Duration { got: f64, want: f64 },
    #[error("program never homes the traverse")]
    NotHomed,
}

pub fn parse(text: &str) -> Result<Program, VerifyError> {
    let mut p = Program::default();
    let (mut mx, mut my) = (0.0f64, 0.0f64); // machine position
    let (mut ox, mut oy) = (0.0f64, 0.0f64); // G92 offsets: work = machine - offset
    let mut inverse_time = false;
    let mut motion: Option<u8> = None;

    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let code = strip_comments(raw);
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if code.len() > MAX_LINE {
            return Err(VerifyError::TooLong(n, code.len()));
        }
        if code == "$H" {
            my = 0.0;
            p.homed = true;
            continue;
        }
        let words = words(code).map_err(|e| VerifyError::Syntax(n, e))?;
        let get = |l: char| words.iter().find(|(c, _)| *c == l).map(|(_, v)| *v);
        let gs: Vec<f64> = words
            .iter()
            .filter(|(c, _)| *c == 'G')
            .map(|(_, v)| *v)
            .collect();
        let mut is_g92 = false;
        let mut probe = false;
        for g in &gs {
            // G-codes in tenths, so G38.2 compares exactly.
            match (g * 10.0).round() as i64 {
                0 => motion = Some(0),
                10 => motion = Some(1),
                210 | 900 => {}
                930 => inverse_time = true,
                940 => inverse_time = false,
                920 => is_g92 = true,
                382 => probe = true,
                _ => return Err(VerifyError::Syntax(n, format!("unsupported G{g}"))),
            }
        }
        for (c, v) in &words {
            match c {
                'M' if *v == 0.0 => p.stops += 1,
                'M' if *v == 2.0 => {}
                'G' | 'X' | 'Y' | 'F' => {}
                _ => return Err(VerifyError::Syntax(n, format!("unsupported word {c}{v}"))),
            }
        }
        let (x, y) = (get('X'), get('Y'));
        if is_g92 {
            if let Some(v) = x {
                ox = mx - v;
            }
            if let Some(v) = y {
                oy = my - v;
            }
            continue;
        }
        if probe {
            // The switch defines the reference: call it machine Y = 0.
            my = 0.0;
            p.homed = true;
            continue;
        }
        if x.is_none() && y.is_none() {
            continue;
        }
        let nx = x.map_or(mx, |v| v + ox);
        let ny = y.map_or(my, |v| v + oy);
        let rapid = match motion {
            Some(0) => true,
            Some(1) => false,
            _ => return Err(VerifyError::Syntax(n, "move without G0/G1".into())),
        };
        let minutes = if rapid {
            0.0
        } else if inverse_time {
            let f = get('F').ok_or_else(|| VerifyError::Syntax(n, "G93 move without F".into()))?;
            if f <= 0.0 {
                return Err(VerifyError::Syntax(n, "F must be positive".into()));
            }
            1.0 / f
        } else {
            return Err(VerifyError::Syntax(
                n,
                "feed moves must be inverse-time (G93)".into(),
            ));
        };
        mx = nx;
        my = ny;
        p.moves.push(Line {
            line_no: n,
            x: mx,
            y: my,
            rapid,
            minutes,
        });
    }
    Ok(p)
}

/// Re-reads `gcode` and checks it lays exactly what `toolpath` describes.
pub fn verify(
    gcode: &str,
    design: &Design,
    toolpath: &Toolpath,
    profile: &MachineProfile,
) -> Result<Program, VerifyError> {
    let p = parse(gcode)?;
    if !p.homed && profile.homing != Homing::None {
        return Err(VerifyError::NotHomed);
    }
    let od = design
        .wire_spec()
        .map_err(|e| VerifyError::Syntax(0, e.to_string()))?
        .od_mm;
    let tol = 1e-3;
    let lo = profile.flange_offset_mm + od / 2.0 - tol;
    let hi = profile.flange_offset_mm + design.bobbin.channel_height_mm - od / 2.0 + tol;
    let step = 1.0 / profile.spindle_steps_per_turn;

    let mut x = 0.0;
    let mut turned = 0.0;
    let mut sign = 0.0;
    let mut minutes = 0.0;
    for m in &p.moves {
        if m.rapid {
            continue;
        }
        if !(lo..=hi).contains(&m.y) {
            return Err(VerifyError::OutsideWindow {
                line: m.line_no,
                y: m.y,
                lo,
                hi,
            });
        }
        let dx = m.x - x;
        if dx.abs() > step / 2.0 {
            let s = dx.signum();
            if sign != 0.0 && s != sign {
                return Err(VerifyError::SpindleReverses(m.line_no));
            }
            sign = s;
        }
        turned += dx.abs();
        x = m.x;
        minutes += m.minutes;
    }
    let want = design.total_turns();
    if (turned - want as f64).abs() > step {
        return Err(VerifyError::TurnCount { got: turned, want });
    }
    let want_stops = design
        .sections
        .iter()
        .filter(|s| s.stop_after.is_some())
        .count();
    if p.stops != want_stops {
        return Err(VerifyError::Stops {
            got: p.stops,
            want: want_stops,
        });
    }
    let want_min = toolpath.stats().minutes;
    if (minutes - want_min).abs() > want_min * 0.005 {
        return Err(VerifyError::Duration {
            got: minutes,
            want: want_min,
        });
    }
    Ok(p)
}

fn strip_comments(line: &str) -> String {
    let line = line.split(';').next().unwrap_or("");
    let mut out = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn words(code: &str) -> Result<Vec<(char, f64)>, String> {
    let mut out = Vec::new();
    let mut chars = code.chars().filter(|c| !c.is_whitespace()).peekable();
    while let Some(c) = chars.next() {
        let letter = c.to_ascii_uppercase();
        if !letter.is_ascii_alphabetic() {
            return Err(format!("unexpected '{c}'"));
        }
        let mut s = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() || d == '.' || d == '-' || d == '+' {
                s.push(d);
                chars.next();
            } else {
                break;
            }
        }
        let v: f64 = s
            .parse()
            .map_err(|_| format!("bad number after {letter}"))?;
        out.push((letter, v));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post_grbl;
    use winder_core::{Bobbin, Design, Direction, Pattern, generate, recipe};

    fn round_trip(d: &Design, profile: &MachineProfile) -> Program {
        let tp = generate(d).unwrap();
        let g = post_grbl(d, &tp, profile).unwrap();
        verify(&g, d, &tp, profile).unwrap_or_else(|e| panic!("{}: {e}", d.name))
    }

    #[test]
    fn every_preset_round_trips() {
        for preset in Bobbin::preset_names() {
            for dir in [Direction::Cw, Direction::Ccw] {
                let mut d = Design::example(preset).unwrap();
                d.direction = dir;
                d.sections[0].stop_after = Some("check".into());
                d.sections[1].dwell_turns = 1.5;
                round_trip(&d, &MachineProfile::default());
            }
        }
    }

    #[test]
    fn scatter_seeds_round_trip() {
        for seed in 0..20 {
            let mut d = Design::example("p90").unwrap();
            if let Pattern::Scatter { seed: s, .. } = &mut d.sections[1].pattern {
                *s = seed;
            }
            round_trip(&d, &MachineProfile::default());
        }
    }

    #[test]
    fn imported_recipes_round_trip_with_flange_offset() {
        let profile = MachineProfile {
            flange_offset_mm: 2.15,
            ..Default::default()
        };
        let json = include_str!("../../../app/winder_recipes.json");
        for i in recipe::import_recipes(json).unwrap() {
            round_trip(&i.design, &profile);
        }
    }

    #[test]
    fn catches_a_guide_outside_the_window() {
        let d = Design::example("strat").unwrap();
        let tp = generate(&d).unwrap();
        let profile = MachineProfile::default();
        let g = post_grbl(&d, &tp, &profile)
            .unwrap()
            .replacen("G1 X1 Y0.0355", "G1 X1 Y11.5", 1);
        assert!(matches!(
            verify(&g, &d, &tp, &profile),
            Err(VerifyError::OutsideWindow { .. })
        ));
    }

    #[test]
    fn catches_a_missing_turn() {
        let d = Design::example("strat").unwrap();
        let tp = generate(&d).unwrap();
        let profile = MachineProfile::default();
        let g = post_grbl(&d, &tp, &profile).unwrap();
        let last_move = g.lines().rfind(|l| l.starts_with("G1 ")).unwrap();
        let g = g.replace(&format!("{last_move}\n"), "");
        assert!(matches!(
            verify(&g, &d, &tp, &profile),
            Err(VerifyError::TurnCount { .. })
        ));
    }

    #[test]
    fn rejects_unknown_codes() {
        assert!(matches!(parse("G2 X1 Y1"), Err(VerifyError::Syntax(1, _))));
        assert!(matches!(parse("S1000 M3"), Err(VerifyError::Syntax(1, _))));
    }
}
