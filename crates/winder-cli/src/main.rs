//! `winder`: simulate a coil design and bake its G-code.

use clap::{Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use winder_core::{Bobbin, CrossSection, Design, estimate, generate, recipe};
use winder_gcode::{MachineProfile, flange_program, post_grbl, probe_ys, solve, verify};

#[derive(Parser)]
#[command(
    name = "winder",
    version,
    about = "Design pickup coils and bake G-code for a GRBL winder"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a starter design for a built-in bobbin (strat, tele-bridge,
    /// tele-neck, paf, p90).
    New {
        preset: String,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Estimate DCR, inductance and resonance, and summarize the toolpath.
    Sim { design: PathBuf },
    /// Generate G-code, read it back to verify it, and write it out.
    Gcode {
        design: PathBuf,
        /// Output file (default: the design's name with .nc).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Machine profile TOML (default: Uno + CNC Shield v3 with GRBL 1.1).
        #[arg(short, long)]
        machine: Option<PathBuf>,
    },
    /// Draw the coil's cross-section (every turn where it lands) as SVG.
    Preview {
        design: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Draw only the first N turns.
        #[arg(long)]
        turns: Option<u32>,
        /// Cut through a short end instead of the middle of a long side.
        #[arg(long)]
        end: bool,
    },
    /// Write the default machine profile and the GRBL settings it expects.
    Machine {
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Find the bobbin's flanges with the feeler on the guide. Without
    /// --probes, write the probing program; with the [PRB:...] lines it
    /// reported, write a machine profile with the measured flange offset.
    Calibrate {
        design: PathBuf,
        /// Machine profile TOML (default: Uno + CNC Shield v3 with GRBL 1.1).
        #[arg(short, long)]
        machine: Option<PathBuf>,
        /// Feeler blade thickness along the traverse (mm).
        #[arg(long, default_value_t = 0.5)]
        feeler: f64,
        /// Stop for you to turn the bobbin and swing the feeler, for a
        /// machine without the feeler solenoid and index sensor.
        #[arg(long)]
        manual: bool,
        /// GRBL's console output from the probing run, or just its three
        /// [PRB:...] lines.
        #[arg(long)]
        probes: Option<String>,
        /// Output file (default: <design>-flanges.nc, or
        /// <design>-machine.toml with --probes).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Convert the Python app's winder_recipes.json into design files.
    ImportRecipes {
        json: PathBuf,
        /// Folder for the design files.
        #[arg(short, long, default_value = ".")]
        out_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type Res = Result<(), String>;

fn run(cmd: Cmd) -> Res {
    match cmd {
        Cmd::New { preset, out } => {
            let d = Design::pickup(&preset).ok_or_else(|| {
                let names: Vec<_> = Bobbin::preset_names().collect();
                format!("unknown preset \"{preset}\" (try: {})", names.join(", "))
            })?;
            emit(out.as_deref(), &d.to_toml())
        }
        Cmd::Sim { design } => sim(&load(&design)?),
        Cmd::Gcode {
            design,
            out,
            machine,
        } => {
            let d = load(&design)?;
            let profile = machine_profile(machine.as_deref())?;
            let tp = generate(&d).map_err(|e| e.to_string())?;
            let g = post_grbl(&d, &tp, &profile).map_err(|e| e.to_string())?;
            let p = verify(&g, &d, &tp, &profile)
                .map_err(|e| format!("generated G-code failed its check: {e}"))?;
            let out = out.unwrap_or_else(|| design.with_extension("nc"));
            fs::write(&out, &g).map_err(|e| format!("{}: {e}", out.display()))?;
            let st = tp.stats();
            let wire = d.wire_spec().map_err(|e| e.to_string())?;
            let e = estimate(d.total_turns(), &d.bobbin, &wire, &d.estimate);
            if e.break_warning() {
                println!(
                    "warning: tension is {:.0}% of the wire's breaking force; keep it under 70%",
                    e.break_risk * 100.0
                );
            }
            println!(
                "wrote {}: {} lines, {} turns, {} passes, about {:.1} min (verified)",
                out.display(),
                g.lines().count(),
                d.total_turns(),
                st.passes,
                st.minutes
            );
            if p.stops > 0 {
                println!(
                    "{} stop point(s): the machine pauses, press resume to continue",
                    p.stops
                );
            }
            Ok(())
        }
        Cmd::Preview {
            design,
            out,
            turns,
            end,
        } => {
            let d = load(&design)?;
            let tp = generate(&d).map_err(|e| e.to_string())?;
            let cs = CrossSection::build(&d, &tp, &d.wire_spec().map_err(|e| e.to_string())?);
            let out = out.unwrap_or_else(|| design.with_extension("svg"));
            let n = turns.map_or(cs.turn_count(), |t| t as usize);
            let station = if end {
                cs.end_station()
            } else {
                cs.side_station()
            };
            fs::write(&out, svg(&d, &cs, station, n))
                .map_err(|e| format!("{}: {e}", out.display()))?;
            println!(
                "wrote {} ({}): build here {:.2} mm, {:.0} m, {:.2} kOhm from the lay",
                out.display(),
                cs.stations[station].label(),
                cs.build_at(station, n),
                cs.length_m(),
                cs.dcr_kohm(d.estimate.temp_c)
            );
            Ok(())
        }
        Cmd::Machine { out } => {
            let p = MachineProfile::default();
            let mut text = String::from("# Send these GRBL settings once:\n");
            for s in p.grbl_settings() {
                text.push_str(&format!("#   {s}\n"));
            }
            text.push('\n');
            text.push_str(&toml::to_string_pretty(&p).expect("profile serializes"));
            emit(out.as_deref(), &text)
        }
        Cmd::Calibrate {
            design,
            machine,
            feeler,
            manual,
            probes,
            out,
        } => {
            let d = load(&design)?;
            let mut profile = machine_profile(machine.as_deref())?;
            let stem = design.with_extension("");
            let Some(probes) = probes else {
                let g = flange_program(&d, &profile, feeler, !manual).map_err(|e| e.to_string())?;
                let out = out.unwrap_or_else(|| suffixed(&stem, "-flanges.nc"));
                fs::write(&out, g).map_err(|e| format!("{}: {e}", out.display()))?;
                println!(
                    "wrote {}: send it with the bobbin mounted, then run\n  winder calibrate {} --probes \"<GRBL's output>\"",
                    out.display(),
                    design.display()
                );
                return Ok(());
            };
            let c = solve(&probe_ys(&probes), feeler).map_err(|e| e.to_string())?;
            let h = d.bobbin.channel_height_mm;
            println!(
                "flange offset {:.3} mm (was {:.3})\nwindow {:.3} mm (design says {:.3})",
                c.flange_offset_mm, profile.flange_offset_mm, c.window_mm, h
            );
            if (c.window_mm - h).abs() > 0.1 {
                println!(
                    "  the window differs from the design by {:+.2} mm: set channel_height_mm = {:.2} in the design",
                    c.window_mm - h,
                    c.window_mm
                );
            }
            profile.flange_offset_mm = (c.flange_offset_mm * 1000.0).round() / 1000.0;
            let out = out.unwrap_or_else(|| suffixed(&stem, "-machine.toml"));
            let text = format!(
                "# Calibrated with the flange finder on {}\n{}",
                d.name,
                toml::to_string_pretty(&profile).expect("profile serializes")
            );
            fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
            println!(
                "wrote {}: use it with  winder gcode {} -m {}",
                out.display(),
                design.display(),
                out.display()
            );
            Ok(())
        }
        Cmd::ImportRecipes { json, out_dir } => {
            let all = recipe::import_recipes(&read(&json)?).map_err(|e| e.to_string())?;
            fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
            for i in all {
                let path = out_dir.join(format!("{}.toml", slug(&i.design.name)));
                fs::write(&path, i.design.to_toml())
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                println!("{}", path.display());
                for n in i.notes {
                    println!("  note: {n}");
                }
            }
            Ok(())
        }
    }
}

fn machine_profile(path: Option<&Path>) -> Result<MachineProfile, String> {
    match path {
        Some(p) => toml::from_str(&read(p)?).map_err(|e| format!("{}: {e}", p.display())),
        None => Ok(MachineProfile::default()),
    }
}

fn suffixed(stem: &Path, suffix: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn sim(d: &Design) -> Res {
    let wire = d.wire_spec().map_err(|e| e.to_string())?;
    let e = estimate(d.total_turns(), &d.bobbin, &wire, &d.estimate);
    let st = generate(d).map_err(|e| e.to_string())?.stats();
    println!(
        "{}  ({} turns of {}, {})",
        d.name,
        d.total_turns(),
        wire.name,
        d.direction.label()
    );
    println!("  layer model (assumes perfect layers):");
    println!(
        "  DCR {:.2} kOhm at {:.0} C   wire {:.0} m ({:.0} ft)",
        e.dcr_kohm,
        e.temp_c,
        e.length_m,
        e.length_m * 3.281
    );
    println!(
        "  L {:.2} H   self-C {:.0} pF   resonance {:.1} kHz   Q {:.1}",
        e.inductance_h, e.cap_pf, e.resonance_khz, e.q_factor
    );
    println!(
        "  {} layers, {:.1} turns/layer, build {:.2} of {:.1} mm",
        e.layers, e.turns_per_layer, e.build_mm, d.bobbin.build_depth_mm
    );
    if e.build_mm > d.bobbin.build_depth_mm {
        println!("  warning: the coil is deeper than the bobbin allows");
    }
    if e.break_warning() {
        println!(
            "  warning: tension is {:.0}% of the wire's breaking force",
            e.break_risk * 100.0
        );
    }
    println!(
        "  toolpath: {} sections, {} passes, {} moves, guide {:.3} to {:.3} mm, about {:.1} min",
        d.sections.len(),
        st.passes,
        st.moves,
        st.min_y_mm,
        st.max_y_mm,
        st.minutes
    );
    let tp = generate(d).map_err(|e| e.to_string())?;
    let l = CrossSection::build(d, &tp, &wire).laid(d);
    println!(
        "  from the lay: DCR {:.2} kOhm, wire {:.0} m, build {:.2} mm long side / {:.2} short end, fill {:.0} %",
        l.dcr_kohm,
        l.length_m,
        l.side_build_mm,
        l.end_build_mm,
        l.fill_factor * 100.0
    );
    println!(
        "                L {:.2} H   self-C {:.0} pF   resonance {:.1} kHz   Q {:.1}",
        l.inductance_h, l.cap_pf, l.resonance_khz, l.q_factor
    );
    if l.build_mm > d.bobbin.build_depth_mm {
        println!("  warning: the laid coil is deeper than the bobbin allows");
    }
    println!("  (DCR, L and C are estimates; log measured coils to calibrate them)");
    Ok(())
}

/// Reads a design and, if it sets a target DCR, solves its fill section's
/// turns, as the editor does.
fn load(path: &Path) -> Result<Design, String> {
    let err = |e: winder_core::DesignError| format!("{}: {e}", path.display());
    let mut d = Design::from_toml(&read(path)?).map_err(err)?;
    if let Some(s) = winder_core::auto::solve(&mut d).map_err(err)?
        && d.sections[s.section].turns != s.turns_before
    {
        eprintln!(
            "target {:.2} kΩ: section {} wound to {} turns (file says {})",
            d.auto.target_dcr_kohm.unwrap_or_default(),
            s.section + 1,
            d.sections[s.section].turns,
            s.turns_before
        );
    }
    Ok(d)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn emit(out: Option<&Path>, text: &str) -> Res {
    match out {
        Some(p) => fs::write(p, text).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    s.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Cross-section as SVG: the channel between the flanges, the core along
/// the bottom, one circle per turn shaded from first (dark) to last (light).
fn svg(d: &Design, cs: &CrossSection, station: usize, n: usize) -> String {
    let scale = 60.0; // px per mm
    let (w_mm, depth) = (
        cs.channel_mm,
        d.bobbin.build_depth_mm.max(cs.build_mm() * 1.05),
    );
    let (pad, flange) = (20.0, 12.0);
    let width = w_mm * scale + 2.0 * (pad + flange);
    let height = depth * scale + 2.0 * pad;
    let x = |y: f64| pad + flange + y * scale;
    let z = |z: f64| pad + (depth - z) * scale;
    let mut s = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{width:.0}' height='{height:.0}' viewBox='0 0 {width:.0} {height:.0}'>\n\
         <rect width='100%' height='100%' fill='#16181c'/>\n\
         <rect x='{:.1}' y='{:.1}' width='{flange}' height='{:.1}' fill='#555'/>\n\
         <rect x='{:.1}' y='{:.1}' width='{flange}' height='{:.1}' fill='#555'/>\n\
         <line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#888' stroke-width='2'/>\n",
        pad,
        pad,
        depth * scale,
        x(w_mm),
        pad,
        depth * scale,
        x(0.0),
        z(0.0),
        x(w_mm),
        z(0.0),
    );
    let total = cs.turn_count().max(1) as f64;
    let r = cs.od_mm / 2.0 * scale;
    for t in cs.cuts[station].iter().take(n) {
        let f = t.n as f64 / total;
        let (cr, cg, cb) = (90.0 + 140.0 * f, 40.0 + 90.0 * f, 18.0 + 40.0 * f);
        s.push_str(&format!(
            "<circle cx='{:.2}' cy='{:.2}' r='{r:.2}' fill='rgb({cr:.0},{cg:.0},{cb:.0})'/>\n",
            x(t.y_mm),
            z(t.z_mm)
        ));
    }
    s.push_str("</svg>\n");
    s
}
