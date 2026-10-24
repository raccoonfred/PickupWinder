//! The ViewCube, after Autodesk's: a labelled cube in the corner of the 3D
//! view that turns with the model. Click a face, an edge or a corner to
//! swing round to that view, drag the cube to orbit, drag the compass ring
//! to turn about the bobbin's axis, and the house goes home. Looking
//! straight at a face, arrows step to the next face over.

use crate::view3d::Camera;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, epaint, pos2, vec2,
};

/// Autodesk's highlight blue.
pub const BLUE: Color32 = Color32::from_rgb(6, 150, 215);
const HOVER: Color32 = Color32::from_rgb(128, 196, 236);
const EDGE: Color32 = Color32::from_gray(128);
const LABEL: Color32 = Color32::from_gray(70);

/// Pixels from the cube's centre to a face.
const HALF: f32 = 36.0;
const RING_R: f32 = 1.55;
/// Where the edge and corner strips start, in face units (−1..1).
const STRIP: f32 = 0.62;

type V3 = [f32; 3];

struct Face {
    normal: V3,
    /// Text right and text up on the face.
    a: V3,
    b: V3,
    label: &'static str,
}

/// World y is the bobbin's axis. FRONT is the +z long side, RIGHT the +x
/// short end, as on the compass: S at the front, E on the right.
const FACES: [Face; 6] = [
    Face {
        normal: [0.0, 0.0, 1.0],
        a: [1.0, 0.0, 0.0],
        b: [0.0, 1.0, 0.0],
        label: "FRONT",
    },
    Face {
        normal: [1.0, 0.0, 0.0],
        a: [0.0, 0.0, -1.0],
        b: [0.0, 1.0, 0.0],
        label: "RIGHT",
    },
    Face {
        normal: [0.0, 0.0, -1.0],
        a: [-1.0, 0.0, 0.0],
        b: [0.0, 1.0, 0.0],
        label: "BACK",
    },
    Face {
        normal: [-1.0, 0.0, 0.0],
        a: [0.0, 0.0, 1.0],
        b: [0.0, 1.0, 0.0],
        label: "LEFT",
    },
    Face {
        normal: [0.0, 1.0, 0.0],
        a: [1.0, 0.0, 0.0],
        b: [0.0, 0.0, -1.0],
        label: "TOP",
    },
    Face {
        normal: [0.0, -1.0, 0.0],
        a: [1.0, 0.0, 0.0],
        b: [0.0, 0.0, 1.0],
        label: "BOTTOM",
    },
];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(p: V3, q: V3, k: f32) -> V3 {
    [p[0] + q[0] * k, p[1] + q[1] * k, p[2] + q[2] * k]
}

/// The camera's screen right, up and toward-the-eye axes in world space.
fn axes(cam: &Camera) -> [V3; 3] {
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    [
        [cy, 0.0, -sy],
        [-sy * sp, cp, -cy * sp],
        [sy * cp, sp, cy * cp],
    ]
}

/// One clickable patch: a face centre, an edge strip or a corner square.
struct Cell {
    quad: [Pos2; 4],
    /// Direction it looks from, with components in {-1, 0, 1}.
    dir: [i8; 3],
}

/// What the pointer is over.
#[derive(Clone, Copy, PartialEq)]
enum Hot {
    Cell([i8; 3]),
    Ring,
    Home,
    Arrow(V3),
}

/// The view cube's corner of `canvas`. Call after the canvas has been
/// allocated, so the cube sits on top of it for the pointer.
pub fn view_cube(ui: &mut Ui, canvas: Rect, cam: &mut Camera) {
    let center = pos2(canvas.right() - 104.0, canvas.top() + 100.0);
    let area = Rect::from_center_size(center, Vec2::splat(HALF * 5.6));
    let resp = ui.interact(
        area,
        ui.scope_id().with("view-cube"),
        Sense::click_and_drag(),
    );
    let ring_id = ui.scope_id().with("view-cube-ring");
    let painter = ui.painter_at(area.expand(4.0));
    let [right, up, eye] = axes(cam);
    let project = |p: V3| {
        pos2(
            center.x + dot(right, p) * HALF,
            center.y - dot(up, p) * HALF,
        )
    };
    let pointer = ui.input(|i| i.pointer.hover_pos());

    // Visible faces cut into the 3×3 hot zones.
    let cuts = [-1.0, -STRIP, STRIP, 1.0];
    let mut cells = Vec::new();
    for f in FACES.iter().filter(|f| dot(f.normal, eye) > 1e-3) {
        for i in 0..3 {
            for j in 0..3 {
                let corner = |u: f32, v: f32| project(add(add(f.normal, f.a, u), f.b, v));
                let (u0, u1, v0, v1) = (cuts[i], cuts[i + 1], cuts[j], cuts[j + 1]);
                let d = add(add(f.normal, f.a, i as f32 - 1.0), f.b, j as f32 - 1.0);
                let dir = d.map(|v| v.round() as i8);
                cells.push(Cell {
                    quad: [
                        corner(u0, v0),
                        corner(u1, v0),
                        corner(u1, v1),
                        corner(u0, v1),
                    ],
                    dir,
                });
            }
        }
    }

    let ring_pt = |a: f32| [RING_R * a.sin(), -1.0, RING_R * a.cos()];
    let ring: Vec<(Pos2, Pos2, f32)> = (0..72)
        .map(|k| {
            let (a0, a1) = (
                k as f32 / 72.0 * std::f32::consts::TAU,
                (k + 1) as f32 / 72.0 * std::f32::consts::TAU,
            );
            let mid = ring_pt((a0 + a1) / 2.0);
            (project(ring_pt(a0)), project(ring_pt(a1)), dot(mid, eye))
        })
        .collect();

    let home_at = pos2(center.x - HALF * 2.15, center.y - HALF * 2.05);
    let face_on = FACES.iter().find(|f| dot(f.normal, eye) > 0.9995);
    let arrows: Vec<(V3, [Pos2; 3])> = if face_on.is_some() {
        [(right, vec2(1.0, 0.0)), (up, vec2(0.0, -1.0))]
            .into_iter()
            .flat_map(|(w, s)| [(w, s), ([-w[0], -w[1], -w[2]], -s)])
            .map(|(w, s)| {
                let tip = center + s * HALF * 2.05;
                let base = center + s * HALF * 1.6;
                let n = vec2(-s.y, s.x) * 7.0;
                (snap(w), [tip, base + n, base - n])
            })
            .collect()
    } else {
        Vec::new()
    };

    let hot = pointer.filter(|p| area.contains(*p)).and_then(|p| {
        if (p - home_at).length() < 10.0 {
            return Some(Hot::Home);
        }
        if let Some((w, _)) = arrows.iter().find(|(_, t)| inside(p, t)) {
            return Some(Hot::Arrow(*w));
        }
        if let Some(c) = cells.iter().find(|c| inside(p, &c.quad)) {
            return Some(Hot::Cell(c.dir));
        }
        let near_ring = ring.iter().any(|(a, b, _)| seg_dist(p, *a, *b) < 6.0);
        near_ring.then_some(Hot::Ring)
    });
    // While dragging, the thing grabbed stays lit.
    let grabbed = ui.memory(|m| m.data.get_temp::<bool>(ring_id));
    if resp.drag_started() {
        let ring_grab = hot == Some(Hot::Ring);
        ui.memory_mut(|m| m.data.insert_temp(ring_id, ring_grab));
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        if grabbed == Some(true) {
            // Turning the compass turns the model about its axis.
            cam.orbit(vec2(d.x, 0.0));
        } else {
            cam.orbit(d);
        }
    }
    if resp.clicked() {
        match hot {
            Some(Hot::Cell(dir)) => cam.look_from(dir.map(f32::from)),
            Some(Hot::Arrow(w)) => cam.look_from(w),
            Some(Hot::Home) => cam.home(),
            _ => {}
        }
    }
    let lit_ring = hot == Some(Hot::Ring) || (resp.dragged() && grabbed == Some(true));

    // Back half of the ring, behind the cube.
    let ring_col = if lit_ring {
        HOVER
    } else {
        Color32::from_gray(196)
    };
    for (a, b, z) in &ring {
        if *z < 0.0 {
            painter.line_segment([*a, *b], Stroke::new(6.0, ring_col));
        }
    }
    compass(&painter, project, eye, false);
    let lit = match hot {
        Some(Hot::Cell(d)) => Some(d),
        _ => None,
    };
    // Each face shaded by how squarely it faces you, with the hovered patch
    // lit across every face it touches.
    for f in FACES.iter().filter(|f| dot(f.normal, eye) > 1e-3) {
        let facing = dot(f.normal, eye);
        let g = (226.0 + 26.0 * facing) as u8;
        let quad = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(u, v)| project(add(add(f.normal, f.a, u), f.b, v)));
        painter.add(Shape::convex_polygon(
            quad.to_vec(),
            Color32::from_gray(g),
            Stroke::NONE,
        ));
    }
    for c in &cells {
        if Some(c.dir) == lit {
            painter.add(Shape::convex_polygon(c.quad.to_vec(), HOVER, Stroke::NONE));
        }
    }
    for f in FACES.iter().filter(|f| dot(f.normal, eye) > 1e-3) {
        let facing = dot(f.normal, eye);
        let quad = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(u, v)| project(add(add(f.normal, f.a, u), f.b, v)));
        painter.add(Shape::closed_line(quad.to_vec(), Stroke::new(1.0, EDGE)));
        if facing > 0.28 {
            // The label lies along the face: turned with it, and shrunk the
            // more the face turns away.
            let mid = project(f.normal);
            let along = project(add(f.normal, f.a, 1.0)) - mid;
            let size = (HALF * 0.34 * facing.sqrt()).max(8.0);
            let galley = ui.painter().layout_no_wrap(
                f.label.to_string(),
                FontId::proportional(size),
                LABEL.gamma_multiply(((facing - 0.28) / 0.3).min(1.0)),
            );
            let pos = mid - galley.size() / 2.0;
            let angle = along.y.atan2(along.x);
            painter.add(
                epaint::TextShape::new(pos, galley, LABEL)
                    .with_angle_and_anchor(angle, Align2::CENTER_CENTER),
            );
        }
    }
    // Front half of the ring, and the compass points on it.
    for (a, b, z) in &ring {
        if *z >= 0.0 {
            painter.line_segment([*a, *b], Stroke::new(6.0, ring_col));
        }
    }
    compass(&painter, project, eye, true);

    // Home.
    let home_col = if hot == Some(Hot::Home) {
        BLUE
    } else {
        Color32::from_gray(110)
    };
    house(&painter, home_at, home_col);

    for (w, tri) in &arrows {
        let col = if hot == Some(Hot::Arrow(*w)) {
            BLUE
        } else {
            Color32::from_gray(150)
        };
        painter.add(Shape::convex_polygon(tri.to_vec(), col, Stroke::NONE));
    }

    if hot.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let tip = match hot {
        Some(Hot::Cell(d)) => Some(cell_name(d)),
        Some(Hot::Ring) => Some("Drag to turn about the bobbin's axis".to_string()),
        Some(Hot::Home) => Some("Home view".to_string()),
        Some(Hot::Arrow(_)) => Some("Next face over".to_string()),
        None => None,
    };
    if let Some(t) = tip {
        resp.on_hover_text_at_pointer(t);
    }
}

/// The compass points on the ring: the ones behind the cube are drawn
/// first, faint, so the cube covers them.
fn compass(painter: &egui::Painter, project: impl Fn(V3) -> Pos2, eye: V3, front: bool) {
    for (letter, a) in [
        ("S", 0.0f32),
        ("E", std::f32::consts::FRAC_PI_2),
        ("N", std::f32::consts::PI),
        ("W", -std::f32::consts::FRAC_PI_2),
    ] {
        let p = [RING_R * a.sin(), -1.0, RING_R * a.cos()];
        if (dot(p, eye) >= 0.0) != front {
            continue;
        }
        painter.text(
            project(p),
            Align2::CENTER_CENTER,
            letter,
            FontId::proportional(10.5),
            if front {
                Color32::from_gray(55)
            } else {
                Color32::from_gray(165)
            },
        );
    }
}

/// An axis direction rounded to the nearest whole axis.
fn snap(w: V3) -> V3 {
    let k = (0..3)
        .max_by(|&i, &j| w[i].abs().total_cmp(&w[j].abs()))
        .unwrap_or(0);
    let mut out = [0.0; 3];
    out[k] = w[k].signum();
    out
}

fn cell_name(d: [i8; 3]) -> String {
    let mut parts = Vec::new();
    match d[1] {
        1 => parts.push("top"),
        -1 => parts.push("bottom"),
        _ => {}
    }
    match d[2] {
        1 => parts.push("front"),
        -1 => parts.push("back"),
        _ => {}
    }
    match d[0] {
        1 => parts.push("right"),
        -1 => parts.push("left"),
        _ => {}
    }
    let mut s = parts.join(" ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    s
}

fn house(painter: &egui::Painter, at: Pos2, col: Color32) {
    let s = 7.0;
    let roof = vec![
        at + vec2(-s - 1.5, -0.5),
        at + vec2(0.0, -s - 1.0),
        at + vec2(s + 1.5, -0.5),
    ];
    painter.add(Shape::line(roof, Stroke::new(1.6, col)));
    let body = vec![
        at + vec2(-s + 1.5, -1.5),
        at + vec2(-s + 1.5, s - 1.0),
        at + vec2(s - 1.5, s - 1.0),
        at + vec2(s - 1.5, -1.5),
    ];
    painter.add(Shape::line(body, Stroke::new(1.6, col)));
    painter.rect_filled(
        Rect::from_min_size(at + vec2(-1.5, 1.5), vec2(3.0, s - 2.5)),
        0.0,
        col,
    );
}

/// Inside a convex polygon, either winding.
fn inside(p: Pos2, poly: &[Pos2]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    sign != 0.0
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_label_upright_and_cover_26_views() {
        let mut dirs = std::collections::HashSet::new();
        for f in &FACES {
            // a × b is the outward normal, so labels read from outside.
            let n = [
                f.a[1] * f.b[2] - f.a[2] * f.b[1],
                f.a[2] * f.b[0] - f.a[0] * f.b[2],
                f.a[0] * f.b[1] - f.a[1] * f.b[0],
            ];
            assert_eq!(n, f.normal, "{}", f.label);
            for i in -1..=1 {
                for j in -1..=1 {
                    let d: Vec<i8> = (0..3)
                        .map(|k| (f.normal[k] + f.a[k] * i as f32 + f.b[k] * j as f32) as i8)
                        .collect();
                    dirs.insert(d);
                }
            }
        }
        assert_eq!(dirs.len(), 26);
    }

    #[test]
    fn clicking_a_face_looks_straight_at_it() {
        let mut cam = Camera::default();
        cam.look_from([1.0, 0.0, 0.0]);
        while cam.step(0.05) {}
        let e = axes(&cam)[2];
        assert!((e[0] - 1.0).abs() < 1e-4 && e[1].abs() < 1e-4, "{e:?}");
        cam.look_from([0.0, 1.0, 0.0]);
        while cam.step(0.05) {}
        let [right, up, eye] = axes(&cam);
        assert!((eye[1] - 1.0).abs() < 1e-4);
        // From the top the cube stays square to the screen.
        assert!(
            right
                .iter()
                .chain(up.iter())
                .all(|v| v.abs() < 1e-4 || (v.abs() - 1.0).abs() < 1e-4)
        );
        assert_eq!(cell_name([1, 1, 1]), "Top front right");
    }

    #[test]
    fn hit_test_works_both_windings() {
        let sq = [
            pos2(0.0, 0.0),
            pos2(1.0, 0.0),
            pos2(1.0, 1.0),
            pos2(0.0, 1.0),
        ];
        let mut rev = sq;
        rev.reverse();
        for poly in [sq, rev] {
            assert!(inside(pos2(0.5, 0.5), &poly));
            assert!(!inside(pos2(1.5, 0.5), &poly));
        }
    }
}
