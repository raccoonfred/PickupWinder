//! Flange finder: a program that touches a feeler on the guide against both
//! flanges of the bobbin on the machine, and the arithmetic that turns GRBL's
//! probe reports into the machine's flange offset and the bobbin's real
//! winding window.
//!
//! The feeler is a thin blade under the guide eyelet, wired to GRBL's probe
//! input beside the traverse switch, so either one stops a `G38.2` move.
//! GRBL reports each stop as `[PRB:x,y,z:1]` in machine coordinates; the
//! first report (the traverse switch) is the reference for the others.
//!
//! In the automatic program a solenoid on the flood-coolant output (`M8`,
//! A3 on the shield) drops the feeler, and a Hall sensor over the platen,
//! also on the probe input, finds an index magnet so the spindle can turn a
//! bobbin end toward the guide. Nothing needs a hand between the two. The
//! Hall sensor takes its power from the spindle PWM output (D11, `M3 S1000`
//! is a steady 5 V), so it is live only for the index search and can never
//! hold the probe input during homing or another touch.

use crate::profile::{Homing, MachineProfile};
use thiserror::Error;
use winder_core::Design;

/// How far past the expected flange each search may go (mm).
const OVERSHOOT_MM: f64 = 3.0;
/// Feed for touching a flange (mm/min): slow, so the stop is repeatable.
const TOUCH_FEED: f64 = 30.0;
/// Back-off after a touch, so the next `G38.2` starts with the probe open.
const BACK_OFF_MM: f64 = 0.5;
/// Turn off the index magnet first, so its search starts with the probe open.
const INDEX_CLEAR_TURNS: f64 = 0.3;
/// How far the index search may turn the spindle.
const INDEX_SEARCH_TURNS: f64 = 1.2;
/// From the index to a bobbin end facing the guide: the nests hold the
/// bobbin's length in line with the magnet, and the guide sits a quarter turn
/// round from the sensor.
const INDEX_TO_GUIDE_TURNS: f64 = 0.25;
/// Index search speed, turns/min.
const INDEX_FEED: f64 = 60.0;

#[derive(Debug, Error, PartialEq)]
pub enum CalibrateError {
    #[error(
        "the flange finder needs probe homing (homing kind = \"probe\") in the machine profile"
    )]
    NeedsProbe,
    #[error(
        "flange_offset_mm is {0} mm; set it roughly (with a ruler, to within 2 mm) so the near-flange search stays clear of the traverse switch"
    )]
    OffsetTooSmall(f64),
    #[error(
        "expected 3 probe reports (switch, near flange, far flange), or 4 with the index, found {0}"
    )]
    ProbeCount(usize),
    #[error(
        "the near-flange touch is {0:.3} mm from the switch: the feeler hit the switch or was not swung in"
    )]
    NearTouch(f64),
    #[error("the flanges came out {0:.3} mm apart, which is not a bobbin window")]
    Window(f64),
}

/// What a flange-finder run measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Switch to the inner face of the near flange, for `flange_offset_mm`.
    pub flange_offset_mm: f64,
    /// Inner face to inner face, the bobbin's real winding window.
    pub window_mm: f64,
}

/// The flange-finder program for this design's bobbin on this machine.
/// `feeler_mm` is the blade's thickness along the traverse. With `auto` the
/// machine orients the bobbin and moves the feeler itself; without it the
/// program stops for you to do both.
pub fn flange_program(
    design: &Design,
    profile: &MachineProfile,
    feeler_mm: f64,
    auto: bool,
) -> Result<String, CalibrateError> {
    let Homing::Probe {
        toward_mm,
        feed_mm_min,
    } = profile.homing
    else {
        return Err(CalibrateError::NeedsProbe);
    };
    let off = profile.flange_offset_mm;
    if off < OVERSHOOT_MM + 1.0 {
        return Err(CalibrateError::OffsetTooSmall(off));
    }
    let h = design.bobbin.channel_height_mm;
    let mid = off + h / 2.0;
    let mut lines = vec![
        "; Pickup Winder flange finder for GRBL 1.1".to_string(),
        format!(
            "; bobbin window {} mm by design, feeler {} mm",
            num(h),
            num(feeler_mm)
        ),
        format!(
            "; Send this with the bobbin on the platen. It reports {}",
            if auto { "four" } else { "three" }
        ),
        "; [PRB:...] lines; paste them into `winder calibrate --probes`.".into(),
        "G21 G90 G94".into(),
        "; 1. find the traverse switch".into(),
        format!("G38.2 Y{} F{}", num(toward_mm), num(feed_mm_min)),
        "G92 X0 Y0".into(),
        "; 2. guide to the middle of the window, feeler still up".into(),
        format!("G0 Y{}", num(mid)),
    ];
    if auto {
        lines.extend([
            "; find the index magnet, then turn a bobbin end to the guide".to_string(),
            format!("G0 X{}", num(INDEX_CLEAR_TURNS)),
            "M3 S1000 (index sensor on)".into(),
            "G4 P0.2".into(),
            format!(
                "G38.2 X{} F{}",
                num(INDEX_CLEAR_TURNS + INDEX_SEARCH_TURNS),
                num(INDEX_FEED)
            ),
            "M5 (index sensor off)".into(),
            format!("G91 G0 X{}", num(INDEX_TO_GUIDE_TURNS)),
            "G90".into(),
            "M8 (feeler down)".into(),
            "G4 P0.5".into(),
        ]);
    } else {
        lines.push("M0 (turn a bobbin end to the guide, swing the feeler in)".into());
    }
    lines.extend([
        "; 3. touch the near flange".to_string(),
        format!("G38.2 Y{} F{}", num(off - OVERSHOOT_MM), num(TOUCH_FEED)),
        format!("G91 G0 Y{}", num(BACK_OFF_MM)),
        "G90".into(),
        "; 4. touch the far flange".into(),
        format!(
            "G38.2 Y{} F{}",
            num(off + h + OVERSHOOT_MM),
            num(TOUCH_FEED)
        ),
        format!("G91 G0 Y{}", num(-BACK_OFF_MM)),
        "G90".into(),
        format!("G0 Y{}", num(mid)),
    ]);
    if auto {
        lines.extend(["M9 (feeler up)".to_string(), "G4 P0.5".into()]);
    } else {
        lines.push("M0 (swing the feeler out)".into());
    }
    lines.push("M2".into());
    Ok(lines.join("\n") + "\n")
}

/// Pull the Y values out of GRBL `[PRB:x,y,z:1]` reports, in order. Reports
/// that did not trip (`:0`) are skipped. Anything else in the text, such as
/// the rest of a console log, is ignored.
pub fn probe_ys(text: &str) -> Vec<f64> {
    let mut ys = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("[PRB:") {
        rest = &rest[i + 5..];
        let Some(end) = rest.find(']') else { break };
        let body = &rest[..end];
        let (coords, ok) = body.rsplit_once(':').unwrap_or((body, "1"));
        if ok.trim() == "1"
            && let Some(y) = coords.split(',').nth(1).and_then(|v| v.trim().parse().ok())
        {
            ys.push(y);
        }
        rest = &rest[end..];
    }
    ys
}

/// Work out the flange offset and window from the probe reports (switch,
/// near flange, far flange), in GRBL machine coordinates. The automatic
/// program's index report, second of four, plays no part.
pub fn solve(ys: &[f64], feeler_mm: f64) -> Result<Calibration, CalibrateError> {
    let (switch, near, far) = match ys {
        [s, n, f] | [s, _, n, f] => (s, n, f),
        _ => return Err(CalibrateError::ProbeCount(ys.len())),
    };
    // The blade touches the near flange with its near face, so its middle,
    // which is in line with the eyelet, stops half a blade inside the window.
    let near_rel = near - switch;
    if near_rel < 1.0 {
        return Err(CalibrateError::NearTouch(near_rel));
    }
    let window = far - near + feeler_mm;
    if !(1.0..=60.0).contains(&window) {
        return Err(CalibrateError::Window(window));
    }
    Ok(Calibration {
        flange_offset_mm: near_rel - feeler_mm / 2.0,
        window_mm: window,
    })
}

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strat() -> Design {
        Design::pickup("strat").unwrap()
    }

    #[test]
    fn program_searches_both_flanges_from_the_middle() {
        let p = MachineProfile::default();
        let g = flange_program(&strat(), &p, 0.5, false).unwrap();
        let h = strat().bobbin.channel_height_mm;
        let off = p.flange_offset_mm;
        assert!(g.contains("G38.2 Y-20 F120"));
        assert!(g.contains(&format!("G0 Y{}", num(off + h / 2.0))));
        assert!(g.contains(&format!("G38.2 Y{} F30", num(off - 3.0))));
        assert!(g.contains(&format!("G38.2 Y{} F30", num(off + h + 3.0))));
        // Every probe move but the first starts with the probe open.
        let backs = g.lines().filter(|l| l.starts_with("G91 G0 Y")).count();
        assert_eq!(backs, 2);
        assert_eq!(g.matches("\nM0 (").count(), 2);
        assert!(g.trim_end().ends_with("M2"));
    }

    #[test]
    fn automatic_program_needs_no_hands() {
        let g = flange_program(&strat(), &MachineProfile::default(), 0.5, true).unwrap();
        assert!(!g.contains("M0"));
        // Off the magnet, find it, a quarter turn, feeler down before the
        // first touch and up after the last.
        let order = [
            "G0 X0.3",
            "M3 S1000",
            "G38.2 X1.5 F60",
            "M5",
            "G91 G0 X0.25",
            "M8",
            "G38.2 Y7",
            "M9",
        ];
        let mut at = 0;
        for o in order {
            at += g[at..]
                .find(o)
                .unwrap_or_else(|| panic!("{o} missing or out of order"));
        }
        // The index search starts away from the traverse switch.
        assert!(g.find("G0 Y15.5").unwrap() < g.find("G38.2 X").unwrap());
    }

    #[test]
    fn program_needs_probe_homing_and_a_rough_offset() {
        let p = MachineProfile {
            homing: Homing::Cycle,
            ..MachineProfile::default()
        };
        assert_eq!(
            flange_program(&strat(), &p, 0.5, true),
            Err(CalibrateError::NeedsProbe)
        );
        let p = MachineProfile {
            flange_offset_mm: 0.0,
            ..MachineProfile::default()
        };
        assert_eq!(
            flange_program(&strat(), &p, 0.5, true),
            Err(CalibrateError::OffsetTooSmall(0.0))
        );
    }

    #[test]
    fn reads_reports_out_of_a_console_log() {
        let log = "ok\n[PRB:0.000,-4.120,0.000:1]\nok\n[MSG:Pgm End]\n\
                   [PRB:0.000,6.380,0.000:1]\n[PRB:0.000,9.000,0.000:0]\n\
                   [PRB:0.000,16.880,0.000:1]\nok";
        assert_eq!(probe_ys(log), vec![-4.12, 6.38, 16.88]);
    }

    #[test]
    fn solves_offset_and_window() {
        // Switch at -4.12; flange faces at 6.13 and 17.13 relative to it.
        let c = solve(&[-4.12, 6.38, 16.88], 0.5).unwrap();
        assert!((c.flange_offset_mm - 10.25).abs() < 1e-9);
        assert!((c.window_mm - 11.0).abs() < 1e-9);
        // The automatic run's index report sits second and is skipped.
        assert_eq!(solve(&[-4.12, 1.0, 6.38, 16.88], 0.5), Ok(c));
    }

    #[test]
    fn rejects_a_touch_on_the_switch() {
        assert!(matches!(
            solve(&[-4.12, -4.0, 6.88], 0.5),
            Err(CalibrateError::NearTouch(_))
        ));
        assert_eq!(solve(&[1.0, 2.0], 0.5), Err(CalibrateError::ProbeCount(2)));
    }
}
