//! Fusion-style chrome: the light theme, toolbar buttons with drawn icons,
//! browser rows and the playback buttons. Icons are drawn with shapes, not
//! font glyphs, so they look the same on every system.

use crate::viewcube::BLUE;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
    Vec2, pos2, vec2,
};

pub const INK: Color32 = Color32::from_gray(48);
pub const MUTED: Color32 = Color32::from_gray(120);
pub const HOVER_FILL: Color32 = Color32::from_rgb(229, 241, 250);
pub const SELECT_FILL: Color32 = Color32::from_rgb(204, 230, 246);
pub const COPPER: Color32 = Color32::from_rgb(196, 112, 52);
pub const PANEL: Color32 = Color32::from_rgb(245, 246, 247);

/// Fusion's light look: pale grey panels, white fields and Autodesk blue
/// for selection.
pub fn fusion_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = egui::Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(252, 252, 253);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_gray(240);
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(200));
    v.window_corner_radius = CornerRadius::same(4);
    v.window_shadow.blur = 14;
    v.window_shadow.color = Color32::from_black_alpha(40);
    v.popup_shadow.color = Color32::from_black_alpha(30);
    v.selection.bg_fill = SELECT_FILL;
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0, 84, 140));
    v.hyperlink_color = BLUE;
    v.widgets.hovered.weak_bg_fill = HOVER_FILL;
    v.widgets.hovered.bg_fill = HOVER_FILL;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(130, 190, 230));
    v.widgets.active.weak_bg_fill = SELECT_FILL;
    v.widgets.active.bg_fill = SELECT_FILL;
    v.widgets.inactive.weak_bg_fill = Color32::from_gray(236);
    v.widgets.noninteractive.fg_stroke.color = INK;
    v.widgets.inactive.fg_stroke.color = INK;
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(6.0, 2.0);
    });
}

/// Toolbar icons.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Layer,
    Helix,
    Scatter,
    Coil3d,
    Section,
    Lay,
    Build,
    Export,
    Machine,
}

/// A big toolbar button: icon over a caption.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(62.0, 50.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 3.0, SELECT_FILL);
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, HOVER_FILL);
    }
    let art = Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), vec2(28.0, 22.0));
    draw_icon(p, icon, art);
    p.text(
        pos2(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        INK,
    );
    resp
}

/// A toolbar group: its buttons, with the caption under them, Fusion
/// style.
pub fn group(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add(ui)
        });
        let w = row.response.rect.width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
        let galley =
            ui.painter()
                .layout_no_wrap(caption.to_string(), FontId::proportional(10.5), MUTED);
        let x = r.center().x - (galley.size().x + 10.0) / 2.0;
        let y = r.center().y - galley.size().y / 2.0;
        let gw = galley.size().x;
        ui.painter().galley(pos2(x, y), galley, MUTED);
        // The little drop arrow after the caption.
        let a = pos2(x + gw + 6.0, r.center().y);
        ui.painter().add(Shape::convex_polygon(
            vec![
                a + vec2(-3.0, -1.5),
                a + vec2(3.0, -1.5),
                a + vec2(0.0, 2.0),
            ],
            MUTED,
            Stroke::NONE,
        ));
    });
}

pub fn draw_icon(p: &egui::Painter, icon: Icon, r: Rect) {
    let line = |pts: Vec<Pos2>, col: Color32, w: f32| {
        p.add(Shape::line(pts, Stroke::new(w, col)));
    };
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let grey = Color32::from_gray(90);
    match icon {
        Icon::Layer => {
            // Wires packed in perfect layers.
            for row in 0..3 {
                for k in 0..5 {
                    let x = 0.14 + k as f32 * 0.18 + if row % 2 == 1 { 0.09 } else { 0.0 };
                    if x > 0.95 {
                        continue;
                    }
                    p.circle(
                        at(x, 0.82 - row as f32 * 0.3),
                        3.2,
                        COPPER,
                        Stroke::new(0.8, Color32::from_rgb(120, 60, 20)),
                    );
                }
            }
        }
        Icon::Helix => {
            line(vec![at(0.0, 0.95), at(1.0, 0.95)], grey, 1.0);
            let pts = (0..=4)
                .map(|k| at(k as f32 * 0.25, if k % 2 == 0 { 0.8 } else { 0.1 }))
                .collect();
            line(pts, COPPER, 2.4);
        }
        Icon::Scatter => {
            line(vec![at(0.0, 0.95), at(1.0, 0.95)], grey, 1.0);
            line(
                vec![
                    at(0.0, 0.7),
                    at(0.35, 0.1),
                    at(0.55, 0.85),
                    at(0.8, 0.25),
                    at(1.0, 0.6),
                ],
                COPPER,
                2.0,
            );
            line(
                vec![at(0.05, 0.2), at(0.3, 0.75), at(0.7, 0.05), at(0.95, 0.85)],
                Color32::from_rgb(222, 150, 90),
                1.6,
            );
        }
        Icon::Coil3d => {
            let s = Stroke::new(1.4, grey);
            let c = r.center();
            let (w, h) = (9.0, 5.0);
            let top = [
                c + vec2(0.0, -h * 2.0),
                c + vec2(w, -h),
                c + vec2(0.0, 0.0),
                c + vec2(-w, -h),
            ];
            p.add(Shape::convex_polygon(
                top.to_vec(),
                Color32::from_rgb(200, 226, 244),
                s,
            ));
            p.add(Shape::convex_polygon(
                vec![
                    top[3],
                    top[2],
                    top[2] + vec2(0.0, 11.0),
                    top[3] + vec2(0.0, 11.0),
                ],
                Color32::from_gray(225),
                s,
            ));
            p.add(Shape::convex_polygon(
                vec![
                    top[2],
                    top[1],
                    top[1] + vec2(0.0, 11.0),
                    top[2] + vec2(0.0, 11.0),
                ],
                Color32::from_gray(205),
                s,
            ));
        }
        Icon::Section => {
            p.rect_filled(Rect::from_min_max(at(0.0, 0.0), at(0.12, 1.0)), 0.0, grey);
            for row in 0..3 {
                for k in 0..4 {
                    let x = 0.26 + k as f32 * 0.2 + if row % 2 == 1 { 0.1 } else { 0.0 };
                    p.circle_filled(at(x, 0.2 + row as f32 * 0.3), 3.0, COPPER);
                }
            }
        }
        Icon::Lay => {
            line(vec![at(0.0, 0.0), at(0.0, 1.0), at(1.0, 1.0)], grey, 1.0);
            line(
                vec![
                    at(0.05, 0.85),
                    at(0.3, 0.1),
                    at(0.55, 0.85),
                    at(0.8, 0.1),
                    at(1.0, 0.6),
                ],
                COPPER,
                1.8,
            );
        }
        Icon::Build => {
            line(vec![at(0.0, 0.0), at(0.0, 1.0), at(1.0, 1.0)], grey, 1.0);
            for (k, h) in [0.35f32, 0.6, 0.8, 0.55].iter().enumerate() {
                let x = 0.12 + k as f32 * 0.22;
                p.rect_filled(
                    Rect::from_min_max(at(x, 1.0 - h), at(x + 0.15, 0.97)),
                    1.0,
                    COPPER,
                );
            }
        }
        Icon::Export => {
            let c = r.center();
            line(
                vec![
                    c + vec2(-10.0, 2.0),
                    c + vec2(-10.0, 10.0),
                    c + vec2(10.0, 10.0),
                    c + vec2(10.0, 2.0),
                ],
                grey,
                1.6,
            );
            line(vec![c + vec2(0.0, -10.0), c + vec2(0.0, 4.0)], BLUE, 2.4);
            p.add(Shape::convex_polygon(
                vec![
                    c + vec2(-6.0, -1.0),
                    c + vec2(6.0, -1.0),
                    c + vec2(0.0, 6.0),
                ],
                BLUE,
                Stroke::NONE,
            ));
        }
        Icon::Machine => {
            let c = r.center();
            p.rect(
                Rect::from_center_size(c, vec2(20.0, 20.0)),
                2.0,
                Color32::from_gray(225),
                Stroke::new(1.4, grey),
                egui::StrokeKind::Inside,
            );
            p.circle(c, 5.5, Color32::WHITE, Stroke::new(1.4, grey));
            p.circle_filled(c, 1.8, grey);
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                p.circle_filled(c + vec2(dx * 7.0, dy * 7.0), 1.2, grey);
            }
        }
    }
}

/// Playback buttons in the status bar.
#[derive(Clone, Copy)]
pub enum Glyph {
    ToStart,
    Play,
    Pause,
    ToEnd,
}

pub fn glyph_button(ui: &mut Ui, g: Glyph) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 3.0, HOVER_FILL);
    }
    let col = if resp.hovered() {
        BLUE
    } else {
        Color32::from_gray(70)
    };
    let c = rect.center();
    let tri = |dir: f32, x: f32| {
        Shape::convex_polygon(
            vec![
                c + vec2(x - 4.0 * dir, -5.0),
                c + vec2(x + 4.0 * dir, 0.0),
                c + vec2(x - 4.0 * dir, 5.0),
            ],
            col,
            Stroke::NONE,
        )
    };
    let bar = |x: f32| {
        Shape::rect_filled(
            Rect::from_center_size(c + vec2(x, 0.0), vec2(2.0, 10.0)),
            0.0,
            col,
        )
    };
    match g {
        Glyph::ToStart => {
            p.add(bar(-5.0));
            p.add(tri(-1.0, 1.0));
        }
        Glyph::Play => {
            p.add(tri(1.0, 0.5));
        }
        Glyph::Pause => {
            p.add(Shape::rect_filled(
                Rect::from_center_size(c + vec2(-3.0, 0.0), vec2(3.0, 10.0)),
                0.0,
                col,
            ));
            p.add(Shape::rect_filled(
                Rect::from_center_size(c + vec2(3.0, 0.0), vec2(3.0, 10.0)),
                0.0,
                col,
            ));
        }
        Glyph::ToEnd => {
            p.add(tri(1.0, -1.0));
            p.add(bar(5.0));
        }
    }
    resp
}

/// What a browser row shows at its left.
#[derive(Clone, Copy)]
pub enum Mark {
    Folder,
    Settings,
    Feature(Icon),
}

/// One row in the browser tree.
pub fn browser_row(
    ui: &mut Ui,
    indent: f32,
    mark: Mark,
    label: &str,
    detail: &str,
    selected: bool,
) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 2.0, SELECT_FILL);
    } else if resp.hovered() {
        p.rect_filled(rect, 2.0, HOVER_FILL);
    }
    let icon = Rect::from_center_size(
        pos2(rect.left() + indent + 8.0, rect.center().y),
        vec2(14.0, 12.0),
    );
    match mark {
        Mark::Folder => {
            p.rect(
                icon,
                1.5,
                Color32::from_rgb(242, 206, 120),
                Stroke::new(1.0, Color32::from_rgb(190, 150, 60)),
                egui::StrokeKind::Inside,
            );
        }
        Mark::Settings => {
            p.circle(icon.center(), 5.0, Color32::WHITE, Stroke::new(1.4, MUTED));
            p.circle_filled(icon.center(), 1.8, MUTED);
        }
        Mark::Feature(i) => draw_icon(p, i, icon.shrink(0.5)),
    }
    let text_x = icon.right() + 6.0;
    let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(12.5), INK);
    let lw = galley.size().x;
    p.galley(
        pos2(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        INK,
    );
    if !detail.is_empty() {
        p.text(
            pos2(text_x + lw + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(11.0),
            MUTED,
        );
    }
    resp
}
