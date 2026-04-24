//! 模型标签页：列出可选 Whisper 模型，支持下载/暂停/切换/删除。

use super::{models::MODELS, ActiveDownload, ModelInfo, SettingsState};
use crate::config::Config;
use crate::download::{self, DlState, DownloadCmd, DownloadProgress};
use crate::theme::{self, Icon};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    // Only re-read config.toml when something upstream marked the cache
    // dirty (first render, tab re-entry, download completion, switch). The
    // previous unconditional Config::load() every frame was a contributor
    // to the "window not responding" UI-thread stalls under CPU load.
    if state.current_model_dirty {
        state.current_model_cache = Config::load()
            .ok()
            .map(|c| c.model.hf_filename)
            .unwrap_or_default();
        state.current_model_dirty = false;
    }
    let current_filename = state.current_model_cache.clone();

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
    handle_sherpa_install_completion(state);

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

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let checking = state.checking_updates;
            let label = if checking { "检查中…" } else { "检查所有模型更新" };
            let color = if checking {
                crate::theme::TEXT_SECONDARY
            } else {
                crate::theme::TEXT_PRIMARY
            };
            if theme::outlined_button(ui, Icon::Refresh, label, color, checking).clicked()
                && !checking
            {
                check_all_updates(state);
            }
        });

        if let Some((msg, color, _)) = &state.status_msg {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(msg).color(*color));
        }
    });
}

fn render_no_model_banner(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(90, 45, 20))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .corner_radius(crate::theme::radius_md())
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(18.0, 18.0),
                    egui::Sense::hover(),
                );
                theme::draw_icon(ui.painter(), rect, Icon::Warning, crate::theme::WARNING);
                ui.label(
                    egui::RichText::new("当前没有可用模型，xsay 无法识别语音")
                        .color(egui::Color32::WHITE)
                        .strong(),
                );
            });
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

    // Sherpa models unpack into a subdirectory with model.int8.onnx +
    // tokens.txt; Whisper models are a single .bin file. Check for the
    // right artifact per backend so the UI shows accurate download state.
    let is_downloaded = if model.backend == "whisper" {
        local_path.is_file()
    } else {
        local_path.join("model.int8.onnx").is_file()
            && local_path.join("tokens.txt").is_file()
    };
    let has_partial = partial_path.exists() && !is_downloaded;
    let local_size = if model.backend == "whisper" {
        local_path.metadata().map(|m| m.len()).unwrap_or(0)
    } else {
        local_path
            .join("model.int8.onnx")
            .metadata()
            .map(|m| m.len())
            .unwrap_or(0)
    };
    let partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);

    let remote = state.remote_sizes.get(model.filename).copied().flatten();

    let frame_color = if is_current {
        crate::theme::BG_SELECTED
    } else {
        crate::theme::BG_CARD
    };

    // Current card gets a 1px accent-green border to match the selected state
    // in the Figma reference.
    let mut frame = egui::Frame::new()
        .fill(frame_color)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .corner_radius(crate::theme::radius_lg());
    if is_current {
        frame = frame.stroke(egui::Stroke::new(1.0, crate::theme::CURRENT));
    }

    frame.show(ui, |ui| {
        // Force the row to span the full tab width — egui::Frame sizes to
        // content by default, which made rows shrink-wrap.
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            let radio = theme::radio_button(ui, is_current, crate::theme::ACCENT);
            if radio.clicked() && is_downloaded && !is_current {
                switch_model(model, &local_path, state);
            }
            ui.add_space(4.0);

            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
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
        ui.label(
            egui::RichText::new(model.name)
                .color(crate::theme::TEXT_PRIMARY)
                .strong(),
        );
        ui.label(
            egui::RichText::new(format!("{} MB  ·  {}", model.size_mb, model.desc))
                .color(crate::theme::TEXT_SECONDARY)
                .small(),
        );

        if is_current {
            // "当前使用" chip with a leading check icon, drawn in-frame so
            // the icon matches the row's accent tone.
            let frame = egui::Frame::new()
                .fill(crate::theme::CURRENT)
                .corner_radius(crate::theme::radius_sm())
                .inner_margin(egui::Margin::symmetric(6, 2));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(12.0, 12.0),
                        egui::Sense::hover(),
                    );
                    theme::draw_icon(ui.painter(), rect, Icon::Check, egui::Color32::WHITE);
                    ui.label(
                        egui::RichText::new("当前使用")
                            .color(egui::Color32::WHITE)
                            .size(crate::theme::FONT_SM),
                    );
                });
            });
        }

        if let Some(remote_size) = remote {
            if is_downloaded {
                if remote_size != local_size {
                    let frame = egui::Frame::new()
                        .fill(crate::theme::WARNING)
                        .corner_radius(crate::theme::radius_sm())
                        .inner_margin(egui::Margin::symmetric(6, 2));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(12.0, 12.0),
                                egui::Sense::hover(),
                            );
                            theme::draw_icon(ui.painter(), rect, Icon::Up, egui::Color32::BLACK);
                            ui.label(
                                egui::RichText::new("有更新")
                                    .color(egui::Color32::BLACK)
                                    .size(crate::theme::FONT_SM),
                            );
                        });
                    });
                } else {
                    crate::theme::chip(
                        ui,
                        "最新",
                        crate::theme::TEXT_SECONDARY,
                        crate::theme::BG_CARD_HOVER,
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
        ui.spacing_mut().item_spacing.x = 16.0;
        if is_this_dl {
            let paused = matches!(dl_state_snap, Some(DlState::Paused));
            if paused {
                if theme::icon_link_button(ui, Icon::Play, "继续", crate::theme::ACCENT).clicked() {
                    start_model_download(model, state);
                }
            } else if theme::icon_link_button(ui, Icon::Pause, "暂停", crate::theme::TEXT_PRIMARY)
                .clicked()
            {
                if let Some(dl) = &state.active_download {
                    let _ = dl.cmd_tx.send(DownloadCmd::Pause);
                }
            }
            if theme::icon_link_button(ui, Icon::X, "取消", crate::theme::DANGER_HOVER).clicked() {
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
                if theme::link_button(ui, "重试", crate::theme::ACCENT).clicked() {
                    state.active_download = None;
                    start_model_download(model, state);
                }
            }
        } else {
            if !is_downloaded {
                // Sherpa ONNX models ship as tar.bz2 archives (model + tokens
                // in a subdirectory) instead of a single .bin file, so the
                // per-file download infrastructure doesn't fit. Show an
                // "安装" action that shells out to curl + tar via a
                // background thread, and let the UI poll install state.
                if model.backend != "whisper" {
                    let installing = state
                        .sherpa_installing
                        .as_ref()
                        .map(|s| s.as_str() == model.filename)
                        .unwrap_or(false);
                    let label = if installing { "安装中..." } else { "安装" };
                    let color = if installing {
                        crate::theme::TEXT_SECONDARY
                    } else {
                        crate::theme::ACCENT
                    };
                    if theme::icon_link_button(ui, Icon::Download, label, color)
                        .clicked()
                        && !installing
                    {
                        start_sherpa_install(model, state);
                    }
                } else {
                    let (icon, label) = if has_partial {
                        (Icon::Play, "继续下载")
                    } else {
                        (Icon::Download, "下载")
                    };
                    let enabled = state.active_download.is_none();
                    let color = if enabled {
                        crate::theme::ACCENT
                    } else {
                        crate::theme::TEXT_DISABLED
                    };
                    let resp = theme::icon_link_button(ui, icon, label, color);
                    if enabled && resp.clicked() {
                        start_model_download(model, state);
                    }
                    if has_partial
                        && theme::icon_link_button(
                            ui,
                            Icon::X,
                            "删除进度",
                            crate::theme::DANGER_HOVER,
                        )
                        .clicked()
                    {
                        let _ = std::fs::remove_file(partial_path);
                    }
                }
            }

            if is_downloaded && !is_current {
                if theme::icon_link_button(ui, Icon::Check, "切换使用", crate::theme::ACCENT)
                    .clicked()
                {
                    switch_model(model, local_path, state);
                }
                if theme::icon_link_button(ui, Icon::Trash, "删除", crate::theme::DANGER_HOVER)
                    .clicked()
                {
                    let _ = std::fs::remove_file(local_path);
                }
            }

            // "删除" on the currently-selected card (user still needs a way
            // to remove it if disk space matters). Same red link style.
            if is_downloaded && is_current
                && theme::icon_link_button(ui, Icon::Trash, "删除", crate::theme::DANGER_HOVER)
                    .clicked()
            {
                let _ = std::fs::remove_file(local_path);
            }
        }
    });
}

fn handle_download_completion(
    dl_state_snap: &Option<DlState>,
    active_dl_filename: &Option<String>,
    state: &mut SettingsState,
) {
    // Sherpa install path: when the tar.bz2 download finishes, transition
    // into the extract phase rather than running the Whisper-file
    // completion logic (which assumes the downloaded file IS the model).
    if let Some(DlState::Completed) = dl_state_snap {
        if let Some(fname) = active_dl_filename {
            if state
                .pending_sherpa_extract
                .as_ref()
                .map(|p| p.slug.as_str() == fname.as_str())
                .unwrap_or(false)
            {
                let plan = state.pending_sherpa_extract.take().unwrap();
                state.active_download = None;
                start_sherpa_extract(plan, state);
                return;
            }
        }
    }
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
                state.current_model_dirty = true;
                state.set_status(
                    format!("✓ {} 下载完成并已启用", nice_name),
                    crate::theme::SUCCESS,
                );
            } else {
                state.set_status(
                    format!("✓ {} 下载完成", nice_name),
                    crate::theme::SUCCESS,
                );
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

/// Download + extract a sherpa-onnx ASR archive (tar.bz2) into
/// ~/.cache/xsay/models/<filename>/. Runs off the UI thread; result is
/// posted back through a crossbeam channel the render loop polls.
///
/// Shells out to system `curl` and `tar` — both ubiquitous on Linux and
/// already required for xsay to compile. Keeps the binary small (no
/// Rust tar/bz2 decoder pulled into every build).
/// Non-blocking check: has the background sherpa-install thread finished?
/// Clears the `sherpa_installing` / `sherpa_install_rx` state and posts
/// success/failure to the status toast.
fn handle_sherpa_install_completion(state: &mut SettingsState) {
    let Some(rx) = &state.sherpa_install_rx else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(filename)) => {
            let nice = MODELS
                .iter()
                .find(|m| m.filename == filename.as_str())
                .map(|m| m.name)
                .unwrap_or(filename.as_str())
                .to_string();
            state.set_status(
                format!("✓ {} 已安装（去点它的切换使用）", nice),
                crate::theme::SUCCESS,
            );
            state.sherpa_installing = None;
            state.sherpa_install_rx = None;
        }
        Ok(Err(e)) => {
            state.set_status(
                format!("安装失败：{}", e),
                crate::theme::DANGER_HOVER,
            );
            state.sherpa_installing = None;
            state.sherpa_install_rx = None;
        }
        Err(_) => {} // still running
    }
}

/// Kick off a sherpa ONNX install. Reuses xsay's streaming downloader
/// (same one as Whisper .bin files) so the user gets the proper
/// progress bar / pause-resume UI while the .tar.bz2 streams in.
/// On download completion `handle_download_completion` spots the
/// `pending_sherpa_extract` entry and fires the untar phase in the
/// background via `start_sherpa_extract`.
fn start_sherpa_install(model: &ModelInfo, state: &mut SettingsState) {
    let archive_path = state.cache_dir.join(format!("{}.tar.bz2", model.filename));
    let extract_to = state.cache_dir.join(model.filename);
    let _ = std::fs::create_dir_all(&state.cache_dir);

    // Reuse an existing progress struct when resuming the same archive —
    // same pattern start_model_download uses for Whisper .bin resumes.
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

    let cmd_tx = download::start_download(
        model.archive_url.to_string(),
        archive_path.clone(),
        Arc::clone(&progress),
    );

    state.active_download = Some(ActiveDownload {
        filename: model.filename.to_string(),
        progress,
        cmd_tx,
    });
    state.pending_sherpa_extract = Some(super::PendingSherpaExtract {
        slug: model.filename.to_string(),
        display_name: model.name.to_string(),
        archive_path,
        extract_to,
    });
    state.sherpa_installing = Some(model.filename.to_string());
    state.set_status(
        format!("{} 正在下载…", model.name),
        crate::theme::ACCENT,
    );
}

/// Called when the sherpa tar.bz2 download finishes. Runs the untar
/// + file-placement phase off the UI thread and posts the result via
/// the same `sherpa_install_rx` channel handle_sherpa_install_completion
/// already polls.
fn start_sherpa_extract(plan: super::PendingSherpaExtract, state: &mut SettingsState) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.sherpa_install_rx = Some(rx);
    state.set_status(
        format!("{} 下载完成，解压中…", plan.display_name),
        crate::theme::ACCENT,
    );
    std::thread::spawn(move || {
        let result = run_sherpa_extract(&plan.archive_path, &plan.extract_to);
        let _ = tx.send(result.map(|_| plan.slug));
    });
}

fn run_sherpa_extract(
    archive: &std::path::Path,
    dest_dir: &std::path::Path,
) -> Result<(), String> {
    let extract_tmp =
        std::env::temp_dir().join(format!("xsay-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&extract_tmp);
    std::fs::create_dir_all(&extract_tmp).map_err(|e| format!("mkdir: {}", e))?;
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("mkdir: {}", e))?;

    log::info!(
        "sherpa extract: tar -xjf {} → {}",
        archive.display(),
        extract_tmp.display()
    );
    let status = std::process::Command::new("tar")
        .arg("-xjf")
        .arg(archive)
        .arg("-C")
        .arg(&extract_tmp)
        .status()
        .map_err(|e| format!("tar spawn: {}", e))?;
    if !status.success() {
        return Err(format!("tar exited {}", status));
    }

    // Archive unpacks into a versioned subdir — find it and copy the
    // two files we need into our flat cache path.
    let inner_dir = std::fs::read_dir(&extract_tmp)
        .map_err(|e| format!("readdir: {}", e))?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .ok_or_else(|| "archive had no inner directory".to_string())?;

    for fname in ["model.int8.onnx", "tokens.txt"] {
        let src = inner_dir.join(fname);
        let dst = dest_dir.join(fname);
        if !src.exists() {
            return Err(format!("archive missing {}", fname));
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("copy {}: {}", fname, e))?;
    }

    // Cleanup (best-effort). Errors here don't fail the install.
    let _ = std::fs::remove_file(archive);
    let _ = std::fs::remove_dir_all(&extract_tmp);
    log::info!("sherpa extract: done → {}", dest_dir.display());
    Ok(())
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
        // Whisper path: point hf_filename at the selected .bin so the
        // transcribe thread picks it up on reload. Sherpa models don't
        // use hf_filename (they live in a subdirectory with fixed names)
        // but we still store it so the UI's "current model" resolver
        // knows which row is active.
        cfg.model.hf_filename = model.filename.to_string();
        // Switching across backends — flip the transcription backend so
        // the next utterance goes through the right recognizer.
        cfg.transcription.backend = model.backend.to_string();
        persist_config(&cfg);
        *state.shared_transcription.lock() = cfg.transcription.clone();
    }
    // Only send a reload signal for Whisper models — the sherpa path
    // initializes on first use and doesn't watch the model_reload channel.
    if model.backend == "whisper" {
        let _ = state.model_reload_tx.send(local_path.clone());
    }
    state.current_model_dirty = true;
    state.set_status(
        format!("✓ 已切换到 {}（后端 = {}）", model.name, model.backend),
        crate::theme::SUCCESS,
    );
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
