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
    // held_keys: which keys the OS currently reports as physically down (used
    // to filter auto-repeat — the OS sends repeated KeyPress without interleaved
    // KeyRelease while a key is held).
    let held_keys: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
    // recording: logical "are we currently in a recording session?" Different
    // from held_keys[target] because toggle mode has press-release-press cycles.
    let recording = Arc::new(AtomicBool::new(false));

    let held_clone = Arc::clone(&held_keys);
    let rec_clone = Arc::clone(&recording);
    let tx = event_tx;
    let cfg = shared_config;
    let capturing = capture_active;

    if let Err(e) = listen(move |event| {
        match event.event_type {
            EventType::KeyPress(key) => {
                let already_down = {
                    let mut held = held_clone.lock();
                    let was = held.contains(&key);
                    held.insert(key.clone());
                    was
                };

                if capturing.load(Ordering::SeqCst) {
                    return;
                }

                if key == Key::Escape {
                    let _ = tx.send(AppEvent::EscapePressed);
                }

                // Ignore OS auto-repeat for hotkey logic.
                if already_down {
                    return;
                }

                let config = cfg.lock();
                let target = parse_key(&config.key);
                let mode = config.mode.clone();
                let held = held_clone.lock();
                let mods_ok = config
                    .modifiers
                    .iter()
                    .all(|m| parse_modifier(m).map(|k| held.contains(&k)).unwrap_or(true));
                drop(held);
                drop(config);

                if key != target || !mods_ok {
                    return;
                }

                if mode == "toggle" {
                    if rec_clone.load(Ordering::SeqCst) {
                        rec_clone.store(false, Ordering::SeqCst);
                        let _ = tx.send(AppEvent::HotkeyReleased);
                    } else {
                        rec_clone.store(true, Ordering::SeqCst);
                        let _ = tx.send(AppEvent::HotkeyPressed);
                    }
                } else {
                    // hold mode
                    if !rec_clone.load(Ordering::SeqCst) {
                        rec_clone.store(true, Ordering::SeqCst);
                        let _ = tx.send(AppEvent::HotkeyPressed);
                    }
                }
            }
            EventType::KeyRelease(key) => {
                held_clone.lock().remove(&key);

                if capturing.load(Ordering::SeqCst) {
                    return;
                }

                let config = cfg.lock();
                let target = parse_key(&config.key);
                let is_hold_mode = config.mode != "toggle";
                drop(config);

                if key == target && is_hold_mode && rec_clone.load(Ordering::SeqCst) {
                    rec_clone.store(false, Ordering::SeqCst);
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
        "Space" => Key::Space,
        "Return" | "Enter" => Key::Return,
        "PrintScreen" => Key::PrintScreen,
        "NumLock" => Key::NumLock,
        "a" | "A" => Key::KeyA,
        "b" | "B" => Key::KeyB,
        "c" | "C" => Key::KeyC,
        "d" | "D" => Key::KeyD,
        "e" | "E" => Key::KeyE,
        "f" | "F" => Key::KeyF,
        "g" | "G" => Key::KeyG,
        "h" | "H" => Key::KeyH,
        "i" | "I" => Key::KeyI,
        "j" | "J" => Key::KeyJ,
        "k" | "K" => Key::KeyK,
        "l" | "L" => Key::KeyL,
        "m" | "M" => Key::KeyM,
        "n" | "N" => Key::KeyN,
        "o" | "O" => Key::KeyO,
        "p" | "P" => Key::KeyP,
        "q" | "Q" => Key::KeyQ,
        "r" | "R" => Key::KeyR,
        "s" | "S" => Key::KeyS,
        "t" | "T" => Key::KeyT,
        "u" | "U" => Key::KeyU,
        "v" | "V" => Key::KeyV,
        "w" | "W" => Key::KeyW,
        "x" | "X" => Key::KeyX,
        "y" | "Y" => Key::KeyY,
        "z" | "Z" => Key::KeyZ,
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
