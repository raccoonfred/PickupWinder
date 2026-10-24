//! Everything the views show, derived from one design. No UI code here.

use std::path::Path;
use winder_core::{
    CrossSection, Design, Estimate, Step, Toolpath, estimate, generate, toolpath::Stats,
};
use winder_gcode::{MachineProfile, post_grbl, verify};

/// The design worked out: toolpath, cross-section and estimates.
pub struct Computed {
    pub stats: Stats,
    pub cross: CrossSection,
    pub estimate: Estimate,
    /// Turn at which each section starts, plus the total at the end.
    pub section_bounds: Vec<f64>,
    /// Turns at which the machine pauses.
    pub stops: Vec<f64>,
    /// Guide position against turns, thinned for plotting.
    pub lay: Vec<(f64, f64)>,
}

impl Computed {
    pub fn new(design: &Design) -> Result<Computed, String> {
        let toolpath = generate(design).map_err(|e| e.to_string())?;
        let wire = design.wire_spec().map_err(|e| e.to_string())?;
        let cross = CrossSection::build(design, &toolpath, &wire);
        let estimate = estimate(
            design.total_turns(),
            &design.bobbin,
            &wire,
            &design.estimate,
        );
        let mut section_bounds = Vec::new();
        let mut stops = Vec::new();
        for s in &toolpath.steps {
            match s {
                Step::SectionStart { turns, .. } => section_bounds.push(*turns),
                Step::Stop { turns, .. } => stops.push(*turns),
                Step::Move(_) => {}
            }
        }
        section_bounds.push(design.total_turns() as f64);
        let lay = thin_lay(&toolpath, 4000);
        Ok(Computed {
            stats: toolpath.stats(),
            cross,
            estimate,
            section_bounds,
            stops,
            lay,
        })
    }
}

/// Guide position against turns, keeping every reversal point. When there
/// are more moves than `max`, plain mid-pass points are dropped first.
fn thin_lay(tp: &Toolpath, max: usize) -> Vec<(f64, f64)> {
    let mut pts = vec![(0.0, tp.start_y_mm)];
    let moves: Vec<_> = tp.moves().collect();
    let keep_every = moves.len().div_ceil(max.max(1)).max(1);
    for (i, m) in moves.iter().enumerate() {
        let reversal = moves.get(i + 1).is_some_and(|n| n.pass != m.pass) || i + 1 == moves.len();
        if reversal || i % keep_every == 0 {
            pts.push((m.turns, m.y_mm));
        }
    }
    pts
}

/// Bake, verify and write G-code. Returns a one-line summary.
pub fn export_gcode(
    design: &Design,
    profile: &MachineProfile,
    path: &Path,
) -> Result<String, String> {
    let tp = generate(design).map_err(|e| e.to_string())?;
    let g = post_grbl(design, &tp, profile).map_err(|e| e.to_string())?;
    verify(&g, design, &tp, profile)
        .map_err(|e| format!("generated G-code failed its check: {e}"))?;
    std::fs::write(path, &g).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!(
        "Wrote {} ({} lines, about {:.1} min, verified)",
        path.display(),
        g.lines().count(),
        tp.stats().minutes
    ))
}

pub fn load_design(path: &Path) -> Result<Design, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Design::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_design(design: &Design, path: &Path) -> Result<(), String> {
    std::fs::write(path, design.to_toml()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_profile(path: &Path) -> Result<MachineProfile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Top of the winding across the channel after the first `n` turns.
pub fn profile_upto(cross: &CrossSection, n: usize, bins: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; bins];
    let w = cross.channel_mm / bins as f64;
    let r = cross.od_mm / 2.0;
    for t in cross.turns.iter().take(n) {
        let lo = ((t.y_mm - r) / w).floor().max(0.0) as usize;
        let hi = (((t.y_mm + r) / w).ceil() as usize).min(bins);
        for (b, slot) in out.iter_mut().enumerate().take(hi).skip(lo) {
            let dy = ((b as f64 + 0.5) * w - t.y_mm).abs().min(r);
            *slot = slot.max(t.z_mm + (r * r - dy * dy).sqrt());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_every_preset() {
        for p in winder_core::Bobbin::preset_names() {
            let d = Design::example(p).unwrap();
            let c = Computed::new(&d).unwrap();
            assert_eq!(c.cross.turns.len() as u32, d.total_turns());
            assert_eq!(c.section_bounds.len(), d.sections.len() + 1);
            assert!(c.lay.len() <= 4000 + c.stats.passes as usize + 2);
        }
    }

    #[test]
    fn lay_plot_keeps_every_reversal() {
        let d = Design::example("strat").unwrap();
        let c = Computed::new(&d).unwrap();
        let (lo, hi) = (c.stats.min_y_mm, c.stats.max_y_mm);
        let touches = c
            .lay
            .iter()
            .filter(|(_, y)| (y - lo).abs() < 1e-9 || (y - hi).abs() < 1e-9)
            .count();
        assert!(touches >= 2);
    }

    #[test]
    fn export_writes_verified_gcode() {
        let dir = std::env::temp_dir().join(format!("winder-app-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = Design::example("paf").unwrap();
        let path = dir.join("paf.nc");
        let msg = export_gcode(&d, &MachineProfile::default(), &path).unwrap();
        assert!(msg.contains("verified"));
        let saved = dir.join("paf.toml");
        save_design(&d, &saved).unwrap();
        assert_eq!(load_design(&saved).unwrap(), d);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn partial_profile_grows() {
        let d = Design::example("strat").unwrap();
        let c = Computed::new(&d).unwrap();
        let early = profile_upto(&c.cross, 500, 100);
        let full = profile_upto(&c.cross, usize::MAX, 100);
        let sum = |v: &[f64]| v.iter().sum::<f64>();
        assert!(sum(&full) > sum(&early));
    }
}
