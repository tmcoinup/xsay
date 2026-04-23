use crate::{
    config::{
        AudioConfig, Config, HotkeyConfig, InjectionConfig, TranscriptionConfig,
    },
    download::{self, DlState, DownloadCmd, DownloadProgress},
};
use crossbeam_channel::Sender;
use eframe::egui;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

// ---------------------------------------------------------------------------
// Model catalogue
// ---------------------------------------------------------------------------

pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub size_mb: u32,
    pub desc: &'static str,
}

pub static MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Tiny",
        filename: "ggml-tiny.bin",
        size_mb: 75,
        desc: "最快，精度一般，适合低配设备",
    },
    ModelInfo {
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 147,
        desc: "快速，精度良好",
    },
    ModelInfo {
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 488,
        desc: "平衡速度与精度",
    },
    ModelInfo {
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
        desc: "高精度，推荐使用",
    },
    ModelInfo {
        name: "Large v3",
        filename: "ggml-large-v3.bin",
        size_mb: 3100,
        desc: "最高精度，速度较慢，需要大量内存",
    },
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

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
}

pub struct SettingsState {
    pub tab: Tab,

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

    // Shared with worker threads for live update
    pub shared_hotkey: Arc<Mutex<HotkeyConfig>>,
    pub shared_audio: Arc<Mutex<AudioConfig>>,
    pub shared_inject: Arc<Mutex<InjectionConfig>>,
    pub shared_transcription: Arc<Mutex<TranscriptionConfig>>,
    pub shared_position: Arc<Mutex<String>>,
    pub capture_active: Arc<AtomicBool>,

    // General tab — list of audio device names (read-only info for now)
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

// ---------------------------------------------------------------------------
// Top-level render
// ---------------------------------------------------------------------------

pub fn render(ctx: &egui::Context, state: &mut SettingsState) {
    // Poll update-check results
    if let Some(rx) = &state.update_rx {
        while let Ok((fname, size)) = rx.try_recv() {
            state.remote_sizes.insert(fname, size);
        }
        if state.remote_sizes.len() >= MODELS.len() {
            state.checking_updates = false;
        }
    }

    // Key capture
    if state.capturing {
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == egui::Key::Escape {
                        state.capturing = false;
                        state.capture_active.store(false, Ordering::SeqCst);
                        return;
                    }
                    if let Some(name) = egui_key_to_rdev(*key) {
                        state.hotkey_key = name.to_string();
                        let mut mods = Vec::new();
                        if modifiers.ctrl {
                            mods.push("ctrl".to_string());
                        }
                        if modifiers.alt {
                            mods.push("alt".to_string());
                        }
                        if modifiers.shift {
                            mods.push("shift".to_string());
                        }
                        if modifiers.mac_cmd || modifiers.command {
                            mods.push("super".to_string());
                        }
                        state.hotkey_mods = mods;
                        state.capturing = false;
                        state.capture_active.store(false, Ordering::SeqCst);
                    }
                }
            }
        });
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(state.tab == Tab::Model, "🤖  模型")
                .clicked()
            {
                state.tab = Tab::Model;
            }
            if ui
                .selectable_label(state.tab == Tab::Hotkey, "⌨  快捷键")
                .clicked()
            {
                state.tab = Tab::Hotkey;
            }
            if ui
                .selectable_label(state.tab == Tab::General, "⚙  常规")
                .clicked()
            {
                state.tab = Tab::General;
            }
        });
        ui.separator();

        match state.tab {
            Tab::Model => render_model_tab(ui, state),
            Tab::Hotkey => render_hotkey_tab(ui, state, ctx),
            Tab::General => render_general_tab(ui, state),
        }
    });
}

// ---------------------------------------------------------------------------
// Model tab
// ---------------------------------------------------------------------------

fn render_model_tab(ui: &mut egui::Ui, state: &mut SettingsState) {
    let current_filename = Config::load()
        .ok()
        .map(|c| c.model.hf_filename)
        .unwrap_or_default();

    let active_dl_filename = state
        .active_download
        .as_ref()
        .map(|d| d.filename.clone());

    // Collect download state snapshot (to avoid borrow issues inside loop)
    let dl_state_snap: Option<DlState> = state.active_download.as_ref().map(|d| {
        d.progress.state.lock().clone()
    });
    let dl_downloaded = state
        .active_download
        .as_ref()
        .map(|d| d.progress.downloaded.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(0);
    let dl_total = state
        .active_download
        .as_ref()
        .map(|d| d.progress.total.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(0);

    // Check if active download just completed
    if let Some(DlState::Completed) = &dl_state_snap {
        if let Some(fname) = &active_dl_filename {
            // Current configured model
            let cur = Config::load()
                .ok()
                .map(|c| c.model.hf_filename)
                .unwrap_or_default();
            let cur_exists = !cur.is_empty() && state.cache_dir.join(&cur).exists();

            let downloaded_path = state.cache_dir.join(fname);
            let nice_name = MODELS
                .iter()
                .find(|m| m.filename == fname.as_str())
                .map(|m| m.name)
                .unwrap_or(fname.as_str());

            if cur == *fname || !cur_exists {
                // Auto-activate: either it's already the selected one, or no model was loaded
                if let Ok(mut c) = Config::load() {
                    c.model.hf_filename = fname.clone();
                    if let Ok(p) = Config::config_path() {
                        if let Ok(t) = toml::to_string_pretty(&c) {
                            let _ = std::fs::write(p, t);
                        }
                    }
                }
                let _ = state.model_reload_tx.send(downloaded_path);
                state.status_msg = Some((
                    format!("✓ {} 下载完成并已启用", nice_name),
                    egui::Color32::from_rgb(80, 200, 80),
                ));
            } else {
                state.status_msg = Some((
                    format!("✓ {} 下载完成", nice_name),
                    egui::Color32::from_rgb(80, 200, 80),
                ));
            }
        }
        state.active_download = None;
    }
    if let Some(DlState::Cancelled) = &dl_state_snap {
        state.active_download = None;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        // Banner if no model is available
        let cur_exists = !current_filename.is_empty()
            && state.cache_dir.join(&current_filename).exists();
        if !cur_exists {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(90, 45, 20))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .rounding(egui::Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("⚠  当前没有可用模型，xsay 无法识别语音")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("推荐下载 Medium (1.5 GB，中英文高精度)")
                            .color(egui::Color32::from_rgb(255, 220, 150))
                            .small(),
                    );
                });
            ui.add_space(6.0);
        }

        for model in MODELS {
            let local_path = state.cache_dir.join(model.filename);
            let partial_path = download::partial_path(&local_path);
            let is_current = model.filename == current_filename;
            let is_this_dl = active_dl_filename.as_deref() == Some(model.filename);

            let is_downloaded = local_path.exists();
            let has_partial = partial_path.exists() && !is_downloaded;
            let local_size = local_path.metadata().map(|m| m.len()).unwrap_or(0);
            let partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);

            let remote = state.remote_sizes.get(model.filename).copied().flatten();

            let frame_color = if is_current {
                egui::Color32::from_rgb(30, 60, 30)
            } else {
                ui.visuals().extreme_bg_color
            };

            egui::Frame::none()
                .fill(frame_color)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .rounding(egui::Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // --- Select radio ---
                        let selected = is_current;
                        let radio = ui.radio(selected, "");
                        if radio.clicked() && is_downloaded && !is_current {
                            switch_model(model, &local_path, state);
                        }

                        ui.vertical(|ui| {
                            // --- Header row ---
                            ui.horizontal(|ui| {
                                ui.strong(model.name);
                                ui.label(
                                    egui::RichText::new(format!("({} MB)", model.size_mb))
                                        .small()
                                        .weak(),
                                );
                                ui.label(egui::RichText::new(model.desc).weak().small());

                                if is_current {
                                    ui.label(
                                        egui::RichText::new(" ✓ 当前使用 ")
                                            .color(egui::Color32::from_rgb(80, 220, 80))
                                            .small(),
                                    );
                                }

                                // Update indicator
                                if let Some(remote_size) = remote {
                                    if is_downloaded {
                                        if remote_size != local_size {
                                            ui.label(
                                                egui::RichText::new("↑ 有更新")
                                                    .color(egui::Color32::YELLOW)
                                                    .small(),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("✓ 最新")
                                                    .color(egui::Color32::DARK_GREEN)
                                                    .small(),
                                            );
                                        }
                                    }
                                }
                            });

                            // --- Progress / size info ---
                            if is_this_dl {
                                if dl_total > 0 {
                                    let frac = dl_downloaded as f32 / dl_total as f32;
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .desired_width(300.0)
                                            .text(format!(
                                                "{:.1}/{:.0} MB  {:.0}%",
                                                dl_downloaded as f32 / 1e6,
                                                dl_total as f32 / 1e6,
                                                frac * 100.0
                                            )),
                                    );
                                } else {
                                    ui.spinner();
                                }
                            } else if has_partial {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "已下载 {:.1}/{} MB，可继续",
                                        partial_size as f32 / 1e6,
                                        model.size_mb
                                    ))
                                    .weak()
                                    .small(),
                                );
                            } else if is_downloaded {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} MB",
                                        local_size as f32 / 1e6
                                    ))
                                    .weak()
                                    .small(),
                                );
                            }

                            // --- Action buttons ---
                            ui.horizontal(|ui| {
                                if is_this_dl {
                                    // Controls for active download
                                    let paused = matches!(dl_state_snap, Some(DlState::Paused));
                                    if paused {
                                        if ui.small_button("▶ 继续").clicked() {
                                            start_model_download(model, state);
                                        }
                                    } else if ui.small_button("⏸ 暂停").clicked() {
                                        if let Some(dl) = &state.active_download {
                                            let _ = dl.cmd_tx.send(DownloadCmd::Pause);
                                        }
                                    }
                                    if ui.small_button("✕ 取消").clicked() {
                                        if let Some(dl) = &state.active_download {
                                            let _ = dl.cmd_tx.send(DownloadCmd::Cancel);
                                        }
                                        state.active_download = None;
                                    }

                                    if let Some(DlState::Failed(ref e)) = dl_state_snap {
                                        ui.label(
                                            egui::RichText::new(format!("错误: {}", e))
                                                .color(egui::Color32::RED)
                                                .small(),
                                        );
                                        if ui.small_button("重试").clicked() {
                                            state.active_download = None;
                                            start_model_download(model, state);
                                        }
                                    }
                                } else {
                                    // Not downloading this model
                                    if !is_downloaded {
                                        let btn_label =
                                            if has_partial { "▶ 继续下载" } else { "⬇ 下载" };
                                        let enabled = state.active_download.is_none();
                                        if ui
                                            .add_enabled(enabled, egui::Button::new(btn_label).small())
                                            .clicked()
                                        {
                                            start_model_download(model, state);
                                        }
                                        if has_partial && ui.small_button("✕ 删除进度").clicked() {
                                            let _ = std::fs::remove_file(&partial_path);
                                        }
                                    }

                                    if is_downloaded && !is_current {
                                        if ui.small_button("✓ 切换使用").clicked() {
                                            switch_model(model, &local_path, state);
                                        }
                                        if ui.small_button("🗑 删除").clicked() {
                                            let _ = std::fs::remove_file(&local_path);
                                        }
                                    }
                                }
                            });
                        });
                    });
                });

            ui.add_space(2.0);
        }

        ui.separator();
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let checking = state.checking_updates;
            if ui
                .add_enabled(
                    !checking,
                    egui::Button::new(if checking {
                        "🔄 检查中..."
                    } else {
                        "🔄 检查所有模型更新"
                    }),
                )
                .clicked()
            {
                check_all_updates(state);
            }
        });

        if let Some((msg, color)) = &state.status_msg {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(msg).color(*color));
        }
    });
}

fn start_model_download(model: &ModelInfo, state: &mut SettingsState) {
    let url = download::hf_url(&state.hf_repo, model.filename);
    let dest = state.cache_dir.join(model.filename);
    let _ = std::fs::create_dir_all(&state.cache_dir);

    // Reuse existing progress if paused on same model, else create new
    let progress = if state
        .active_download
        .as_ref()
        .map(|d| d.filename == model.filename)
        .unwrap_or(false)
    {
        state
            .active_download
            .as_ref()
            .map(|d| Arc::clone(&d.progress))
            .unwrap()
    } else {
        DownloadProgress::new()
    };

    let cmd_tx = download::start_download(url, dest, Arc::clone(&progress));
    state.active_download = Some(ActiveDownload {
        filename: model.filename.to_string(),
        progress,
        cmd_tx,
    });
}

fn switch_model(model: &ModelInfo, local_path: &PathBuf, state: &mut SettingsState) {
    // Persist to config.toml
    if let Ok(mut cfg) = Config::load() {
        cfg.model.hf_filename = model.filename.to_string();
        if let Ok(path) = Config::config_path() {
            if let Ok(text) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(path, text);
            }
        }
    }
    // Live reload in the transcribe thread
    let _ = state.model_reload_tx.send(local_path.clone());
    state.status_msg = Some((
        format!("✓ 已切换到 {} 模型（后台加载中）", model.name),
        egui::Color32::from_rgb(80, 220, 80),
    ));
}

fn check_all_updates(state: &mut SettingsState) {
    let (tx, rx) = crossbeam_channel::unbounded();
    state.update_rx = Some(rx);
    state.remote_sizes.clear();
    state.checking_updates = true;

    for model in MODELS {
        let url = download::hf_url(&state.hf_repo, model.filename);
        download::check_remote_size(url, tx.clone(), model.filename.to_string());
    }
}

// ---------------------------------------------------------------------------
// Hotkey tab
// ---------------------------------------------------------------------------

fn render_hotkey_tab(ui: &mut egui::Ui, state: &mut SettingsState, _ctx: &egui::Context) {
    ui.add_space(8.0);

    // Current hotkey display
    ui.group(|ui| {
        ui.label(egui::RichText::new("当前快捷键").strong());
        ui.add_space(4.0);

        let display = if state.hotkey_mods.is_empty() {
            state.hotkey_key.clone()
        } else {
            format!("{} + {}", state.hotkey_mods.join(" + "), state.hotkey_key)
        };
        ui.label(
            egui::RichText::new(&display)
                .monospace()
                .size(18.0)
                .color(egui::Color32::from_rgb(120, 200, 255)),
        );
    });

    ui.add_space(12.0);

    // Capture button
    ui.horizontal(|ui| {
        ui.label("点击设置新按键：");
        let (btn_text, btn_color) = if state.capturing {
            (
                "⌨  请按下目标按键...".to_string(),
                egui::Color32::from_rgb(255, 180, 50),
            )
        } else {
            ("  捕捉按键  ".to_string(), ui.visuals().text_color())
        };
        let btn = ui.add(
            egui::Button::new(egui::RichText::new(&btn_text).color(btn_color))
                .min_size(egui::vec2(180.0, 30.0)),
        );
        if btn.clicked() {
            state.capturing = !state.capturing;
            state.capture_active.store(state.capturing, Ordering::SeqCst);
        }
    });

    if state.capturing {
        ui.label(
            egui::RichText::new("按下任意功能键 (F1-F12, Home, End 等) 或字母键，按 Esc 取消")
                .weak()
                .small(),
        );
    }

    ui.add_space(10.0);

    // Manual text edit (for keys not capturable via egui)
    ui.horizontal(|ui| {
        ui.label("或手动输入键名：");
        ui.add(egui::TextEdit::singleline(&mut state.hotkey_key).desired_width(120.0));
        ui.label(
            egui::RichText::new("如 Pause, ScrollLock, CapsLock")
                .weak()
                .small(),
        );
    });

    ui.add_space(10.0);

    // Trigger mode
    ui.label(egui::RichText::new("触发模式：").strong());
    ui.horizontal(|ui| {
        if ui
            .radio(state.hotkey_mode == "hold", "按住说话（松开识别）")
            .clicked()
        {
            state.hotkey_mode = "hold".to_string();
        }
        if ui
            .radio(state.hotkey_mode == "toggle", "点按切换（再按结束）")
            .clicked()
        {
            state.hotkey_mode = "toggle".to_string();
        }
    });

    ui.add_space(10.0);

    // Modifiers
    ui.label(egui::RichText::new("修饰键（可选）：").strong());
    ui.horizontal(|ui| {
        for (key, label) in &[("ctrl", "Ctrl"), ("alt", "Alt"), ("shift", "Shift"), ("super", "Super")] {
            let mut checked = state.hotkey_mods.contains(&key.to_string());
            if ui.checkbox(&mut checked, *label).clicked() {
                if checked {
                    if !state.hotkey_mods.contains(&key.to_string()) {
                        state.hotkey_mods.push(key.to_string());
                    }
                } else {
                    state.hotkey_mods.retain(|x| x.as_str() != *key);
                }
            }
        }
    });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Save / revert buttons
    let current = state.shared_hotkey.lock();
    let saved_key = current.key.clone();
    let saved_mods = current.modifiers.clone();
    let saved_mode = current.mode.clone();
    drop(current);

    let changed = state.hotkey_key != saved_key
        || state.hotkey_mods != saved_mods
        || state.hotkey_mode != saved_mode;

    ui.horizontal(|ui| {
        let save_btn = ui.add_enabled(changed, egui::Button::new("💾  保存快捷键"));
        if save_btn.clicked() {
            // Live update (hotkey thread reads this)
            {
                let mut hk = state.shared_hotkey.lock();
                hk.key = state.hotkey_key.clone();
                hk.modifiers = state.hotkey_mods.clone();
                hk.mode = state.hotkey_mode.clone();
            }
            // Persist to config.toml
            if let Ok(mut cfg) = Config::load() {
                cfg.hotkey.key = state.hotkey_key.clone();
                cfg.hotkey.modifiers = state.hotkey_mods.clone();
                cfg.hotkey.mode = state.hotkey_mode.clone();
                if let Ok(path) = Config::config_path() {
                    if let Ok(text) = toml::to_string_pretty(&cfg) {
                        let _ = std::fs::write(path, text);
                    }
                }
            }
            let mode_label = if state.hotkey_mode == "toggle" {
                "点按切换"
            } else {
                "按住说话"
            };
            state.status_msg = Some((
                format!("✓ 快捷键已更新为 {}（{}）", state.hotkey_key, mode_label),
                egui::Color32::from_rgb(80, 220, 80),
            ));
        }

        let revert_btn = ui.add_enabled(changed, egui::Button::new("↩  还原"));
        if revert_btn.clicked() {
            state.hotkey_key = saved_key;
            state.hotkey_mods = saved_mods;
            state.hotkey_mode = saved_mode;
        }
    });

    if let Some((msg, color)) = &state.status_msg {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(msg).color(*color));
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(4.0);
    let tip = if state.hotkey_mode == "toggle" {
        "提示：点按快捷键开始录音，再按一次结束并输入。停顿 1.5 秒自动识别。Esc 取消。"
    } else {
        "提示：按住快捷键录音，松开转写输入。停顿 1.5 秒自动识别。Esc 取消。"
    };
    ui.label(egui::RichText::new(tip).weak().small());
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// General tab
// ---------------------------------------------------------------------------

fn render_general_tab(ui: &mut egui::Ui, state: &mut SettingsState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        // Snapshot current shared configs
        let mut tx = state.shared_transcription.lock().clone();
        let mut inj = state.shared_inject.lock().clone();
        let mut aud = state.shared_audio.lock().clone();

        let mut changed = false;

        // ----- 语音识别 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("语音识别").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("语言：");
                let langs: &[(&str, &str)] = &[
                    ("auto", "自动检测"),
                    ("zh", "中文"),
                    ("en", "English"),
                    ("ja", "日本語"),
                    ("ko", "한국어"),
                    ("fr", "Français"),
                    ("de", "Deutsch"),
                    ("es", "Español"),
                    ("ru", "Русский"),
                ];
                let current_label = langs
                    .iter()
                    .find(|(c, _)| *c == tx.language)
                    .map(|(_, l)| *l)
                    .unwrap_or("自动检测");

                egui::ComboBox::from_id_salt("lang")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for (code, label) in langs {
                            if ui
                                .selectable_label(tx.language == *code, *label)
                                .clicked()
                            {
                                tx.language = code.to_string();
                                changed = true;
                            }
                        }
                    });
            });

            if ui.checkbox(&mut tx.translate, "翻译为英文输出").changed() {
                changed = true;
            }

            ui.horizontal(|ui| {
                ui.label("推理线程：");
                let mut n = tx.n_threads as i32;
                if ui
                    .add(egui::Slider::new(&mut n, 1..=16).integer())
                    .changed()
                {
                    tx.n_threads = n;
                    changed = true;
                }
            });
        });

        // ----- 文字注入 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("文字注入").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("方式：");
                let methods = [("clipboard", "剪贴板 (Ctrl+V)"), ("type", "键盘模拟")];
                let current_label = methods
                    .iter()
                    .find(|(c, _)| *c == inj.method)
                    .map(|(_, l)| *l)
                    .unwrap_or("剪贴板 (Ctrl+V)");

                egui::ComboBox::from_id_salt("inj_method")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for (code, label) in &methods {
                            if ui
                                .selectable_label(inj.method == *code, *label)
                                .clicked()
                            {
                                inj.method = code.to_string();
                                changed = true;
                            }
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("剪贴板延迟：");
                let mut delay = inj.clipboard_delay_ms as u64;
                if ui
                    .add(egui::Slider::new(&mut delay, 0..=500).suffix(" ms"))
                    .changed()
                {
                    inj.clipboard_delay_ms = delay;
                    changed = true;
                }
            });
            ui.label(
                egui::RichText::new("CJK 字符推荐剪贴板方式；慢设备上请调大延迟")
                    .weak()
                    .small(),
            );
        });

        // ----- 音频 / 停顿检测 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("音频与停顿检测").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("静音阈值：");
                let mut t = aud.silence_threshold;
                if ui
                    .add(
                        egui::Slider::new(&mut t, 0.001..=0.1)
                            .logarithmic(true)
                            .fixed_decimals(3),
                    )
                    .changed()
                {
                    aud.silence_threshold = t;
                    changed = true;
                }
                ui.label(egui::RichText::new("（越小越灵敏）").weak().small());
            });

            ui.horizontal(|ui| {
                ui.label("停顿长度：");
                let mut f = aud.silence_frames as i32;
                if ui
                    .add(egui::Slider::new(&mut f, 8..=80).integer())
                    .changed()
                {
                    aud.silence_frames = f as u32;
                    changed = true;
                }
                let approx_secs = (aud.silence_frames as f32) * 1024.0 / 16000.0;
                ui.label(
                    egui::RichText::new(format!("约 {:.1} 秒", approx_secs))
                        .weak()
                        .small(),
                );
            });

            ui.horizontal(|ui| {
                ui.label("最长录音：");
                let mut m = aud.max_record_seconds as i32;
                if ui
                    .add(egui::Slider::new(&mut m, 5..=180).integer().suffix(" 秒"))
                    .changed()
                {
                    aud.max_record_seconds = m as u32;
                    changed = true;
                }
            });
        });

        // ----- 麦克风 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("麦克风").strong());
            ui.add_space(4.0);

            ui.label(
                egui::RichText::new(format!("可用设备 ({})", state.audio_devices.len()))
                    .small(),
            );
            for name in &state.audio_devices {
                ui.label(
                    egui::RichText::new(format!("  • {}", name))
                        .small()
                        .weak(),
                );
            }
            ui.label(
                egui::RichText::new("目前使用系统默认设备，切换设备需在 config.toml 中指定")
                    .weak()
                    .small(),
            );
        });

        // ----- 系统 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("系统").strong());
            ui.add_space(4.0);

            let mut autostart_on = crate::autostart::is_enabled();
            let prev = autostart_on;
            ui.horizontal(|ui| {
                ui.checkbox(&mut autostart_on, "开机自启动");
                ui.label(
                    egui::RichText::new("（登录后自动启动 xsay）")
                        .weak()
                        .small(),
                );
            });
            if autostart_on != prev {
                let result = if autostart_on {
                    crate::autostart::enable()
                } else {
                    crate::autostart::disable()
                };
                match result {
                    Ok(()) => {
                        state.status_msg = Some((
                            if autostart_on {
                                "✓ 开机自启动已启用".to_string()
                            } else {
                                "✓ 开机自启动已关闭".to_string()
                            },
                            egui::Color32::from_rgb(80, 200, 80),
                        ));
                    }
                    Err(e) => {
                        state.status_msg = Some((
                            format!("✗ 自启动设置失败: {}", e),
                            egui::Color32::from_rgb(255, 120, 120),
                        ));
                    }
                }
            }
        });

        // ----- 浮层 -----
        ui.group(|ui| {
            ui.label(egui::RichText::new("浮层").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("位置：");
                let positions: &[(&str, &str)] = &[
                    ("top-right", "右上角"),
                    ("top-left", "左上角"),
                    ("bottom-right", "右下角"),
                    ("bottom-left", "左下角"),
                    ("center", "居中"),
                ];
                let current_code = state.shared_position.lock().clone();
                let current_label = positions
                    .iter()
                    .find(|(c, _)| *c == current_code.as_str())
                    .map(|(_, l)| *l)
                    .unwrap_or("右上角");
                egui::ComboBox::from_id_salt("overlay_pos")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for (code, label) in positions {
                            let is_sel = current_code == *code;
                            if ui.selectable_label(is_sel, *label).clicked() {
                                *state.shared_position.lock() = code.to_string();
                                if let Ok(mut cfg) = Config::load() {
                                    cfg.overlay.position = code.to_string();
                                    if let Ok(p) = Config::config_path() {
                                        if let Ok(t) = toml::to_string_pretty(&cfg) {
                                            let _ = std::fs::write(p, t);
                                        }
                                    }
                                }
                            }
                        }
                    });
            });
        });

        // Persist + broadcast live changes
        if changed {
            *state.shared_transcription.lock() = tx.clone();
            *state.shared_inject.lock() = inj.clone();
            *state.shared_audio.lock() = aud.clone();

            if let Ok(mut cfg) = Config::load() {
                cfg.transcription = tx;
                cfg.injection = inj;
                cfg.audio = aud;
                if let Ok(p) = Config::config_path() {
                    if let Ok(t) = toml::to_string_pretty(&cfg) {
                        let _ = std::fs::write(p, t);
                    }
                }
            }

            state.status_msg = Some((
                "✓ 已保存并生效".to_string(),
                egui::Color32::from_rgb(80, 200, 80),
            ));
        }

        if let Some((msg, color)) = &state.status_msg {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(msg).color(*color).small());
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn egui_key_to_rdev(key: egui::Key) -> Option<&'static str> {
    match key {
        egui::Key::F1 => Some("F1"),
        egui::Key::F2 => Some("F2"),
        egui::Key::F3 => Some("F3"),
        egui::Key::F4 => Some("F4"),
        egui::Key::F5 => Some("F5"),
        egui::Key::F6 => Some("F6"),
        egui::Key::F7 => Some("F7"),
        egui::Key::F8 => Some("F8"),
        egui::Key::F9 => Some("F9"),
        egui::Key::F10 => Some("F10"),
        egui::Key::F11 => Some("F11"),
        egui::Key::F12 => Some("F12"),
        egui::Key::Home => Some("Home"),
        egui::Key::End => Some("End"),
        egui::Key::PageUp => Some("PageUp"),
        egui::Key::PageDown => Some("PageDown"),
        egui::Key::Delete => Some("Delete"),
        egui::Key::Insert => Some("Insert"),
        egui::Key::Tab => Some("Tab"),
        egui::Key::A => Some("a"),
        egui::Key::B => Some("b"),
        egui::Key::C => Some("c"),
        egui::Key::D => Some("d"),
        egui::Key::E => Some("e"),
        egui::Key::F => Some("f"),
        egui::Key::G => Some("g"),
        egui::Key::H => Some("h"),
        egui::Key::I => Some("i"),
        egui::Key::J => Some("j"),
        egui::Key::K => Some("k"),
        egui::Key::L => Some("l"),
        egui::Key::M => Some("m"),
        egui::Key::N => Some("n"),
        egui::Key::O => Some("o"),
        egui::Key::P => Some("p"),
        egui::Key::Q => Some("q"),
        egui::Key::R => Some("r"),
        egui::Key::S => Some("s"),
        egui::Key::T => Some("t"),
        egui::Key::U => Some("u"),
        egui::Key::V => Some("v"),
        egui::Key::W => Some("w"),
        egui::Key::X => Some("x"),
        egui::Key::Y => Some("y"),
        egui::Key::Z => Some("z"),
        _ => None,
    }
}
