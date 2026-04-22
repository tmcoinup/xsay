use crate::config::HotkeyConfig;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use rdev::{listen, EventType, Key};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug, Clone)]
pub enum AppEvent {
    HotkeyPressed,
    HotkeyReleased,
    EscapePressed,
}

/// `shared_config` is read on every key event so hotkey changes take effect immediately.
/// `capture_active` is set by the settings UI when capturing a new hotkey; while true,
/// the hotkey fires are suppressed so rdev doesn't interfere with the egui key capture.
pub fn run_hotkey_thread(
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
) {
    let held_keys: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
    let hotkey_active = Arc::new(AtomicBool::new(false));

    let held_clone = Arc::clone(&held_keys);
    let active_clone = Arc::clone(&hotkey_active);
    let tx = event_tx;
    let cfg = shared_config;
    let capturing = capture_active;

    if let Err(e) = listen(move |event| {
        match event.event_type {
            EventType::KeyPress(key) => {
                let mut held = held_clone.lock();
                held.insert(key.clone());

                // While settings is capturing a hotkey, suppress normal hotkey events
                if capturing.load(Ordering::SeqCst) {
                    return;
                }

                let config = cfg.lock();
                let target = parse_key(&config.key);
                let mods_ok = config
                    .modifiers
                    .iter()
                    .all(|m| parse_modifier(m).map(|k| held.contains(&k)).unwrap_or(true));

                if key == target && mods_ok && !active_clone.load(Ordering::SeqCst) {
                    active_clone.store(true, Ordering::SeqCst);
                    let _ = tx.send(AppEvent::HotkeyPressed);
                }

                if key == Key::Escape {
                    let _ = tx.send(AppEvent::EscapePressed);
                }
            }
            EventType::KeyRelease(key) => {
                let mut held = held_clone.lock();
                held.remove(&key);

                if capturing.load(Ordering::SeqCst) {
                    return;
                }

                let config = cfg.lock();
                let target = parse_key(&config.key);

                if key == target && active_clone.load(Ordering::SeqCst) {
                    active_clone.store(false, Ordering::SeqCst);
                    let _ = tx.send(AppEvent::HotkeyReleased);
                }
            }
            _ => {}
        }
    }) {
        log::error!("Hotkey listener error: {:?}", e);
        eprintln!("热键监听失败: {:?}", e);
        eprintln!("Linux 请确保使用 X11（不是 Wayland）并有足够权限。");
    }
}

pub fn parse_key(name: &str) -> Key {
    match name {
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "CapsLock" => Key::CapsLock,
        "ScrollLock" => Key::ScrollLock,
        "Pause" => Key::Pause,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Delete" => Key::Delete,
        "Tab" => Key::Tab,
        "BackSlash" => Key::BackSlash,
        "RightAlt" | "AltGr" => Key::AltGr,
        other => {
            log::warn!("未知按键 '{}'，回退到 F9", other);
            Key::F9
        }
    }
}

fn parse_modifier(name: &str) -> Option<Key> {
    match name {
        "ctrl" | "control" => Some(Key::ControlLeft),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::ShiftLeft),
        "super" | "meta" => Some(Key::MetaLeft),
        _ => None,
    }
}
