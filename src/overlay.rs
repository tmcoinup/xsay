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

    // Settings window
    show_settings: bool,
    settings: SettingsState,

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
        Self {
            shared_state,
            animation_phase: 0.0,
            dots_phase: 0.0,
            show_settings: false,
            settings,
            shared_position,
            last_positioned_size: egui::vec2(0.0, 0.0),
            last_positioned_corner: String::new(),
        }
    }
}

fn compute_corner_position(monitor: egui::Vec2, window: egui::Vec2, corner: &str) -> egui::Pos2 {
    let margin = 20.0;
    match corner {
        "top-left" => egui::pos2(margin, margin),
        "bottom-left" => egui::pos2(margin, monitor.y - window.y - margin),
        "bottom-right" => egui::pos2(
            monitor.x - window.x - margin,
            monitor.y - window.y - margin,
        ),
        "center" => egui::pos2(
            (monitor.x - window.x) * 0.5,
            (monitor.y - window.y) * 0.5,
        ),
        // "top-right" and fallback
        _ => egui::pos2(monitor.x - window.x - margin, margin),
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
                TrayAction::ShowSettings => self.show_settings = true,
                TrayAction::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let state = self.shared_state.lock().clone();

        ctx.request_repaint_after(Duration::from_millis(33));

        // Idle = main viewport hidden entirely (no desktop badge). User opens
        // the settings window through the tray menu. Recording/Transcribing/
        // Injecting bring the overlay back as a 120×120 feedback widget.
        let is_idle = matches!(state, AppState::Idle);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(!is_idle));

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

        // Settings window (separate viewport)
        if self.show_settings {
            let show_ref = &mut self.show_settings;
            let settings_ref = &mut self.settings;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("xsay_settings"),
                egui::ViewportBuilder::default()
                    .with_title("xsay 设置")
                    .with_inner_size([640.0, 540.0])
                    .with_min_inner_size([560.0, 420.0])
                    .with_resizable(true)
                    .with_decorations(false),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        *show_ref = false;
                    }
                    if settings_ui::render(ctx, settings_ref) {
                        *show_ref = false;
                    }
                },
            );
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
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true) // pure feedback widget; no clicks
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size([120.0, 120.0])
            .with_position(egui::pos2(1200.0, 20.0)),
        ..Default::default()
    }
}
