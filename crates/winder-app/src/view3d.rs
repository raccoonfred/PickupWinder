//! The coil in 3D: the bobbin, and the wire as one continuous strand
//! wrapped around the core, every turn where the lay model put it.
//!
//! Drawn with OpenGL through egui's paint callback, with a depth buffer, so
//! every turn of a full coil is real geometry and the outer surface you see
//! is the actual wire: crossing passes, the step of a perfect layer and
//! scatter show the way they do on a wound bobbin. The wire is a ribbon
//! turned to face the camera and shaded as a round wire.

use crate::model::Computed;
use eframe::{
    egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2},
    egui_glow,
    glow::{self, HasContext},
};
use std::{
    f32::consts::{FRAC_PI_2, PI},
    sync::{Arc, Mutex},
};
use winder_core::Design;

/// What a left-drag does in the 3D view, picked on the navigation bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Orbit,
    Pan,
    Zoom,
}

/// Orbit camera. Angles in radians; `zoom` multiplies the fitted scale.
/// Yaw 0 looks at the FRONT (the +z long side), pitch π/2 from the TOP.
#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
    pub top_flange: bool,
    pub perspective: bool,
    pub tool: Tool,
    anim: Option<Anim>,
}

/// A camera move in progress, eased from one pose to another.
#[derive(Clone, Copy, PartialEq)]
struct Anim {
    from: Pose,
    to: Pose,
    t: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct Pose {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: Vec2,
}

/// The home view: front, right and top all in sight, as in Fusion.
pub const HOME_YAW: f32 = std::f32::consts::FRAC_PI_4;
pub const HOME_PITCH: f32 = 0.615_479_7; // asin(1/√3)

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: HOME_YAW,
            pitch: HOME_PITCH,
            zoom: 1.0,
            pan: Vec2::ZERO,
            top_flange: true,
            perspective: true,
            tool: Tool::Orbit,
            anim: None,
        }
    }
}

impl Camera {
    fn pose(&self) -> Pose {
        Pose {
            yaw: self.yaw,
            pitch: self.pitch,
            zoom: self.zoom,
            pan: self.pan,
        }
    }

    /// Swing round to look along `-dir` (so `dir` points at the eye),
    /// keeping zoom and pan.
    pub fn look_from(&mut self, dir: [f32; 3]) {
        let (yaw, pitch) = angles_of(dir, self.yaw);
        let to = Pose {
            yaw,
            pitch,
            ..self.pose()
        };
        self.animate(to);
    }

    /// Back to the home view, fitted.
    pub fn home(&mut self) {
        self.animate(Pose {
            yaw: HOME_YAW,
            pitch: HOME_PITCH,
            zoom: 1.0,
            pan: Vec2::ZERO,
        });
    }

    /// Fit the whole coil in the view, keeping the direction.
    pub fn fit(&mut self) {
        self.animate(Pose {
            zoom: 1.0,
            pan: Vec2::ZERO,
            ..self.pose()
        });
    }

    fn animate(&mut self, mut to: Pose) {
        let from = self.pose();
        // The short way round.
        to.yaw = from.yaw + wrap_angle(to.yaw - from.yaw);
        self.anim = Some(Anim { from, to, t: 0.0 });
    }

    pub fn orbit(&mut self, d: Vec2) {
        self.anim = None;
        self.yaw = wrap_angle(self.yaw - d.x * 0.008);
        self.pitch = (self.pitch + d.y * 0.008).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    /// Zoom by `factor` about `at`, given relative to the view centre.
    pub fn zoom_about(&mut self, factor: f32, at: Vec2) {
        self.anim = None;
        let old = self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.2, 400.0);
        self.pan -= (at - self.pan) * (self.zoom / old - 1.0);
    }

    /// Move an animation on by `dt` seconds; true while one is running.
    pub fn step(&mut self, dt: f32) -> bool {
        let Some(a) = &mut self.anim else {
            return false;
        };
        a.t = (a.t + dt / 0.45).min(1.0);
        let e = a.t * a.t * (3.0 - 2.0 * a.t);
        let (f, t) = (a.from, a.to);
        let lerp = |x: f32, y: f32| x + (y - x) * e;
        self.yaw = lerp(f.yaw, t.yaw);
        self.pitch = lerp(f.pitch, t.pitch);
        // Zoom eases in log space, so it doesn't lurch.
        self.zoom = (lerp(f.zoom.ln(), t.zoom.ln())).exp();
        self.pan = f.pan + (t.pan - f.pan) * e;
        if a.t >= 1.0 {
            self.yaw = wrap_angle(self.yaw);
            self.anim = None;
        }
        true
    }
}

/// Yaw and pitch that look from `dir`. Straight down or up keeps the
/// current yaw, squared to the nearest side so the cube reads upright.
fn angles_of(dir: [f32; 3], yaw_now: f32) -> (f32, f32) {
    let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
        .sqrt()
        .max(1e-9);
    let pitch = (dir[1] / l).clamp(-1.0, 1.0).asin();
    let flat = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt() / l;
    let yaw = if flat < 1e-4 {
        // Ties go toward the front, so TOP from home has FRONT at the bottom.
        ((yaw_now - 1e-3) / FRAC_PI_2).round() * FRAC_PI_2
    } else {
        dir[0].atan2(dir[2])
    };
    (yaw, pitch)
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// Points per turn along the wire.
const SAMPLES: usize = 40;
/// Flatwork thickness (mm), as on a Fender-style bobbin.
const FLANGE_MM: f32 = 1.6;

/// The core outline: a rounded rectangle, walked from where the wire runs
/// onto the first short end, as the lay model's stations are.
struct Core {
    pts: Vec<[f32; 2]>,
    normals: Vec<[f32; 2]>,
    /// Arc length at each point, as a share of the whole outline.
    u: Vec<f32>,
}

impl Core {
    fn new(width: f32, length: f32) -> Core {
        let (a, b) = (length / 2.0, width / 2.0);
        let r = (width.min(length) * 0.3).max(0.5);
        let corners = [
            ([a - r, b - r], 0.0f32),
            ([-a + r, b - r], 90.0),
            ([-a + r, -b + r], 180.0),
            ([a - r, -b + r], 270.0),
        ];
        let mut pts = vec![[a, -b + r]];
        let mut normals = vec![[1.0, 0.0]];
        for (c, start) in corners {
            for k in 0..=12 {
                let ang = (start + 90.0 * k as f32 / 12.0).to_radians();
                let n = [ang.cos(), ang.sin()];
                pts.push([c[0] + r * n[0], c[1] + r * n[1]]);
                normals.push(n);
            }
        }
        pts.push([a, -b + r]);
        normals.push([1.0, 0.0]);
        let mut len = vec![0.0f32];
        for w in pts.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            len.push(len.last().unwrap() + d);
        }
        let total = *len.last().unwrap();
        Core {
            pts,
            normals,
            u: len.iter().map(|l| l / total).collect(),
        }
    }

    /// Point `h` mm out from the core at share `u` of the way round, and the
    /// outward normal there, as world (x, z).
    fn at(&self, u: f32, h: f32) -> ([f32; 2], [f32; 2]) {
        let u = u.rem_euclid(1.0);
        let i = self
            .u
            .partition_point(|&x| x <= u)
            .clamp(1, self.u.len() - 1);
        let (u0, u1) = (self.u[i - 1], self.u[i]);
        let f = if u1 > u0 { (u - u0) / (u1 - u0) } else { 0.0 };
        let lerp = |p: [f32; 2], q: [f32; 2]| [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f];
        let p = lerp(self.pts[i - 1], self.pts[i]);
        let n = lerp(self.normals[i - 1], self.normals[i]);
        let l = (n[0] * n[0] + n[1] * n[1]).sqrt().max(1e-6);
        let n = [n[0] / l, n[1] / l];
        ([p[0] + n[0] * h, p[1] + n[1] * h], n)
    }
}

/// Everything uploaded to the GPU for one design.
pub struct Geometry {
    /// Wire: per vertex pos(3) tangent(3) normal(3) side(1) turn(1).
    wire: Vec<f32>,
    /// Bobbin, opaque, then the top flange: pos(3) normal(3) rgba(4).
    solid: Vec<f32>,
    top: Vec<f32>,
    turns: usize,
    od: f32,
    channel: f32,
    span_mm: f32,
}

const WIRE_STRIDE: usize = 11;
const SOLID_STRIDE: usize = 10;

impl Geometry {
    pub fn build(d: &Design, c: &Computed) -> Geometry {
        let b = &d.bobbin;
        let (w, l, h) = (
            b.core_width_mm as f32,
            b.core_length_mm as f32,
            b.channel_height_mm as f32,
        );
        let core = Core::new(w, l);
        let cross = &c.cross;
        let turns = cross.turn_count();
        let s_n = cross.stations.len();

        // Position of turn n (0-based) at share u, between the stations.
        let turn_at = |n: usize, u: f32| -> (f32, f32) {
            let pos = u * s_n as f32 - 0.5;
            let mut n0 = n as i64;
            let mut j0 = pos.floor() as i64;
            if j0 < 0 {
                j0 += s_n as i64;
                n0 -= 1;
            }
            let (mut n1, mut j1) = (n0, j0 + 1);
            if j1 >= s_n as i64 {
                j1 -= s_n as i64;
                n1 += 1;
            }
            let clamp = |k: i64| k.clamp(0, turns as i64 - 1) as usize;
            let f = (pos - pos.floor()) as f64;
            let a = cross.cuts[j0 as usize][clamp(n0)];
            let q = cross.cuts[j1 as usize][clamp(n1)];
            (
                (a.y_mm + (q.y_mm - a.y_mm) * f) as f32,
                (a.z_mm + (q.z_mm - a.z_mm) * f) as f32,
            )
        };
        let mut pts: Vec<([f32; 3], [f32; 3], f32)> = Vec::with_capacity(turns * SAMPLES + 1);
        for n in 0..turns {
            for s in 0..SAMPLES {
                let u = s as f32 / SAMPLES as f32;
                let (y, z) = turn_at(n, u);
                let (p, nn) = core.at(u, z);
                pts.push(([p[0], y, p[1]], [nn[0], 0.0, nn[1]], n as f32 + u));
            }
        }
        if turns > 0 {
            let (y, z) = turn_at(turns - 1, 0.9999);
            let (p, nn) = core.at(0.9999, z);
            pts.push(([p[0], y, p[1]], [nn[0], 0.0, nn[1]], turns as f32));
        }
        let mut wire = Vec::with_capacity(pts.len() * 2 * WIRE_STRIDE);
        for i in 0..pts.len() {
            let a = pts[i.saturating_sub(1)].0;
            let q = pts[(i + 1).min(pts.len() - 1)].0;
            let t = [q[0] - a[0], q[1] - a[1], q[2] - a[2]];
            let (p, nn, turn) = pts[i];
            for side in [-1.0f32, 1.0] {
                wire.extend_from_slice(&p);
                wire.extend_from_slice(&t);
                wire.extend_from_slice(&nn);
                wire.push(side);
                wire.push(turn);
            }
        }

        // Bobbin: core wall and two flatwork plates.
        let mut solid = Vec::new();
        let mut top = Vec::new();
        let ring = 96;
        let core_col = [0.55, 0.55, 0.58, 1.0];
        for i in 0..ring {
            let (u0, u1) = (i as f32 / ring as f32, (i + 1) as f32 / ring as f32);
            let (p0, n0) = core.at(u0, 0.0);
            let (p1, n1) = core.at(u1, 0.0);
            let v = |p: [f32; 2], n: [f32; 2], y: f32| ([p[0], y, p[1]], [n[0], 0.0, n[1]]);
            tri(
                &mut solid,
                [v(p0, n0, 0.0), v(p1, n1, 0.0), v(p1, n1, h)],
                core_col,
            );
            tri(
                &mut solid,
                [v(p0, n0, 0.0), v(p1, n1, h), v(p0, n0, h)],
                core_col,
            );
        }
        let reach = (b.build_depth_mm as f32).max(c.laid.build_mm as f32) + 1.5;
        let fiber = [0.13, 0.13, 0.14, 1.0];
        plate(&mut solid, &core, -FLANGE_MM, 0.0, reach, fiber);
        plate(
            &mut top,
            &core,
            h,
            h + FLANGE_MM,
            reach,
            [0.13, 0.13, 0.14, 0.35],
        );

        Geometry {
            wire,
            solid,
            top,
            turns,
            od: c.cross.od_mm as f32,
            channel: h,
            span_mm: (l + 2.0 * reach).max(h * 1.6),
        }
    }
}

type V = ([f32; 3], [f32; 3]);

fn tri(out: &mut Vec<f32>, v: [V; 3], col: [f32; 4]) {
    for (p, n) in v {
        out.extend_from_slice(&p);
        out.extend_from_slice(&n);
        out.extend_from_slice(&col);
    }
}

/// A flatwork plate from `y0` to `y1`, reaching `reach` mm beyond the core.
fn plate(out: &mut Vec<f32>, core: &Core, y0: f32, y1: f32, reach: f32, col: [f32; 4]) {
    let ring = 96;
    let up = [0.0, 1.0, 0.0];
    let down = [0.0, -1.0, 0.0];
    for i in 0..ring {
        let (u0, u1) = (i as f32 / ring as f32, (i + 1) as f32 / ring as f32);
        let (o0, n0) = core.at(u0, reach);
        let (o1, n1) = core.at(u1, reach);
        for (y, n) in [(y1, up), (y0, down)] {
            tri(
                out,
                [
                    ([0.0, y, 0.0], n),
                    ([o0[0], y, o0[1]], n),
                    ([o1[0], y, o1[1]], n),
                ],
                col,
            );
        }
        let s0 = [n0[0], 0.0, n0[1]];
        let s1 = [n1[0], 0.0, n1[1]];
        tri(
            out,
            [
                ([o0[0], y0, o0[1]], s0),
                ([o1[0], y0, o1[1]], s1),
                ([o1[0], y1, o1[1]], s1),
            ],
            col,
        );
        tri(
            out,
            [
                ([o0[0], y0, o0[1]], s0),
                ([o1[0], y1, o1[1]], s1),
                ([o0[0], y1, o0[1]], s0),
            ],
            col,
        );
    }
}

/// Camera transform shared by the GPU and the overlay drawn with egui.
struct View {
    rows: [[f32; 3]; 3],
    center: [f32; 3],
    /// Pixels per mm at the centre's depth.
    scale: f32,
    /// Eye distance from the centre (mm) for perspective; None for an
    /// orthographic view.
    eye: Option<f32>,
}

impl View {
    fn new(cam: &Camera, mid_y: f32, scale: f32, span_mm: f32) -> View {
        let (sy, cy) = cam.yaw.sin_cos();
        let (sp, cp) = cam.pitch.sin_cos();
        View {
            rows: [
                [cy, 0.0, -sy],
                [-sy * sp, cp, -cy * sp],
                [sy * cp, sp, cy * cp],
            ],
            center: [0.0, mid_y, 0.0],
            scale,
            // About a 30° lens at the fitted zoom: some depth, little
            // distortion. Zooming narrows the lens rather than moving the
            // eye, so the coil never passes the near plane.
            eye: cam.perspective.then_some(span_mm * 2.4),
        }
    }

    fn to_view(&self, p: [f32; 3]) -> [f32; 3] {
        let q = [
            p[0] - self.center[0],
            p[1] - self.center[1],
            p[2] - self.center[2],
        ];
        let r = &self.rows;
        [
            r[0][0] * q[0] + r[0][1] * q[1] + r[0][2] * q[2],
            r[1][0] * q[0] + r[1][1] * q[1] + r[1][2] * q[2],
            r[2][0] * q[0] + r[2][1] * q[1] + r[2][2] * q[2],
        ]
    }

    /// The eye's position in the world, for perspective.
    fn eye_pos(&self) -> [f32; 3] {
        let d = self.eye.unwrap_or(1e4);
        let r = self.rows[2];
        [
            self.center[0] + r[0] * d,
            self.center[1] + r[1] * d,
            self.center[2] + r[2] * d,
        ]
    }

    /// Clip-space matrix (column-major) for a viewport of `size` px, with
    /// the scene shifted by `pan` px.
    fn mvp(&self, size: Vec2, pan: Vec2) -> [f32; 16] {
        let sx = self.scale / (size.x / 2.0);
        let sy = self.scale / (size.y / 2.0);
        let (px, py) = (pan.x / (size.x / 2.0), -pan.y / (size.y / 2.0));
        // World to view (mm, eye looking down -z).
        let r = &self.rows;
        let c = self.center;
        let dot = |row: [f32; 3]| row[0] * c[0] + row[1] * c[1] + row[2] * c[2];
        let view = [
            [r[0][0], r[0][1], r[0][2], -dot(r[0])],
            [r[1][0], r[1][1], r[1][2], -dot(r[1])],
            [r[2][0], r[2][1], r[2][2], -dot(r[2])],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let proj = match self.eye {
            None => {
                let depth = 1.0 / 400.0;
                [
                    [sx, 0.0, 0.0, px],
                    [0.0, sy, 0.0, py],
                    [0.0, 0.0, -depth, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]
            }
            Some(d) => {
                // w = distance from the eye / d, so the centre's depth keeps
                // the orthographic scale.
                let (n, f) = (d * 0.25, d * 2.5);
                let a = (f + n) / (f - n);
                let b = 2.0 * f * n / (f - n);
                [
                    [sx, 0.0, -px / d, px],
                    [0.0, sy, -py / d, py],
                    [0.0, 0.0, -a / d, a - b / d],
                    [0.0, 0.0, -1.0 / d, 1.0],
                ]
            }
        };
        let mut m = [[0.0f32; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..4).map(|k| proj[i][k] * view[k][j]).sum();
            }
        }
        let mut out = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                out[col * 4 + row] = m[row][col];
            }
        }
        out
    }

    fn to_screen(&self, p: [f32; 3], origin: Pos2) -> Pos2 {
        let v = self.to_view(p);
        let k = self.eye.map_or(1.0, |d| d / (d - v[2]).max(1e-3));
        pos2(
            origin.x + v[0] * self.scale * k,
            origin.y - v[1] * self.scale * k,
        )
    }
}

/// GPU side: programs and buffers, shared with the paint callback.
pub struct Renderer {
    wire_prog: glow::Program,
    solid_prog: glow::Program,
    wire_vao: glow::VertexArray,
    wire_vbo: glow::Buffer,
    solid_vao: glow::VertexArray,
    solid_vbo: glow::Buffer,
    top_vao: glow::VertexArray,
    top_vbo: glow::Buffer,
    wire_points: usize,
    solid_verts: usize,
    top_verts: usize,
    pending: Option<Geometry>,
    od: f32,
    turns: usize,
    channel: f32,
    span_mm: f32,
}

const WIRE_VS: &str = r#"
uniform mat4 u_mvp;
uniform vec3 u_to_eye;
uniform vec3 u_eye;
uniform float u_persp;
uniform float u_radius;
in vec3 a_pos;
in vec3 a_tan;
in vec3 a_nrm;
in float a_side;
in float a_turn;
out vec3 v_side_dir;
out vec3 v_nrm;
out vec3 v_to_eye;
out float v_side;
out float v_turn;
void main() {
    vec3 e = u_persp > 0.5 ? normalize(u_eye - a_pos) : u_to_eye;
    vec3 s = cross(a_tan, e);
    float l = length(s);
    s = l > 1e-6 ? s / l : vec3(0.0, 1.0, 0.0);
    vec3 p = a_pos + s * a_side * u_radius;
    gl_Position = u_mvp * vec4(p, 1.0);
    v_side_dir = s;
    v_nrm = a_nrm;
    v_to_eye = e;
    v_side = a_side;
    v_turn = a_turn;
}
"#;

const WIRE_FS: &str = r#"
precision mediump float;
uniform vec3 u_light;
uniform float u_total;
uniform float u_upto;
in vec3 v_side_dir;
in vec3 v_nrm;
in vec3 v_to_eye;
in float v_side;
in float v_turn;
out vec4 out_color;
void main() {
    vec3 to_eye = normalize(v_to_eye);
    float k = clamp(v_side, -1.0, 1.0);
    float c = sqrt(max(0.0, 1.0 - k * k));
    // A round wire seen side on: across the ribbon the surface turns from
    // one side, through facing the eye, to the other.
    vec3 n = normalize(v_side_dir * k + normalize(to_eye + 0.6 * v_nrm) * c);
    float diff = max(dot(n, u_light), 0.0);
    vec3 h = normalize(u_light + to_eye);
    float spec = pow(max(dot(n, h), 0.0), 40.0);
    // Dark copper for the first turns, light for the last; the newest
    // turns glow so the wire being laid stands out while playing.
    float age = clamp(v_turn / max(u_total, 1.0), 0.0, 1.0);
    vec3 base = mix(vec3(0.42, 0.20, 0.08), vec3(0.93, 0.56, 0.28), age);
    float fresh = clamp(1.0 - (u_upto - v_turn) / 3.0, 0.0, 1.0);
    base = mix(base, vec3(1.0, 0.85, 0.55), fresh);
    vec3 col = base * (0.25 + 0.85 * diff) + vec3(1.0, 0.9, 0.75) * spec * 0.55;
    out_color = vec4(col, 1.0);
}
"#;

const SOLID_VS: &str = r#"
uniform mat4 u_mvp;
in vec3 a_pos;
in vec3 a_nrm;
in vec4 a_col;
out vec3 v_nrm;
out vec4 v_col;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_nrm = a_nrm;
    v_col = a_col;
}
"#;

const SOLID_FS: &str = r#"
precision mediump float;
uniform vec3 u_light;
in vec3 v_nrm;
in vec4 v_col;
out vec4 out_color;
void main() {
    float d = abs(dot(normalize(v_nrm), u_light));
    out_color = vec4(v_col.rgb * (0.4 + 0.6 * d), v_col.a);
}
"#;

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    unsafe {
        let version = egui_glow::ShaderVersion::get(gl);
        if !version.is_new_shader_interface() {
            return Err(format!("OpenGL too old for the 3D view ({version:?})"));
        }
        let prog = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, &format!("{}\n{}", version.version_declaration(), src));
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(gl.get_shader_info_log(sh));
            }
            gl.attach_shader(prog, sh);
            shaders.push(sh);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(gl.get_program_info_log(prog));
        }
        for sh in shaders {
            gl.detach_shader(prog, sh);
            gl.delete_shader(sh);
        }
        Ok(prog)
    }
}

/// Points the program's float attributes at a buffer of back-to-back
/// vertices.
unsafe fn attributes(gl: &glow::Context, prog: glow::Program, names: &[(&str, i32)]) {
    unsafe {
        let stride: i32 = names.iter().map(|(_, n)| n).sum::<i32>() * 4;
        let mut offset = 0;
        for (name, n) in names {
            if let Some(loc) = gl.get_attrib_location(prog, name) {
                gl.enable_vertex_attrib_array(loc);
                gl.vertex_attrib_pointer_f32(loc, *n, glow::FLOAT, false, stride, offset);
            }
            offset += n * 4;
        }
    }
}

impl Renderer {
    pub fn new(gl: &glow::Context) -> Result<Renderer, String> {
        unsafe {
            let wire_prog = program(gl, WIRE_VS, WIRE_FS)?;
            let solid_prog = program(gl, SOLID_VS, SOLID_FS)?;
            let make = |prog, names: &[(&str, i32)]| -> Result<_, String> {
                let vao = gl.create_vertex_array()?;
                let vbo = gl.create_buffer()?;
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                attributes(gl, prog, names);
                gl.bind_vertex_array(None);
                Ok((vao, vbo))
            };
            let wire_layout = [
                ("a_pos", 3),
                ("a_tan", 3),
                ("a_nrm", 3),
                ("a_side", 1),
                ("a_turn", 1),
            ];
            let solid_layout = [("a_pos", 3), ("a_nrm", 3), ("a_col", 4)];
            let (wire_vao, wire_vbo) = make(wire_prog, &wire_layout)?;
            let (solid_vao, solid_vbo) = make(solid_prog, &solid_layout)?;
            let (top_vao, top_vbo) = make(solid_prog, &solid_layout)?;
            Ok(Renderer {
                wire_prog,
                solid_prog,
                wire_vao,
                wire_vbo,
                solid_vao,
                solid_vbo,
                top_vao,
                top_vbo,
                wire_points: 0,
                solid_verts: 0,
                top_verts: 0,
                pending: None,
                od: 0.07,
                turns: 0,
                channel: 10.0,
                span_mm: 60.0,
            })
        }
    }

    /// Queue new geometry; it is uploaded on the next paint.
    pub fn set(&mut self, g: Geometry) {
        self.od = g.od;
        self.turns = g.turns;
        self.channel = g.channel;
        self.span_mm = g.span_mm;
        self.pending = Some(g);
    }

    unsafe fn upload(&mut self, gl: &glow::Context) {
        let Some(g) = self.pending.take() else {
            return;
        };
        unsafe {
            for (vbo, data) in [
                (self.wire_vbo, &g.wire),
                (self.solid_vbo, &g.solid),
                (self.top_vbo, &g.top),
            ] {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            }
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.wire_points = g.wire.len() / WIRE_STRIDE / 2;
        self.solid_verts = g.solid.len() / SOLID_STRIDE;
        self.top_verts = g.top.len() / SOLID_STRIDE;
    }

    unsafe fn paint(&mut self, gl: &glow::Context, f: &Frame) {
        unsafe {
            self.upload(gl);
            let light = f.light;
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::BLEND);

            let set = |prog: glow::Program| {
                gl.use_program(Some(prog));
                let u = |n: &str| gl.get_uniform_location(prog, n);
                gl.uniform_matrix_4_f32_slice(u("u_mvp").as_ref(), false, &f.mvp);
                gl.uniform_3_f32(u("u_light").as_ref(), light[0], light[1], light[2]);
                let e = f.to_eye;
                gl.uniform_3_f32(u("u_to_eye").as_ref(), e[0], e[1], e[2]);
                let p = f.eye;
                gl.uniform_3_f32(u("u_eye").as_ref(), p[0], p[1], p[2]);
                gl.uniform_1_f32(u("u_persp").as_ref(), f32::from(u8::from(f.perspective)));
                gl.uniform_1_f32(u("u_radius").as_ref(), f.radius);
                gl.uniform_1_f32(u("u_total").as_ref(), self.turns as f32);
                gl.uniform_1_f32(u("u_upto").as_ref(), f.upto);
            };

            set(self.solid_prog);
            gl.bind_vertex_array(Some(self.solid_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, self.solid_verts as i32);

            // The wire up to the scrub point, as one strip.
            let points = ((f.upto.max(0.0) * SAMPLES as f32) as usize + 1).min(self.wire_points);
            if points >= 2 {
                set(self.wire_prog);
                gl.bind_vertex_array(Some(self.wire_vao));
                gl.draw_arrays(glow::TRIANGLE_STRIP, 0, (points * 2) as i32);
            }

            if f.top_flange {
                set(self.solid_prog);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.depth_mask(false);
                gl.bind_vertex_array(Some(self.top_vao));
                gl.draw_arrays(glow::TRIANGLES, 0, self.top_verts as i32);
                gl.depth_mask(true);
            }
            gl.bind_vertex_array(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }
}

/// What one paint needs, copied into the callback.
struct Frame {
    mvp: [f32; 16],
    to_eye: [f32; 3],
    eye: [f32; 3],
    perspective: bool,
    light: [f32; 3],
    radius: f32,
    upto: f32,
    top_flange: bool,
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// The 3D view, navigated the way Fusion's canvas is: scroll zooms at the
/// pointer, the middle button pans, Shift + middle orbits, and a left-drag
/// does whatever the navigation bar has picked (orbit unless told
/// otherwise). The ViewCube sits top right.
pub fn coil_3d(
    ui: &mut Ui,
    d: &Design,
    c: &Computed,
    upto: usize,
    cam: &mut Camera,
    renderer: Option<&Arc<Mutex<Renderer>>>,
) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    backdrop(&painter, rect);
    let Some(renderer) = renderer else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "The 3D view needs OpenGL 3, which isn't available here.",
            FontId::proportional(14.0),
            Color32::from_gray(90),
        );
        return;
    };

    let (shift, dt, scroll, pinch) = ui.input(|i| {
        (
            i.modifiers.shift,
            i.stable_dt.min(0.1),
            i.smooth_scroll_delta.y,
            i.zoom_delta(),
        )
    });
    use egui::PointerButton::{Middle, Primary, Secondary};
    let dl = resp.drag_delta();
    if resp.dragged_by(Middle) && shift {
        cam.orbit(dl);
    } else if resp.dragged_by(Middle) || resp.dragged_by(Secondary) {
        cam.pan += dl;
        cam.anim = None;
    } else if resp.dragged_by(Primary) {
        match cam.tool {
            Tool::Orbit => cam.orbit(dl),
            Tool::Pan => {
                cam.pan += dl;
                cam.anim = None;
            }
            Tool::Zoom => cam.zoom_about((-dl.y * 0.01).exp(), Vec2::ZERO),
        }
    }
    if resp.hovered() {
        let at = resp.hover_pos().map_or(Vec2::ZERO, |p| p - rect.center());
        if scroll != 0.0 {
            // Zoom about the pointer, so you can dive onto one spot.
            cam.zoom_about((scroll * 0.004).exp(), at);
        }
        if pinch != 1.0 {
            cam.zoom_about(pinch, at);
        }
    }
    if resp.double_clicked_by(Middle) || resp.double_clicked_by(Primary) {
        cam.fit();
    }
    if resp.dragged() {
        ui.ctx()
            .set_cursor_icon(match (cam.tool, resp.dragged_by(Primary)) {
                (Tool::Pan, true) => egui::CursorIcon::Grabbing,
                (Tool::Zoom, true) => egui::CursorIcon::ResizeVertical,
                _ if resp.dragged_by(Middle) && !shift => egui::CursorIcon::Grabbing,
                _ if resp.dragged_by(Secondary) => egui::CursorIcon::Grabbing,
                _ => egui::CursorIcon::AllScroll,
            });
    }

    if cam.step(dt) {
        ui.ctx().request_repaint();
    }

    let (span, channel, od, turns) = {
        let r = renderer.lock().expect("renderer lock");
        (r.span_mm, r.channel, r.od, r.turns)
    };
    let scale = rect.width().min(rect.height()) / span * 0.9 * cam.zoom;
    let view = View::new(cam, channel / 2.0, scale, span);
    let r = &view.rows;
    // A light over the viewer's shoulder, so whatever you turn to is lit.
    let light = norm([
        0.35 * r[0][0] + 0.6 * r[1][0] + 0.72 * r[2][0],
        0.35 * r[0][1] + 0.6 * r[1][1] + 0.72 * r[2][1],
        0.35 * r[0][2] + 0.6 * r[1][2] + 0.72 * r[2][2],
    ]);
    let frame = Frame {
        mvp: view.mvp(rect.size(), cam.pan),
        to_eye: view.rows[2],
        eye: view.eye_pos(),
        perspective: view.eye.is_some(),
        light,
        // Never thinner than about a pixel, so a zoomed-out coil still
        // reads as wire rather than gaps.
        radius: (od / 2.0).max(0.6 / scale),
        upto: upto.min(turns) as f32,
        top_flange: cam.top_flange,
    };
    let r = renderer.clone();
    painter.add(egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |_info, p| {
            let mut r = r.lock().expect("renderer lock");
            unsafe { r.paint(p.gl(), &frame) };
        })),
    });

    // Drawn over the coil, so after it; a change they make shows next
    // frame.
    let before = *cam;
    crate::viewcube::view_cube(ui, rect, cam);
    nav_bar(ui, rect, cam);
    if *cam != before {
        ui.ctx().request_repaint();
    }

    // The guide and the free wire to the lay point, while winding.
    let origin = rect.center() + cam.pan;
    let n = upto.min(c.cross.turn_count());
    if n > 0 && n < c.cross.turn_count() {
        let side = c.cross.side_station();
        let t = c.cross.cuts[side][n - 1];
        let core = Core::new(
            d.bobbin.core_width_mm as f32,
            d.bobbin.core_length_mm as f32,
        );
        let u = c.cross.stations[side].u as f32;
        let (p, nn) = core.at(u, t.z_mm as f32);
        let lay = [p[0], t.y_mm as f32, p[1]];
        let span = d.lay.free_span_mm.max(5.0) as f32;
        let g = [
            p[0] + nn[0] * span,
            guide_at(c, n as f64) as f32,
            p[1] + nn[1] * span,
        ];
        let (gp, lp) = (view.to_screen(g, origin), view.to_screen(lay, origin));
        if rect.contains(gp) || rect.contains(lp) {
            painter.line_segment([gp, lp], Stroke::new(1.0, Color32::from_rgb(214, 120, 48)));
            painter.circle_filled(gp, 5.0, Color32::from_rgb(255, 122, 77));
            painter.text(
                gp + Vec2::new(8.0, -8.0),
                Align2::LEFT_BOTTOM,
                "guide",
                FontId::proportional(11.0),
                Color32::from_gray(60),
            );
        }
    }
}

/// Fusion's canvas: a soft grey-blue fading to white at the top.
fn backdrop(painter: &egui::Painter, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    let top = Color32::from_rgb(252, 253, 254);
    let bottom = Color32::from_rgb(196, 206, 218);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The navigation bar along the bottom of the canvas.
fn nav_bar(ui: &mut Ui, canvas: Rect, cam: &mut Camera) {
    let size = Vec2::new(440.0, 30.0);
    let bar = Rect::from_center_size(pos2(canvas.center().x, canvas.bottom() - 26.0), size);
    ui.painter().rect(
        bar,
        4.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, 236),
        Stroke::new(1.0, Color32::from_gray(200)),
        egui::StrokeKind::Inside,
    );
    let inner = bar.shrink2(Vec2::new(8.0, 3.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, label, tip) in [
                (
                    Tool::Orbit,
                    "Orbit",
                    "Left-drag turns the view (Shift + middle-drag too)",
                ),
                (
                    Tool::Pan,
                    "Pan",
                    "Left-drag moves the view (middle-drag too)",
                ),
                (
                    Tool::Zoom,
                    "Zoom",
                    "Left-drag up and down zooms (scroll zooms at the pointer)",
                ),
            ] {
                ui.selectable_value(&mut cam.tool, tool, label)
                    .on_hover_text(tip);
            }
            ui.separator();
            if ui
                .button("Fit")
                .on_hover_text("Fit the coil to the view (double-click the middle button)")
                .clicked()
            {
                cam.fit();
            }
            if ui.button("Home").on_hover_text("Home view").clicked() {
                cam.home();
            }
            ui.separator();
            let label = if cam.perspective {
                "Perspective"
            } else {
                "Orthographic"
            };
            if ui
                .selectable_label(false, label)
                .on_hover_text("Switch between perspective and orthographic")
                .clicked()
            {
                cam.perspective = !cam.perspective;
            }
            ui.separator();
            ui.toggle_value(&mut cam.top_flange, "Top flange")
                .on_hover_text("Show the top flatwork, see-through");
        },
    );
}

/// Guide position at `t` turns, from the thinned lay plot.
fn guide_at(c: &Computed, t: f64) -> f64 {
    let lay = &c.lay;
    let i = lay.partition_point(|&(tt, _)| tt < t);
    if i == 0 {
        return lay.first().map_or(0.0, |p| p.1);
    }
    if i >= lay.len() {
        return lay.last().map_or(0.0, |p| p.1);
    }
    let (t0, y0) = lay[i - 1];
    let (t1, y1) = lay[i];
    if t1 > t0 {
        y0 + (y1 - y0) * (t - t0) / (t1 - t0)
    } else {
        y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_is_one_strip_through_every_turn() {
        let d = Design::example("paf").unwrap();
        let c = Computed::new(&d).unwrap();
        let g = Geometry::build(&d, &c);
        let points = g.wire.len() / WIRE_STRIDE / 2;
        assert_eq!(points, 5000 * SAMPLES + 1);
        assert!(g.wire.iter().all(|v| v.is_finite()));
        assert_eq!(g.solid.len() % (SOLID_STRIDE * 3), 0);
        // Every wire point sits between the flanges and outside the core.
        for v in g.wire.chunks(WIRE_STRIDE) {
            assert!((-1e-3..=6.5 + 1e-3).contains(&v[1]), "{}", v[1]);
        }
    }

    #[test]
    fn core_outline_closes_and_starts_on_the_short_end() {
        let core = Core::new(16.0, 46.0);
        let (p0, n0) = core.at(0.0, 0.0);
        let (p1, _) = core.at(0.99999, 0.0);
        assert!((p0[0] - 23.0).abs() < 1e-3 && n0 == [1.0, 0.0]);
        assert!((p0[0] - p1[0]).abs() < 1e-2 && (p0[1] - p1[1]).abs() < 1e-2);
        let (out, _) = core.at(0.3, 2.0);
        let (inn, _) = core.at(0.3, 0.0);
        let d = ((out[0] - inn[0]).powi(2) + (out[1] - inn[1]).powi(2)).sqrt();
        assert!((d - 2.0).abs() < 1e-4);
    }
}
