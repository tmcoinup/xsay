//! Settings window — split into one submodule per tab to keep each file
//! focused and readable. The top-level `render()` dispatches to the active
//! tab; shared state lives in `SettingsState` here.

mod general_tab;
mod history_tab;
mod hotkey_tab;
mod model_tab;
mod models;

pub use models::{ModelInfo, MODELS};

use crate::config::{AudioConfig, Config, HotkeyConfig, InjectionConfig, TranscriptionConfig};
use crate::download::{DownloadCmd, DownloadProgress};
use crossbeam_channel::Sender;
use eframe::egui;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

pub struct ActiveDownload {
    pub filename: String,
    pub progress: Arc<DownloadProgress>,
    pub cmd_tx: Sender<DownloadCmd>,
}

#[derive(PartialEq)]
pub enum Tab {
    Model,
    Hotkey,
    General,
    History,
}

pub struct SettingsState {
    pub tab: Tab,

    // Has the settings viewport's egui::Context been given a CJK font yet?
    // Each viewport has its own Context, so fonts must be installed per-viewport.
    pub fonts_installed: bool,

    // Model tab
    pub active_download: Option<ActiveDownload>,
    pub remote_sizes: HashMap<String, Option<u64>>,
    pub update_rx: Option<crossbeam_channel::Receiver<(String, Option<u64>)>>,
    pub checking_updates: bool,

    // Hotkey tab
    pub hotkey_key: String,
    pub hotkey_mods: Vec<String>,
    pub hotkey_mode: String,
    pub capturing: bool,

    // Shared state (worker threads read these live)
    pub shared_hotkey: Arc<Mutex<HotkeyConfig>>,
    pub shared_audio: Arc<Mutex<AudioConfig>>,
    pub shared_inject: Arc<Mutex<InjectionConfig>>,
    pub shared_transcription: Arc<Mutex<TranscriptionConfig>>,
    pub shared_position: Arc<Mutex<String>>,
    pub capture_active: Arc<AtomicBool>,
    pub capture_slot: Arc<crate::hotkey::CaptureSlot>,
    pub backend_info: Arc<crate::hotkey::BackendInfo>,

    pub audio_devices: Vec<String>,
    pub cache_dir: PathBuf,
    pub hf_repo: String,
    pub model_reload_tx: crossbeam_channel::Sender<PathBuf>,

    pub status_msg: Option<(String, egui::Color32)>,
}

impl SettingsState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &Config,
        shared_hotkey: Arc<Mutex<HotkeyConfig>>,
        shared_audio: Arc<Mutex<AudioConfig>>,
        shared_inject: Arc<Mutex<InjectionConfig>>,
        shared_transcription: Arc<Mutex<TranscriptionConfig>>,
        shared_position: Arc<Mutex<String>>,
        capture_active: Arc<AtomicBool>,
        capture_slot: Arc<crate::hotkey::CaptureSlot>,
        backend_info: Arc<crate::hotkey::BackendInfo>,
        model_reload_tx: crossbeam_channel::Sender<PathBuf>,
    ) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_default()
            .join("xsay")
            .join("models");

        Self {
            tab: Tab::Model,
            fonts_installed: false,
            active_download: None,
            remote_sizes: HashMap::new(),
            update_rx: None,
            checking_updates: false,
            hotkey_key: config.hotkey.key.clone(),
            hotkey_mods: config.hotkey.modifiers.clone(),
            hotkey_mode: config.hotkey.mode.clone(),
            capturing: false,
            shared_hotkey,
            shared_audio,
            shared_inject,
            shared_transcription,
            shared_position,
            capture_active,
            capture_slot,
            backend_info,
            audio_devices: crate::audio::input_device_names(),
            cache_dir,
            hf_repo: config.model.hf_repo.clone(),
            model_reload_tx,
            status_msg: None,
        }
    }
}

/// Entry point called each frame by the settings viewport in `overlay.rs`.
/// Returns `true` when the user clicked the custom close button.
pub fn render(ctx: &egui::Context, state: &mut SettingsState) -> bool {
    // Settings runs in its own viewport, which has its own Context/fonts.
    // Install CJK font once so Chinese labels don't render as tofu.
    if !state.fonts_installed {
        crate::fonts::install(ctx);
        state.fonts_installed = true;
    }

    // Drain remote update-check results — the checker spawns one thread per
    // model and sends results back through `update_rx`.
    if let Some(rx) = &state.update_rx {
        while let Ok((fname, size)) = rx.try_recv() {
            state.remote_sizes.insert(fname, size);
        }
        if state.remote_sizes.len() >= MODELS.len() {
            state.checking_updates = false;
        }
    }

    // Key capture runs at the top level so it works regardless of active tab.
    hotkey_tab::handle_key_capture(ctx, state);

    let mut close_requested = false;

    // Custom title bar in its own top panel — replaces the OS decoration
    // (which we disable via with_decorations(false)) so we can:
    //   - render "xsay 设置" using the injected CJK font
    //   - match the dark theme instead of a white Gnome/Yaru bar
    //   - draw macOS-style traffic lights to match the Figma reference
    egui::TopBottomPanel::top("xsay_titlebar")
        .exact_height(36.0)
        .frame(
            egui::Frame::none()
                .fill(crate::theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 0.0)),
        )
        .show(ctx, |ui| {
            if render_title_bar(ui, ctx) {
                close_requested = true;
            }
        });

    // Content panel: BG_WINDOW
    let panel_frame = egui::Frame::none()
        .fill(crate::theme::BG_WINDOW)
        .inner_margin(egui::Margin::symmetric(16.0, 12.0));

    egui::CentralPanel::default()
        .frame(panel_frame)
        .show(ctx, |ui| {
            render_tab_bar(ui, state);
            ui.add_space(12.0);

            match state.tab {
                Tab::Model => model_tab::render(ui, state),
                Tab::Hotkey => hotkey_tab::render(ui, state),
                Tab::General => general_tab::render(ui, state),
                Tab::History => history_tab::render(ui, state),
            }
        });

    close_requested
}

/// Custom window title bar. macOS-style traffic lights on the left (red =
/// close, yellow/green decorative), "xsay 设置" centered, empty space on the
/// right. The whole bar is a drag surface.
///
/// Returns true if the red close button was clicked.
fn render_title_bar(ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
    let bar_rect = ui.max_rect();
    let mut close_clicked = false;

    // Whole-bar drag region — any non-interactive click on the bar starts
    // moving the window.
    let bar_response = ui.interact(
        bar_rect,
        egui::Id::new("titlebar_drag"),
        egui::Sense::click_and_drag(),
    );
    if bar_response.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    // Centered title
    ui.painter().text(
        bar_rect.center(),
        egui::Align2::CENTER_CENTER,
        "xsay 设置",
        egui::FontId::proportional(crate::theme::FONT_BODY),
        crate::theme::TEXT_PRIMARY,
    );

    // Three macOS traffic-light dots on the left, 12px diameter, 8px gap.
    let dot_y = bar_rect.center().y;
    let dot_r = 6.0;
    let dot_start_x = bar_rect.min.x + 6.0;
    let colors = [
        egui::Color32::from_rgb(0xFF, 0x5F, 0x57), // red (close)
        egui::Color32::from_rgb(0xFE, 0xBC, 0x2E), // yellow
        egui::Color32::from_rgb(0x28, 0xC8, 0x40), // green
    ];
    for (i, color) in colors.iter().enumerate() {
        let center = egui::pos2(dot_start_x + dot_r + i as f32 * (dot_r * 2.0 + 4.0), dot_y);
        let rect = egui::Rect::from_center_size(center, egui::vec2(dot_r * 2.0, dot_r * 2.0));
        let resp = ui.interact(
            rect,
            egui::Id::new(("titlebar_dot", i)),
            egui::Sense::click(),
        );
        let hovered = resp.hovered();
        let fill = if hovered {
            color.linear_multiply(1.3)
        } else {
            *color
        };
        ui.painter().circle_filled(center, dot_r, fill);
        if hovered {
            ui.painter().circle_stroke(
                center,
                dot_r,
                egui::Stroke::new(0.8, egui::Color32::from_rgba_premultiplied(0, 0, 0, 120)),
            );
        }
        if i == 0 && resp.clicked() {
            close_clicked = true;
        }
    }

    close_clicked
}

/// Custom tab bar: active tab filled with theme::ACCENT, others are plain
/// text on the window bg. Replaces egui's default selectable_label (which
/// uses a subtle grey fill that doesn't match the design). Icons are
/// painter-drawn to avoid emoji tofu.
fn render_tab_bar(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tab, icon, label) in [
            (Tab::Model, crate::theme::Icon::Box, "模型"),
            (Tab::Hotkey, crate::theme::Icon::Keyboard, "快捷键"),
            (Tab::General, crate::theme::Icon::Gear, "常规"),
            (Tab::History, crate::theme::Icon::Document, "历史记录"),
        ] {
            let active = state.tab == tab;
            if render_tab_button(ui, icon, label, active) {
                state.tab = tab;
            }
        }
    });
}

fn render_tab_button(
    ui: &mut egui::Ui,
    icon: crate::theme::Icon,
    label: &str,
    active: bool,
) -> bool {
    let icon_size = 14.0;
    let gap = 6.0;
    let height = 30.0;
    let pad_x = 12.0;

    let font_id = egui::FontId::proportional(crate::theme::FONT_BODY);
    let text_size = ui.fonts(|f| {
        f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::WHITE)
            .size()
    });

    let total = egui::vec2(
        pad_x * 2.0 + icon_size + gap + text_size.x,
        height,
    );
    let (rect, response) = ui.allocate_exact_size(total, egui::Sense::click());

    let (bg, fg) = if active {
        (crate::theme::ACCENT, egui::Color32::WHITE)
    } else if response.hovered() {
        (crate::theme::BG_CARD, crate::theme::TEXT_PRIMARY)
    } else {
        (egui::Color32::TRANSPARENT, crate::theme::TEXT_SECONDARY)
    };

    ui.painter()
        .rect_filled(rect, crate::theme::radius_md(), bg);

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + pad_x, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    crate::theme::draw_icon(ui.painter(), icon_rect, icon, fg);

    ui.painter().text(
        egui::pos2(rect.min.x + pad_x + icon_size + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font_id,
        fg,
    );

    response.clicked()
}
