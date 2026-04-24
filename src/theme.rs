//! Design tokens extracted from the Figma Make reference (bundle hash
//! `965fafa0c4522141713b07c7f5f25d79349fedc9`). Keep this the single
//! source of truth — do not hard-code colors/radii in rendering code.
//!
//! Some tokens (BG_DEEPEST, FONT_H1, radius_xs/xl, etc.) aren't wired up
//! in the current UI but are preserved so rendering code can reach for
//! them as the design evolves without rebuilding the design language.

#![allow(dead_code)]

use eframe::egui::{Color32, CornerRadius};

// `Rounding` was renamed to `CornerRadius` in egui 0.34. Keep a local alias
// so helper names (`radius_*()`) still read naturally.
pub type Rounding = CornerRadius;

/// Deepest page background (behind the settings window)
pub const BG_DEEPEST: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x12);
/// Panel / tab bar background
pub const BG_PANEL: Color32 = Color32::from_rgb(0x14, 0x14, 0x1A);
/// Settings window / card container
pub const BG_WINDOW: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x22);
/// Default card background
pub const BG_CARD: Color32 = Color32::from_rgb(0x26, 0x26, 0x2B);
/// Card hover / slightly brighter
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2E);
/// "Current" / selected card (green tint)
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x1E, 0x3C, 0x1E);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xEC);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA8);
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x60, 0x60, 0x68);

/// Primary accent (active tab, radio fill, links)
pub const ACCENT: Color32 = Color32::from_rgb(0x3D, 0xA5, 0xFF);
/// Success (status messages, OK toast)
pub const SUCCESS: Color32 = Color32::from_rgb(0x50, 0xDC, 0x50);
/// "当前使用" chip color
pub const CURRENT: Color32 = Color32::from_rgb(0x3A, 0x9A, 0x3A);

/// Recording indicator red
pub const REC: Color32 = Color32::from_rgb(0xC8, 0x32, 0x32);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x40, 0x40);
pub const DANGER_HOVER: Color32 = Color32::from_rgb(0xFF, 0x60, 0x60);
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xB4, 0x3C);

// --- Corner radii ---

pub fn radius_xs() -> CornerRadius {
    CornerRadius::same(2)
}
pub fn radius_sm() -> CornerRadius {
    CornerRadius::same(4)
}
pub fn radius_md() -> CornerRadius {
    CornerRadius::same(6)
}
/// Cards, frames
pub fn radius_lg() -> CornerRadius {
    CornerRadius::same(8)
}
/// Windows
pub fn radius_xl() -> CornerRadius {
    CornerRadius::same(10)
}
/// Hero elements
pub fn radius_xxl() -> CornerRadius {
    CornerRadius::same(12)
}

// --- Font sizes ---

pub const FONT_XS: f32 = 10.0;
pub const FONT_SM: f32 = 11.0;
pub const FONT_MD: f32 = 12.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_HEADING: f32 = 14.0;
pub const FONT_HERO: f32 = 20.0;
pub const FONT_H1: f32 = 28.0;

// ---------------------------------------------------------------------------
// Small UI primitives matching the Figma reference
// ---------------------------------------------------------------------------

use eframe::egui::{self, Painter, Rect, Response, Stroke, StrokeKind, Ui};
use std::f32::consts::PI;

/// Chip-style label with rounded background — e.g. "✓ 当前使用" / "↑ 有更新".
pub fn chip(ui: &mut Ui, text: &str, fg: egui::Color32, bg: egui::Color32) -> Response {
    let frame = egui::Frame::new()
        .fill(bg)
        .corner_radius(radius_sm())
        .inner_margin(egui::Margin::symmetric(6, 2));

    frame
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(fg).size(FONT_SM));
        })
        .response
}

/// Icons we draw via `egui::Painter` so they never render as tofu boxes.
/// Noto CJK doesn't cover the SMP emoji range, and even BMP symbols like ⚙
/// are inconsistent across systems, so we just paint our own 14×14 glyphs.
#[derive(Clone, Copy)]
pub enum Icon {
    Check,
    X,
    Trash,
    Download,
    Pause,
    Play,
    Refresh,
    Up,
    Warning,
    // Tab icons
    Box,
    Keyboard,
    Gear,
    Document,
}

pub fn draw_icon(painter: &Painter, rect: Rect, icon: Icon, color: egui::Color32) {
    let stroke = Stroke::new(1.5, color);
    let r = rect;
    let w = r.width();
    let h = r.height();
    let c = r.center();

    match icon {
        Icon::Check => {
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.15, c.y + h * 0.05),
                    egui::pos2(r.min.x + w * 0.4, r.max.y - h * 0.15),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.4, r.max.y - h * 0.15),
                    egui::pos2(r.max.x - w * 0.1, r.min.y + h * 0.2),
                ],
                stroke,
            );
        }
        Icon::X => {
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.2, r.min.y + h * 0.2),
                    egui::pos2(r.max.x - w * 0.2, r.max.y - h * 0.2),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.max.x - w * 0.2, r.min.y + h * 0.2),
                    egui::pos2(r.min.x + w * 0.2, r.max.y - h * 0.2),
                ],
                stroke,
            );
        }
        Icon::Trash => {
            let lid_y = r.min.y + h * 0.32;
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.1, lid_y),
                    egui::pos2(r.max.x - w * 0.1, lid_y),
                ],
                stroke,
            );
            let handle_w = w * 0.3;
            let hx0 = c.x - handle_w / 2.0;
            let hx1 = c.x + handle_w / 2.0;
            let hy = r.min.y + h * 0.2;
            painter.line_segment([egui::pos2(hx0, hy), egui::pos2(hx0, lid_y)], stroke);
            painter.line_segment([egui::pos2(hx0, hy), egui::pos2(hx1, hy)], stroke);
            painter.line_segment([egui::pos2(hx1, hy), egui::pos2(hx1, lid_y)], stroke);
            let tl = egui::pos2(r.min.x + w * 0.22, lid_y);
            let tr = egui::pos2(r.max.x - w * 0.22, lid_y);
            let bl = egui::pos2(r.min.x + w * 0.28, r.max.y - h * 0.1);
            let br = egui::pos2(r.max.x - w * 0.28, r.max.y - h * 0.1);
            painter.line_segment([tl, bl], stroke);
            painter.line_segment([bl, br], stroke);
            painter.line_segment([tr, br], stroke);
        }
        Icon::Download => {
            painter.line_segment(
                [
                    egui::pos2(c.x, r.min.y + h * 0.15),
                    egui::pos2(c.x, r.min.y + h * 0.62),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(c.x - w * 0.2, r.min.y + h * 0.42),
                    egui::pos2(c.x, r.min.y + h * 0.62),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(c.x + w * 0.2, r.min.y + h * 0.42),
                    egui::pos2(c.x, r.min.y + h * 0.62),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.15, r.max.y - h * 0.1),
                    egui::pos2(r.max.x - w * 0.15, r.max.y - h * 0.1),
                ],
                stroke,
            );
        }
        Icon::Pause => {
            let bar_h = h * 0.55;
            let bar_w = w * 0.14;
            let gap = w * 0.12;
            let y_top = c.y - bar_h / 2.0;
            painter.rect_filled(
                Rect::from_min_size(
                    egui::pos2(c.x - gap / 2.0 - bar_w, y_top),
                    egui::vec2(bar_w, bar_h),
                ),
                CornerRadius::same(1),
                color,
            );
            painter.rect_filled(
                Rect::from_min_size(
                    egui::pos2(c.x + gap / 2.0, y_top),
                    egui::vec2(bar_w, bar_h),
                ),
                CornerRadius::same(1),
                color,
            );
        }
        Icon::Play => {
            let pts = vec![
                egui::pos2(r.min.x + w * 0.28, r.min.y + h * 0.2),
                egui::pos2(r.max.x - w * 0.2, c.y),
                egui::pos2(r.min.x + w * 0.28, r.max.y - h * 0.2),
            ];
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Refresh => {
            draw_refresh_arc(painter, rect, color, 0.0);
        }
        Icon::Up => {
            painter.line_segment(
                [
                    egui::pos2(c.x, r.max.y - h * 0.15),
                    egui::pos2(c.x, r.min.y + h * 0.2),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.25, r.min.y + h * 0.4),
                    egui::pos2(c.x, r.min.y + h * 0.2),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.max.x - w * 0.25, r.min.y + h * 0.4),
                    egui::pos2(c.x, r.min.y + h * 0.2),
                ],
                stroke,
            );
        }
        Icon::Warning => {
            let pts = vec![
                egui::pos2(c.x, r.min.y + h * 0.1),
                egui::pos2(r.min.x + w * 0.1, r.max.y - h * 0.1),
                egui::pos2(r.max.x - w * 0.1, r.max.y - h * 0.1),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
            painter.line_segment(
                [
                    egui::pos2(c.x, r.min.y + h * 0.35),
                    egui::pos2(c.x, r.min.y + h * 0.65),
                ],
                stroke,
            );
            painter.circle_filled(egui::pos2(c.x, r.max.y - h * 0.22), 1.2, color);
        }
        Icon::Box => {
            painter.rect_stroke(
                r.shrink(w * 0.12),
                CornerRadius::same(1),
                stroke,
                StrokeKind::Middle,
            );
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.12, c.y),
                    egui::pos2(r.max.x - w * 0.12, c.y),
                ],
                stroke,
            );
        }
        Icon::Keyboard => {
            painter.rect_stroke(
                Rect::from_min_max(
                    egui::pos2(r.min.x + w * 0.05, r.min.y + h * 0.25),
                    egui::pos2(r.max.x - w * 0.05, r.max.y - h * 0.15),
                ),
                CornerRadius::same(1),
                stroke,
                StrokeKind::Middle,
            );
            let row1_y = r.min.y + h * 0.44;
            let row2_y = r.min.y + h * 0.60;
            for col in 0..4 {
                let x = r.min.x + w * (0.18 + col as f32 * 0.2);
                painter.circle_filled(egui::pos2(x, row1_y), 0.8, color);
                painter.circle_filled(egui::pos2(x, row2_y), 0.8, color);
            }
            painter.line_segment(
                [
                    egui::pos2(r.min.x + w * 0.22, r.max.y - h * 0.24),
                    egui::pos2(r.max.x - w * 0.22, r.max.y - h * 0.24),
                ],
                stroke,
            );
        }
        Icon::Gear => {
            painter.circle_stroke(c, w * 0.22, stroke);
            painter.circle_stroke(c, w * 0.08, stroke);
            for i in 0..6 {
                let a = i as f32 / 6.0 * 2.0 * PI;
                let r_in = w * 0.26;
                let r_out = w * 0.42;
                painter.line_segment(
                    [
                        c + egui::vec2(a.cos() * r_in, a.sin() * r_in),
                        c + egui::vec2(a.cos() * r_out, a.sin() * r_out),
                    ],
                    Stroke::new(2.0, color),
                );
            }
        }
        Icon::Document => {
            painter.rect_stroke(
                Rect::from_min_max(
                    egui::pos2(r.min.x + w * 0.22, r.min.y + h * 0.08),
                    egui::pos2(r.max.x - w * 0.22, r.max.y - h * 0.08),
                ),
                CornerRadius::same(1),
                stroke,
                StrokeKind::Middle,
            );
            for i in 0..3 {
                let y = r.min.y + h * (0.3 + i as f32 * 0.18);
                painter.line_segment(
                    [
                        egui::pos2(r.min.x + w * 0.32, y),
                        egui::pos2(r.max.x - w * 0.32, y),
                    ],
                    Stroke::new(1.0, color),
                );
            }
        }
    }
}

/// Circular arrow used for the "refresh" icon, with an angle offset so the
/// same drawing can be animated by passing in a time-dependent rotation.
fn draw_refresh_arc(painter: &Painter, rect: Rect, color: egui::Color32, rotation: f32) {
    let stroke = Stroke::new(1.5, color);
    let c = rect.center();
    let radius = rect.width() * 0.32;
    let start_angle = -PI * 0.2 + rotation;
    let end_angle = PI * 1.3 + rotation;
    let steps = 20;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = start_angle + (end_angle - start_angle) * t;
        pts.push(c + egui::vec2(a.cos() * radius, a.sin() * radius));
    }
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
    let last = *pts.last().unwrap();
    let prev = pts[pts.len() - 2];
    let dir = (last - prev).normalized();
    let perp = egui::vec2(-dir.y, dir.x);
    let asz = rect.width() * 0.18;
    painter.line_segment([last, last - dir * asz + perp * asz * 0.5], stroke);
    painter.line_segment([last, last - dir * asz - perp * asz * 0.5], stroke);
}

fn brighten(c: egui::Color32, factor: f32) -> egui::Color32 {
    let [r, g, b, a] = c.to_array();
    let b_fn = |v: u8| ((v as f32 * factor).min(255.0)) as u8;
    egui::Color32::from_rgba_premultiplied(b_fn(r), b_fn(g), b_fn(b), a)
}

/// Link-style text button — no fill, no border. Color brightens on hover.
pub fn link_button(ui: &mut Ui, text: &str, color: egui::Color32) -> Response {
    let font_id = egui::FontId::proportional(FONT_MD);
    let text_size = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), color)
        .size();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(text_size.x + 4.0, text_size.y.max(20.0)),
        egui::Sense::click(),
    );
    let col = if response.hovered() {
        brighten(color, 1.2)
    } else {
        color
    };
    ui.painter().text(
        egui::pos2(rect.min.x + 2.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font_id,
        col,
    );
    response
}

/// Icon + text link button — matches the Figma reference (transparent, color
/// brightens on hover). Used for all action buttons in model rows.
pub fn icon_link_button(ui: &mut Ui, icon: Icon, text: &str, color: egui::Color32) -> Response {
    let icon_size = 14.0;
    let gap = 4.0;
    let font_id = egui::FontId::proportional(FONT_MD);
    let text_size = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), color)
        .size();
    let total = egui::vec2(
        icon_size + gap + text_size.x + 4.0,
        icon_size.max(text_size.y).max(20.0),
    );
    let (rect, response) = ui.allocate_exact_size(total, egui::Sense::click());
    let col = if response.hovered() {
        brighten(color, 1.2)
    } else {
        color
    };
    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.min.x, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    draw_icon(ui.painter(), icon_rect, icon, col);
    ui.painter().text(
        egui::pos2(rect.min.x + icon_size + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font_id,
        col,
    );
    response
}

/// Outlined pill button — BG_CARD fill, thin border, icon + text inside.
/// Used for footer actions like "检查所有模型更新". When `spinning` is true,
/// the refresh icon rotates continuously and the frame asks egui for a
/// repaint so the animation stays smooth.
pub fn outlined_button(
    ui: &mut Ui,
    icon: Icon,
    text: &str,
    color: egui::Color32,
    spinning: bool,
) -> Response {
    let icon_size = 14.0;
    let gap = 6.0;
    let pad_x = 14.0;
    let pad_y = 7.0;

    let font_id = egui::FontId::proportional(FONT_BODY);
    let text_size = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), color)
        .size();

    let total = egui::vec2(
        pad_x * 2.0 + icon_size + gap + text_size.x,
        pad_y * 2.0 + icon_size.max(text_size.y),
    );
    let (rect, response) = ui.allocate_exact_size(total, egui::Sense::click());

    let hovered = response.hovered();
    let bg = if hovered { BG_CARD_HOVER } else { BG_CARD };
    let border = if hovered {
        TEXT_SECONDARY
    } else {
        TEXT_DISABLED
    };
    let text_color = if hovered { brighten(color, 1.15) } else { color };

    ui.painter().rect_filled(rect, radius_md(), bg);
    ui.painter().rect_stroke(
        rect,
        radius_md(),
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );

    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.min.x + pad_x, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );

    if spinning && matches!(icon, Icon::Refresh) {
        // Drive the animation from the egui clock so timing is independent
        // of our frame rate, and nudge egui to keep drawing.
        let time = ui.ctx().input(|i| i.time) as f32;
        let rotation = time * 2.0 * PI; // one full turn per second
        draw_refresh_arc(ui.painter(), icon_rect, text_color, rotation);
        ui.ctx().request_repaint();
    } else {
        draw_icon(ui.painter(), icon_rect, icon, text_color);
    }

    ui.painter().text(
        egui::pos2(rect.min.x + pad_x + icon_size + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font_id,
        text_color,
    );

    response
}

/// Custom radio button — outer ring, filled dot when selected. Matches the
/// Figma solid-blue style (egui's default radio uses a thin tick glyph that
/// looks inconsistent against the rest of the UI).
pub fn radio_button(ui: &mut Ui, selected: bool, color: egui::Color32) -> Response {
    let size = egui::vec2(18.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let center = rect.center();
    let ring_color = if selected {
        color
    } else if response.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    ui.painter()
        .circle_stroke(center, 7.5, Stroke::new(1.5, ring_color));
    if selected {
        ui.painter().circle_filled(center, 4.0, color);
    }
    response
}

/// Custom checkbox with the same visual language as `radio_button` — rounded
/// square frame, filled with accent on check. Returns the click response.
pub fn checkbox(ui: &mut Ui, checked: bool, color: egui::Color32) -> Response {
    let size = egui::vec2(18.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let r = Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
    let frame_color = if checked {
        color
    } else if response.hovered() {
        TEXT_PRIMARY
    } else {
        TEXT_SECONDARY
    };
    if checked {
        ui.painter().rect_filled(r, CornerRadius::same(3), color);
        let inner = r.shrink(3.0);
        let p1 = egui::pos2(inner.min.x + inner.width() * 0.15, inner.center().y + 1.0);
        let p2 = egui::pos2(
            inner.min.x + inner.width() * 0.42,
            inner.max.y - inner.height() * 0.2,
        );
        let p3 = egui::pos2(inner.max.x - inner.width() * 0.1, inner.min.y + 2.0);
        let stroke = Stroke::new(1.8, egui::Color32::WHITE);
        ui.painter().line_segment([p1, p2], stroke);
        ui.painter().line_segment([p2, p3], stroke);
    } else {
        ui.painter()
            .rect_stroke(r, CornerRadius::same(3), Stroke::new(1.5, frame_color), StrokeKind::Middle);
    }
    response
}

/// Card container matching the Figma section style: BG_CARD background,
/// radius_lg corners, generous inner margin. Title rendered on its own row;
/// body receives a Ui with `set_min_width(available_width)` already applied.
pub fn section_card<R>(
    ui: &mut Ui,
    title: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut inner: Option<R> = None;
    egui::Frame::new()
        .fill(BG_CARD)
        .corner_radius(radius_lg())
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .color(TEXT_PRIMARY)
                    .strong()
                    .size(FONT_HEADING),
            );
            ui.add_space(8.0);
            inner = Some(add_contents(ui));
        });
    inner.expect("section_card body always runs")
}

/// A "form row" — label on the left in TEXT_SECONDARY, control on the right,
/// baseline aligned. Used by the General and Hotkey tabs so every row reads
/// the same height and color.
pub fn form_row<R>(
    ui: &mut Ui,
    label: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut inner: Option<R> = None;
    ui.horizontal(|ui| {
        ui.set_min_width(ui.available_width());
        ui.add_sized(
            egui::vec2(96.0, 20.0),
            egui::Label::new(
                egui::RichText::new(label)
                    .color(TEXT_SECONDARY)
                    .size(FONT_BODY),
            )
            .selectable(false),
        );
        inner = Some(add_contents(ui));
    });
    inner.expect("form_row body always runs")
}

/// Small secondary line under a form row — weak text, small size.
pub fn helper_text(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .color(TEXT_SECONDARY)
            .size(FONT_SM),
    );
}
