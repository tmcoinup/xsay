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
    last_positioned_size: egui::Vec2,
    last_positioned_corner: String,
    quit_requested: bool,
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
            last_positioned_size: egui::vec2(0.0, 0.0),
            last_positioned_corner: String::new(),
            quit_requested: false,
        }
    }
}

fn compute_corner_position(monitor: egui::Vec2, window: egui::Vec2, corner: &str) -> egui::Pos2 {
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
        "center" => egui::pos2((monitor.x - window.x) * 0.5, (monitor.y - window.y) * 0.5),
        _ => egui::pos2(monitor.x - window.x - side_margin, top_margin),
    }
}

impl eframe::App for XsayOverlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        for action in tray::poll_events() {
            match action {
                TrayAction::ShowSettings => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayAction::Quit => {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit_requested {
                return;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        if settings_ui::render(ctx, &mut self.settings) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        let state = self.shared_state.lock().clone();
        let is_idle = matches!(state, AppState::Idle);
        let became_active = self.was_idle && !is_idle;
        self.was_idle = is_idle;

        self.render_floating_overlay(ctx, state, became_active);

        let repaint_ms = if is_idle { 250 } else { 33 };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }
}

impl XsayOverlay {
    fn render_floating_overlay(
        &mut self,
        root_ctx: &egui::Context,
        state: AppState,
        became_active: bool,
    ) {
        let is_idle = matches!(state, AppState::Idle);
        let overlay_size = if is_idle {
            egui::vec2(44.0, 44.0)
        } else {
            egui::vec2(120.0, 120.0)
        };
        let corner = self.shared_position.lock().clone();
        let overlay_pos = root_ctx
            .input(|i| i.viewport().monitor_size)
            .filter(|m| m.x > 0.0 && m.y > 0.0)
            .map(|m| compute_corner_position(m, overlay_size, &corner));
        let needs_position = became_active
            || overlay_size != self.last_positioned_size
            || corner != self.last_positioned_corner;

        let mut builder = egui::ViewportBuilder::default()
            .with_title("xsay")
            .with_app_id("xsay-overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(!is_idle)
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size([overlay_size.x, overlay_size.y]);
        if let Some(p) = overlay_pos {
            builder = builder.with_position(p);
        }

        let overlay_state = state.clone();
        let animation_phase = &mut self.animation_phase;
        let dots_phase = &mut self.dots_phase;

        root_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("xsay_overlay"),
            builder,
            |ctx, _class| {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!is_idle));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(overlay_size));
                if needs_position {
                    if let Some(p) = overlay_pos {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
                    }
                }

                match &overlay_state {
                    AppState::Idle => render_idle_badge(ctx),
                    AppState::Recording { .. } => {
                        *animation_phase += 0.08;
                        render_mic_glyph(
                            ctx,
                            crate::theme::REC,
                            "REC",
                            crate::theme::REC,
                            true,
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
                            false,
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
                            false,
                            *animation_phase,
                            *dots_phase,
                        );
                    }
                }
            },
        );

        self.last_positioned_size = overlay_size;
        self.last_positioned_corner = corner;
    }
}

fn render_idle_badge(ctx: &egui::Context) {
    let frame = egui::Frame::new().fill(egui::Color32::TRANSPARENT);

    #[allow(deprecated)]
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let rect = ui.max_rect().shrink(3.0);
        let response = ui.interact(rect, egui::Id::new("xsay_idle_badge"), egui::Sense::click());
        paint_idle_logo(ui.painter(), rect);

        if response.hovered() {
            ui.painter().rect_stroke(
                rect.expand(1.0),
                crate::theme::radius_lg(),
                egui::Stroke::new(1.0, crate::theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }

        if response.clicked() {
            tray::request_show_settings();
        }
    });
}

fn paint_idle_logo(painter: &egui::Painter, rect: egui::Rect) {
    let r = rect.width().min(rect.height());
    let c = rect.center();
    let blue = egui::Color32::from_rgb(0x3B, 0x82, 0xF6);
    painter.circle_filled(c, r * 0.43, blue);
    painter.circle_stroke(
        c,
        r * 0.43,
        egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_premultiplied(255, 255, 255, 90),
        ),
    );

    let mic_rect = egui::Rect::from_center_size(
        c + egui::vec2(0.0, -r * 0.06),
        egui::vec2(r * 0.18, r * 0.3),
    );
    painter.rect_filled(
        mic_rect,
        egui::CornerRadius::same((r * 0.09) as u8),
        egui::Color32::WHITE,
    );

    let white = egui::Stroke::new((r * 0.055).max(1.8), egui::Color32::WHITE);
    let y = c.y + r * 0.08;
    painter.line_segment(
        [egui::pos2(c.x - r * 0.17, y), egui::pos2(c.x + r * 0.17, y)],
        white,
    );
    painter.line_segment(
        [
            egui::pos2(c.x - r * 0.17, y),
            egui::pos2(c.x - r * 0.17, y - r * 0.08),
        ],
        white,
    );
    painter.line_segment(
        [
            egui::pos2(c.x + r * 0.17, y),
            egui::pos2(c.x + r * 0.17, y - r * 0.08),
        ],
        white,
    );
    painter.line_segment([egui::pos2(c.x, y), egui::pos2(c.x, c.y + r * 0.25)], white);
}

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

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            let prefix = if pulse { "● " } else { "" };
            let dots = if pulse {
                String::new()
            } else {
                ".".repeat((dots_phase as usize % 4) + 1)
            };
            ui.label(
                egui::RichText::new(format!("{}{}{}", prefix, bottom_label, dots))
                    .color(label_color)
                    .size(crate::theme::FONT_XS),
            );
        });
    });
}

pub fn build_native_options(
    _config: &crate::config::OverlayConfig,
    show_window: bool,
) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xsay 设置")
            .with_app_id("xsay")
            .with_icon(app_icon())
            .with_decorations(false)
            .with_transparent(false)
            .with_visible(show_window)
            .with_resizable(true)
            .with_taskbar(true)
            .with_inner_size([700.0, 660.0])
            .with_min_inner_size([640.0, 540.0]),
        ..Default::default()
    }
}
