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

// Single fixed viewport size for every state. We used to grow 44×44 → 120×120
// on idle→active and the inverse on active→idle, but X11 / mutter applies
// `XConfigureWindow(size)` and `XConfigureWindow(position)` in two separate
// frames — between them the window has the new size pinned at the *old*
// top-left, so the icon visibly snaps from one corner before sliding back.
// Users described it as the badge "growing from the top-left and flashing".
// Holding the OS window at the active size and just painting smaller content
// inside it for idle eliminates that whole class of resize race; the disk
// centre stays exactly where it was last frame.
const OVERLAY_SIZE: egui::Vec2 = egui::vec2(120.0, 120.0);
const IDLE_DISK_RADIUS: f32 = 20.0;

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
    last_overlay_corner: String,
    overlay_geometry_set: bool,
    last_passthrough: Option<bool>,
    window_level_set: bool,
    quit_requested: bool,
    settings_visible: bool,
    idle_badge: Option<egui::TextureHandle>,
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
            last_overlay_corner: String::new(),
            overlay_geometry_set: false,
            last_passthrough: None,
            window_level_set: false,
            quit_requested: false,
            settings_visible: false,
            idle_badge: None,
        }
    }
}

/// Decode the idle-badge PNG into an egui [`ColorImage`]. Run once per
/// process via [`XsayOverlay::idle_badge`]; the resulting `TextureHandle`
/// is reused every frame.
fn decode_idle_badge() -> egui::ColorImage {
    let bytes = include_bytes!("../assets/idle-badge.png");
    let icon = eframe::icon_data::from_png_bytes(bytes)
        .expect("idle-badge PNG must decode at startup");
    egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    )
}

/// Where on the screen the disk center should sit for a given corner.
/// Both idle and active windows are square with the disk at their visual
/// center, so we just compute the anchor with idle dimensions and let
/// `window_pos_for_anchor` recenter each state's window on it.
fn compute_icon_anchor(monitor: egui::Vec2, corner: &str) -> egui::Pos2 {
    let side_margin = 20.0;
    // Bottom margin from the screen edge to where the OVERLAY_SIZE-bounded
    // viewport bottom sits. The disk centre ends up `OVERLAY_SIZE.y * 0.5`
    // higher, which on the standard GNOME layout lands the badge just above
    // the dock without crowding it.
    let bottom_margin = 88.0;
    let top_margin = 20.0;
    let half = OVERLAY_SIZE.x * 0.5;
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

        // Treat any close on the overlay viewport as a quit. GNOME's dock
        // right-click menu sends WM_DELETE_WINDOW to all of an app's
        // top-level windows, including ours despite `with_taskbar(false)`,
        // so the user's expectation is that "退出" actually exits.
        // Earlier (0.1.21–0.1.23) we worried this would cause the daemon
        // to vanish silently from spurious closes during launch, but those
        // turned out to come from the GTK / winit race that 0.1.25 removed
        // by dropping the tray icon. With GTK gone, close events here are
        // unambiguously user intent.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.quit_requested = true;
            return;
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
        _became_active: bool,
    ) {
        let corner = self.shared_position.lock().clone();
        let overlay_pos = ctx
            .input(|i| i.viewport().monitor_size)
            .filter(|m| m.x > 0.0 && m.y > 0.0)
            .map(|m| window_pos_for_anchor(compute_icon_anchor(m, &corner), OVERLAY_SIZE));
        let corner_changed = corner != self.last_overlay_corner;

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
        // The window stays at OVERLAY_SIZE forever — InnerSize only fires the
        // first frame to confirm the requested size, then we never resize again.
        // OuterPosition only updates if the user changed the corner config.
        let needs_geometry = !self.overlay_geometry_set || corner_changed;
        if needs_geometry
            && let Some(p) = overlay_pos
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(OVERLAY_SIZE));
            self.overlay_geometry_set = true;
        }

        self.last_overlay_corner = corner;
    }

    fn render_overlay_content(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let rect = ui.max_rect();
        let painter = ui.painter().clone();
        match state {
            AppState::Idle => {
                let ctx = ui.ctx().clone();
                let texture = self.idle_badge.get_or_insert_with(|| {
                    ctx.load_texture(
                        "xsay_idle_badge",
                        decode_idle_badge(),
                        egui::TextureOptions::LINEAR,
                    )
                });
                // The PNG is square (480×480) and contains its own visual
                // padding around the disk + label, so we draw it filling the
                // full 120×120 window rect. Tint stays WHITE so the bundled
                // colours pass through unchanged.
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(1.0, 1.0),
                    ),
                    egui::Color32::WHITE,
                );
                // PNG is now disk-only (no label below), so the click region
                // is centred on the window. Surrounding transparent padding
                // keeps swallowed clicks to a minimum without overlapping
                // with anything the user might be trying to click behind us.
                let disk_rect = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(IDLE_DISK_RADIUS * 2.0, IDLE_DISK_RADIUS * 2.0),
                );
                let response = ui.interact(
                    disk_rect,
                    egui::Id::new("xsay_idle_badge"),
                    egui::Sense::click(),
                );
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
            .with_inner_size([OVERLAY_SIZE.x, OVERLAY_SIZE.y]),
        ..Default::default()
    }
}
