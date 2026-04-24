//! 常规标签页：语言、注入方式、VAD 参数、麦克风、系统（自启动）、浮层位置。

use super::SettingsState;
use crate::config::Config;
use crate::theme::{self, Icon};
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;

        let mut tx = state.shared_transcription.lock().clone();
        let mut inj = state.shared_inject.lock().clone();
        let mut aud = state.shared_audio.lock().clone();
        let mut changed = false;

        render_transcription_card(ui, &mut tx, &mut changed);
        render_injection_card(ui, &mut inj, &mut changed);
        render_audio_card(ui, &mut aud, &mut changed);
        render_microphone_card(ui, state);
        render_system_card(ui, state);
        render_overlay_card(ui, state);

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

            state.set_status("已保存并生效", crate::theme::SUCCESS);
        }

        if let Some((msg, color, _)) = &state.status_msg {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(msg)
                    .color(*color)
                    .size(crate::theme::FONT_SM),
            );
        }
    });
}

fn render_transcription_card(
    ui: &mut egui::Ui,
    tx: &mut crate::config::TranscriptionConfig,
    changed: &mut bool,
) {
    theme::section_card(ui, "语音识别", |ui| {
        theme::form_row(ui, "识别语言", |ui| {
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
                            *changed = true;
                        }
                    }
                });
        });
        theme::helper_text(
            ui,
            "自动检测就能处理中英混说（如 \"这个 API 怎么 deploy\"），一般保持默认",
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if theme::checkbox(ui, tx.translate, crate::theme::ACCENT).clicked() {
                tx.translate = !tx.translate;
                *changed = true;
            }
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("翻译为英文输出")
                        .color(crate::theme::TEXT_PRIMARY)
                        .size(crate::theme::FONT_BODY),
                );
                ui.label(
                    egui::RichText::new(
                        "勾选后不论说什么语言都强制输出英文。中文用户通常不勾",
                    )
                    .color(crate::theme::TEXT_SECONDARY)
                    .size(crate::theme::FONT_SM),
                );
            });
        });

        ui.add_space(6.0);
        theme::form_row(ui, "推理线程", |ui| {
            let mut n = tx.n_threads;
            if ui.add(egui::Slider::new(&mut n, 1..=16).integer()).changed() {
                tx.n_threads = n;
                *changed = true;
            }
        });
    });
}

fn render_injection_card(
    ui: &mut egui::Ui,
    inj: &mut crate::config::InjectionConfig,
    changed: &mut bool,
) {
    theme::section_card(ui, "文字注入", |ui| {
        theme::form_row(ui, "注入方式", |ui| {
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
                        if ui.selectable_label(inj.method == *code, *label).clicked() {
                            inj.method = code.to_string();
                            *changed = true;
                        }
                    }
                });
        });

        ui.add_space(6.0);
        theme::form_row(ui, "剪贴板延迟", |ui| {
            let mut delay = inj.clipboard_delay_ms;
            if ui
                .add(egui::Slider::new(&mut delay, 0..=500).suffix(" ms"))
                .changed()
            {
                inj.clipboard_delay_ms = delay;
                *changed = true;
            }
        });
        theme::helper_text(ui, "中文等 CJK 字符请选剪贴板方式；慢设备可调大延迟。");

        // Paste shortcut selector — Wayland auto-paste via uinput has to
        // know which key combo the target app expects. Terminals and CLI
        // tools (Claude Code, Codex) use Ctrl+Shift+V; most GUI apps use
        // Ctrl+V; "both" sends both with a short delay for mixed usage.
        ui.add_space(6.0);
        theme::form_row(ui, "粘贴快捷键", |ui| {
            ui.vertical(|ui| {
                let options: &[(&str, &str, &str)] = &[
                    ("ctrl-v", "Ctrl + V", "GUI 文本框（浏览器、编辑器）"),
                    ("ctrl-shift-v", "Ctrl + Shift + V", "终端 / Claude Code / Codex CLI"),
                    ("both", "两者都试", "兼容最广，但部分应用会弹出\"粘贴特殊\"对话框"),
                ];
                for (code, title, subtitle) in options {
                    let selected = inj.paste_shortcut == *code;
                    ui.horizontal(|ui| {
                        if theme::radio_button(ui, selected, crate::theme::ACCENT).clicked()
                            && !selected
                        {
                            inj.paste_shortcut = code.to_string();
                            *changed = true;
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(*title)
                                    .color(crate::theme::TEXT_PRIMARY)
                                    .size(crate::theme::FONT_BODY),
                            );
                            ui.label(
                                egui::RichText::new(*subtitle)
                                    .color(crate::theme::TEXT_SECONDARY)
                                    .size(crate::theme::FONT_SM),
                            );
                        });
                    });
                    ui.add_space(2.0);
                }
            });
        });
    });
}

fn render_audio_card(
    ui: &mut egui::Ui,
    aud: &mut crate::config::AudioConfig,
    changed: &mut bool,
) {
    theme::section_card(ui, "音频与停顿检测", |ui| {
        theme::form_row(ui, "静音阈值", |ui| {
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
                *changed = true;
            }
        });
        theme::helper_text(ui, "越小越灵敏，环境嘈杂时调大。");

        ui.add_space(6.0);
        theme::form_row(ui, "停顿长度", |ui| {
            let mut f = aud.silence_frames as i32;
            if ui.add(egui::Slider::new(&mut f, 8..=80).integer()).changed() {
                aud.silence_frames = f as u32;
                *changed = true;
            }
            let approx_secs = (aud.silence_frames as f32) * 1024.0 / 16000.0;
            ui.label(
                egui::RichText::new(format!("约 {:.1} 秒", approx_secs))
                    .color(crate::theme::TEXT_SECONDARY)
                    .size(crate::theme::FONT_SM),
            );
        });

        ui.add_space(6.0);
        theme::form_row(ui, "最长录音", |ui| {
            let mut m = aud.max_record_seconds as i32;
            if ui
                .add(egui::Slider::new(&mut m, 5..=180).integer().suffix(" 秒"))
                .changed()
            {
                aud.max_record_seconds = m as u32;
                *changed = true;
            }
        });
    });
}

fn render_microphone_card(ui: &mut egui::Ui, state: &SettingsState) {
    theme::section_card(ui, "麦克风", |ui| {
        ui.label(
            egui::RichText::new(format!("可用设备：{} 个", state.audio_devices.len()))
                .color(crate::theme::TEXT_SECONDARY)
                .size(crate::theme::FONT_SM),
        );
        ui.add_space(4.0);
        for name in &state.audio_devices {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("•")
                        .color(crate::theme::TEXT_SECONDARY)
                        .size(crate::theme::FONT_BODY),
                );
                ui.label(
                    egui::RichText::new(name)
                        .color(crate::theme::TEXT_PRIMARY)
                        .size(crate::theme::FONT_BODY),
                );
            });
        }
        theme::helper_text(ui, "目前使用系统默认设备，切换设备需在 config.toml 中指定。");
    });
}

fn render_system_card(ui: &mut egui::Ui, state: &mut SettingsState) {
    theme::section_card(ui, "系统", |ui| {
        let mut autostart_on = crate::autostart::is_enabled();
        let prev = autostart_on;

        ui.horizontal(|ui| {
            if theme::checkbox(ui, autostart_on, crate::theme::ACCENT).clicked() {
                autostart_on = !autostart_on;
            }
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("开机自启动")
                        .color(crate::theme::TEXT_PRIMARY)
                        .size(crate::theme::FONT_BODY),
                );
                ui.label(
                    egui::RichText::new("登录后自动启动 xsay")
                        .color(crate::theme::TEXT_SECONDARY)
                        .size(crate::theme::FONT_SM),
                );
            });
        });

        if autostart_on != prev {
            let result = if autostart_on {
                crate::autostart::enable()
            } else {
                crate::autostart::disable()
            };
            match result {
                Ok(()) => {
                    state.set_status(
                        if autostart_on {
                            "开机自启动已启用"
                        } else {
                            "开机自启动已关闭"
                        },
                        crate::theme::SUCCESS,
                    );
                }
                Err(e) => {
                    state.set_status(
                        format!("自启动设置失败：{}", e),
                        crate::theme::DANGER_HOVER,
                    );
                }
            }
        }
    });
}

fn render_overlay_card(ui: &mut egui::Ui, state: &SettingsState) {
    theme::section_card(ui, "浮层位置", |ui| {
        let positions: &[(&str, &str, Icon)] = &[
            ("top-left", "左上角", Icon::Box),
            ("top-center", "顶部居中", Icon::Box),
            ("top-right", "右上角", Icon::Box),
            ("bottom-left", "左下角", Icon::Box),
            ("bottom-center", "底部居中", Icon::Box),
            ("bottom-right", "右下角", Icon::Box),
            ("center", "屏幕正中", Icon::Box),
        ];
        let current_code = state.shared_position.lock().clone();

        // Fixed 3-column Grid so the 7 options always fit inside the
        // settings window (previously a horizontal_wrapped flow pushed
        // the 7th item "屏幕正中" off the right edge because egui doesn't
        // break inside a nested ui::horizontal atom).
        egui::Grid::new("overlay_positions_grid")
            .num_columns(3)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                for (i, (code, label, _)) in positions.iter().enumerate() {
                    let selected = current_code == *code;
                    ui.horizontal(|ui| {
                        if theme::radio_button(ui, selected, crate::theme::ACCENT).clicked() {
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
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(*label)
                                .color(crate::theme::TEXT_PRIMARY)
                                .size(crate::theme::FONT_BODY),
                        );
                    });
                    if (i + 1) % 3 == 0 {
                        ui.end_row();
                    }
                }
            });
    });
}
