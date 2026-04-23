//! 模型标签页：列出可选 Whisper 模型，支持下载/暂停/切换/删除。

use super::{models::MODELS, ActiveDownload, ModelInfo, SettingsState};
use crate::config::Config;
use crate::download::{self, DlState, DownloadCmd, DownloadProgress};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    let current_filename = Config::load()
        .ok()
        .map(|c| c.model.hf_filename)
        .unwrap_or_default();

    let active_dl_filename = state.active_download.as_ref().map(|d| d.filename.clone());

    let dl_state_snap: Option<DlState> = state
        .active_download
        .as_ref()
        .map(|d| d.progress.state.lock().clone());
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

    handle_download_completion(&dl_state_snap, &active_dl_filename, state);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        let cur_exists =
            !current_filename.is_empty() && state.cache_dir.join(&current_filename).exists();
        if !cur_exists {
            render_no_model_banner(ui);
            ui.add_space(6.0);
        }

        for model in MODELS {
            render_model_row(
                ui,
                model,
                state,
                &current_filename,
                active_dl_filename.as_deref(),
                &dl_state_snap,
                dl_downloaded,
                dl_total,
            );
            ui.add_space(2.0);
        }

        ui.separator();
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let checking = state.checking_updates;
            let label = if checking { "🔄 检查中..." } else { "🔄 检查所有模型更新" };
            if ui.add_enabled(!checking, egui::Button::new(label)).clicked() {
                check_all_updates(state);
            }
        });

        if let Some((msg, color)) = &state.status_msg {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(msg).color(*color));
        }
    });
}

fn render_no_model_banner(ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(90, 45, 20))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .rounding(crate::theme::radius_md())
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("⚠  当前没有可用模型，xsay 无法识别语音")
                    .color(egui::Color32::WHITE)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("推荐下载 Medium (1.5 GB，中英文高精度)")
                    .color(crate::theme::WARNING)
                    .small(),
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn render_model_row(
    ui: &mut egui::Ui,
    model: &ModelInfo,
    state: &mut SettingsState,
    current_filename: &str,
    active_dl_filename: Option<&str>,
    dl_state_snap: &Option<DlState>,
    dl_downloaded: u64,
    dl_total: u64,
) {
    let local_path = state.cache_dir.join(model.filename);
    let partial_path = download::partial_path(&local_path);
    let is_current = model.filename == current_filename;
    let is_this_dl = active_dl_filename == Some(model.filename);

    let is_downloaded = local_path.exists();
    let has_partial = partial_path.exists() && !is_downloaded;
    let local_size = local_path.metadata().map(|m| m.len()).unwrap_or(0);
    let partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);

    let remote = state.remote_sizes.get(model.filename).copied().flatten();

    let frame_color = if is_current {
        crate::theme::BG_SELECTED
    } else {
        crate::theme::BG_CARD
    };

    egui::Frame::none()
        .fill(frame_color)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .rounding(crate::theme::radius_lg())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let radio = ui.radio(is_current, "");
                if radio.clicked() && is_downloaded && !is_current {
                    switch_model(model, &local_path, state);
                }

                ui.vertical(|ui| {
                    render_header_row(ui, model, is_current, is_downloaded, remote, local_size);
                    render_progress_row(
                        ui,
                        model,
                        is_this_dl,
                        has_partial,
                        is_downloaded,
                        dl_downloaded,
                        dl_total,
                        partial_size,
                        local_size,
                    );
                    render_action_row(
                        ui,
                        model,
                        state,
                        is_this_dl,
                        is_downloaded,
                        is_current,
                        has_partial,
                        &local_path,
                        &partial_path,
                        dl_state_snap,
                    );
                });
            });
        });
}

fn render_header_row(
    ui: &mut egui::Ui,
    model: &ModelInfo,
    is_current: bool,
    is_downloaded: bool,
    remote: Option<u64>,
    local_size: u64,
) {
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
                    .color(crate::theme::CURRENT)
                    .small(),
            );
        }

        if let Some(remote_size) = remote {
            if is_downloaded {
                if remote_size != local_size {
                    ui.label(
                        egui::RichText::new("↑ 有更新")
                            .color(crate::theme::WARNING)
                            .small(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("✓ 最新")
                            .color(crate::theme::TEXT_SECONDARY)
                            .small(),
                    );
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_progress_row(
    ui: &mut egui::Ui,
    model: &ModelInfo,
    is_this_dl: bool,
    has_partial: bool,
    is_downloaded: bool,
    dl_downloaded: u64,
    dl_total: u64,
    partial_size: u64,
    local_size: u64,
) {
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
            egui::RichText::new(format!("{:.1} MB", local_size as f32 / 1e6))
                .weak()
                .small(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_action_row(
    ui: &mut egui::Ui,
    model: &ModelInfo,
    state: &mut SettingsState,
    is_this_dl: bool,
    is_downloaded: bool,
    is_current: bool,
    has_partial: bool,
    local_path: &PathBuf,
    partial_path: &PathBuf,
    dl_state_snap: &Option<DlState>,
) {
    ui.horizontal(|ui| {
        if is_this_dl {
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

            if let Some(DlState::Failed(e)) = dl_state_snap {
                ui.label(
                    egui::RichText::new(format!("错误: {}", e))
                        .color(crate::theme::DANGER)
                        .small(),
                );
                if ui.small_button("重试").clicked() {
                    state.active_download = None;
                    start_model_download(model, state);
                }
            }
        } else {
            if !is_downloaded {
                let btn_label = if has_partial { "▶ 继续下载" } else { "⬇ 下载" };
                let enabled = state.active_download.is_none();
                if ui
                    .add_enabled(enabled, egui::Button::new(btn_label).small())
                    .clicked()
                {
                    start_model_download(model, state);
                }
                if has_partial && ui.small_button("✕ 删除进度").clicked() {
                    let _ = std::fs::remove_file(partial_path);
                }
            }

            if is_downloaded && !is_current {
                if ui.small_button("✓ 切换使用").clicked() {
                    switch_model(model, local_path, state);
                }
                if ui.small_button("🗑 删除").clicked() {
                    let _ = std::fs::remove_file(local_path);
                }
            }
        }
    });
}

fn handle_download_completion(
    dl_state_snap: &Option<DlState>,
    active_dl_filename: &Option<String>,
    state: &mut SettingsState,
) {
    if let Some(DlState::Completed) = dl_state_snap {
        if let Some(fname) = active_dl_filename {
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
                if let Ok(mut c) = Config::load() {
                    c.model.hf_filename = fname.clone();
                    persist_config(&c);
                }
                let _ = state.model_reload_tx.send(downloaded_path);
                state.status_msg = Some((
                    format!("✓ {} 下载完成并已启用", nice_name),
                    crate::theme::SUCCESS,
                ));
            } else {
                state.status_msg = Some((
                    format!("✓ {} 下载完成", nice_name),
                    crate::theme::SUCCESS,
                ));
            }
        }
        state.active_download = None;
    }
    if let Some(DlState::Cancelled) = dl_state_snap {
        state.active_download = None;
    }
}

fn persist_config(cfg: &Config) {
    if let Ok(path) = Config::config_path() {
        if let Ok(text) = toml::to_string_pretty(cfg) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn start_model_download(model: &ModelInfo, state: &mut SettingsState) {
    let url = download::hf_url(&state.hf_repo, model.filename);
    let dest = state.cache_dir.join(model.filename);
    let _ = std::fs::create_dir_all(&state.cache_dir);

    // Reuse existing progress struct when resuming a paused download on the
    // same file — avoids wiping the downloaded-byte counter.
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
    if let Ok(mut cfg) = Config::load() {
        cfg.model.hf_filename = model.filename.to_string();
        persist_config(&cfg);
    }
    let _ = state.model_reload_tx.send(local_path.clone());
    state.status_msg = Some((
        format!("✓ 已切换到 {} 模型（后台加载中）", model.name),
        crate::theme::SUCCESS,
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
