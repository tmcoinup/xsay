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
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        // Drive repaint at ~30fps so the state machine + overlay animation
        // stay live. request_repaint() on the ctx makes eframe keep
        // polling even while the window is hidden (close-to-tray case).
        ctx.request_repaint_after(Duration::from_millis(33));

        // -----------------------------------------------------------------
        // Tray menu actions. "Show Settings" re-shows + focuses the
        // main window (whether it was hidden, minimized, or occluded).
        // "Quit" cleanly closes the main viewport, ending the process.
        // -----------------------------------------------------------------
        for action in tray::poll_events() {
            match action {
                TrayAction::ShowSettings => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayAction::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // -----------------------------------------------------------------
        // Close-to-tray intercept. When the user clicks the window's red
        // traffic light (or the compositor's own close button, if they
        // override our decorations), eframe wants to exit. We CancelClose
        // and send Visible(false) instead — xsay keeps running, the tray
        // icon stays, and the user can reopen from there.
        // -----------------------------------------------------------------
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // -----------------------------------------------------------------
        // Main window content = settings panel. Renders directly into the
        // root viewport's egui::Context. `settings_ui::render` returns
        // true when the user clicks our custom red traffic light — treat
        // that as "hide to tray" just like the compositor's close button.
        // -----------------------------------------------------------------
        if settings_ui::render(ctx, &mut self.settings) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // -----------------------------------------------------------------
        // Recording feedback overlay — nested viewport that renders
        // only when xsay is actively recording / transcribing / injecting.
        // Always-on-top + transparent + mouse-passthrough + skip-taskbar
        // so it's a pure visual indicator, not a window the user has to
        // manage. Stays alive across state transitions (re-created if
        // state drops back to Idle and later becomes active again).
        // -----------------------------------------------------------------
        let state = self.shared_state.lock().clone();
        let is_idle = matches!(state, AppState::Idle);
        if !is_idle {
            let became_active = self.was_idle;
            let overlay_size = egui::vec2(120.0, 120.0);
            let corner = self.shared_position.lock().clone();

            // Precompute overlay position from monitor dimensions.
            let overlay_pos = ctx
                .input(|i| i.viewport().monitor_size)
                .filter(|m| m.x > 0.0 && m.y > 0.0)
                .map(|m| compute_corner_position(m, overlay_size, &corner));

            let mut builder = egui::ViewportBuilder::default()
                .with_title("xsay recording")
                .with_app_id("xsay-overlay")
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_mouse_passthrough(true)
                .with_resizable(false)
                .with_taskbar(false)
                .with_inner_size([overlay_size.x, overlay_size.y]);
            if let Some(p) = overlay_pos {
                builder = builder.with_position(p);
            }

            // Split borrows so the closure can mutate animation state
            // without also holding &self (render_mic_glyph is a free
            // function that doesn't need Self).
            let overlay_state = state.clone();
            let animation_phase = &mut self.animation_phase;
            let dots_phase = &mut self.dots_phase;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("xsay_overlay"),
                builder,
                |ctx, _class| {
                    // Re-assert always-on-top + position when the
                    // overlay transitions from Idle → active. Builder
                    // hints only apply at viewport creation; a
                    // long-running xsay where state oscillates needs
                    // explicit commands so the overlay keeps returning
                    // to the right corner, above everything else.
                    if became_active {
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::AlwaysOnTop,
                        ));
                        if let Some(p) = overlay_pos {
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
                        }
                    }

                    match &overlay_state {
                        AppState::Idle => {}
                        AppState::Recording { .. } => {
                            *animation_phase += 0.08;
                            render_mic_glyph(
                                ctx,
                                crate::theme::REC,
                                "● REC",
                                crate::theme::REC,
                                /*pulse=*/ true,
                                *animation_phase,
                                *dots_phase,
                            );
                        }
                        AppState::Transcribing => {
                            *dots_phase += 0.05;
                            render_mic_glyph(
                                ctx,
                                crate::theme::ACCENT,
                                "识别中",
                                crate::theme::ACCENT,
                                /*pulse=*/ false,
                                *animation_phase,
                                *dots_phase,
                            );
                        }
                        AppState::Injecting => {
                            *dots_phase += 0.05;
                            render_mic_glyph(
                                ctx,
                                crate::theme::SUCCESS,
                                "输入中",
                                crate::theme::SUCCESS,
                                /*pulse=*/ false,
                                *animation_phase,
                                *dots_phase,
                            );
                        }
                    }
                },
            );
        }
        self.was_idle = is_idle;

        // Silence unused-field warnings — these still get used by
        // callers elsewhere in the file.
        let _ = &self.settings_focus_requested;
        let _ = &self.settings_centered;
        let _ = &self.shared_position;
        let _ = &self.last_positioned_size;
        let _ = &self.last_positioned_corner;
        let _ = &self.show_settings;
    }
}

#[allow(dead_code)] // kept for future standalone-overlay mode
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

    /// Same as `render_state_with_mic` but receives animation state
    /// explicitly rather than reading from `&self`, so it can be called
    /// from inside a nested-viewport callback where `self` is already
    /// borrowed elsewhere.
    fn render_state_with_mic_explicit(
        &self,
        ctx: &egui::Context,
        circle_color: egui::Color32,
        bottom_label: &str,
        label_color: egui::Color32,
        pulse: bool,
        animation_phase: f32,
        dots_phase: f32,
    ) {
        render_mic_glyph(
            ctx,
            circle_color,
            bottom_label,
            label_color,
            pulse,
            animation_phase,
            dots_phase,
        );
    }

    /// Draws a colored filled circle with a white microphone glyph in the
    /// center, plus a bottom label. Used by Recording (pulsing red),
    /// Transcribing (blue) and Injecting (green) — same visual language
    /// across all active states.
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    fn render_status(&self, ctx: &egui::Context, label: &str, color: egui::Color32) {
        self.render_state_with_mic(ctx, color, label, color, /*pulse=*/ false);
    }
}

/// Free-function mic-glyph renderer: pulled out of XsayOverlay so the
/// nested-viewport closure can call it without holding a second borrow
/// of `self` (the closure already mutably borrows animation_phase/dots).
fn render_mic_glyph(
    ctx: &egui::Context,
    circle_color: egui::Color32,
    bottom_label: &str,
    label_color: egui::Color32,
    pulse: bool,
    animation_phase: f32,
    dots_phase: f32,
) {
    let bg = egui::Color32::from_rgba_premultiplied(0x14, 0x14, 0x1A, 210);
    let frame = egui::Frame::new()
        .fill(bg)
        .corner_radius(crate::theme::radius_xxl());

    #[allow(deprecated)]
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let painter = ui.painter();
        let rect = ui.max_rect();
        let center = rect.center();

        if pulse {
            let p = animation_phase.sin() * 0.5 + 0.5;
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
                ".".repeat((dots_phase as usize % 4) + 1)
            };
            ui.label(
                egui::RichText::new(format!("{}{}", bottom_label, dots))
                    .color(label_color)
                    .size(crate::theme::FONT_XS),
            );
        });
    });
}

pub fn build_native_options(_config: &crate::config::OverlayConfig) -> eframe::NativeOptions {
    // Main viewport is the settings window — a real, decorated, resizable
    // app window that appears in the GNOME taskbar and responds to
    // close/minimize/focus like every other desktop app. The recording
    // overlay is a separate nested viewport spawned only when xsay is
    // actively recording / transcribing / injecting.
    //
    // The window starts VISIBLE (show_settings used to gate this but is
    // no longer needed now that the main viewport IS the settings
    // panel). User "close" triggers a CancelClose + Visible(false) in
    // App::ui so the window hides to tray instead of killing xsay.
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xsay")
            .with_app_id("xsay")
            .with_decorations(false) // we draw our own custom title bar
            .with_transparent(false)
            .with_resizable(true)
            .with_inner_size([700.0, 660.0])
            .with_min_inner_size([640.0, 540.0]),
        ..Default::default()
    }
}
