use crate::{
    config::{AudioConfig, Config, HotkeyConfig, InjectionConfig, TranscriptionConfig},
    settings_ui::{self, SettingsState},
    state::{AppState, SharedState},
    tray::{self, TrayAction},
};
use eframe::egui;
use parking_lot::Mutex;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

pub struct XsayOverlay {
    shared_state: SharedState,
    animation_phase: f32,
    dots_phase: f32,
    /// Previous frame's idle-ness, so we can detect the Idle → active
    /// transition and re-assert always-on-top + focus. Some compositors
    /// (GNOME under X11 focus-stealing-prevention, KWin) quietly drop
    /// the initial ABOVE hint when the window becomes visible, letting
    /// other windows cover our feedback badge.
    was_idle: bool,

    // Settings window
    show_settings: bool,
    settings: SettingsState,
    /// True for exactly one frame after the tray or a keyboard-invoked
    /// action asks to show settings. We translate it into a Focus +
    /// OuterPosition command on the nested viewport so an already-open but
    /// occluded window comes back to the foreground.
    settings_focus_requested: bool,
    /// Whether we've centered the settings viewport since it was (re-)opened.
    /// Reset when the window is closed so re-open re-centers once rather
    /// than snapping on every frame.
    settings_centered: bool,

    // Viewport positioning — configured corner (shared with settings UI so
    // changes re-anchor immediately) + last applied size to detect changes.
    shared_position: Arc<Mutex<String>>,
    last_positioned_size: egui::Vec2,
    last_positioned_corner: String,
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
        // Settings is xsay's main UI. Show it at startup every time, so
        // the app behaves like a conventional desktop application: user
        // launches, sees the settings window, can close it (red dot) to
        // hide into the tray. The actual recording overlay is a separate
        // 120×120 transparent feedback widget that only has content
        // during an active utterance.
        Self {
            shared_state,
            animation_phase: 0.0,
            dots_phase: 0.0,
            was_idle: true,
            show_settings: true,
            settings,
            settings_focus_requested: true,
            settings_centered: false,
            shared_position,
            last_positioned_size: egui::vec2(0.0, 0.0),
            last_positioned_corner: String::new(),
        }
    }
}

fn compute_corner_position(monitor: egui::Vec2, window: egui::Vec2, corner: &str) -> egui::Pos2 {
    // Top and side margins are cosmetic — 20px keeps the widget clear of
    // the top panel without wasting screen. Bottom margin is larger
    // because many desktop environments (Ubuntu's bottom dock, KDE's
    // default taskbar, macOS Dock) park a 60-80px tall strip at the
    // bottom that the widget would otherwise be hidden behind.
    let side_margin = 20.0;
    let bottom_margin = 88.0;
    let top_margin = 20.0;
    match corner {
        "top-left" => egui::pos2(side_margin, top_margin),
        "top-center" => egui::pos2((monitor.x - window.x) * 0.5, top_margin),
        "bottom-left" => egui::pos2(side_margin, monitor.y - window.y - bottom_margin),
        "bottom-right" => egui::pos2(
            monitor.x - window.x - side_margin,
            monitor.y - window.y - bottom_margin,
        ),
        "bottom-center" => egui::pos2(
            (monitor.x - window.x) * 0.5,
            monitor.y - window.y - bottom_margin,
        ),
        "center" => egui::pos2(
            (monitor.x - window.x) * 0.5,
            (monitor.y - window.y) * 0.5,
        ),
        // "top-right" and fallback
        _ => egui::pos2(monitor.x - window.x - side_margin, top_margin),
    }
}

impl eframe::App for XsayOverlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // egui 0.34 renamed `App::update` → `App::ui` and passes a root Ui
        // instead of a Context. Most of our code still thinks in terms of
        // viewport commands keyed on Context, so we take a clone and keep
        // the original body largely unchanged.
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Handle tray menu events
        for action in tray::poll_events() {
            match action {
                TrayAction::ShowSettings => {
                    // If the settings viewport is already open but
                    // obscured, setting show_settings=true alone has no
                    // effect. Raise a flag the settings viewport callback
                    // reads next frame to send a Focus command.
                    self.show_settings = true;
                    self.settings_focus_requested = true;
                }
                TrayAction::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let state = self.shared_state.lock().clone();

        ctx.request_repaint_after(Duration::from_millis(33));

        // Window is always visible (transparent + mouse-passthrough when
        // Idle). On Idle → active transition, re-assert AlwaysOnTop and
        // kick an immediate repaint so the first frame of content lands
        // ASAP, not waiting for the scheduled 33ms tick.
        let is_idle = matches!(state, AppState::Idle);
        let became_active = self.was_idle && !is_idle;
        if became_active {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.request_repaint();
        }
        self.was_idle = is_idle;

        let target_size = egui::vec2(120.0, 120.0);

        match &state {
            AppState::Idle => {
                // Nothing to render — viewport is hidden.
            }
            AppState::Recording { .. } => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                self.animation_phase += 0.08;
                self.render_recording(ctx);
            }
            AppState::Transcribing => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                self.dots_phase += 0.05;
                self.render_status(ctx, "识别中", crate::theme::ACCENT);
            }
            AppState::Injecting => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                self.dots_phase += 0.05;
                self.render_status(ctx, "输入中", crate::theme::SUCCESS);
            }
        }

        // Re-anchor the feedback widget to the configured corner, only while
        // visible. Skipped during Idle to save cycles.
        if !is_idle {
            let corner = self.shared_position.lock().clone();
            if target_size != self.last_positioned_size || corner != self.last_positioned_corner {
                if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                    if monitor.x > 0.0 && monitor.y > 0.0 {
                        let pos = compute_corner_position(monitor, target_size, &corner);
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                        self.last_positioned_size = target_size;
                        self.last_positioned_corner = corner;
                    }
                }
            }
        }

        // Settings window (separate viewport). Centered on first open each
        // session, re-raised to foreground when the user asks again via
        // tray (handles the "window obscured behind another" case).
        if self.show_settings {
            let inner_size = egui::vec2(700.0, 660.0);
            // Compute a monitor-center position to use on initial creation.
            // egui only applies `with_position` once per viewport lifetime;
            // subsequent re-shows need an explicit OuterPosition command,
            // which we send below when the centering flag is reset.
            let center_pos = ctx
                .input(|i| i.viewport().monitor_size)
                .filter(|m| m.x > 0.0 && m.y > 0.0)
                .map(|m| {
                    egui::pos2(
                        ((m.x - inner_size.x) * 0.5).max(0.0),
                        ((m.y - inner_size.y) * 0.5).max(0.0),
                    )
                });

            let mut builder = egui::ViewportBuilder::default()
                .with_title("xsay 设置")
                .with_inner_size([inner_size.x, inner_size.y])
                .with_min_inner_size([640.0, 540.0])
                .with_resizable(true)
                .with_decorations(false);
            if let Some(p) = center_pos {
                builder = builder.with_position(p);
            }

            let show_ref = &mut self.show_settings;
            let settings_ref = &mut self.settings;
            let focus_req = std::mem::take(&mut self.settings_focus_requested);
            let needs_recenter = !self.settings_centered;
            self.settings_centered = true;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("xsay_settings"),
                builder,
                |ctx, _class| {
                    // On (re-)open, move to screen center explicitly —
                    // with_position alone only applies to the first-ever
                    // creation; a reopen would otherwise land wherever the
                    // user last moved it, which can be offscreen.
                    if needs_recenter {
                        if let Some(p) = center_pos {
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
                        }
                    }
                    // Tray "Show Settings" while the window is already
                    // open → restore from minimized + raise to foreground
                    // + focus. Using only Focus doesn't un-minimize on most
                    // compositors, leaving the tray click looking broken.
                    if focus_req {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if ctx.input(|i| i.viewport().close_requested()) {
                        *show_ref = false;
                    }
                    if settings_ui::render(ctx, settings_ref) {
                        *show_ref = false;
                    }
                },
            );
        } else {
            // Reset the "centered" flag so the NEXT open re-centers once.
            self.settings_centered = false;
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl XsayOverlay {
    fn render_recording(&self, ctx: &egui::Context) {
        self.render_state_with_mic(
            ctx,
            crate::theme::REC,
            "● REC",
            crate::theme::REC,
            /*pulse=*/ true,
        );
    }

    /// Draws a colored filled circle with a white microphone glyph in the
    /// center, plus a bottom label. Used by Recording (pulsing red),
    /// Transcribing (blue) and Injecting (green) — same visual language
    /// across all active states.
    fn render_state_with_mic(
        &self,
        ctx: &egui::Context,
        circle_color: egui::Color32,
        bottom_label: &str,
        label_color: egui::Color32,
        pulse: bool,
    ) {
        let bg = egui::Color32::from_rgba_premultiplied(0x14, 0x14, 0x1A, 210);
        let frame = egui::Frame::new()
            .fill(bg)
            .corner_radius(crate::theme::radius_xxl());

        // Using `show` on a Context is the right call here: `render_state_
        // _with_mic` is invoked from `App::ui` where we only pass a cloned
        // Context. Suppress the deprecation note — the alternative
        // `show_inside(&mut Ui)` would require threading the root Ui
        // through every render helper, which obscures the flow.
        #[allow(deprecated)]
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let painter = ui.painter();
            let rect = ui.max_rect();
            let center = rect.center();

            if pulse {
                let p = self.animation_phase.sin() * 0.5 + 0.5;
                let ring_r = 32.0 + p * 10.0;
                let [r, g, b, _] = circle_color.to_array();
                let alpha = (180.0 * (1.0 - p * 0.4)) as u8;
                painter.circle_stroke(
                    center,
                    ring_r,
                    egui::Stroke::new(
                        2.5,
                        egui::Color32::from_rgba_premultiplied(r, g, b, alpha),
                    ),
                );
            }

            painter.circle_filled(center, 22.0, circle_color);

            // Mic body
            let mic_rect = egui::Rect::from_center_size(
                center + egui::vec2(0.0, -6.0),
                egui::vec2(10.0, 18.0),
            );
            painter.rect_filled(mic_rect, egui::CornerRadius::same(5), egui::Color32::WHITE);

            // Stand
            let sy = center.y + 11.0;
            let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
            painter.line_segment(
                [egui::pos2(center.x - 12.0, sy), egui::pos2(center.x + 12.0, sy)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x - 12.0, sy), egui::pos2(center.x - 12.0, sy - 5.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x + 12.0, sy), egui::pos2(center.x + 12.0, sy - 5.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x, sy), egui::pos2(center.x, sy + 6.0)],
                stroke,
            );

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let dots = if pulse {
                    String::new()
                } else {
                    ".".repeat((self.dots_phase as usize % 4) + 1)
                };
                ui.label(
                    egui::RichText::new(format!("{}{}", bottom_label, dots))
                        .color(label_color)
                        .size(crate::theme::FONT_XS),
                );
            });
        });
    }

    fn render_status(&self, ctx: &egui::Context, label: &str, color: egui::Color32) {
        self.render_state_with_mic(ctx, color, label, color, /*pulse=*/ false);
    }
}

pub fn build_native_options(_config: &crate::config::OverlayConfig) -> eframe::NativeOptions {
    // Start invisible — we go to Idle immediately and only show the window
    // when the user starts recording. The first update() call flips Visible
    // based on AppState.
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Title + app_id so GNOME shows "xsay" in its "not responding"
            // prompts and task switcher instead of "Unknown". Also helps
            // the compositor group this window with the tray icon.
            .with_title("xsay")
            .with_app_id("xsay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true) // pure feedback widget; no clicks
            .with_resizable(false)
            // Start VISIBLE (transparent + mouse-passthrough = invisible
            // to the user but tracked by the compositor). Previously we
            // started with_visible(false) and flipped Visible(true) only
            // on Idle→active transitions, which raced short hotkey taps:
            // if the state machine returned to Idle before the Visible
            // command reached the compositor, the overlay never showed.
            // An always-live transparent window eliminates that race.
            .with_inner_size([120.0, 120.0])
            .with_position(egui::pos2(1200.0, 20.0)),
        ..Default::default()
    }
}
