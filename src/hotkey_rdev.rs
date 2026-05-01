//! rdev-based global hotkey listener. Used on macOS and Windows where
//! rdev's native key-tap APIs (CGEventTap, SetWindowsHookEx) work without
//! the foot-guns of its Linux path.
//!
//! On Linux this module is not compiled at all — Linux uses
//! `hotkey_evdev` instead. rdev's Linux backend opens an XRecord context
//! that intercepts every input event in the X server; combined with our
//! own X11 windows (eframe winit + tray-icon GTK) in the same process,
//! mutter ends up wedged: keyboard events get stuck inside the server,
//! apps stop responding to redraws, only the hardware mouse cursor still
//! moves. See commit 012950b for the freeze repro and write-up.

use crate::config::HotkeyConfig;
use crate::hotkey::{AppEvent, CaptureSlot};
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use rdev::{EventType, Key, listen};
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// `shared_config` is read on every key event so hotkey changes take effect immediately.
/// `capture_active` is set by the settings UI when capturing a new hotkey; while true,
/// the hotkey fires are suppressed so rdev doesn't interfere with the egui key capture.
pub fn run_hotkey_thread(
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
    capture_slot: Arc<CaptureSlot>,
) {
    let held_keys: Arc<Mutex<HashSet<Key>>> = Arc::new(Mutex::new(HashSet::new()));
    let recording = Arc::new(AtomicBool::new(false));

    let held_clone = Arc::clone(&held_keys);
    let rec_clone = Arc::clone(&recording);
    let tx = event_tx;
    let cfg = shared_config;
    let capturing = capture_active;
    let capture_slot = capture_slot;

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
                    if !already_down {
                        record_capture(&key, &held_clone, &capture_slot);
                    }
                    return;
                }

                if key == Key::Escape {
                    let _ = tx.send(AppEvent::EscapePressed);
                }

                if already_down {
                    return;
                }

                let config = cfg.lock();
                if !config.internal_listener {
                    return;
                }
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
                } else if !rec_clone.load(Ordering::SeqCst) {
                    rec_clone.store(true, Ordering::SeqCst);
                    let _ = tx.send(AppEvent::HotkeyPressed);
                }
            }
            EventType::KeyRelease(key) => {
                held_clone.lock().remove(&key);

                if capturing.load(Ordering::SeqCst) {
                    return;
                }

                let config = cfg.lock();
                if !config.internal_listener {
                    return;
                }
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

fn rdev_key_to_name(key: &Key) -> Option<&'static str> {
    Some(match key {
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::CapsLock => "CapsLock",
        Key::ScrollLock => "ScrollLock",
        Key::Pause => "Pause",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Delete => "Delete",
        Key::Tab => "Tab",
        Key::BackSlash => "BackSlash",
        Key::AltGr => "RightAlt",
        Key::Space => "Space",
        Key::Return => "Return",
        Key::PrintScreen => "PrintScreen",
        Key::NumLock => "NumLock",
        Key::KeyA => "a",
        Key::KeyB => "b",
        Key::KeyC => "c",
        Key::KeyD => "d",
        Key::KeyE => "e",
        Key::KeyF => "f",
        Key::KeyG => "g",
        Key::KeyH => "h",
        Key::KeyI => "i",
        Key::KeyJ => "j",
        Key::KeyK => "k",
        Key::KeyL => "l",
        Key::KeyM => "m",
        Key::KeyN => "n",
        Key::KeyO => "o",
        Key::KeyP => "p",
        Key::KeyQ => "q",
        Key::KeyR => "r",
        Key::KeyS => "s",
        Key::KeyT => "t",
        Key::KeyU => "u",
        Key::KeyV => "v",
        Key::KeyW => "w",
        Key::KeyX => "x",
        Key::KeyY => "y",
        Key::KeyZ => "z",
        _ => return None,
    })
}

fn record_capture(key: &Key, held: &Arc<Mutex<HashSet<Key>>>, slot: &Arc<CaptureSlot>) {
    if matches!(
        key,
        Key::ControlLeft
            | Key::ControlRight
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
    ) {
        return;
    }

    if matches!(key, Key::Escape) {
        *slot.latest.lock() = Some(("__cancel__".to_string(), Vec::new()));
        return;
    }

    let Some(name) = rdev_key_to_name(key) else {
        return;
    };

    let mut mods = Vec::new();
    let held_snapshot = held.lock();
    if held_snapshot.contains(&Key::ControlLeft) || held_snapshot.contains(&Key::ControlRight) {
        mods.push("ctrl".to_string());
    }
    if held_snapshot.contains(&Key::Alt) {
        mods.push("alt".to_string());
    }
    if held_snapshot.contains(&Key::ShiftLeft) || held_snapshot.contains(&Key::ShiftRight) {
        mods.push("shift".to_string());
    }
    if held_snapshot.contains(&Key::MetaLeft) || held_snapshot.contains(&Key::MetaRight) {
        mods.push("super".to_string());
    }
    drop(held_snapshot);

    *slot.latest.lock() = Some((name.to_string(), mods));
}
