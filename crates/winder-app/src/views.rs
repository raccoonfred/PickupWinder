//! The three drawings: cross-section, lay plot and build profile.

use crate::model::{Computed, profile_upto};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2};
use winder_core::Design;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Coil3d,
    CrossSection,
    Lay,
    Build,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Coloring {
    /// Dark copper for the first turns, light for the last.
    Order,
    /// One hue per section.
    Section,
}

const GRID: Color32 = Color32::from_gray(70);
const TEXT: Color32 = Color32::from_gray(170);
const ACCENT: Color32 = Color32::from_rgb(255, 122, 77);
const WARN: Color32 = Color32::from_rgb(224, 122, 90);

const SECTION_COLORS: [Color32; 6] = [
    Color32::from_rgb(214, 120, 48),
    Color32::from_rgb(86, 156, 214),
    Color32::from_rgb(120, 190, 110),
    Color32::from_rgb(200, 120, 200),
    Color32::from_rgb(220, 200, 90),
    Color32::from_rgb(90, 200, 190),
];

fn copper(f: f32) -> Color32 {
    Color32::from_rgb(
        (90.0 + 150.0 * f) as u8,
        (40.0 + 95.0 * f) as u8,
        (18.0 + 45.0 * f) as u8,
    )
}

/// Maps mm in the channel (y across, z up) onto the screen, same scale on
/// both axes so wires stay round.
struct Frame {
    origin: Pos2,
    px_per_mm: f32,
}

impl Frame {
    fn fit(rect: Rect, width_mm: f64, height_mm: f64) -> Frame {
        // Room for labels beside the flanges and above/below the channel.
        let avail = rect.shrink2(Vec2::new(84.0, 40.0));
        let s = (avail.width() / width_mm as f32).min(avail.height() / height_mm as f32);
        let used = Vec2::new(width_mm as f32 * s, height_mm as f32 * s);
        let left = avail.center().x - used.x / 2.0;
        let bottom = avail.center().y + used.y / 2.0;
        Frame {
            origin: pos2(left, bottom),
            px_per_mm: s,
        }
    }

    fn pt(&self, y_mm: f64, z_mm: f64) -> Pos2 {
        pos2(
            self.origin.x + y_mm as f32 * self.px_per_mm,
            self.origin.y - z_mm as f32 * self.px_per_mm,
        )
    }
}

pub fn cross_section(
    ui: &mut Ui,
    d: &Design,
    c: &Computed,
    station: usize,
    upto: usize,
    coloring: Coloring,
) {
    let cut = &c.cross.cuts[station];
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let rect = resp.rect;
    let channel = c.cross.channel_mm;
    let depth = d.bobbin.build_depth_mm.max(c.cross.build_mm() * 1.05);
    let f = Frame::fit(rect, channel, depth);
    let flange = 10.0;

    // Flanges, core and the build-depth limit.
    let top = f.pt(0.0, depth);
    let bot = f.pt(channel, 0.0);
    painter.rect_filled(
        Rect::from_min_max(pos2(top.x - flange, top.y), pos2(top.x, bot.y + 6.0)),
        2.0,
        GRID,
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(bot.x, top.y), pos2(bot.x + flange, bot.y + 6.0)),
        2.0,
        GRID,
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(top.x, bot.y), pos2(bot.x, bot.y + 6.0)),
        0.0,
        Color32::from_gray(95),
    );
    let limit_y = f.pt(0.0, d.bobbin.build_depth_mm).y;
    painter.line_segment(
        [pos2(top.x, limit_y), pos2(bot.x, limit_y)],
        Stroke::new(1.0, Color32::from_gray(90)),
    );
    painter.text(
        pos2(bot.x + flange + 4.0, limit_y),
        Align2::LEFT_CENTER,
        "build limit",
        FontId::proportional(11.0),
        TEXT,
    );

    // Every turn laid so far.
    let n = upto.min(cut.len());
    let total = cut.len().max(1) as f32;
    let r = (c.cross.od_mm as f32 / 2.0 * f.px_per_mm).max(0.6);
    for t in &cut[..n] {
        let col = match coloring {
            Coloring::Order => copper(t.n as f32 / total),
            Coloring::Section => SECTION_COLORS[t.section % SECTION_COLORS.len()],
        };
        painter.circle_filled(f.pt(t.y_mm, t.z_mm), r, col);
    }

    // Guide and the turn being laid.
    if let Some(t) = n.checked_sub(1).map(|i| cut[i]) {
        let p = f.pt(t.y_mm, t.z_mm);
        painter.circle_stroke(p, r + 2.5, Stroke::new(1.5, ACCENT));
        let guide = pos2(p.x, top.y - 14.0);
        painter.line_segment([guide, p], Stroke::new(1.0, ACCENT.gamma_multiply(0.6)));
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(guide.x - 6.0, guide.y - 8.0),
                pos2(guide.x + 6.0, guide.y - 8.0),
                guide,
            ],
            ACCENT,
            Stroke::NONE,
        ));
    }

    // Dimensions.
    painter.text(
        pos2((top.x + bot.x) / 2.0, bot.y + 20.0),
        Align2::CENTER_CENTER,
        format!("channel {channel:.2} mm"),
        FontId::proportional(12.0),
        TEXT,
    );
    let build = c.cross.build_at(station, n);
    let over = build > d.bobbin.build_depth_mm;
    painter.text(
        pos2(top.x - flange - 6.0, f.pt(0.0, build).y),
        Align2::RIGHT_CENTER,
        format!("{build:.2} mm"),
        FontId::proportional(12.0),
        if over { WARN } else { TEXT },
    );

    // Hover: which turn is this?
    if let Some(pos) = resp.hover_pos() {
        let hit = cut[..n]
            .iter()
            .rev()
            .find(|t| f.pt(t.y_mm, t.z_mm).distance(pos) <= r.max(3.0));
        if let Some(t) = hit {
            resp.on_hover_ui_at_pointer(|ui| {
                ui.label(format!("turn {} · section {}", t.n, t.section + 1));
                ui.label(format!("{:.3} mm across, {:.3} mm up", t.y_mm, t.z_mm));
            });
        }
    }
}

/// Guide position against turn number.
pub fn lay_plot(ui: &mut Ui, d: &Design, c: &Computed, upto: usize) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let rect = resp.rect.shrink2(Vec2::new(56.0, 32.0));
    let total = c.stats.turns.max(1.0);
    let channel = c.cross.channel_mm;
    let x = |t: f64| rect.left() + (t / total) as f32 * rect.width();
    let y = |mm: f64| rect.bottom() - (mm / channel) as f32 * rect.height();

    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, GRID), egui::StrokeKind::Inside);
    for (i, w) in c.section_bounds.windows(2).enumerate() {
        let (a, b) = (x(w[0]), x(w[1]));
        if i % 2 == 1 {
            painter.rect_filled(
                Rect::from_x_y_ranges(a..=b, rect.y_range()),
                0.0,
                Color32::from_white_alpha(6),
            );
        }
        painter.text(
            pos2((a + b) / 2.0, rect.top() - 10.0),
            Align2::CENTER_CENTER,
            format!("{}. {}", i + 1, d.sections[i].pattern.label()),
            FontId::proportional(11.0),
            TEXT,
        );
    }
    let pts: Vec<Pos2> = c.lay.iter().map(|&(t, mm)| pos2(x(t), y(mm))).collect();
    painter.line(pts, Stroke::new(1.0, Color32::from_rgb(214, 120, 48)));
    // Where the wire actually landed, mid long side: it trails the guide.
    let cut = &c.cross.cuts[c.cross.side_station()];
    let every = cut.len().div_ceil(4000).max(1);
    let laid: Vec<Pos2> = cut
        .iter()
        .step_by(every)
        .map(|t| pos2(x(t.n as f64), y(t.y_mm)))
        .collect();
    painter.line(laid, Stroke::new(1.0, Color32::from_rgb(86, 156, 214)));
    for &s in &c.stops {
        painter.line_segment(
            [pos2(x(s), rect.top()), pos2(x(s), rect.bottom())],
            Stroke::new(1.5, WARN),
        );
    }
    let sx = x(upto as f64);
    painter.line_segment(
        [pos2(sx, rect.top()), pos2(sx, rect.bottom())],
        Stroke::new(1.5, ACCENT),
    );

    painter.text(
        rect.center_bottom() + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        "orange: guide · blue: where the wire landed, mid long side",
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(0.0, 14.0),
        Align2::LEFT_CENTER,
        "turn 0",
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.right_bottom() + Vec2::new(0.0, 14.0),
        Align2::RIGHT_CENTER,
        format!("{:.0}", total),
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.left_bottom() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        "0 mm",
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.left_top() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("{channel:.1} mm"),
        FontId::proportional(11.0),
        TEXT,
    );
    if let Some(pos) = resp.hover_pos().filter(|p| rect.contains(*p)) {
        let t = ((pos.x - rect.left()) / rect.width()) as f64 * total;
        resp.on_hover_text_at_pointer(format!("turn {t:.0}"));
    }
}

/// Top of the winding across the channel, now and when finished.
pub fn build_plot(ui: &mut Ui, d: &Design, c: &Computed, station: usize, upto: usize) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let rect = resp.rect.shrink2(Vec2::new(56.0, 32.0));
    let bins = 220;
    let full = profile_upto(&c.cross, station, usize::MAX, bins);
    let now = profile_upto(&c.cross, station, upto, bins);
    let max = d
        .bobbin
        .build_depth_mm
        .max(full.iter().cloned().fold(0.0, f64::max) * 1.05);
    let x = |i: usize| rect.left() + (i as f32 + 0.5) / bins as f32 * rect.width();
    let y = |mm: f64| rect.bottom() - (mm / max) as f32 * rect.height();

    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, GRID), egui::StrokeKind::Inside);
    let limit = y(d.bobbin.build_depth_mm);
    painter.line_segment(
        [pos2(rect.left(), limit), pos2(rect.right(), limit)],
        Stroke::new(1.0, WARN),
    );
    painter.text(
        pos2(rect.right(), limit - 8.0),
        Align2::RIGHT_CENTER,
        "build limit",
        FontId::proportional(11.0),
        WARN,
    );
    let line = |v: &[f64]| {
        v.iter()
            .enumerate()
            .map(|(i, &h)| pos2(x(i), y(h)))
            .collect::<Vec<_>>()
    };
    painter.line(line(&full), Stroke::new(1.0, Color32::from_gray(110)));
    painter.line(line(&now), Stroke::new(2.0, ACCENT));
    painter.text(
        rect.left_top() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("{max:.1} mm"),
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.left_bottom() - Vec2::new(6.0, 0.0),
        Align2::RIGHT_CENTER,
        "core",
        FontId::proportional(11.0),
        TEXT,
    );
    painter.text(
        rect.center_bottom() + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        "across the channel, flange 1 to flange 2 (grey: finished coil, orange: so far)",
        FontId::proportional(11.0),
        TEXT,
    );
}
