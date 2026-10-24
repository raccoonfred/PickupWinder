//! `winder`: simulate a coil design and bake its G-code.

use clap::{Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use winder_core::{Bobbin, Design, estimate, generate, recipe};
use winder_gcode::{MachineProfile, post_grbl, verify};

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
    /// Write the default machine profile and the GRBL settings it expects.
    Machine {
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
            let d = Design::example(&preset).ok_or_else(|| {
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
            let profile = match machine {
                Some(p) => {
                    toml::from_str(&read(&p)?).map_err(|e| format!("{}: {e}", p.display()))?
                }
                None => MachineProfile::default(),
            };
            let tp = generate(&d).map_err(|e| e.to_string())?;
            let g = post_grbl(&d, &tp, &profile).map_err(|e| e.to_string())?;
            let p = verify(&g, &d, &tp, &profile)
                .map_err(|e| format!("generated G-code failed its check: {e}"))?;
            let out = out.unwrap_or_else(|| design.with_extension("nc"));
            fs::write(&out, &g).map_err(|e| format!("{}: {e}", out.display()))?;
            let st = tp.stats();
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
    println!("  (DCR, L and C are estimates; log measured coils to calibrate them)");
    Ok(())
}

fn load(path: &Path) -> Result<Design, String> {
    Design::from_toml(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
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
