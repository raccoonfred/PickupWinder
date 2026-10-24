//! Lays the same coil with different patterns and prints what each measures.
//!
//!   cargo run --release -p winder-core --example patterns [free_span_mm]

use winder_core::{CrossSection, Design, Pattern, Section, generate};

fn row(name: &str, d: &Design) {
    let tp = generate(d).expect("valid design");
    let l = CrossSection::build(d, &tp, &d.wire_spec().expect("known wire")).laid(d);
    println!(
        "{name:24} {:>6.2} {:>7.2} {:>6.2} {:>6.0} {:>6.0} {:>7.1}",
        l.dcr_kohm,
        l.side_build_mm,
        l.end_build_mm,
        l.fill_factor * 100.0,
        l.cap_pf,
        l.resonance_khz
    );
}

fn main() {
    let span: f64 = std::env::args()
        .nth(1)
        .map_or(25.0, |s| s.parse().expect("free span in mm"));
    for preset in ["strat", "paf"] {
        let mut base = Design::example(preset).expect("preset");
        base.lay.free_span_mm = span;
        let n = base.total_turns();
        let full = (base.bobbin.channel_height_mm - 0.071) / 0.071;
        println!(
            "\n{preset}, {n} turns of 42 AWG, side by side = {full:.0} turns per pass, free span {span} mm"
        );
        println!("pattern                     kOhm  side mm  end mm  fill%     pF     kHz");
        row("example design", &base);
        let with = |s: Section| {
            let mut d = base.clone();
            d.sections = vec![s];
            d
        };
        row("perfect layer", &with(Section::layer(n)));
        for share in [1.0, 0.8, 0.6, 0.4] {
            let tpl = (full * share).round();
            row(
                &format!("helix {tpl:.0} tpl"),
                &with(Section::helix(n, tpl)),
            );
        }
        let scatter = Pattern::Scatter {
            tpl_min: (full * 0.4).round(),
            tpl_max: (full * 0.9).round(),
            reversal_jitter_mm: 0.4,
            speed_wander: winder_core::design::SPEED_WANDER,
            seed: 5,
        };
        row("scatter 40-90 %", &with(Section::with(n, scatter)));
    }
}
