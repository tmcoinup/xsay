use crate::{
    config::{AudioConfig, Config, HotkeyConfig, InjectionConfig, TranscriptionConfig},
    settings_ui::{self, SettingsState},
    state::{AppState, SharedState},
    tray::{self, TrayAction},
};
use eframe::egui;
use parking_lot::Mutex;
use std::{
    sync::{Arc, LazyLock, atomic::AtomicBool},
    time::Duration,
};

// Both states use square viewports; the disk centers in each window so the
// anchor logic stays simple. Earlier 0.1.21 tried a 96×124 idle window so
// it could carry a label below the disk — on the user's GNOME / mutter
// session that combination of (transparent + always-on-top + no
// decorations + skip-taskbar + non-square) caused the WM to never map
// the window, the daemon ran but no overlay ever appeared. Reverting to
// the known-good 44×44 idle size that worked through 0.1.20.
const OVERLAY_IDLE_SIZE: egui::Vec2 = egui::vec2(44.0, 44.0);
const OVERLAY_ACTIVE_SIZE: egui::Vec2 = egui::vec2(120.0, 120.0);

const SETTINGS_VIEWPORT_ID: &str = "xsay_settings";
const SETTINGS_SIZE: egui::Vec2 = egui::vec2(700.0, 660.0);
const SETTINGS_MIN_SIZE: egui::Vec2 = egui::vec2(640.0, 540.0);

fn app_icon() -> Arc<egui::IconData> {
    static ICON: LazyLock<Arc<egui::IconData>> = LazyLock::new(|| {
        let bytes = include_bytes!("../share/icons/hicolor/256x256/apps/xsay.png");
        Arc::new(eframe::icon_data::from_png_bytes(bytes).unwrap_or_default())
    });
    Arc::clone(&ICON)
}

pub struct XsayOverlay {
    shared_state: SharedState,
    animation_phase: f32,
    dots_phase: f32,
    was_idle: bool,
    settings: SettingsState,
    shared_position: Arc<Mutex<String>>,
    last_overlay_size: egui::Vec2,
    last_overlay_corner: String,
    last_passthrough: Option<bool>,
    window_level_set: bool,
    quit_requested: bool,
    settings_visible: bool,
}

impl XsayOverlay {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shared_state: SharedState,
        shared_hotkey: Arc<Mutex<HotkeyConfig>>,
        shared_audio: Arc<Mutex<AudioConfig>>,
        shared_inject: Arc<Mutex<InjectionConfig>>,
        shared_transcription: Arc<Mutex<TranscriptionConfig>>,
        shared_position: Arc<Mutex<String>>,
        capture_active: Arc<AtomicBool>,
        capture_slot: Arc<crate::hotkey::CaptureSlot>,
        backend_info: Arc<crate::hotkey::BackendInfo>,
        model_reload_tx: crossbeam_channel::Sender<std::path::PathBuf>,
    ) -> Self {
        let config = Config::load().unwrap_or_default();
        let settings = SettingsState::new(
            &config,
            shared_hotkey,
            shared_audio,
            shared_inject,
            shared_transcription,
            Arc::clone(&shared_position),
            capture_active,
            capture_slot,
            backend_info,
            model_reload_tx,
        );

        Self {
            shared_state,
            animation_phase: 0.0,
            dots_phase: 0.0,
            was_idle: true,
            settings,
            shared_position,
            last_overlay_size: egui::vec2(0.0, 0.0),
            last_overlay_corner: String::new(),
            last_passthrough: None,
            window_level_set: false,
            quit_requested: false,
            settings_visible: false,
        }
    }
}

/// Where on the screen the disk center should sit for a given corner.
/// Both idle and active windows are square with the disk at their visual
/// center, so we just compute the anchor with idle dimensions and let
/// `window_pos_for_anchor` recenter each state's window on it.
fn compute_icon_anchor(monitor: egui::Vec2, corner: &str) -> egui::Pos2 {
    let side_margin = 20.0;
    let bottom_margin = 88.0;
    let top_margin = 20.0;
    let half = OVERLAY_IDLE_SIZE.x * 0.5;
    match corner {
        "top-left" => egui::pos2(side_margin + half, top_margin + half),
        "top-center" => egui::pos2(monitor.x * 0.5, top_margin + half),
        "bottom-left" => egui::pos2(side_margin + half, monitor.y - bottom_margin - half),
        "bottom-right" => egui::pos2(
            monitor.x - side_margin - half,
            monitor.y - bottom_margin - half,
        ),
        "bottom-center" => egui::pos2(monitor.x * 0.5, monitor.y - bottom_margin - half),
        "center" => egui::pos2(monitor.x * 0.5, monitor.y * 0.5),
        _ => egui::pos2(monitor.x - side_margin - half, top_margin + half),
    }
}

fn window_pos_for_anchor(anchor: egui::Pos2, window: egui::Vec2) -> egui::Pos2 {
    egui::pos2(anchor.x - window.x * 0.5, anchor.y - window.y * 0.5)
}

impl eframe::App for XsayOverlay {
    /// Fully transparent clear so the overlay has no halo. eframe's default
    /// is `(12, 12, 12, 180)` — a translucent near-black designed to make
    /// shadows look right on opaque desktop apps. On our transparent
    /// always-on-top viewport that becomes a dark square framing the mic
    /// icon, which is what users describe as the "死黑底" / plastic look.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Drain tray events before rendering so visibility/quit toggles take
        // effect this frame.
        for action in tray::poll_events() {
            match action {
                TrayAction::ShowSettings => {
                    self.settings_visible = true;
                }
                TrayAction::Quit => {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Overlay close: cancel and keep running. The overlay isn't in any
        // taskbar (with_taskbar(false)), so the only way close fires here
        // is forced WM action (xkill, wmctrl -c). We don't quit blindly
        // because the same window-creation path also seems to surface an
        // unwanted close on certain compositors at startup, which would
        // make the daemon vanish silently. Real "quit from taskbar" is
        // handled in the settings viewport's WM-close branch below — when
        // settings is visible it appears in taskbar, and that's where
        // GNOME Activities' Quit lands.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit_requested {
                return;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }

        let state = self.shared_state.lock().clone();
        let is_idle = matches!(state, AppState::Idle);
        let became_active = self.was_idle && !is_idle;
        self.was_idle = is_idle;

        self.apply_overlay_viewport_props(&ctx, is_idle, became_active);
        self.render_overlay_content(ui, &state);

        // Settings is a sibling viewport spawned on demand. We re-call
        // show_viewport_immediate every frame while it should be visible —
        // dropping the call destroys the viewport, which is exactly what
        // close-button / WM-close should do.
        if self.settings_visible {
            self.render_settings_viewport(&ctx);
        }

        let repaint_ms = if is_idle && !self.settings_visible {
            250
        } else {
            33
        };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }
}

impl XsayOverlay {
    fn apply_overlay_viewport_props(
        &mut self,
        ctx: &egui::Context,
        is_idle: bool,
        became_active: bool,
    ) {
        let overlay_size = if is_idle {
            OVERLAY_IDLE_SIZE
        } else {
            OVERLAY_ACTIVE_SIZE
        };
        let corner = self.shared_position.lock().clone();
        let overlay_pos = ctx
            .input(|i| i.viewport().monitor_size)
            .filter(|m| m.x > 0.0 && m.y > 0.0)
            .map(|m| window_pos_for_anchor(compute_icon_anchor(m, &corner), overlay_size));
        let needs_position = became_active
            || overlay_size != self.last_overlay_size
            || corner != self.last_overlay_corner;

        // Only push viewport commands when the value actually changes —
        // X11/_NET_WM_STATE roundtrips at 30 fps starve the GNOME compositor
        // and made the desktop unresponsive on the user's session.
        if !self.window_level_set {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            self.window_level_set = true;
        }
        // Mouse passthrough is on whenever the indicator is showing recording
        // / transcribing / injecting state, so the user can keep typing into
        // whatever field they were targeting. When idle the badge is the
        // only entry point to settings, so it must be clickable.
        let passthrough = !is_idle;
        if self.last_passthrough != Some(passthrough) {
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(passthrough));
            self.last_passthrough = Some(passthrough);
        }
        if overlay_size != self.last_overlay_size {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(overlay_size));
        }
        if needs_position
            && let Some(p) = overlay_pos
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
        }

        self.last_overlay_size = overlay_size;
        self.last_overlay_corner = corner;
    }

    fn render_overlay_content(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let rect = ui.max_rect();
        let painter = ui.painter().clone();
        match state {
            AppState::Idle => {
                let response = ui.interact(
                    rect,
                    egui::Id::new("xsay_idle_badge"),
                    egui::Sense::click(),
                );
                let disk_radius = rect.width().min(rect.height()) * 0.5 * 0.92;
                paint_idle_logo(&painter, rect.center(), disk_radius, response.hovered());
                if response.clicked() {
                    self.settings_visible = true;
                }
            }
            AppState::Recording { .. } => {
                self.animation_phase += 0.08;
                paint_active_pill(
                    ui,
                    crate::theme::REC,
                    "REC",
                    crate::theme::REC,
                    true,
                    self.animation_phase,
                    self.dots_phase,
                );
            }
            AppState::Transcribing => {
                self.dots_phase += 0.05;
                paint_active_pill(
                    ui,
                    crate::theme::ACCENT,
                    "识别中",
                    crate::theme::ACCENT,
                    false,
                    self.animation_phase,
                    self.dots_phase,
                );
            }
            AppState::Injecting => {
                self.dots_phase += 0.05;
                paint_active_pill(
                    ui,
                    crate::theme::SUCCESS,
                    "输入中",
                    crate::theme::SUCCESS,
                    false,
                    self.animation_phase,
                    self.dots_phase,
                );
            }
        }
    }

    fn render_settings_viewport(&mut self, root_ctx: &egui::Context) {
        let builder = egui::ViewportBuilder::default()
            .with_title("xsay 设置")
            .with_app_id("xsay")
            .with_icon(app_icon())
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(true)
            .with_taskbar(true)
            .with_inner_size([SETTINGS_SIZE.x, SETTINGS_SIZE.y])
            .with_min_inner_size([SETTINGS_MIN_SIZE.x, SETTINGS_MIN_SIZE.y]);

        let settings = &mut self.settings;
        let mut button_close = false;
        let mut wm_close = false;
        root_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(SETTINGS_VIEWPORT_ID),
            builder,
            |child_ui, _class| {
                let child_ctx = child_ui.ctx().clone();
                // Distinguish: red traffic-light click = "user dismissed
                // the dialog" (hide), WM/taskbar close = "user invoked
                // Quit" on the only window of ours that's in any taskbar
                // (overlay is `with_taskbar(false)`). The latter is the
                // only way GNOME Activities / KDE Plasma "Quit" can reach
                // us, so we treat it as a real exit.
                if child_ctx.input(|i| i.viewport().close_requested()) {
                    child_ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    wm_close = true;
                }
                if settings_ui::render(&child_ctx, settings) {
                    button_close = true;
                }
            },
        );

        if wm_close {
            self.quit_requested = true;
            root_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if button_close {
            self.settings_visible = false;
        }
    }
}

/// Flat blue mic badge for the idle state. Matches the mockup the user
/// shipped: solid `theme::ACCENT` disk, white mic glyph centered, no
/// gloss / inner highlights / drop shadow. Hover paints a soft outer
/// halo so the click target is still discoverable.
fn paint_idle_logo(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    hovered: bool,
) {
    if hovered {
        let [r, g, b, _] = crate::theme::ACCENT.to_array();
        painter.circle_filled(
            center,
            radius * 1.12,
            egui::Color32::from_rgba_premultiplied(r, g, b, 50),
        );
    }
    painter.circle_filled(center, radius, crate::theme::ACCENT);
    paint_mic_glyph(painter, center, radius);
}

/// White microphone glyph used by the idle badge.
/// `unit` is the disk radius the glyph should fit inside; all dimensions
/// derive from it so the same routine produces a 14 px glyph in the idle
/// 44 px badge and would scale linearly for any other disk size.
///
/// Design: tall capsule body (the mic head) wrapped underneath by a shallow
/// half-circle cradle, with a short vertical stand pole below the cradle's
/// lowest point. The curved cradle is the visual cue that distinguishes
/// "microphone" from "lightbulb on a base" — without it the white capsule
/// inside a blue disk reads as a bulb on a screw cap.
fn paint_mic_glyph(painter: &egui::Painter, center: egui::Pos2, unit: f32) {
    // Mic head: rounded capsule, taller than wide so the silhouette feels
    // mic-like rather than bulb-like.
    let body_w = unit * 0.40;
    let body_h = unit * 0.62;
    let body_center = egui::pos2(center.x, center.y - unit * 0.10);
    let body_rect = egui::Rect::from_center_size(body_center, egui::vec2(body_w, body_h));
    painter.rect_filled(
        body_rect,
        egui::CornerRadius::same((body_w * 0.5).round() as u8),
        egui::Color32::WHITE,
    );

    let stroke_w = (unit * 0.09).max(1.5);
    let stroke = egui::Stroke::new(stroke_w, egui::Color32::WHITE);

    // Cradle: a half-circle that wraps under the body. The cradle radius is
    // larger than half the body width so the arc clearly extends past the
    // capsule on both sides. Tip y is set just below the body bottom so the
    // arms appear to "catch" the mic — without that overlap the cradle
    // detaches from the body and the whole thing reads as two stacked shapes.
    let body_bottom = body_center.y + body_h * 0.5;
    let cradle_radius = unit * 0.42;
    let cradle_center = egui::pos2(center.x, body_bottom - unit * 0.04);

    // Approximate the lower half of a circle with line segments. egui has no
    // arc primitive; 18 segments on a 22 px disk reads as smooth.
    let segments = 18;
    for i in 0..segments {
        let t1 = (i as f32 / segments as f32) * std::f32::consts::PI;
        let t2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::PI;
        let p1 = cradle_center + egui::vec2(t1.cos() * cradle_radius, t1.sin() * cradle_radius);
        let p2 = cradle_center + egui::vec2(t2.cos() * cradle_radius, t2.sin() * cradle_radius);
        painter.line_segment([p1, p2], stroke);
    }

    // Stand pole drops from the bottom of the cradle arc.
    let stand_top = cradle_center + egui::vec2(0.0, cradle_radius);
    let stand_bottom = stand_top + egui::vec2(0.0, unit * 0.18);
    painter.line_segment([stand_top, stand_bottom], stroke);
}

fn paint_active_pill(
    ui: &mut egui::Ui,
    circle_color: egui::Color32,
    bottom_label: &str,
    label_color: egui::Color32,
    pulse: bool,
    animation_phase: f32,
    dots_phase: f32,
) {
    let rect = ui.max_rect();
    let painter = ui.painter().clone();

    let bg = egui::Color32::from_rgba_premultiplied(0x14, 0x14, 0x1A, 210);
    painter.rect_filled(rect, crate::theme::radius_xxl(), bg);

    let center = rect.center();

    if pulse {
        let p = animation_phase.sin() * 0.5 + 0.5;
        let ring_r = 32.0 + p * 10.0;
        let [r, g, b, _] = circle_color.to_array();
        let alpha = (180.0 * (1.0 - p * 0.4)) as u8;
        painter.circle_stroke(
            center,
            ring_r,
            egui::Stroke::new(2.5, egui::Color32::from_rgba_premultiplied(r, g, b, alpha)),
        );
    }

    painter.circle_filled(center, 22.0, circle_color);

    let mic_rect =
        egui::Rect::from_center_size(center + egui::vec2(0.0, -6.0), egui::vec2(10.0, 18.0));
    painter.rect_filled(mic_rect, egui::CornerRadius::same(5), egui::Color32::WHITE);

    let sy = center.y + 11.0;
    let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
    painter.line_segment(
        [
            egui::pos2(center.x - 12.0, sy),
            egui::pos2(center.x + 12.0, sy),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 12.0, sy),
            egui::pos2(center.x - 12.0, sy - 5.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 12.0, sy),
            egui::pos2(center.x + 12.0, sy - 5.0),
        ],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(center.x, sy), egui::pos2(center.x, sy + 6.0)],
        stroke,
    );

    let prefix = if pulse { "● " } else { "" };
    let dots = if pulse {
        String::new()
    } else {
        ".".repeat((dots_phase as usize % 4) + 1)
    };
    let label = format!("{}{}{}", prefix, bottom_label, dots);
    painter.text(
        egui::pos2(center.x, rect.max.y - 12.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(crate::theme::FONT_XS),
        label_color,
    );
}

/// Build eframe options with the floating overlay as the main viewport.
///
/// The overlay is what users always see. Settings is rendered later as an
/// immediate child viewport spawned from inside `ui()` whenever
/// `settings_visible` is true. Making the overlay the root avoids a chicken-
/// and-egg with eframe's invisible-window optimization: when the main
/// viewport is hidden, eframe skips `App::ui()`, which would also skip the
/// child overlay viewport — i.e. nothing on screen at all.
pub fn build_native_options(_config: &crate::config::OverlayConfig) -> eframe::NativeOptions {
    // No `with_active(false)` here: on some winit/X11 paths it suppresses the
    // initial paint, leaving the overlay window mapped but blank — the user
    // sees a black square that never repaints. The window still doesn't
    // steal keyboard focus in practice because the WM treats a no-decorations
    // taskbar-skipping always-on-top utility window as non-activating.
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xsay")
            .with_app_id("xsay")
            .with_icon(app_icon())
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size([OVERLAY_IDLE_SIZE.x, OVERLAY_IDLE_SIZE.y]),
        ..Default::default()
    }
}
