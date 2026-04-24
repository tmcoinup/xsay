//! 快捷键标签页：捕获新按键、选择触发模式（按住/切换）。
//!
//! 捕获到新快捷键或切换模式时立即写入磁盘 —— 没有单独的「保存」按钮。

use super::SettingsState;
use crate::config::Config;
use crate::theme::{self, Icon};
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
/// Whichever path fires first wins. On success we immediately persist the
/// new hotkey (auto-save) and show a status toast.
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
                    // On Linux/Windows, `mac_cmd` is the Super/Meta key.
                    // `command` maps to Ctrl on those platforms, so don't use
                    // it — that was the bug where pressing Ctrl+Z would
                    // falsely add "super" to the captured combo.
                    if modifiers.mac_cmd {
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
        save_hotkey_now(state);
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
        save_hotkey_now(state);
    }
}

fn end_capture(state: &mut SettingsState) {
    state.capturing = false;
    state.capture_active.store(false, Ordering::SeqCst);
    state.capture_slot.active.store(false, Ordering::SeqCst);
    *state.capture_slot.latest.lock() = None;
}

/// Persist the current hotkey selection to the shared worker state and to
/// config.toml, then show a success toast. Called after a successful capture
/// and after a mode radio click.
fn save_hotkey_now(state: &mut SettingsState) {
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
    let mut parts: Vec<String> = state.hotkey_mods.iter().map(|m| pretty_mod(m)).collect();
    parts.push(pretty_key(&state.hotkey_key));
    let combo = parts.join(" + ");
    state.set_status(
        format!("快捷键已更新为 {}（{}）", combo, mode_label),
        crate::theme::SUCCESS,
    );
}

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;

        render_backend_warning(ui, state);
        // External-trigger card ("xsay toggle" + GNOME custom shortcut) is
        // Unix-only — it relies on the IPC module, which uses Unix sockets.
        #[cfg(unix)]
        render_external_trigger_card(ui, state);
        render_current_card(ui, state);
        render_capture_card(ui, state);
        render_mode_card(ui, state);

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

/// External-trigger card: Flameshot-style. User binds a system shortcut to
/// the xsay binary + "toggle"; we show that exact command plus a copy
/// button plus a brief howto. Placed above the in-app capture card because
/// it's more reliable on Wayland (no /dev/input access required). Unix-only.
///
/// The displayed command uses `std::env::current_exe()` rather than a bare
/// "xsay toggle" because GNOME Custom Shortcuts don't inherit the user's
/// shell PATH — if xsay isn't in /usr/local/bin or ~/.local/bin the bare
/// form fails silently. An absolute path always works.
#[cfg(unix)]
fn render_external_trigger_card(ui: &mut egui::Ui, state: &mut SettingsState) {
    theme::section_card(ui, "外部触发（推荐）", |ui| {
        ui.label(
            egui::RichText::new("在系统快捷键中调用命令，兼容 X11 / Wayland，组合键不受限。")
                .color(crate::theme::TEXT_SECONDARY)
                .size(crate::theme::FONT_SM),
        );
        ui.add_space(6.0);

        let exe = std::env::current_exe()
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "xsay".to_string());
        let command = format!("{} toggle", exe);

        // Command pill with a copy action on the right. Monospace so the
        // shell command is visually distinct from Chinese copy around it.
        // Let the pill wrap inside the available width so long absolute
        // paths (target/debug/xsay) don't push the copy button off-screen.
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(crate::theme::BG_CARD_HOVER)
                .corner_radius(crate::theme::radius_sm())
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(&command)
                            .color(crate::theme::TEXT_PRIMARY)
                            .monospace()
                            .size(crate::theme::FONT_SM),
                    );
                });
            if theme::icon_link_button(ui, Icon::Check, "复制命令", crate::theme::ACCENT)
                .clicked()
            {
                ui.ctx().copy_text(command.clone());
                state.set_status("命令已复制到剪贴板", crate::theme::SUCCESS);
            }
        });

        ui.add_space(6.0);
        theme::helper_text(
            ui,
            "配置步骤：系统设置 → 键盘 → 自定义快捷键 → 新建\n\
             名称任取，命令粘贴上面那行，快捷键按你喜欢的组合（如 Super+Z）。\n\
             按下后切换录音，再按一次或停顿 1.5 秒自动识别。",
        );
    });
}

fn render_current_card(ui: &mut egui::Ui, state: &SettingsState) {
    theme::section_card(ui, "当前快捷键", |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let mut parts: Vec<String> =
                state.hotkey_mods.iter().map(|m| pretty_mod(m)).collect();
            parts.push(pretty_key(&state.hotkey_key));
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    ui.label(
                        egui::RichText::new("+")
                            .color(crate::theme::TEXT_SECONDARY)
                            .size(crate::theme::FONT_BODY),
                    );
                }
                key_chip(ui, part);
            }
        });

        let mode_hint = if state.hotkey_mode == "toggle" {
            "点按切换：按一次开始录音，再按一次结束并输入。停顿 1.5 秒自动识别。"
        } else {
            "按住说话：按住快捷键录音，松开转写输入。停顿 1.5 秒自动识别。"
        };
        theme::helper_text(ui, mode_hint);
    });
}

/// Single pill rendering one modifier or the primary key. Outlined with
/// ACCENT, slightly darker interior — matches the Figma "key-cap" look.
fn key_chip(ui: &mut egui::Ui, text: &str) {
    let font_id = egui::FontId::monospace(crate::theme::FONT_BODY);
    let text_size = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), crate::theme::ACCENT)
        .size();
    let pad_x = 10.0;
    let pad_y = 5.0;
    let total = egui::vec2(pad_x * 2.0 + text_size.x, pad_y * 2.0 + text_size.y);
    let (rect, _) = ui.allocate_exact_size(total, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        crate::theme::radius_sm(),
        crate::theme::BG_CARD_HOVER,
    );
    ui.painter().rect_stroke(
        rect,
        crate::theme::radius_sm(),
        egui::Stroke::new(1.0, crate::theme::ACCENT),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font_id,
        crate::theme::ACCENT,
    );
}

fn pretty_mod(name: &str) -> String {
    match name {
        "ctrl" => "Ctrl",
        "alt" => "Alt",
        "shift" => "Shift",
        "super" => "Super",
        other => other,
    }
    .to_string()
}

fn pretty_key(name: &str) -> String {
    // Single-character keys display uppercased ("z" → "Z") so they read as
    // key-cap labels rather than variable names.
    if name.chars().count() == 1 {
        name.to_uppercase()
    } else {
        name.to_string()
    }
}

fn render_capture_card(ui: &mut egui::Ui, state: &mut SettingsState) {
    theme::section_card(ui, "设置新按键", |ui| {
        let (icon, label, color) = if state.capturing {
            (Icon::Keyboard, "请按下目标按键...", crate::theme::WARNING)
        } else {
            (Icon::Keyboard, "捕捉按键", crate::theme::ACCENT)
        };
        if theme::outlined_button(ui, icon, label, color, false).clicked() {
            state.capturing = !state.capturing;
            state
                .capture_active
                .store(state.capturing, Ordering::SeqCst);
            state
                .capture_slot
                .active
                .store(state.capturing, Ordering::SeqCst);
            *state.capture_slot.latest.lock() = None;
        }
        if state.capturing {
            theme::helper_text(
                ui,
                "支持 F1–F12、Home/End/PageUp/PageDown、字母键等。按 Esc 取消。",
            );
        } else {
            theme::helper_text(ui, "按下后捕获任意组合键，会自动保存。");
        }

        // Wayland + rdev fallback silently drops Super key presses. Warn in
        // the capture card itself so users don't wonder why Super+Z produces
        // only "Z" — the banner at the top of the tab is easy to miss when
        // they're focused on the button.
        if matches!(
            state.backend_info.backend.lock().clone(),
            Some(crate::hotkey::Backend::RdevWaylandFallback { .. })
        ) {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "⚠ 当前 Wayland 回退后端无法捕获 Super 键。执行 \
                     sudo usermod -aG input $USER 并重新登录后即可使用 Super 组合。",
                )
                .color(crate::theme::WARNING)
                .size(crate::theme::FONT_SM),
            );
        }
    });
}

fn render_mode_card(ui: &mut egui::Ui, state: &mut SettingsState) {
    theme::section_card(ui, "触发模式", |ui| {
        render_mode_row(ui, state, "hold", "按住说话", "松开后开始识别");
        ui.add_space(6.0);
        render_mode_row(ui, state, "toggle", "点按切换", "再按一次结束");
    });
}

fn render_mode_row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    value: &str,
    title: &str,
    subtitle: &str,
) {
    let selected = state.hotkey_mode == value;
    ui.horizontal(|ui| {
        if theme::radio_button(ui, selected, crate::theme::ACCENT).clicked() && !selected {
            state.hotkey_mode = value.to_string();
            save_hotkey_now(state);
        }
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(title)
                    .color(crate::theme::TEXT_PRIMARY)
                    .size(crate::theme::FONT_BODY),
            );
            ui.label(
                egui::RichText::new(subtitle)
                    .color(crate::theme::TEXT_SECONDARY)
                    .size(crate::theme::FONT_SM),
            );
        });
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
                Icon::Check,
                crate::theme::SUCCESS,
                &format!(
                    "Wayland + evdev 后端已启用，监听 {} 个键盘设备。快捷键在任何窗口都有效。",
                    devices
                ),
                None,
            );
        }
        Backend::RdevWaylandFallback { evdev_error } => {
            banner(
                ui,
                egui::Color32::from_rgb(0x5A, 0x2D, 0x14),
                Icon::Warning,
                crate::theme::WARNING,
                "Wayland 会话 + 只有 rdev 后端，快捷键仅在 X11 / XWayland 窗口有效，原生 Wayland 应用不会触发。",
                Some(&format!(
                    "修复方法一：sudo usermod -aG input $USER，注销重登后 xsay 自动切换到 evdev。\n\
                     修复方法二：改用 X11 会话（登录界面选择 GNOME on Xorg）。\n\
                     evdev 报错：{}",
                    evdev_error
                )),
            );
        }
    }
}

fn banner(
    ui: &mut egui::Ui,
    bg: egui::Color32,
    icon: Icon,
    title_color: egui::Color32,
    title: &str,
    subline: Option<&str>,
) {
    egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .corner_radius(crate::theme::radius_lg())
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                theme::draw_icon(ui.painter(), rect, icon, title_color);
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(title)
                        .color(title_color)
                        .strong()
                        .size(crate::theme::FONT_BODY),
                );
            });
            if let Some(s) = subline {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(s)
                        .color(crate::theme::TEXT_SECONDARY)
                        .size(crate::theme::FONT_SM),
                );
            }
        });
}
