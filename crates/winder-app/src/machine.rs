//! Solid models of the bobbin and the winder around it, built from the
//! bobbin's real kind and size.
//!
//! - **Single coils (Strat, Tele)** are two plates of black fibre flatwork
//!   held apart by six rod magnets; the wire winds straight onto the
//!   magnets. The Strat's magnets are staggered, the bottom plate is thicker
//!   and carries the two lead eyelets.
//! - **Humbucker and P-90** bobbins are moulded black plastic with a core
//!   wall and six pole screws; the P-90 has mounting ears.
//! - **The machine:** the bobbin sits on a round aluminium platen on the
//!   spindle, driven by a stepper below. Beside it a traverse rail parallel
//!   to the spindle carries the guide arm and its ceramic eyelet. The
//!   platen and bobbin spin; the rail and the motor stay put.
//!
//! World y is the spindle axis, 0 at the inner face of the bottom flange;
//! x runs along the bobbin's length, z across it.

use std::f32::consts::{PI, TAU};
use winder_core::{Bobbin, Style};

/// Vertex layout: pos(3) normal(3) rgba(4).
pub const STRIDE: usize = 10;

const FIBRE: [f32; 4] = [0.10, 0.10, 0.11, 1.0];
const PLASTIC: [f32; 4] = [0.07, 0.07, 0.075, 1.0];
const ALNICO: [f32; 4] = [0.62, 0.62, 0.60, 1.0];
const NICKEL: [f32; 4] = [0.80, 0.80, 0.78, 1.0];
const BRASS: [f32; 4] = [0.80, 0.62, 0.30, 1.0];
const ALU: [f32; 4] = [0.78, 0.80, 0.83, 1.0];
const STEEL: [f32; 4] = [0.45, 0.46, 0.48, 1.0];
const MOTOR: [f32; 4] = [0.16, 0.16, 0.17, 1.0];
const CERAMIC: [f32; 4] = [0.95, 0.95, 0.93, 1.0];
const HOLE: [f32; 4] = [0.03, 0.03, 0.03, 1.0];

#[derive(Default)]
pub struct Mesh(pub Vec<f32>);

impl Mesh {
    fn vert(&mut self, p: [f32; 3], n: [f32; 3], c: [f32; 4]) {
        self.0.extend_from_slice(&p);
        self.0.extend_from_slice(&n);
        self.0.extend_from_slice(&c);
    }

    fn tri(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], n: [f32; 3], col: [f32; 4]) {
        self.vert(a, n, col);
        self.vert(b, n, col);
        self.vert(c, n, col);
    }

    /// A flat-topped slab with `outline` (x, z), star-shaped about its
    /// centroid, from `y0` to `y1`. Side normals are smooth.
    pub fn prism(&mut self, outline: &[[f32; 2]], y0: f32, y1: f32, col: [f32; 4]) {
        let n = outline.len();
        let c = outline.iter().fold([0.0, 0.0], |a, p| {
            [a[0] + p[0] / n as f32, a[1] + p[1] / n as f32]
        });
        let normal = |i: usize| {
            let a = outline[(i + n - 1) % n];
            let b = outline[(i + 1) % n];
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dz * dz).sqrt().max(1e-6);
            // Outward for a counter-clockwise (x→z) outline.
            [dz / l, 0.0, -dx / l]
        };
        let sign = if area(outline) >= 0.0 { 1.0 } else { -1.0 };
        for i in 0..n {
            let (p, q) = (outline[i], outline[(i + 1) % n]);
            let (np, nq) = (normal(i), normal((i + 1) % n));
            let (np, nq) = (np.map(|v| v * sign), nq.map(|v| v * sign));
            self.tri(
                [c[0], y1, c[1]],
                [p[0], y1, p[1]],
                [q[0], y1, q[1]],
                [0.0, 1.0, 0.0],
                col,
            );
            self.tri(
                [c[0], y0, c[1]],
                [q[0], y0, q[1]],
                [p[0], y0, p[1]],
                [0.0, -1.0, 0.0],
                col,
            );
            self.vert([p[0], y0, p[1]], np, col);
            self.vert([q[0], y0, q[1]], nq, col);
            self.vert([q[0], y1, q[1]], nq, col);
            self.vert([p[0], y0, p[1]], np, col);
            self.vert([q[0], y1, q[1]], nq, col);
            self.vert([p[0], y1, p[1]], np, col);
        }
    }

    /// An upright cylinder.
    pub fn cylinder(&mut self, x: f32, z: f32, r: f32, y0: f32, y1: f32, col: [f32; 4]) {
        let seg = if r > 8.0 { 64 } else { 20 };
        let ring: Vec<[f32; 2]> = (0..seg)
            .map(|k| {
                let a = k as f32 / seg as f32 * TAU;
                [x + r * a.cos(), z + r * a.sin()]
            })
            .collect();
        self.prism(&ring, y0, y1, col);
    }

    /// A box between two corners.
    pub fn cuboid(&mut self, min: [f32; 3], max: [f32; 3], col: [f32; 4]) {
        let outline = [
            [min[0], min[2]],
            [max[0], min[2]],
            [max[0], max[2]],
            [min[0], max[2]],
        ];
        // Flat sides: one prism per face keeps the normals square.
        for i in 0..4 {
            let (p, q) = (outline[i], outline[(i + 1) % 4]);
            let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
            let l = (dx * dx + dz * dz).sqrt().max(1e-6);
            let n = [dz / l, 0.0, -dx / l];
            self.tri(
                [p[0], min[1], p[1]],
                [q[0], min[1], q[1]],
                [q[0], max[1], q[1]],
                n,
                col,
            );
            self.tri(
                [p[0], min[1], p[1]],
                [q[0], max[1], q[1]],
                [p[0], max[1], p[1]],
                n,
                col,
            );
        }
        let (a, b, c, d) = (outline[0], outline[1], outline[2], outline[3]);
        for (y, n) in [(max[1], [0.0, 1.0, 0.0]), (min[1], [0.0, -1.0, 0.0])] {
            self.tri([a[0], y, a[1]], [b[0], y, b[1]], [c[0], y, c[1]], n, col);
            self.tri([a[0], y, a[1]], [c[0], y, c[1]], [d[0], y, d[1]], n, col);
        }
    }
}

fn area(o: &[[f32; 2]]) -> f32 {
    let n = o.len();
    (0..n)
        .map(|i| {
            let (p, q) = (o[i], o[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f32>()
        / 2.0
}

/// A rectangle `len` along x by `wid` along z with corners of radius `r`.
pub fn rounded_rect(len: f32, wid: f32, r: f32) -> Vec<[f32; 2]> {
    let r = r.clamp(0.0, len.min(wid) / 2.0);
    let (a, b) = (len / 2.0 - r, wid / 2.0 - r);
    let mut out = Vec::new();
    for (c, start) in [
        ([a, b], 0.0f32),
        ([-a, b], 90.0),
        ([-a, -b], 180.0),
        ([a, -b], 270.0),
    ] {
        for k in 0..=10 {
            let ang = (start + 9.0 * k as f32).to_radians();
            out.push([c[0] + r * ang.cos(), c[1] + r * ang.sin()]);
        }
    }
    out
}

/// The model, split by how it moves.
pub struct Parts {
    /// Turns with the spindle: bobbin, poles, platen.
    pub spin: Mesh,
    /// The top flange or flatwork, drawn last so it can be hidden.
    pub top: Mesh,
    /// Never moves: motor and traverse rail.
    pub still: Mesh,
    /// Rides the rail: carriage, arm and eyelet, eyelet centre at y = 0.
    pub carriage: Mesh,
    /// Eyelet's distance from the spindle axis, and its direction (rad,
    /// from +x toward +z).
    pub guide_r: f32,
    pub guide_angle: f32,
    /// Overall length of the bobbin, to frame the view.
    pub length: f32,
}

/// Pole positions along x, evenly spread over `spread`.
fn poles(count: u32, spread: f32) -> impl Iterator<Item = (usize, f32)> {
    let n = count as usize;
    (0..n).map(move |i| {
        let x = if n > 1 {
            -spread / 2.0 + spread * i as f32 / (n - 1) as f32
        } else {
            0.0
        };
        (i, x)
    })
}

/// The sizes the model is drawn with: the kind's usual flatwork and poles,
/// with anything set in the bobbin's `detail` taking their place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dims {
    pub flange_length: f32,
    pub flange_width: f32,
    pub flange_corner: f32,
    pub top_thickness: f32,
    pub bottom_thickness: f32,
    pub pole_count: u32,
    pub pole_spread: f32,
    pub pole_diameter: f32,
    /// How far the tallest pole stands above the top flange.
    pub pole_height: f32,
}

/// The kind's usual sizes for this core and build depth, ignoring `detail`.
pub fn usual(b: &Bobbin, depth: f32) -> Dims {
    let (w, l) = (b.core_width_mm as f32, b.core_length_mm as f32);
    let r_core = b.corner_r() as f32;
    let d = w + 2.0 * depth;
    let (flange_length, flange_width, flange_corner, top_thickness, bottom_thickness) =
        match b.style {
            Style::Strat => (l + 11.0, d + 0.8, d * 0.42, 1.6, 2.4),
            // Top flatwork 19.6 mm wide over a 72 x 38 mm base with ears.
            Style::TeleBridge => (l + 12.0, d + 0.85, 4.0, 1.6, 1.6),
            Style::TeleNeck => (l + 8.0, d + 1.0, d * 0.45, 1.6, 1.6),
            // PAF: 66.7 x 17.5 x 9.5 mm over a 55.6 x 6.35 mm core.
            Style::Humbucker => (l + 11.1, d, 1.0, 1.57, 1.57),
            // P-90: 1.2" (30.5 mm) wide, 0.375" (9.5 mm) high.
            Style::P90 => (l + 11.0, d + 1.15, 3.0, 1.25, 1.25),
            Style::Generic => (l + 2.0 * depth + 3.0, d + 3.0, r_core + depth, 1.6, 1.6),
        };
    let (pole_count, pole_spread, pole_diameter, pole_height) = match b.style {
        Style::Strat => (6, l - w, w, 3.2),
        Style::TeleBridge | Style::TeleNeck => (6, l - w, w, 0.8),
        Style::Humbucker => (6, 49.2, 5.0, 1.4),
        Style::P90 => (6, 49.2, 4.8, 1.4),
        Style::Generic => (0, l - w, w.min(5.0), 1.0),
    };
    Dims {
        flange_length,
        flange_width,
        flange_corner,
        top_thickness,
        bottom_thickness,
        pole_count,
        pole_spread: pole_spread.max(0.0),
        pole_diameter,
        pole_height,
    }
}

/// The sizes to draw: the usual ones with `detail` laid over them.
pub fn dims(b: &Bobbin, depth: f32) -> Dims {
    let u = usual(b, depth);
    let t = &b.detail;
    let or = |v: Option<f64>, d: f32| v.map_or(d, |v| v as f32);
    Dims {
        flange_length: or(t.flange_length_mm, u.flange_length),
        flange_width: or(t.flange_width_mm, u.flange_width),
        flange_corner: or(t.flange_corner_mm, u.flange_corner),
        top_thickness: or(t.top_thickness_mm, u.top_thickness),
        bottom_thickness: or(t.bottom_thickness_mm, u.bottom_thickness),
        pole_count: t.pole_count.unwrap_or(u.pole_count),
        pole_spread: or(t.pole_spread_mm, u.pole_spread),
        pole_diameter: or(t.pole_diameter_mm, u.pole_diameter),
        pole_height: or(t.pole_height_mm, u.pole_height),
    }
}

pub fn build(b: &Bobbin, build_mm: f32, free_span: f32) -> Parts {
    let (w, l, h) = (
        b.core_width_mm as f32,
        b.core_length_mm as f32,
        b.channel_height_mm as f32,
    );
    let depth = (b.build_depth_mm as f32).max(build_mm);
    let r_core = b.corner_r() as f32;
    let mut spin = Mesh::default();
    let mut top = Mesh::default();

    let dm = dims(b, b.build_depth_mm as f32);
    // A flange never comes out smaller than the core it holds.
    let flange_len = dm.flange_length.max(l);
    let flange_wid = dm.flange_width.max(w);
    let flange_r = dm.flange_corner;
    let (t_top, t_bottom) = (dm.top_thickness.max(0.2), dm.bottom_thickness.max(0.2));
    let outline = rounded_rect(flange_len, flange_wid, flange_r);
    let fibre = matches!(b.style, Style::Strat | Style::TeleBridge | Style::TeleNeck);
    let body = if fibre { FIBRE } else { PLASTIC };
    spin.prism(&outline, -t_bottom, 0.0, body);
    top.prism(&outline, h, h + t_top, body);
    let mut length = flange_len;

    let spread = dm.pole_spread;
    let pole_r = dm.pole_diameter / 2.0;
    match b.style {
        Style::Strat | Style::TeleBridge | Style::TeleNeck => {
            // Rod magnets from the bottom of the bottom plate, through the
            // top plate. The Strat's '54-style stagger (in inches):
            // .684 .687 .717 .718 .636 .657.
            let stagger = [0.684f32, 0.687, 0.717, 0.718, 0.636, 0.657];
            let tallest = h + t_top + dm.pole_height;
            for (i, x) in poles(dm.pole_count, spread) {
                let top_y = match b.style {
                    Style::Strat if dm.pole_count == 6 => {
                        -t_bottom + stagger[i] / 0.718 * (t_bottom + tallest)
                    }
                    _ => tallest,
                };
                spin.cylinder(x, 0.0, pole_r, -t_bottom - 0.01, top_y, ALNICO);
            }
            // Lead-out eyelets in the bottom plate.
            let ez = flange_wid / 2.0 - 1.3;
            for x in [-spread * 0.2, spread * 0.2] {
                spin.cylinder(x, ez, 0.9, -t_bottom - 0.2, 0.25, BRASS);
                spin.cylinder(x, ez, 0.45, -t_bottom - 0.25, 0.3, HOLE);
            }
            if b.style == Style::TeleBridge {
                // Three mounting ears on the bottom plate.
                for (x, z) in [
                    (-flange_len / 2.0 - 1.5, 0.0),
                    (flange_len / 2.0 + 1.5, flange_wid * 0.18),
                    (flange_len / 2.0 + 1.5, -flange_wid * 0.18),
                ] {
                    spin.cylinder(x, z, 4.0, -t_bottom, 0.0, FIBRE);
                    spin.cylinder(x, z, 1.4, -t_bottom - 0.05, 0.05, HOLE);
                }
                length += 11.0;
            }
        }
        Style::Humbucker | Style::P90 => {
            // Moulded core wall. It is wound empty: the pole screws go in
            // afterwards, so the top shows their holes.
            let core = rounded_rect(l, w, r_core);
            spin.prism(&core, 0.0, h, PLASTIC);
            holes(&mut top, &dm, h + t_top);
            if b.style == Style::P90 {
                // Mounting ears at each end.
                for s in [-1.0f32, 1.0] {
                    let x = s * (flange_len / 2.0 + 4.0);
                    spin.cylinder(x, 0.0, 6.0, -t_bottom, 0.0, PLASTIC);
                    spin.cylinder(x, 0.0, 1.8, -t_bottom - 0.05, 0.05, HOLE);
                }
                length += 20.0;
            }
        }
        Style::Generic => {
            let core = rounded_rect(l, w, r_core);
            spin.prism(&core, 0.0, h, [0.55, 0.55, 0.58, 1.0]);
            screws(&mut spin, &dm, h + t_top);
        }
    }

    // Platen and hub, spinning with the bobbin. Two clamps hold the ends.
    let reach_bobbin = outline
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
        .fold(0.0, f32::max)
        .max(length / 2.0);
    let platen_r = (reach_bobbin + 6.0).max(32.0);
    let base = -t_bottom;
    spin.cylinder(0.0, 0.0, platen_r, base - 5.0, base, ALU);
    spin.cylinder(0.0, 0.0, 11.0, base - 17.0, base - 5.0, ALU);
    for s in [-1.0f32, 1.0] {
        let x = s * (flange_len / 2.0 + 2.0);
        spin.cuboid(
            [x - 2.5, base, -5.0],
            [x + 2.5, base + 1.2 + t_bottom, 5.0],
            STEEL,
        );
        spin.cylinder(
            x,
            0.0,
            1.6,
            base + 1.2 + t_bottom,
            base + 2.6 + t_bottom,
            STEEL,
        );
    }

    // The still parts: shaft, coupler and the stepper below.
    let mut still = Mesh::default();
    let motor_top = base - 30.0;
    still.cylinder(0.0, 0.0, 2.5, motor_top, base - 17.0, STEEL);
    still.cylinder(0.0, 0.0, 6.0, motor_top + 4.0, motor_top + 13.0, ALU);
    still.cylinder(0.0, 0.0, 11.0, motor_top, motor_top + 2.0, ALU);
    still.cuboid(
        [-21.15, motor_top - 40.0, -21.15],
        [21.15, motor_top, 21.15],
        MOTOR,
    );
    still.cuboid(
        [-21.2, motor_top - 6.0, -21.2],
        [21.2, motor_top - 0.01, 21.2],
        ALU,
    );
    still.cuboid(
        [-21.2, motor_top - 40.0, -21.2],
        [21.2, motor_top - 34.0, 21.2],
        ALU,
    );

    // Traverse: a 20 mm extrusion parallel to the spindle, a carriage, an
    // arm reaching in, and the ceramic eyelet the wire runs through.
    // Behind and to the right from the home view, so it stays out of the way.
    let guide_angle = -0.75f32;
    let guide_r = (b.core_length_mm as f32 / 2.0 + depth + free_span).max(reach_bobbin + 10.0);
    let rail_r = guide_r.max(platen_r + 8.0) + 26.0;
    let (ca, sa) = (guide_angle.cos(), guide_angle.sin());
    let at = |r: f32| [r * ca, r * sa];
    let rail = at(rail_r);
    let rail_outline: Vec<[f32; 2]> = rounded_rect(20.0, 20.0, 1.5)
        .into_iter()
        .map(|p| rot(p, guide_angle, rail))
        .collect();
    still.prism(&rail_outline, motor_top - 40.0, h + 45.0, ALU);
    // Slots down the extrusion's faces.
    for k in 0..4 {
        let a = guide_angle + k as f32 * PI / 2.0;
        let c = [rail[0] + 10.05 * a.cos(), rail[1] + 10.05 * a.sin()];
        let slot: Vec<[f32; 2]> = rounded_rect(1.2, 6.0, 0.0)
            .into_iter()
            .map(|p| rot(p, a, c))
            .collect();
        still.prism(&slot, motor_top - 40.0, h + 45.0, STEEL);
    }

    let mut carriage = Mesh::default();
    let block: Vec<[f32; 2]> = rounded_rect(14.0, 30.0, 2.0)
        .into_iter()
        .map(|p| rot(p, guide_angle, at(rail_r - 17.0)))
        .collect();
    carriage.prism(&block, -12.0, 12.0, [0.25, 0.27, 0.30, 1.0]);
    let arm_len = rail_r - 24.0 - guide_r;
    let arm: Vec<[f32; 2]> = rounded_rect(arm_len.max(1.0), 6.0, 1.0)
        .into_iter()
        .map(|p| rot(p, guide_angle, at(guide_r + 3.0 + arm_len / 2.0)))
        .collect();
    carriage.prism(&arm, -1.5, 1.5, ALU);
    let eye = at(guide_r);
    carriage.cylinder(eye[0], eye[1], 3.2, -2.0, 2.0, CERAMIC);
    carriage.cylinder(eye[0], eye[1], 1.2, -2.05, 2.05, HOLE);

    Parts {
        spin,
        top,
        still,
        carriage,
        guide_r,
        guide_angle,
        length,
    }
}

/// Pole holes in a moulded top flange whose top is at `y`.
fn holes(top: &mut Mesh, dm: &Dims, y: f32) {
    for (_, x) in poles(dm.pole_count, dm.pole_spread) {
        top.cylinder(x, 0.0, dm.pole_diameter / 2.0, y - 0.02, y + 0.03, HOLE);
    }
}

/// Slotted pole screws standing on the top flange at height `y`.
fn screws(spin: &mut Mesh, dm: &Dims, y: f32) {
    let r = dm.pole_diameter / 2.0;
    let top = y + dm.pole_height;
    for (_, x) in poles(dm.pole_count, dm.pole_spread) {
        spin.cylinder(x, 0.0, r, y, top, NICKEL);
        spin.cuboid([x - r, top - 0.4, -0.35], [x + r, top + 0.05, 0.35], HOLE);
    }
}

/// `p` turned by `a` about the origin, then moved to `to`.
fn rot(p: [f32; 2], a: f32, to: [f32; 2]) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [to[0] + p[0] * c - p[1] * s, to[1] + p[0] * s + p[1] * c]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_overrides_the_kind() {
        let mut b = Bobbin::preset("strat").unwrap();
        let u = usual(&b, 6.0);
        assert_eq!(u.pole_count, 6);
        b.detail.pole_count = Some(4);
        b.detail.flange_length_mm = Some(80.0);
        let d = dims(&b, 6.0);
        assert_eq!(d.pole_count, 4);
        assert_eq!(d.flange_length, 80.0);
        assert_eq!(d.flange_width, u.flange_width);
        // Four magnets fewer than six: the model has fewer triangles.
        let six = build(&Bobbin::preset("strat").unwrap(), 4.0, 25.0);
        let four = build(&b, 4.0, 25.0);
        assert!(four.spin.0.len() < six.spin.0.len());
        assert!((four.length - 80.0).abs() < 1e-3);
    }

    #[test]
    fn every_preset_builds_a_closed_model() {
        for name in Bobbin::preset_names() {
            let b = Bobbin::preset(name).unwrap();
            let p = build(&b, 4.0, 25.0);
            for m in [&p.spin, &p.top, &p.still, &p.carriage] {
                assert!(!m.0.is_empty(), "{name}");
                assert_eq!(m.0.len() % (STRIDE * 3), 0, "{name}");
                assert!(m.0.iter().all(|v| v.is_finite()), "{name}");
            }
            // The eyelet clears everything that spins.
            assert!(p.guide_r > p.length / 2.0, "{name}");
        }
    }

    #[test]
    fn rounded_rect_spans_its_size() {
        let o = rounded_rect(60.0, 18.0, 9.0);
        let max_x = o.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        let max_z = o.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((max_x - 30.0).abs() < 1e-4 && (max_z - 9.0).abs() < 1e-4);
        assert!(area(&o) > 0.0);
    }
}
