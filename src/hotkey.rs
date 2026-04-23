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

/// Writable slot used by the settings UI to capture the next pressed key.
///
/// When `active = true`, the hotkey backend (rdev or evdev) writes the next
/// key press it observes into `latest` *instead of* firing the normal
/// recording logic. The UI polls `latest` each frame and applies it.
///
/// This is the OS-level capture path. The settings UI also has an egui
/// event-based path that works while the settings window has keyboard focus;
/// the two run in parallel and whichever fires first wins.
#[derive(Debug, Default)]
pub struct CaptureSlot {
    pub active: AtomicBool,
    /// (key_name, modifiers) captured by the global backend.
    pub latest: Mutex<Option<(String, Vec<String>)>>,
}

impl CaptureSlot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            latest: Mutex::new(None),
        })
    }
}

/// Which backend ended up running, surfaced to the settings UI so it can
/// warn the user when we're on Wayland with only rdev (i.e. global hotkeys
/// won't capture native-Wayland application focus).
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    /// rdev on X11 (or macOS / Windows) — global shortcuts work.
    RdevX11,
    /// rdev falling back on Wayland — only sees XWayland keystrokes.
    RdevWaylandFallback { evdev_error: String },
    /// evdev direct — works on both X11 and Wayland.
    EvdevWayland { devices: usize },
}

#[derive(Debug, Default)]
pub struct BackendInfo {
    pub backend: Mutex<Option<Backend>>,
}

impl BackendInfo {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            backend: Mutex::new(None),
        })
    }
}

/// `shared_config` is read on every key event so hotkey changes take effect immediately.
/// `capture_active` is set by the settings UI when capturing a new hotkey; while true,
/// the hotkey fires are suppressed so rdev doesn't interfere with the egui key capture.
pub fn run_hotkey_thread(
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
    capture_slot: Arc<CaptureSlot>,
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

                // Capture mode: record the key into the slot (unless it's a
                // bare modifier, which the user probably doesn't want as a
                // standalone hotkey). Don't fire normal hotkey logic.
                if capturing.load(Ordering::SeqCst) {
                    if !already_down {
                        record_capture(&key, &held_clone, &capture_slot);
                    }
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

/// Reverse of `parse_key` — turns an rdev Key into the string name we persist
/// in config.toml. Returns None for keys we don't support as hotkeys (bare
/// modifiers, media keys, unknown).
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

/// Write the captured key + currently held modifiers into the slot. Skipped
/// for bare modifier presses (ctrl, shift, alt, etc.) and for keys we don't
/// know how to name.
fn record_capture(
    key: &Key,
    held: &Arc<Mutex<HashSet<Key>>>,
    slot: &Arc<CaptureSlot>,
) {
    // Ignore bare modifiers — user is probably still composing the chord.
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

    // Escape cancels — stored as a sentinel we recognize in the UI layer.
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
