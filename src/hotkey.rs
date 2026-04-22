use crate::config::HotkeyConfig;
use crossbeam_channel::Sender;
use rdev::{EventType, Key, listen};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub enum AppEvent {
    HotkeyPressed,
    HotkeyReleased,
    EscapePressed,
}

pub fn run_hotkey_thread(event_tx: Sender<AppEvent>, config: HotkeyConfig) {
    let target_key = parse_key(&config.key);
    let required_mods: Vec<Key> = config
        .modifiers
        .iter()
        .filter_map(|m| parse_modifier(m))
        .collect();

    let held_keys: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
    let hotkey_active = Arc::new(AtomicBool::new(false));

    let held_clone = Arc::clone(&held_keys);
    let active_clone = Arc::clone(&hotkey_active);
    let tx = event_tx;

    if let Err(e) = listen(move |event| {
        match event.event_type {
            EventType::KeyPress(key) => {
                let mut held = held_clone.lock().unwrap();
                held.insert(key.clone());

                let mods_ok = required_mods.iter().all(|m| held.contains(m));
                if key == target_key && mods_ok && !active_clone.load(Ordering::SeqCst) {
                    active_clone.store(true, Ordering::SeqCst);
                    let _ = tx.send(AppEvent::HotkeyPressed);
                }

                if key == Key::Escape {
                    let _ = tx.send(AppEvent::EscapePressed);
                }
            }
            EventType::KeyRelease(key) => {
                let mut held = held_clone.lock().unwrap();
                held.remove(&key);

                if key == target_key && active_clone.load(Ordering::SeqCst) {
                    active_clone.store(false, Ordering::SeqCst);
                    let _ = tx.send(AppEvent::HotkeyReleased);
                }
            }
            _ => {}
        }
    }) {
        log::error!("Hotkey listener error: {:?}", e);
        eprintln!("Error: Could not start hotkey listener: {:?}", e);
        eprintln!("On Linux, ensure you have X11 (not Wayland) and sufficient permissions.");
    }
}

fn parse_key(name: &str) -> Key {
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
            log::warn!("Unknown key '{}', falling back to F9", other);
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
