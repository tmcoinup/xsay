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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        // Target window size for this frame
        let target_size = match &state {
            AppState::Idle => egui::vec2(90.0, 30.0),
            _ => egui::vec2(120.0, 120.0),
        };

        match &state {
            AppState::Idle => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
                if !self.show_settings {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                }
                self.render_idle_badge(ctx);
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
                self.render_status(ctx, "识别中", egui::Color32::from_rgb(60, 120, 220));
            }
            AppState::Injecting => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
                self.render_status(ctx, "输入中", egui::Color32::from_rgb(40, 160, 80));
            }
        }

        // Re-anchor to the configured corner whenever the size changed, the
        // corner changed (from settings), or we haven't positioned yet.
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

        // Settings window (separate viewport)
        if self.show_settings {
            let show_ref = &mut self.show_settings;
            let settings_ref = &mut self.settings;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("xsay_settings"),
                egui::ViewportBuilder::default()
                    .with_title("xsay 设置")
                    .with_inner_size([580.0, 480.0])
                    .with_resizable(false)
                    .with_always_on_top(),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        *show_ref = false;
                    }
                    settings_ui::render(ctx, settings_ref);
                },
            );
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl XsayOverlay {
    fn render_idle_badge(&mut self, ctx: &egui::Context) {
        let bg = egui::Color32::from_rgba_premultiplied(30, 30, 30, 180);
        let frame = egui::Frame::none()
            .fill(bg)
            .rounding(egui::Rounding::same(8.0));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("⚙ xsay")
                            .color(egui::Color32::from_rgb(180, 180, 180))
                            .size(12.0),
                    )
                    .frame(false),
                );
                if btn.clicked() {
                    self.show_settings = true;
                }
                if btn.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            });
        });
    }

    fn render_recording(&self, ctx: &egui::Context) {
        let bg = egui::Color32::from_rgba_premultiplied(20, 20, 20, 210);
        let frame = egui::Frame::none()
            .fill(bg)
            .rounding(egui::Rounding::same(16.0));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let painter = ui.painter();
            let rect = ui.max_rect();
            let center = rect.center();

            // Pulsing ring
            let pulse = self.animation_phase.sin() * 0.5 + 0.5;
            let ring_r = 32.0 + pulse * 10.0;
            let alpha = (180.0 * (1.0 - pulse * 0.4)) as u8;
            painter.circle_stroke(
                center,
                ring_r,
                egui::Stroke::new(2.5, egui::Color32::from_rgba_premultiplied(220, 60, 60, alpha)),
            );

            // Inner circle
            painter.circle_filled(center, 22.0, egui::Color32::from_rgb(200, 50, 50));

            // Microphone body
            let mic_rect = egui::Rect::from_center_size(
                center + egui::vec2(0.0, -6.0),
                egui::vec2(10.0, 18.0),
            );
            painter.rect_filled(mic_rect, egui::Rounding::same(5.0), egui::Color32::WHITE);

            // Stand
            let sy = center.y + 11.0;
            let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
            painter.line_segment([egui::pos2(center.x - 12.0, sy), egui::pos2(center.x + 12.0, sy)], stroke);
            painter.line_segment([egui::pos2(center.x - 12.0, sy), egui::pos2(center.x - 12.0, sy - 5.0)], stroke);
            painter.line_segment([egui::pos2(center.x + 12.0, sy), egui::pos2(center.x + 12.0, sy - 5.0)], stroke);
            painter.line_segment([egui::pos2(center.x, sy), egui::pos2(center.x, sy + 6.0)], stroke);

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("● REC")
                        .color(egui::Color32::from_rgb(255, 80, 80))
                        .size(10.0),
                );
            });
        });
    }

    fn render_status(&self, ctx: &egui::Context, label: &str, color: egui::Color32) {
        let bg = egui::Color32::from_rgba_premultiplied(20, 20, 20, 200);
        let frame = egui::Frame::none()
            .fill(bg)
            .rounding(egui::Rounding::same(16.0));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                let dots_n = (self.dots_phase as usize % 4) + 1;
                let dots = ".".repeat(dots_n);
                ui.label(
                    egui::RichText::new(format!("{}{}", label, dots))
                        .color(color)
                        .size(13.0),
                );
            });
        });
    }
}

pub fn build_native_options(_config: &crate::config::OverlayConfig) -> eframe::NativeOptions {
    // Initial position is a conservative top-right estimate; re-anchored
    // precisely once the compositor reports monitor_size on the first frame.
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(false) // starts as badge (clickable)
            .with_resizable(false)
            .with_inner_size([90.0, 30.0])
            .with_position(egui::pos2(1200.0, 20.0)),
        ..Default::default()
    }
}
