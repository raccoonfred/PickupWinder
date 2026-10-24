//! The coil in 3D: the bobbin, and the wire as one continuous strand
//! wrapped around the core, every turn where the lay model put it.
//!
//! Drawn with OpenGL through egui's paint callback, with a depth buffer, so
//! every turn of a full coil is real geometry and the outer surface you see
//! is the actual wire: crossing passes, the step of a perfect layer and
//! scatter show the way they do on a wound bobbin. The wire is a ribbon
//! turned to face the camera and shaded as a round wire.

use crate::{machine, model::Computed};
use eframe::{
    egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2},
    egui_glow,
    glow::{self, HasContext},
};
use std::f32::consts::TAU;
use std::{
    f32::consts::{FRAC_PI_2, PI},
    sync::{Arc, Mutex},
};
use winder_core::{Design, Direction};

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
    /// Show the winder: spindle, motor, traverse and guide.
    pub machine: bool,
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
            machine: true,
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
/// Longest step along the core between wire samples, as a share of the way
/// round: one lay station. The rounded ends get a sample every 15° on top.
const MAX_STEP: f32 = 1.0 / 16.0;

/// The core outline: a rounded rectangle, walked from where the wire runs
/// onto the first short end, as the lay model's stations are.
struct Core {
    pts: Vec<[f32; 2]>,
    normals: Vec<[f32; 2]>,
    /// Arc length at each point, as a share of the whole outline.
    u: Vec<f32>,
}

impl Core {
    fn new(width: f32, length: f32, r: f32) -> Core {
        let (a, b) = (length / 2.0, width / 2.0);
        let r = r.clamp(0.05, width.min(length) / 2.0);
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

    /// Where to sample a turn, as shares of the way round: every 15° round
    /// each corner, so a turn is round there however deep it lies, and
    /// evenly along the straight sides.
    fn samples(&self) -> Vec<f32> {
        let mut keys: Vec<f32> = self
            .u
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, &u)| u)
            .collect();
        keys.push(1.0);
        keys.dedup_by(|b, a| (*b - *a).abs() < 1e-6);
        let mut out = Vec::new();
        for w in keys.windows(2) {
            let k = ((w[1] - w[0]) / MAX_STEP).ceil().max(1.0) as usize;
            for j in 0..k {
                out.push(w[0] + (w[1] - w[0]) * j as f32 / k as f32);
            }
        }
        out
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
    /// Solids, pos(3) normal(3) rgba(4), split by how they move.
    parts: machine::Parts,
    turns: usize,
    /// Where each turn is sampled (shares of the way round).
    us: Vec<f32>,
    od: f32,
    channel: f32,
    span_mm: f32,
}

const WIRE_STRIDE: usize = 11;
const SOLID_STRIDE: usize = machine::STRIDE;

/// Where turn `n` (0-based) lies at share `u` of the way round: (y, z),
/// between the lay model's stations.
fn lay_at(cross: &winder_core::CrossSection, n: usize, u: f32) -> (f32, f32) {
    let turns = cross.turn_count();
    let s_n = cross.stations.len();
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
}

fn core_of(d: &Design) -> Core {
    let b = &d.bobbin;
    Core::new(
        b.core_width_mm as f32,
        b.core_length_mm as f32,
        b.corner_r() as f32,
    )
}

impl Geometry {
    pub fn build(d: &Design, c: &Computed) -> Geometry {
        let b = &d.bobbin;
        let h = b.channel_height_mm as f32;
        let core = core_of(d);
        let cross = &c.cross;
        let turns = cross.turn_count();

        let us = core.samples();
        // Counter-clockwise coils are the mirror image (see `Feed::mirrored`).
        let flip = if d.direction == Direction::Ccw {
            -1.0
        } else {
            1.0
        };
        let mut pts: Vec<([f32; 3], [f32; 3], f32)> = Vec::with_capacity(turns * us.len() + 1);
        for n in 0..turns {
            for &u in &us {
                let (y, z) = lay_at(cross, n, u);
                let (p, nn) = core.at(u, z);
                pts.push((
                    [p[0], y, p[1] * flip],
                    [nn[0], 0.0, nn[1] * flip],
                    n as f32 + u,
                ));
            }
        }
        if turns > 0 {
            let (y, z) = lay_at(cross, turns - 1, 0.9999);
            let (p, nn) = core.at(0.9999, z);
            pts.push((
                [p[0], y, p[1] * flip],
                [nn[0], 0.0, nn[1] * flip],
                turns as f32,
            ));
        }
        let mut wire = Vec::with_capacity(pts.len() * 2 * WIRE_STRIDE);
        strip(&mut wire, &pts);

        let parts = machine::build(
            b,
            c.laid.build_mm as f32,
            d.lay.free_span_mm.max(5.0) as f32,
        );
        Geometry {
            wire,
            turns,
            us,
            od: c.cross.od_mm as f32,
            channel: h,
            // Fit the bobbin and the guide beside it.
            span_mm: (parts.length + 8.0).max(2.0 * parts.guide_r + 20.0),
            parts,
        }
    }
}

/// Wire vertices for a run of points: two per point, either side.
fn strip(out: &mut Vec<f32>, pts: &[([f32; 3], [f32; 3], f32)]) {
    for i in 0..pts.len() {
        let a = pts[i.saturating_sub(1)].0;
        let q = pts[(i + 1).min(pts.len() - 1)].0;
        let t = [q[0] - a[0], q[1] - a[1], q[2] - a[2]];
        let (p, nn, turn) = pts[i];
        for side in [-1.0f32, 1.0] {
            out.extend_from_slice(&p);
            out.extend_from_slice(&t);
            out.extend_from_slice(&nn);
            out.push(side);
            out.push(turn);
        }
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
    wire: Buf,
    /// The free wire from the guide's eyelet to the coil, rebuilt each frame.
    free: Buf,
    spin: Buf,
    top: Buf,
    still: Buf,
    carriage: Buf,
    pending: Option<Geometry>,
    od: f32,
    turns: usize,
    us: Vec<f32>,
    channel: f32,
    span_mm: f32,
    guide_r: f32,
    guide_angle: f32,
}

/// One vertex array and its buffer, and how many vertices it holds.
struct Buf {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    count: usize,
}

const WIRE_VS: &str = r#"
uniform mat4 u_mvp;
uniform mat4 u_model;
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
    vec3 pos = (u_model * vec4(a_pos, 1.0)).xyz;
    vec3 tan = mat3(u_model) * a_tan;
    vec3 e = u_persp > 0.5 ? normalize(u_eye - pos) : u_to_eye;
    vec3 s = cross(tan, e);
    float l = length(s);
    s = l > 1e-6 ? s / l : vec3(0.0, 1.0, 0.0);
    vec3 p = pos + s * a_side * u_radius;
    gl_Position = u_mvp * vec4(p, 1.0);
    v_side_dir = s;
    v_nrm = mat3(u_model) * a_nrm;
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
uniform float u_alpha;
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
    out_color = vec4(col, u_alpha);
}
"#;

const SOLID_VS: &str = r#"
uniform mat4 u_mvp;
uniform mat4 u_model;
in vec3 a_pos;
in vec3 a_nrm;
in vec4 a_col;
out vec3 v_nrm;
out vec3 v_pos;
out vec4 v_col;
void main() {
    vec3 pos = (u_model * vec4(a_pos, 1.0)).xyz;
    gl_Position = u_mvp * vec4(pos, 1.0);
    v_nrm = mat3(u_model) * a_nrm;
    v_pos = pos;
    v_col = a_col;
}
"#;

const SOLID_FS: &str = r#"
precision mediump float;
uniform vec3 u_light;
uniform vec3 u_to_eye;
uniform vec3 u_eye;
uniform float u_persp;
uniform float u_alpha;
in vec3 v_nrm;
in vec3 v_pos;
in vec4 v_col;
out vec4 out_color;
void main() {
    vec3 n = normalize(v_nrm);
    vec3 e = u_persp > 0.5 ? normalize(u_eye - v_pos) : u_to_eye;
    if (dot(n, e) < 0.0) n = -n;
    float d = max(dot(n, u_light), 0.0);
    // A little sheen so metal parts read as metal.
    float spec = pow(max(dot(n, normalize(u_light + e)), 0.0), 30.0);
    float shine = smoothstep(0.35, 0.8, dot(v_col.rgb, vec3(0.333)));
    vec3 col = v_col.rgb * (0.38 + 0.62 * d) + vec3(spec * (0.08 + 0.4 * shine));
    out_color = vec4(col, v_col.a * u_alpha);
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

/// Column-major matrix: turn by `theta` about the spindle (y), then lift
/// by `lift`.
fn model(theta: f32, lift: f32) -> [f32; 16] {
    let (s, c) = theta.sin_cos();
    [
        c, 0.0, s, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        -s, 0.0, c, 0.0, //
        0.0, lift, 0.0, 1.0,
    ]
}

/// `p` turned by `theta` about the spindle, as `model` does.
fn spun(p: [f32; 3], theta: f32) -> [f32; 3] {
    let (s, c) = theta.sin_cos();
    [p[0] * c - p[2] * s, p[1], p[0] * s + p[2] * c]
}

impl Renderer {
    pub fn new(gl: &glow::Context) -> Result<Renderer, String> {
        unsafe {
            let wire_prog = program(gl, WIRE_VS, WIRE_FS)?;
            let solid_prog = program(gl, SOLID_VS, SOLID_FS)?;
            let make = |prog, names: &[(&str, i32)]| -> Result<Buf, String> {
                let vao = gl.create_vertex_array()?;
                let vbo = gl.create_buffer()?;
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                attributes(gl, prog, names);
                gl.bind_vertex_array(None);
                Ok(Buf { vao, vbo, count: 0 })
            };
            let wire_layout = [
                ("a_pos", 3),
                ("a_tan", 3),
                ("a_nrm", 3),
                ("a_side", 1),
                ("a_turn", 1),
            ];
            let solid_layout = [("a_pos", 3), ("a_nrm", 3), ("a_col", 4)];
            Ok(Renderer {
                wire: make(wire_prog, &wire_layout)?,
                free: make(wire_prog, &wire_layout)?,
                spin: make(solid_prog, &solid_layout)?,
                top: make(solid_prog, &solid_layout)?,
                still: make(solid_prog, &solid_layout)?,
                carriage: make(solid_prog, &solid_layout)?,
                wire_prog,
                solid_prog,
                pending: None,
                od: 0.07,
                turns: 0,
                us: vec![0.0],
                channel: 10.0,
                span_mm: 60.0,
                guide_r: 50.0,
                guide_angle: 0.0,
            })
        }
    }

    /// Queue new geometry; it is uploaded on the next paint.
    pub fn set(&mut self, g: Geometry) {
        self.od = g.od;
        self.turns = g.turns;
        self.us = g.us.clone();
        self.channel = g.channel;
        self.span_mm = g.span_mm;
        self.guide_r = g.parts.guide_r;
        self.guide_angle = g.parts.guide_angle;
        self.pending = Some(g);
    }

    unsafe fn fill(gl: &glow::Context, b: &mut Buf, data: &[f32], stride: usize, usage: u32) {
        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(b.vbo));
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, usage);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        b.count = data.len() / stride;
    }

    unsafe fn upload(&mut self, gl: &glow::Context) {
        let Some(g) = self.pending.take() else {
            return;
        };
        unsafe {
            let s = glow::STATIC_DRAW;
            Self::fill(gl, &mut self.wire, &g.wire, WIRE_STRIDE * 2, s);
            Self::fill(gl, &mut self.spin, &g.parts.spin.0, SOLID_STRIDE, s);
            Self::fill(gl, &mut self.top, &g.parts.top.0, SOLID_STRIDE, s);
            Self::fill(gl, &mut self.still, &g.parts.still.0, SOLID_STRIDE, s);
            Self::fill(gl, &mut self.carriage, &g.parts.carriage.0, SOLID_STRIDE, s);
        }
    }

    unsafe fn paint(&mut self, gl: &glow::Context, f: &Frame) {
        unsafe {
            self.upload(gl);
            Self::fill(
                gl,
                &mut self.free,
                &f.free,
                WIRE_STRIDE * 2,
                glow::STREAM_DRAW,
            );
            let light = f.light;
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::BLEND);

            let set = |prog: glow::Program, m: [f32; 16], alpha: f32| {
                gl.use_program(Some(prog));
                let u = |n: &str| gl.get_uniform_location(prog, n);
                gl.uniform_matrix_4_f32_slice(u("u_mvp").as_ref(), false, &f.mvp);
                gl.uniform_matrix_4_f32_slice(u("u_model").as_ref(), false, &m);
                gl.uniform_3_f32(u("u_light").as_ref(), light[0], light[1], light[2]);
                let e = f.to_eye;
                gl.uniform_3_f32(u("u_to_eye").as_ref(), e[0], e[1], e[2]);
                let p = f.eye;
                gl.uniform_3_f32(u("u_eye").as_ref(), p[0], p[1], p[2]);
                gl.uniform_1_f32(u("u_persp").as_ref(), f32::from(u8::from(f.perspective)));
                gl.uniform_1_f32(u("u_radius").as_ref(), f.radius);
                gl.uniform_1_f32(u("u_total").as_ref(), self.turns as f32);
                gl.uniform_1_f32(u("u_upto").as_ref(), f.upto);
                gl.uniform_1_f32(u("u_alpha").as_ref(), alpha);
            };
            let draw = |b: &Buf, mode: u32, n: usize| {
                gl.bind_vertex_array(Some(b.vao));
                gl.draw_arrays(mode, 0, n as i32);
            };
            let still = model(0.0, 0.0);
            let points = {
                let up = f.upto.max(0.0);
                let n = up.floor();
                let part = self.us.partition_point(|&u| u <= up - n);
                (n as usize * self.us.len() + part + 1).min(self.wire.count)
            };

            // Parts that don't turn, solid.
            if f.machine {
                set(self.solid_prog, still, 1.0);
                draw(&self.still, glow::TRIANGLES, self.still.count);
                set(self.solid_prog, model(0.0, f.guide_y), 1.0);
                draw(&self.carriage, glow::TRIANGLES, self.carriage.count);
                if self.free.count >= 2 {
                    set(self.wire_prog, still, 1.0);
                    draw(&self.free, glow::TRIANGLE_STRIP, self.free.count * 2);
                }
            }

            // The spindle and everything on it. When it turns further in one
            // frame than the eye can follow, it is drawn as a camera would
            // see it: smeared over the angle it swept during the frame.
            // One copy every ~11°, up to a full turn.
            let ghosts = if f.sweep > BLUR_FROM {
                ((f.sweep.min(TAU) / 0.2).ceil() as usize).clamp(8, BLUR_GHOSTS)
            } else {
                1
            };
            let alpha = if ghosts > 1 {
                1.0 - 0.1f32.powf(1.0 / ghosts as f32)
            } else {
                1.0
            };
            if ghosts > 1 {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.depth_mask(false);
            }
            for k in 0..ghosts {
                let back = f.sweep.min(TAU) * k as f32 / ghosts as f32;
                let spin = model(f.theta - f.dir * back, 0.0);
                set(self.solid_prog, spin, alpha);
                draw(&self.spin, glow::TRIANGLES, self.spin.count);
                if points >= 2 {
                    set(self.wire_prog, spin, alpha);
                    draw(&self.wire, glow::TRIANGLE_STRIP, points * 2);
                }
                if f.top_flange {
                    set(self.solid_prog, spin, alpha);
                    draw(&self.top, glow::TRIANGLES, self.top.count);
                }
            }
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }
}

/// What one paint needs, copied into the callback.
/// Turning more than this in one frame (rad) strobes, so the spindle is
/// drawn blurred, from at most this many copies.
const BLUR_FROM: f32 = 2.0;
const BLUR_GHOSTS: usize = 32;

struct Frame {
    mvp: [f32; 16],
    to_eye: [f32; 3],
    eye: [f32; 3],
    perspective: bool,
    light: [f32; 3],
    radius: f32,
    upto: f32,
    top_flange: bool,
    machine: bool,
    /// Spindle angle (rad), how far it turned since the last frame (rad),
    /// and the guide's height (mm).
    theta: f32,
    sweep: f32,
    /// Which way theta grows as the wind goes on (±1).
    dir: f32,
    guide_y: f32,
    /// Free wire vertices, in the world.
    free: Vec<f32>,
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// The 3D view, navigated the way Fusion's canvas is: scroll zooms at the
/// pointer, the middle button pans, Shift + middle orbits, and a left-drag
/// does whatever the navigation bar has picked (orbit unless told
/// otherwise). The ViewCube sits top right.
#[allow(clippy::too_many_arguments)]
pub fn coil_3d(
    ui: &mut Ui,
    d: &Design,
    c: &Computed,
    upto: f64,
    phase: f32,
    sweep: f32,
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

    let (span, channel, od, turns, guide_r, guide_angle) = {
        let r = renderer.lock().expect("renderer lock");
        (
            r.span_mm,
            r.channel,
            r.od,
            r.turns,
            r.guide_r,
            r.guide_angle,
        )
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
    let upto = upto.clamp(0.0, turns as f64);
    let feed = Feed::at(d, c, upto, phase, guide_r, guide_angle);
    let upto = feed.drawn;
    let mut free = Vec::new();
    strip(&mut free, &feed.free_wire(upto as f32));
    let frame = Frame {
        mvp: view.mvp(rect.size(), cam.pan),
        to_eye: view.rows[2],
        eye: view.eye_pos(),
        perspective: view.eye.is_some(),
        light,
        // Never thinner than about a pixel, so a zoomed-out coil still
        // reads as wire rather than gaps.
        radius: (od / 2.0).max(0.6 / scale),
        upto: upto as f32,
        top_flange: cam.top_flange,
        machine: cam.machine,
        theta: feed.theta,
        sweep,
        dir: feed.dir,
        guide_y: feed.eyelet[1],
        free,
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

    if cam.machine {
        let origin = rect.center() + cam.pan;
        let gp = view.to_screen(feed.eyelet, origin);
        if rect.contains(gp) {
            painter.text(
                gp + Vec2::new(10.0, -10.0),
                Align2::LEFT_BOTTOM,
                "guide",
                FontId::proportional(11.0),
                Color32::from_gray(60),
            );
        }
    }
}

/// Where the wire is fed from at a moment in the wind: the spindle's angle
/// and the guide's eyelet, which stays put except for the traverse.
///
/// The guide doesn't put wire on the coil itself: wire leaves the eyelet
/// and runs straight to where it meets the turning coil, touching it on a
/// tangent. So the bobbin is turned until the last laid point is that
/// tangent point, as seen from the eyelet.
struct Feed {
    theta: f32,
    /// Which way theta grows as the wind goes on (±1).
    dir: f32,
    eyelet: [f32; 3],
    /// Where the wire meets the coil, in the world.
    contact: [f32; 3],
    /// Turns laid so far, to the contact point.
    drawn: f64,
}

/// The point at radius `r` behind the contact at `u`, along the coil's
/// tangent there: where the eyelet must be for the wire to touch at `u`.
fn eyelet_from(core: &Core, u: f32, z: f32, r: f32) -> [f32; 2] {
    let (p, _) = core.at(u, z);
    let (q, _) = core.at(u + 1e-3, z);
    let t = {
        let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
        let l = (dx * dx + dz * dz).sqrt().max(1e-9);
        [dx / l, dz / l]
    };
    let pt = p[0] * t[0] + p[1] * t[1];
    let pp = p[0] * p[0] + p[1] * p[1];
    let back = pt + (pt * pt - pp + r * r).max(0.0).sqrt();
    [p[0] - t[0] * back, p[1] - t[1] * back]
}

impl Feed {
    /// The feed at `upto` turns, with the spindle `phase` of the way
    /// through the turn under way (0..1).
    ///
    /// The spindle turns steadily, so the angle is the clock, not the share
    /// of the wire laid. Along a long side the tangent from the eyelet
    /// barely moves while the side sweeps past, so that whole side is laid
    /// in a moment; round the ends the contact creeps. So the contact point
    /// is found from the angle, not the other way round.
    fn at(d: &Design, c: &Computed, upto: f64, phase: f32, guide_r: f32, guide_angle: f32) -> Feed {
        let core = core_of(d);
        let total = c.cross.turn_count();
        let guide_y = guide_at(c, upto) as f32;
        let eyelet = [
            guide_r * guide_angle.cos(),
            guide_y,
            guide_r * guide_angle.sin(),
        ];
        if total == 0 {
            return Feed {
                theta: 0.0,
                dir: 1.0,
                eyelet,
                contact: eyelet,
                drawn: 0.0,
            };
        }
        let done = upto >= total as f64;
        let n = (upto.floor() as usize).min(total - 1);
        // Where the eyelet sits, seen from the bobbin, for each contact
        // point round turn n: it only ever moves one way, once round.
        const K: usize = 256;
        let mut table = Vec::with_capacity(K + 1);
        let mut last: Option<f32> = None;
        for k in 0..=K {
            let u = k as f32 / K as f32 * 0.9999;
            let (_, z) = lay_at(&c.cross, n, u);
            let g = eyelet_from(&core, u, z, guide_r);
            let mut a = g[1].atan2(g[0]);
            if let Some(prev) = last {
                a = prev + wrap_angle(a - prev);
            }
            table.push((u, a));
            last = Some(a);
        }
        let (a0, a1) = (table[0].1, table[K].1);
        let want = a0 + (a1 - a0) * phase.clamp(0.0, 0.9999);
        let i = table.partition_point(|&(_, a)| a < want).clamp(1, K);
        let ((u0, b0), (u1, b1)) = (table[i - 1], table[i]);
        let u = if b1 > b0 {
            u0 + (u1 - u0) * (want - b0) / (b1 - b0)
        } else {
            u1
        };
        let (y, z) = lay_at(&c.cross, n, u);
        let (p, _) = core.at(u, z);
        let theta = guide_angle - want;
        let drawn = if done {
            total as f64
        } else {
            n as f64 + u as f64
        };
        // The guide is where the toolpath has it at this moment.
        let eyelet = [eyelet[0], guide_at(c, drawn) as f32, eyelet[2]];
        let feed = Feed {
            theta,
            dir: if a1 >= a0 { -1.0 } else { 1.0 },
            eyelet,
            contact: spun([p[0], y, p[1]], theta),
            drawn,
        };
        if d.direction == Direction::Ccw {
            feed.mirrored(guide_angle)
        } else {
            feed
        }
    }

    /// The same moment wound the other way. A counter-clockwise coil is the
    /// mirror image of a clockwise one, so the scene is reflected across the
    /// plane through the spindle and the eyelet: the eyelet stays where it
    /// is, the spindle turns the other way and the wire meets the coil on
    /// the bobbin's other side. (The coil is drawn mirrored to match.)
    fn mirrored(self, guide_angle: f32) -> Feed {
        let (s2, c2) = (2.0 * guide_angle).sin_cos();
        let [x, y, z] = self.contact;
        Feed {
            theta: 2.0 * guide_angle - self.theta,
            dir: -self.dir,
            contact: [x * c2 + z * s2, y, x * s2 - z * c2],
            ..self
        }
    }

    /// The wire from the spool's side, through the eyelet, to the coil.
    fn free_wire(&self, turn: f32) -> Vec<([f32; 3], [f32; 3], f32)> {
        // Down from the tensioner above, round the eyelet, out to the coil.
        let e = self.eyelet;
        let from = [e[0], e[1] + 90.0, e[2]];
        let up = [0.0, 1.0, 0.0];
        vec![(from, up, turn), (e, up, turn), (self.contact, up, turn)]
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
    let size = Vec2::new(510.0, 30.0);
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
                .on_hover_text("Show the top flange or flatwork");
            ui.toggle_value(&mut cam.machine, "Machine")
                .on_hover_text("Show the winder: spindle, motor, traverse and guide");
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
    fn turns_are_round_at_the_ends() {
        // A Strat core: the round ends are a small share of the way round,
        // but still get a sample every 15°.
        let core = Core::new(4.9, 56.9, 2.45);
        let us = core.samples();
        assert!(us.windows(2).all(|w| w[1] > w[0]));
        let on_end = |u: &&f32| {
            let (p, _) = core.at(**u, 0.0);
            p[0].abs() > 56.9 / 2.0 - 2.45 + 1e-3
        };
        let n = us.iter().filter(on_end).count();
        assert!(n >= 2 * 11, "{n} samples on the ends of {}", us.len());
        assert!(us.len() <= 64, "{}", us.len());
    }

    #[test]
    fn wire_is_one_strip_through_every_turn() {
        let d = Design::example("paf").unwrap();
        let c = Computed::new(&d).unwrap();
        let g = Geometry::build(&d, &c);
        let points = g.wire.len() / WIRE_STRIDE / 2;
        assert_eq!(points, 5000 * g.us.len() + 1);
        assert!(g.wire.iter().all(|v| v.is_finite()));
        assert_eq!(g.parts.spin.0.len() % (SOLID_STRIDE * 3), 0);
        // Every wire point sits between the flanges and outside the core.
        for v in g.wire.chunks(WIRE_STRIDE) {
            assert!((-1e-3..=6.5 + 1e-3).contains(&v[1]), "{}", v[1]);
        }
    }

    #[test]
    fn wire_reaches_the_coil_on_a_tangent_from_the_eyelet() {
        // And counter-clockwise turns the spindle the other way.
        let cw = tangent_from_the_eyelet(Direction::Cw);
        let ccw = tangent_from_the_eyelet(Direction::Ccw);
        assert_eq!(cw, -ccw);
    }

    /// Checks the feed one way round, and says which way the spindle turns.
    fn tangent_from_the_eyelet(dir: Direction) -> f32 {
        let mut d = Design::example("strat").unwrap();
        d.direction = dir;
        let flip = if dir == Direction::Ccw { -1.0 } else { 1.0 };
        let mut spins = Vec::new();
        let c = Computed::new(&d).unwrap();
        let g = Geometry::build(&d, &c);
        let (r, a) = (g.parts.guide_r, g.parts.guide_angle);
        let core = core_of(&d);
        let mut last = None;
        for (turn, phase) in [(10.0, 0.1), (10.0, 0.3), (1234.0, 0.6), (4000.0, 0.95)] {
            let f = Feed::at(&d, &c, turn, phase, r, a);
            spins.push(f.dir);
            // A later moment never lays less wire.
            if let Some((t, dr)) = last
                && t == turn
            {
                assert!(f.drawn >= dr);
            }
            last = Some((turn, f.drawn));
            let upto = f.drawn;
            // The eyelet stays where the machine has it.
            let er = (f.eyelet[0].powi(2) + f.eyelet[2].powi(2)).sqrt();
            assert!((er - r).abs() < 1e-3);
            // From the eyelet the wire runs along the coil's tangent at the
            // contact, in the direction the wire is being laid.
            let (n, u) = (upto.floor() as usize, upto.fract() as f32);
            let (_, z) = lay_at(&c.cross, n, u);
            let (p0, _) = core.at(u, z);
            let (p1, _) = core.at(u + 1e-3, z);
            let t = spun([p1[0] - p0[0], 0.0, (p1[1] - p0[1]) * flip], f.theta);
            // The contact is on the (mirrored) coil as drawn.
            let on = spun([p0[0], 0.0, p0[1] * flip], f.theta);
            assert!(
                (on[0] - f.contact[0]).abs() < 0.05 && (on[2] - f.contact[2]).abs() < 0.05,
                "{dir:?} upto {upto}"
            );
            let w = [f.contact[0] - f.eyelet[0], 0.0, f.contact[2] - f.eyelet[2]];
            let (lt, lw) = (
                (t[0] * t[0] + t[2] * t[2]).sqrt(),
                (w[0] * w[0] + w[2] * w[2]).sqrt(),
            );
            let cos = (t[0] * w[0] + t[2] * w[2]) / (lt * lw);
            assert!(cos > 0.995, "{dir:?} upto {upto}: cos {cos}");
        }
        // The spindle turns one way all through.
        assert!(spins.iter().all(|&s| s == spins[0]));
        spins[0]
    }

    #[test]
    fn core_outline_closes_and_starts_on_the_short_end() {
        let core = Core::new(16.0, 46.0, 4.8);
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
