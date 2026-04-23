//! evdev-based global hotkey listener for Wayland support.
//! Reads keyboard events directly from /dev/input/event*, which works on both
//! X11 and Wayland. Requires the user to be in the `input` group.

use crate::config::HotkeyConfig;
use crate::hotkey::AppEvent;
use crossbeam_channel::Sender;
use evdev::{Device, EventType, InputEvent, KeyCode as EvKey};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub fn is_wayland_session() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        && std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}

/// Spawn one listener thread per accessible keyboard device.
/// Returns the number of devices on success, or an error if none are readable.
pub fn spawn_hotkey_threads(
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
) -> Result<usize, String> {
    let devices: Vec<_> = evdev::enumerate()
        .filter(|(_, dev)| {
            dev.supported_keys()
                .map(|keys| keys.contains(EvKey::KEY_ESC))
                .unwrap_or(false)
        })
        .collect();

    if devices.is_empty() {
        return Err(
            "no accessible keyboard devices (add user to 'input' group)".to_string(),
        );
    }

    // recording: logical "are we currently recording?" Different from the
    // physical-key-down held_keys set, because toggle mode cycles it with taps.
    let recording = Arc::new(AtomicBool::new(false));
    let held_keys: Arc<Mutex<HashSet<u16>>> = Arc::new(Mutex::new(HashSet::new()));

    let n = devices.len();
    for (path, device) in devices {
        let event_tx = event_tx.clone();
        let shared_config = Arc::clone(&shared_config);
        let capture_active = Arc::clone(&capture_active);
        let recording = Arc::clone(&recording);
        let held_keys = Arc::clone(&held_keys);

        std::thread::spawn(move || {
            log::info!("evdev monitoring: {}", path.display());
            run_device_loop(
                device,
                event_tx,
                shared_config,
                capture_active,
                recording,
                held_keys,
            );
        });
    }

    Ok(n)
}

fn run_device_loop(
    mut device: Device,
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    held_keys: Arc<Mutex<HashSet<u16>>>,
) {
    loop {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() == EventType::KEY {
                        handle_key(
                            &ev,
                            &event_tx,
                            &shared_config,
                            &capture_active,
                            &recording,
                            &held_keys,
                        );
                    }
                }
            }
            Err(e) => {
                log::error!("evdev device error, stopping thread: {}", e);
                break;
            }
        }
    }
}

fn handle_key(
    ev: &InputEvent,
    event_tx: &Sender<AppEvent>,
    shared_config: &Arc<Mutex<HotkeyConfig>>,
    capture_active: &Arc<AtomicBool>,
    recording: &Arc<AtomicBool>,
    held_keys: &Arc<Mutex<HashSet<u16>>>,
) {
    let code = ev.code();
    let value = ev.value();
    let pressed = value == 1;
    let released = value == 0;
    if !pressed && !released {
        return; // ignore auto-repeat (value == 2)
    }

    {
        let mut held = held_keys.lock();
        if pressed {
            held.insert(code);
        } else {
            held.remove(&code);
        }
    }

    if capture_active.load(Ordering::SeqCst) {
        return;
    }

    if pressed && code == EvKey::KEY_ESC.code() {
        let _ = event_tx.send(AppEvent::EscapePressed);
    }

    let cfg = shared_config.lock().clone();
    let target = key_name_to_evdev(&cfg.key);
    let is_toggle = cfg.mode == "toggle";
    let mods_ok = cfg.modifiers.iter().all(|m| {
        modifier_to_evdev(m)
            .map(|c| held_keys.lock().contains(&c))
            .unwrap_or(true)
    });

    if Some(code) != target {
        return;
    }

    if pressed && mods_ok {
        if is_toggle {
            if recording.load(Ordering::SeqCst) {
                recording.store(false, Ordering::SeqCst);
                let _ = event_tx.send(AppEvent::HotkeyReleased);
            } else {
                recording.store(true, Ordering::SeqCst);
                let _ = event_tx.send(AppEvent::HotkeyPressed);
            }
        } else if !recording.load(Ordering::SeqCst) {
            recording.store(true, Ordering::SeqCst);
            let _ = event_tx.send(AppEvent::HotkeyPressed);
        }
    }

    if released && !is_toggle && recording.load(Ordering::SeqCst) {
        recording.store(false, Ordering::SeqCst);
        let _ = event_tx.send(AppEvent::HotkeyReleased);
    }
}

fn key_name_to_evdev(name: &str) -> Option<u16> {
    let k = match name {
        "F1" => EvKey::KEY_F1,
        "F2" => EvKey::KEY_F2,
        "F3" => EvKey::KEY_F3,
        "F4" => EvKey::KEY_F4,
        "F5" => EvKey::KEY_F5,
        "F6" => EvKey::KEY_F6,
        "F7" => EvKey::KEY_F7,
        "F8" => EvKey::KEY_F8,
        "F9" => EvKey::KEY_F9,
        "F10" => EvKey::KEY_F10,
        "F11" => EvKey::KEY_F11,
        "F12" => EvKey::KEY_F12,
        "CapsLock" => EvKey::KEY_CAPSLOCK,
        "ScrollLock" => EvKey::KEY_SCROLLLOCK,
        "Pause" => EvKey::KEY_PAUSE,
        "Home" => EvKey::KEY_HOME,
        "End" => EvKey::KEY_END,
        "PageUp" => EvKey::KEY_PAGEUP,
        "PageDown" => EvKey::KEY_PAGEDOWN,
        "Delete" => EvKey::KEY_DELETE,
        "Insert" => EvKey::KEY_INSERT,
        "Tab" => EvKey::KEY_TAB,
        "Space" => EvKey::KEY_SPACE,
        "Return" | "Enter" => EvKey::KEY_ENTER,
        "PrintScreen" => EvKey::KEY_SYSRQ,
        "NumLock" => EvKey::KEY_NUMLOCK,
        "RightAlt" | "AltGr" => EvKey::KEY_RIGHTALT,
        "a" | "A" => EvKey::KEY_A,
        "b" | "B" => EvKey::KEY_B,
        "c" | "C" => EvKey::KEY_C,
        "d" | "D" => EvKey::KEY_D,
        "e" | "E" => EvKey::KEY_E,
        "f" | "F" => EvKey::KEY_F,
        "g" | "G" => EvKey::KEY_G,
        "h" | "H" => EvKey::KEY_H,
        "i" | "I" => EvKey::KEY_I,
        "j" | "J" => EvKey::KEY_J,
        "k" | "K" => EvKey::KEY_K,
        "l" | "L" => EvKey::KEY_L,
        "m" | "M" => EvKey::KEY_M,
        "n" | "N" => EvKey::KEY_N,
        "o" | "O" => EvKey::KEY_O,
        "p" | "P" => EvKey::KEY_P,
        "q" | "Q" => EvKey::KEY_Q,
        "r" | "R" => EvKey::KEY_R,
        "s" | "S" => EvKey::KEY_S,
        "t" | "T" => EvKey::KEY_T,
        "u" | "U" => EvKey::KEY_U,
        "v" | "V" => EvKey::KEY_V,
        "w" | "W" => EvKey::KEY_W,
        "x" | "X" => EvKey::KEY_X,
        "y" | "Y" => EvKey::KEY_Y,
        "z" | "Z" => EvKey::KEY_Z,
        _ => return None,
    };
    Some(k.code())
}

fn modifier_to_evdev(name: &str) -> Option<u16> {
    let k = match name {
        "ctrl" | "control" => EvKey::KEY_LEFTCTRL,
        "alt" => EvKey::KEY_LEFTALT,
        "shift" => EvKey::KEY_LEFTSHIFT,
        "super" | "meta" => EvKey::KEY_LEFTMETA,
        _ => return None,
    };
    Some(k.code())
}
