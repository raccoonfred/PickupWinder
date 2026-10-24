//! Settings that follow other settings, so a design stays what you meant
//! when you change part of it.
//!
//! - **Keep the lay.** Turns per pass only mean something against a wire
//!   and a window: 123 TPL is a 1.2-wire pitch with 42 AWG on a Strat
//!   bobbin, but a 0.9-wire pitch with 43 AWG. When the wire, the channel or
//!   a section's insets change, each section's TPL is scaled so its pitch in
//!   wire diameters stays the same, and each perfect-layer section keeps its
//!   number of layers.
//! - **Target DCR.** Set the resistance you want and one section, the fill
//!   section, gets the turns that reach it on the laid coil. The others keep
//!   theirs, so a perfect-layer base and cover stay as they are.

use crate::{
    design::{Design, DesignError, Pattern},
    lay::CrossSection,
    toolpath::generate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auto {
    /// Keep each section's pitch and layer count when the wire or the
    /// window changes.
    pub keep_lay: bool,
    /// Solve the fill section's turns to give this DCR (kΩ).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dcr_kohm: Option<f64>,
    /// Which section takes up the difference (1-based). Default: the one
    /// with the most turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_section: Option<usize>,
}

impl Default for Auto {
    fn default() -> Self {
        Auto {
            keep_lay: true,
            target_dcr_kohm: None,
            fill_section: None,
        }
    }
}

impl Auto {
    pub fn is_default(&self) -> bool {
        *self == Auto::default()
    }
}

/// Turns side by side across a section's window: the TPL of a pitch of one
/// wire.
fn side_by_side(d: &Design, i: usize, od: f64) -> f64 {
    (d.window_mm(&d.sections[i], od) / od).max(1.0)
}

/// What `follow` changed, for the status line.
#[derive(Debug, Clone, PartialEq)]
pub struct Followed {
    pub section: usize,
    pub what: String,
}

/// After an edit from `old` to `new`, scale each section's turns per pass
/// (and each perfect-layer section's turns) so the lay stays the same if
/// the wire or the window changed. Sections are matched by position and
/// left alone where the edit changed their pattern itself.
pub fn follow(old: &Design, new: &mut Design) -> Vec<Followed> {
    let mut out = Vec::new();
    if !new.auto.keep_lay || old.sections.len() != new.sections.len() {
        return out;
    }
    let (Ok(w0), Ok(w1)) = (old.wire_spec(), new.wire_spec()) else {
        return out;
    };
    for i in 0..new.sections.len() {
        let (a, b) = (
            side_by_side(old, i, w0.od_mm),
            side_by_side(new, i, w1.od_mm),
        );
        if (a - b).abs() < 1e-9 || old.sections[i].pattern != new.sections[i].pattern {
            continue;
        }
        let k = b / a;
        let s = &mut new.sections[i];
        let scale = |v: &mut f64| *v = (*v * k).round().max(1.0);
        let what = match &mut s.pattern {
            Pattern::Layer { .. } => {
                // Same number of layers: turns per layer went from a to b.
                let before = s.turns;
                s.turns = ((s.turns as f64 * k).round() as u32).max(1);
                format!("{before} → {} turns (same layers)", s.turns)
            }
            Pattern::Helix { tpl } => {
                let before = *tpl;
                scale(tpl);
                format!("TPL {before:.0} → {tpl:.0} (same pitch)")
            }
            Pattern::Scatter {
                tpl_min, tpl_max, ..
            } => {
                let before = (*tpl_min, *tpl_max);
                scale(tpl_min);
                scale(tpl_max);
                format!(
                    "TPL {:.0}–{:.0} → {tpl_min:.0}–{tpl_max:.0} (same pitch)",
                    before.0, before.1
                )
            }
        };
        out.push(Followed { section: i, what });
    }
    out
}

/// The section a target DCR adjusts (0-based).
pub fn fill_index(d: &Design) -> usize {
    d.auto
        .fill_section
        .and_then(|n| n.checked_sub(1))
        .filter(|&i| i < d.sections.len())
        .unwrap_or_else(|| {
            (0..d.sections.len())
                .max_by_key(|&i| d.sections[i].turns)
                .unwrap_or(0)
        })
}

/// DCR of the laid coil (kΩ).
pub fn laid_dcr(d: &Design) -> Result<f64, DesignError> {
    let tp = generate(d)?;
    let wire = d.wire_spec()?;
    Ok(CrossSection::build(d, &tp, &wire).laid(d).dcr_kohm)
}

/// Close enough: a few turns of pickup wire.
const TOLERANCE_KOHM: f64 = 0.003;

/// How a target was met.
#[derive(Debug, Clone, PartialEq)]
pub struct Solved {
    pub section: usize,
    pub turns_before: u32,
    pub dcr_kohm: f64,
}

/// Set the fill section's turns so the laid coil reaches the target DCR.
/// Does nothing without a target. DCR grows a little faster than turns
/// (each layer is longer than the last), so this steps by the secant and
/// settles in two or three lays.
pub fn solve(d: &mut Design) -> Result<Option<Solved>, DesignError> {
    let Some(target) = d.auto.target_dcr_kohm else {
        return Ok(None);
    };
    if !(target > 0.0 && target.is_finite()) {
        return Err(DesignError::Invalid(
            "auto.target_dcr_kohm must be positive".into(),
        ));
    }
    let i = fill_index(d);
    let before = d.sections[i].turns;
    let others = d.total_turns() - before;
    let mut n0 = before as f64;
    let mut r0 = laid_dcr(d)?;
    if (r0 - target).abs() < TOLERANCE_KOHM {
        return Ok(Some(Solved {
            section: i,
            turns_before: before,
            dcr_kohm: r0,
        }));
    }
    // First guess: resistance in proportion to all the turns.
    let total = (others as f64 + n0) * target / r0;
    let mut n1 = (total - others as f64).max(1.0);
    let mut r1 = r0;
    for _ in 0..6 {
        let n = n1.round().max(1.0) as u32;
        if n == d.sections[i].turns && r1 != r0 {
            break;
        }
        d.sections[i].turns = n;
        r1 = laid_dcr(d)?;
        if (r1 - target).abs() < TOLERANCE_KOHM || n == 1 {
            break;
        }
        let slope = if (n as f64 - n0).abs() > 0.5 {
            (r1 - r0) / (n as f64 - n0)
        } else {
            r1 / (others as f64 + n as f64)
        };
        let next = n as f64 + (target - r1) / slope.max(1e-9);
        n0 = n as f64;
        r0 = r1;
        n1 = next.max(1.0);
    }
    Ok(Some(Solved {
        section: i,
        turns_before: before,
        dcr_kohm: r1,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::WireRef;

    #[test]
    fn thinner_wire_keeps_pitch_and_layers() {
        let old = Design::example("strat").unwrap();
        let mut new = old.clone();
        new.wire = WireRef::Named("43 AWG".into());
        let changed = follow(&old, &mut new);
        assert_eq!(changed.len(), 3);
        let od0 = old.wire_spec().unwrap().od_mm;
        let od1 = new.wire_spec().unwrap().od_mm;
        for i in 0..3 {
            let pitch = |d: &Design, od: f64| match &d.sections[i].pattern {
                Pattern::Scatter { tpl_min, .. } => d.window_mm(&d.sections[i], od) / tpl_min / od,
                _ => d.sections[i].turns as f64 / side_by_side(d, i, od),
            };
            let (a, b) = (pitch(&old, od0), pitch(&new, od1));
            assert!((a - b).abs() / a < 0.01, "section {i}: {a} vs {b}");
        }
        // Editing a pattern by hand is left as typed.
        let mut hand = old.clone();
        hand.wire = WireRef::Named("43 AWG".into());
        hand.sections[1].pattern = Pattern::Helix { tpl: 99.0 };
        follow(&old, &mut hand);
        assert_eq!(hand.sections[1].pattern, Pattern::Helix { tpl: 99.0 });
    }

    #[test]
    fn nothing_follows_when_switched_off_or_unchanged() {
        let old = Design::example("strat").unwrap();
        let mut same = old.clone();
        same.speed.rpm = 900.0;
        assert!(follow(&old, &mut same).is_empty());
        let mut off = old.clone();
        off.auto.keep_lay = false;
        off.bobbin.channel_height_mm = 9.0;
        assert!(follow(&old, &mut off).is_empty());
    }

    #[test]
    fn target_dcr_sets_the_fill_section() {
        let mut d = Design::example("strat").unwrap();
        d.auto.target_dcr_kohm = Some(6.5);
        let s = solve(&mut d).unwrap().unwrap();
        assert_eq!(s.section, 1);
        assert!((s.dcr_kohm - 6.5).abs() < 0.01, "{}", s.dcr_kohm);
        assert!(d.sections[1].turns > s.turns_before);
        assert_eq!(d.sections[0].turns, 200);
        // Solving again changes nothing.
        let again = d.clone();
        solve(&mut d).unwrap();
        assert_eq!(d, again);
    }
}
