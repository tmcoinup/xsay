//! 常规标签页：语言、注入方式、VAD 参数、麦克风、系统（自启动）、浮层位置。

use super::SettingsState;
use crate::config::Config;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        let mut tx = state.shared_transcription.lock().clone();
        let mut inj = state.shared_inject.lock().clone();
        let mut aud = state.shared_audio.lock().clone();
        let mut changed = false;

        render_transcription_group(ui, &mut tx, &mut changed);
        render_injection_group(ui, &mut inj, &mut changed);
        render_audio_group(ui, &mut aud, &mut changed);
        render_microphone_group(ui, state);
        render_system_group(ui, state);
        render_overlay_group(ui, state);

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
                crate::theme::SUCCESS,
            ));
        }

        if let Some((msg, color)) = &state.status_msg {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(msg).color(*color).small());
        }
    });
}

fn render_transcription_group(
    ui: &mut egui::Ui,
    tx: &mut crate::config::TranscriptionConfig,
    changed: &mut bool,
) {
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
                            *changed = true;
                        }
                    }
                });
        });

        if ui.checkbox(&mut tx.translate, "翻译为英文输出").changed() {
            *changed = true;
        }

        ui.horizontal(|ui| {
            ui.label("推理线程：");
            let mut n = tx.n_threads;
            if ui.add(egui::Slider::new(&mut n, 1..=16).integer()).changed() {
                tx.n_threads = n;
                *changed = true;
            }
        });
    });
}

fn render_injection_group(
    ui: &mut egui::Ui,
    inj: &mut crate::config::InjectionConfig,
    changed: &mut bool,
) {
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
                        if ui.selectable_label(inj.method == *code, *label).clicked() {
                            inj.method = code.to_string();
                            *changed = true;
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("剪贴板延迟：");
            let mut delay = inj.clipboard_delay_ms;
            if ui
                .add(egui::Slider::new(&mut delay, 0..=500).suffix(" ms"))
                .changed()
            {
                inj.clipboard_delay_ms = delay;
                *changed = true;
            }
        });
        ui.label(
            egui::RichText::new("CJK 字符推荐剪贴板方式；慢设备上请调大延迟")
                .weak()
                .small(),
        );
    });
}

fn render_audio_group(
    ui: &mut egui::Ui,
    aud: &mut crate::config::AudioConfig,
    changed: &mut bool,
) {
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
                *changed = true;
            }
            ui.label(egui::RichText::new("（越小越灵敏）").weak().small());
        });

        ui.horizontal(|ui| {
            ui.label("停顿长度：");
            let mut f = aud.silence_frames as i32;
            if ui.add(egui::Slider::new(&mut f, 8..=80).integer()).changed() {
                aud.silence_frames = f as u32;
                *changed = true;
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
                *changed = true;
            }
        });
    });
}

fn render_microphone_group(ui: &mut egui::Ui, state: &mut SettingsState) {
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
}

fn render_system_group(ui: &mut egui::Ui, state: &mut SettingsState) {
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
                        crate::theme::SUCCESS,
                    ));
                }
                Err(e) => {
                    state.status_msg = Some((
                        format!("✗ 自启动设置失败: {}", e),
                        crate::theme::DANGER_HOVER,
                    ));
                }
            }
        }
    });
}

fn render_overlay_group(ui: &mut egui::Ui, state: &mut SettingsState) {
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
}
