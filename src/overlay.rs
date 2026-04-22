use crate::{config::OverlayConfig, state::{AppState, SharedState}};
use eframe::egui;
use std::time::Duration;

pub struct XsayOverlay {
    shared_state: SharedState,
    animation_phase: f32,
    dots_phase: f32,
}

impl XsayOverlay {
    pub fn new(shared_state: SharedState) -> Self {
        Self {
            shared_state,
            animation_phase: 0.0,
            dots_phase: 0.0,
        }
    }
}

impl eframe::App for XsayOverlay {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.shared_state.lock().clone();

        ctx.request_repaint_after(Duration::from_millis(33)); // ~30 fps

        match &state {
            AppState::Idle => {
                // Render fully transparent so window is invisible
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                    .show(ctx, |_ui| {});
            }
            AppState::Recording { .. } => {
                self.animation_phase += 0.08;
                self.render_recording(ctx);
            }
            AppState::Transcribing => {
                self.dots_phase += 0.05;
                self.render_status(ctx, "Transcribing", egui::Color32::from_rgb(60, 120, 220));
            }
            AppState::Injecting => {
                self.render_status(ctx, "Typing...", egui::Color32::from_rgb(40, 160, 80));
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

impl XsayOverlay {
    fn render_recording(&self, ctx: &egui::Context) {
        let bg = egui::Color32::from_rgba_premultiplied(20, 20, 20, 210);
        let frame = egui::Frame::none()
            .fill(bg)
            .rounding(egui::Rounding::same(16.0));

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let painter = ui.painter();
            let rect = ui.max_rect();
            let center = rect.center();

            // Outer pulsing ring
            let pulse = (self.animation_phase.sin() * 0.5 + 0.5) as f32;
            let ring_radius = 32.0 + pulse * 10.0;
            painter.circle_stroke(
                center,
                ring_radius,
                egui::Stroke::new(2.5, egui::Color32::from_rgba_premultiplied(220, 60, 60, (180.0 * (1.0 - pulse * 0.4)) as u8)),
            );

            // Inner filled circle
            painter.circle_filled(
                center,
                22.0,
                egui::Color32::from_rgb(200, 50, 50),
            );

            // Microphone body (white rounded rect)
            let mic_top = center + egui::vec2(0.0, -18.0);
            let mic_rect = egui::Rect::from_center_size(
                center + egui::vec2(0.0, -8.0),
                egui::vec2(10.0, 18.0),
            );
            let _ = mic_top; // suppress warning
            painter.rect_filled(
                mic_rect,
                egui::Rounding::same(5.0),
                egui::Color32::WHITE,
            );

            // Microphone stand (arc approximated as three lines)
            let stand_y = center.y + 10.0;
            painter.line_segment(
                [egui::pos2(center.x - 12.0, stand_y), egui::pos2(center.x - 12.0, stand_y + 4.0)],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
            painter.line_segment(
                [egui::pos2(center.x + 12.0, stand_y), egui::pos2(center.x + 12.0, stand_y + 4.0)],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
            painter.line_segment(
                [egui::pos2(center.x - 12.0, stand_y + 4.0), egui::pos2(center.x + 12.0, stand_y + 4.0)],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
            painter.line_segment(
                [egui::pos2(center.x, stand_y + 4.0), egui::pos2(center.x, stand_y + 8.0)],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );

            // REC label
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

pub fn build_native_options(config: &OverlayConfig) -> eframe::NativeOptions {
    let size = [120.0_f32, 120.0_f32];

    // Default position: top-right with a small margin.
    // We can't know screen size at startup on all platforms without extra work,
    // so use a reasonable fixed offset; user can adjust via config.
    let position = match config.position.as_str() {
        "top-left" => egui::pos2(20.0, 20.0),
        "bottom-left" => egui::pos2(20.0, 900.0),
        "bottom-right" => egui::pos2(1780.0, 900.0),
        "center" => egui::pos2(900.0, 450.0),
        _ => egui::pos2(1780.0, 20.0), // top-right default
    };

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_resizable(false)
            .with_inner_size(size)
            .with_position(position),
        ..Default::default()
    }
}
