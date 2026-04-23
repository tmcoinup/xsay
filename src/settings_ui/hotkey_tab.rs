//! 快捷键标签页：捕获新按键、编辑修饰键、选择触发模式（按住/切换）。

use super::SettingsState;
use crate::config::Config;
use eframe::egui;
use std::sync::atomic::Ordering;

/// Top-level key capture handler, invoked each frame by the settings window.
///
/// Two capture paths run in parallel while `state.capturing` is true:
///
/// 1. **egui events** — works when the settings window has keyboard focus.
///    Fast path (no extra lock contention), handles modifiers via the egui
///    event's `modifiers` field directly.
///
/// 2. **hotkey thread via CaptureSlot** — works regardless of focus, using
///    the OS-level rdev/evdev listener. Critical for keys the compositor
///    eats before forwarding to our window (F-keys on some laptops, etc.)
///    and for users on Wayland where the settings window's own keyboard
///    handling may not deliver global-shortcut-style captures.
///
/// Whichever path fires first wins.
pub fn handle_key_capture(ctx: &egui::Context, state: &mut SettingsState) {
    if !state.capturing {
        return;
    }

    // Path 1: egui window-focused events
    let mut captured_via_egui = false;
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
                    end_capture(state);
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
                    captured_via_egui = true;
                }
            }
        }
    });
    if captured_via_egui {
        end_capture(state);
        return;
    }

    // Path 2: OS-level rdev/evdev capture slot
    let captured = state.capture_slot.latest.lock().take();
    if let Some((name, mods)) = captured {
        if name == "__cancel__" {
            end_capture(state);
            return;
        }
        state.hotkey_key = name;
        state.hotkey_mods = mods;
        end_capture(state);
    }
}

fn end_capture(state: &mut SettingsState) {
    state.capturing = false;
    state.capture_active.store(false, Ordering::SeqCst);
    state.capture_slot.active.store(false, Ordering::SeqCst);
    *state.capture_slot.latest.lock() = None;
}

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.add_space(4.0);

    render_backend_warning(ui, state);

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
                .size(crate::theme::FONT_HERO)
                .color(crate::theme::ACCENT),
        );
    });

    ui.add_space(12.0);

    ui.horizontal(|ui| {
        ui.label("点击设置新按键：");
        let (btn_text, btn_color) = if state.capturing {
            (
                "⌨  请按下目标按键...".to_string(),
                crate::theme::WARNING,
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
            state.capture_slot.active.store(state.capturing, Ordering::SeqCst);
            // Clear any stale capture when starting or stopping.
            *state.capture_slot.latest.lock() = None;
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

    ui.label(egui::RichText::new("修饰键（可选）：").strong());
    ui.horizontal(|ui| {
        for (key, label) in &[
            ("ctrl", "Ctrl"),
            ("alt", "Alt"),
            ("shift", "Shift"),
            ("super", "Super"),
        ] {
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

    render_save_buttons(ui, state);

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

fn render_save_buttons(ui: &mut egui::Ui, state: &mut SettingsState) {
    let (saved_key, saved_mods, saved_mode) = {
        let hk = state.shared_hotkey.lock();
        (hk.key.clone(), hk.modifiers.clone(), hk.mode.clone())
    };

    let changed = state.hotkey_key != saved_key
        || state.hotkey_mods != saved_mods
        || state.hotkey_mode != saved_mode;

    ui.horizontal(|ui| {
        let save_btn = ui.add_enabled(changed, egui::Button::new("💾  保存快捷键"));
        if save_btn.clicked() {
            {
                let mut hk = state.shared_hotkey.lock();
                hk.key = state.hotkey_key.clone();
                hk.modifiers = state.hotkey_mods.clone();
                hk.mode = state.hotkey_mode.clone();
            }
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
                crate::theme::SUCCESS,
            ));
        }

        let revert_btn = ui.add_enabled(changed, egui::Button::new("↩  还原"));
        if revert_btn.clicked() {
            state.hotkey_key = saved_key;
            state.hotkey_mods = saved_mods;
            state.hotkey_mode = saved_mode;
        }
    });
}

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

/// Show a colored banner describing the current hotkey backend status.
/// The most important case: rdev on Wayland (falling back) means global
/// shortcuts won't work in native-Wayland apps — we must tell the user how
/// to fix it.
fn render_backend_warning(ui: &mut egui::Ui, state: &SettingsState) {
    use crate::hotkey::Backend;
    let backend = state.backend_info.backend.lock().clone();
    let Some(backend) = backend else {
        return;
    };

    match backend {
        Backend::RdevX11 => {
            // No banner — everything works.
        }
        Backend::EvdevWayland { devices } => {
            banner(
                ui,
                crate::theme::BG_CARD,
                crate::theme::SUCCESS,
                &format!(
                    "✓ Wayland + evdev 后端已启用，监听 {} 个键盘设备。快捷键在任何窗口都有效。",
                    devices
                ),
                None,
            );
        }
        Backend::RdevWaylandFallback { evdev_error } => {
            banner(
                ui,
                egui::Color32::from_rgb(0x5A, 0x2D, 0x14),
                crate::theme::WARNING,
                "⚠ Wayland 会话 + 只有 rdev 后端，快捷键仅在 X11 / XWayland 窗口有效，在原生 Wayland 应用中不会触发。",
                Some(&format!(
                    "解决方案一：执行 sudo usermod -aG input $USER，注销后重新登录，xsay 将自动切换到 evdev。\n\
                     解决方案二：改用 X11 会话（登录界面选择 \"GNOME on Xorg\"）。\n\
                     evdev 报错：{}",
                    evdev_error
                )),
            );
        }
    }
    ui.add_space(6.0);
}

fn banner(
    ui: &mut egui::Ui,
    bg: egui::Color32,
    title_color: egui::Color32,
    title: &str,
    subline: Option<&str>,
) {
    egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .corner_radius(crate::theme::radius_md())
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .color(title_color)
                    .strong()
                    .size(crate::theme::FONT_BODY),
            );
            if let Some(s) = subline {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(s)
                        .color(crate::theme::TEXT_SECONDARY)
                        .size(crate::theme::FONT_SM),
                );
            }
        });
}
