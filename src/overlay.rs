use crate::{
    config::{Config, HotkeyConfig},
    settings_ui::{self, SettingsState},
    state::{AppState, SharedState},
};
use eframe::egui;
use parking_lot::Mutex;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub struct XsayOverlay {
    shared_state: SharedState,
    animation_phase: f32,
    dots_phase: f32,

    // Settings window
    show_settings: bool,
    settings: SettingsState,
}

impl XsayOverlay {
    pub fn new(
        shared_state: SharedState,
        shared_hotkey: Arc<Mutex<HotkeyConfig>>,
        capture_active: Arc<AtomicBool>,
    ) -> Self {
        let config = Config::load().unwrap_or_default();
        let settings = SettingsState::new(&config, shared_hotkey, capture_active);
        Self {
            shared_state,
            animation_phase: 0.0,
            dots_phase: 0.0,
            show_settings: false,
            settings,
        }
    }
}

impl eframe::App for XsayOverlay {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.shared_state.lock().clone();

        ctx.request_repaint_after(Duration::from_millis(33));

        match &state {
            AppState::Idle => {
                // Small badge, clickable, not passthrough
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
                if !self.show_settings {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(90.0, 30.0)));
                }
                self.render_idle_badge(ctx);
            }
            AppState::Recording { .. } => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(120.0, 120.0)));
                self.animation_phase += 0.08;
                self.render_recording(ctx);
            }
            AppState::Transcribing => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(120.0, 120.0)));
                self.dots_phase += 0.05;
                self.render_status(ctx, "识别中", egui::Color32::from_rgb(60, 120, 220));
            }
            AppState::Injecting => {
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(120.0, 120.0)));
                self.render_status(ctx, "输入中", egui::Color32::from_rgb(40, 160, 80));
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

pub fn build_native_options(config: &crate::config::OverlayConfig) -> eframe::NativeOptions {
    let position = match config.position.as_str() {
        "top-left" => egui::pos2(20.0, 20.0),
        "bottom-left" => egui::pos2(20.0, 900.0),
        "bottom-right" => egui::pos2(1780.0, 900.0),
        "center" => egui::pos2(900.0, 450.0),
        _ => egui::pos2(1780.0, 20.0),
    };

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(false) // starts as badge (clickable)
            .with_resizable(false)
            .with_inner_size([90.0, 30.0])
            .with_position(position),
        ..Default::default()
    }
}
