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
            audio_devices: crate::audio::input_device_names(),
            cache_dir,
            hf_repo: config.model.hf_repo.clone(),
            model_reload_tx,
            status_msg: None,
        }
    }
}

/// Entry point called each frame by the settings viewport in `overlay.rs`.
pub fn render(ctx: &egui::Context, state: &mut SettingsState) {
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

    // Fill the outer viewport with the window bg so there's no default
    // light-grey gap around the panel.
    let panel_frame = egui::Frame::none()
        .fill(crate::theme::BG_WINDOW)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0));

    egui::CentralPanel::default()
        .frame(panel_frame)
        .show(ctx, |ui| {
            render_tab_bar(ui, state);
            ui.add_space(10.0);

            match state.tab {
                Tab::Model => model_tab::render(ui, state),
                Tab::Hotkey => hotkey_tab::render(ui, state),
                Tab::General => general_tab::render(ui, state),
                Tab::History => history_tab::render(ui, state),
            }
        });
}

/// Custom tab bar: active tab filled with theme::ACCENT, others are plain
/// white text on the window bg. Replaces egui's default selectable_label
/// (which uses a subtle grey fill that doesn't match the design).
fn render_tab_bar(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.horizontal(|ui| {
        for (tab, label) in [
            (Tab::Model, "🤖  模型"),
            (Tab::Hotkey, "⌨  快捷键"),
            (Tab::General, "⚙  常规"),
            (Tab::History, "📜  历史记录"),
        ] {
            let active = state.tab == tab;
            let (bg, fg) = if active {
                (crate::theme::ACCENT, egui::Color32::WHITE)
            } else {
                (egui::Color32::TRANSPARENT, crate::theme::TEXT_SECONDARY)
            };

            let text = egui::RichText::new(label)
                .color(fg)
                .size(crate::theme::FONT_HEADING);
            let btn = egui::Button::new(text)
                .fill(bg)
                .rounding(crate::theme::radius_md())
                .min_size(egui::vec2(0.0, 28.0));

            if ui.add(btn).clicked() {
                state.tab = tab;
            }
        }
    });
}
