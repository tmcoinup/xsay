//! evdev-based global hotkey listener for Wayland support.
//! Reads keyboard events directly from /dev/input/event*, which works on both
//! X11 and Wayland. Requires the user to be in the `input` group.

use crate::config::HotkeyConfig;
use crate::hotkey::{AppEvent, CaptureSlot};
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
    capture_slot: Arc<CaptureSlot>,
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
        let capture_slot = Arc::clone(&capture_slot);
        let recording = Arc::clone(&recording);
        let held_keys = Arc::clone(&held_keys);

        std::thread::spawn(move || {
            log::info!("evdev monitoring: {}", path.display());
            run_device_loop(
                device,
                event_tx,
                shared_config,
                capture_active,
                capture_slot,
                recording,
                held_keys,
            );
        });
    }

    Ok(n)
}

/// Per-iteration signal from handle_key back to run_device_loop. We used
/// to distinguish Grab/Ungrab here so we could take exclusive access to
/// the physical device and suppress the main hotkey key from reaching
/// the focused app. That approach had a subtle race: every grab/ungrab
/// pair gave the compositor a brief window during which held-key state
/// could get "stuck", and a user who mashed the hotkey hard enough to
/// rapid-cycle could end up with an unresponsive keyboard requiring
/// logout/reboot.
///
/// Trading correctness for safety: we no longer grab. Letter-based
/// hotkeys in hold mode leak characters (one initial character from the
/// physical key-down that the compositor sees before we can react), but
/// the kernel-level keyboard path stays clean and pristine. Users who
/// need no-leak hotkeys should bind a system-level shortcut (e.g. GNOME
/// Custom Shortcuts → `xsay toggle`) or use F-keys which don't produce
/// text.
enum PostKey {
    None,
}

fn run_device_loop(
    mut device: Device,
    event_tx: Sender<AppEvent>,
    shared_config: Arc<Mutex<HotkeyConfig>>,
    capture_active: Arc<AtomicBool>,
    capture_slot: Arc<CaptureSlot>,
    recording: Arc<AtomicBool>,
    held_keys: Arc<Mutex<HashSet<u16>>>,
) {
    // Exponential backoff for transient fetch_events errors. Common
    // triggers: laptop suspend/resume (which releases the grab), USB
    // keyboard re-enumeration, or the kernel buffer momentarily
    // overflowing under heavy I/O. Breaking out of the loop the way we
    // used to meant a single hiccup silently killed xsay's hotkey
    // handling for the rest of the session ("按着按着快捷键就出不来
    // 了"). A short sleep + retry keeps us alive across those events;
    // if the device is genuinely gone we bail after enough failures.
    const MAX_CONSECUTIVE_ERRORS: u32 = 30;
    const BASE_BACKOFF_MS: u64 = 50;
    let mut consecutive_errors: u32 = 0;

    loop {
        match device.fetch_events() {
            Ok(events) => {
                consecutive_errors = 0;
                for ev in events {
                    if ev.event_type() == EventType::KEY {
                        let PostKey::None = handle_key(
                            &ev,
                            &event_tx,
                            &shared_config,
                            &capture_active,
                            &capture_slot,
                            &recording,
                            &held_keys,
                        );
                    }
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                let backoff =
                    BASE_BACKOFF_MS * (1u64 << consecutive_errors.min(8));
                log::warn!(
                    "evdev fetch_events error #{}: {} — sleeping {}ms and retrying",
                    consecutive_errors,
                    e,
                    backoff
                );
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    log::error!(
                        "evdev device gave {} consecutive errors; abandoning it",
                        consecutive_errors
                    );
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(backoff));
            }
        }
    }
}

fn handle_key(
    ev: &InputEvent,
    event_tx: &Sender<AppEvent>,
    shared_config: &Arc<Mutex<HotkeyConfig>>,
    capture_active: &Arc<AtomicBool>,
    capture_slot: &Arc<CaptureSlot>,
    recording: &Arc<AtomicBool>,
    held_keys: &Arc<Mutex<HashSet<u16>>>,
) -> PostKey {
    let code = ev.code();
    let value = ev.value();
    let pressed = value == 1;
    let released = value == 0;
    if !pressed && !released {
        return PostKey::None; // ignore auto-repeat (value == 2)
    }

    {
        let mut held = held_keys.lock();
        if pressed {
            held.insert(code);
        } else {
            held.remove(&code);
        }
    }

    // Capture mode: write to shared slot, don't fire normal hotkey logic.
    if capture_active.load(Ordering::SeqCst) {
        if pressed {
            record_capture_evdev(code, held_keys, capture_slot);
        }
        return PostKey::None;
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
        return PostKey::None;
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

    PostKey::None
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

fn evdev_code_to_name(code: u16) -> Option<&'static str> {
    let k = EvKey::new(code);
    Some(match k {
        k if k == EvKey::KEY_F1 => "F1",
        k if k == EvKey::KEY_F2 => "F2",
        k if k == EvKey::KEY_F3 => "F3",
        k if k == EvKey::KEY_F4 => "F4",
        k if k == EvKey::KEY_F5 => "F5",
        k if k == EvKey::KEY_F6 => "F6",
        k if k == EvKey::KEY_F7 => "F7",
        k if k == EvKey::KEY_F8 => "F8",
        k if k == EvKey::KEY_F9 => "F9",
        k if k == EvKey::KEY_F10 => "F10",
        k if k == EvKey::KEY_F11 => "F11",
        k if k == EvKey::KEY_F12 => "F12",
        k if k == EvKey::KEY_CAPSLOCK => "CapsLock",
        k if k == EvKey::KEY_SCROLLLOCK => "ScrollLock",
        k if k == EvKey::KEY_PAUSE => "Pause",
        k if k == EvKey::KEY_HOME => "Home",
        k if k == EvKey::KEY_END => "End",
        k if k == EvKey::KEY_PAGEUP => "PageUp",
        k if k == EvKey::KEY_PAGEDOWN => "PageDown",
        k if k == EvKey::KEY_DELETE => "Delete",
        k if k == EvKey::KEY_INSERT => "Insert",
        k if k == EvKey::KEY_TAB => "Tab",
        k if k == EvKey::KEY_SPACE => "Space",
        k if k == EvKey::KEY_ENTER => "Return",
        k if k == EvKey::KEY_SYSRQ => "PrintScreen",
        k if k == EvKey::KEY_NUMLOCK => "NumLock",
        k if k == EvKey::KEY_A => "a",
        k if k == EvKey::KEY_B => "b",
        k if k == EvKey::KEY_C => "c",
        k if k == EvKey::KEY_D => "d",
        k if k == EvKey::KEY_E => "e",
        k if k == EvKey::KEY_F => "f",
        k if k == EvKey::KEY_G => "g",
        k if k == EvKey::KEY_H => "h",
        k if k == EvKey::KEY_I => "i",
        k if k == EvKey::KEY_J => "j",
        k if k == EvKey::KEY_K => "k",
        k if k == EvKey::KEY_L => "l",
        k if k == EvKey::KEY_M => "m",
        k if k == EvKey::KEY_N => "n",
        k if k == EvKey::KEY_O => "o",
        k if k == EvKey::KEY_P => "p",
        k if k == EvKey::KEY_Q => "q",
        k if k == EvKey::KEY_R => "r",
        k if k == EvKey::KEY_S => "s",
        k if k == EvKey::KEY_T => "t",
        k if k == EvKey::KEY_U => "u",
        k if k == EvKey::KEY_V => "v",
        k if k == EvKey::KEY_W => "w",
        k if k == EvKey::KEY_X => "x",
        k if k == EvKey::KEY_Y => "y",
        k if k == EvKey::KEY_Z => "z",
        _ => return None,
    })
}

fn record_capture_evdev(
    code: u16,
    held_keys: &Arc<Mutex<HashSet<u16>>>,
    slot: &Arc<CaptureSlot>,
) {
    // Ignore bare modifier presses.
    let mods_codes = [
        EvKey::KEY_LEFTCTRL.code(),
        EvKey::KEY_RIGHTCTRL.code(),
        EvKey::KEY_LEFTSHIFT.code(),
        EvKey::KEY_RIGHTSHIFT.code(),
        EvKey::KEY_LEFTALT.code(),
        EvKey::KEY_RIGHTALT.code(),
        EvKey::KEY_LEFTMETA.code(),
        EvKey::KEY_RIGHTMETA.code(),
    ];
    if mods_codes.contains(&code) {
        return;
    }

    if code == EvKey::KEY_ESC.code() {
        *slot.latest.lock() = Some(("__cancel__".to_string(), Vec::new()));
        return;
    }

    let Some(name) = evdev_code_to_name(code) else {
        return;
    };

    let mut mods = Vec::new();
    let held = held_keys.lock();
    if held.contains(&EvKey::KEY_LEFTCTRL.code()) || held.contains(&EvKey::KEY_RIGHTCTRL.code()) {
        mods.push("ctrl".to_string());
    }
    if held.contains(&EvKey::KEY_LEFTALT.code()) || held.contains(&EvKey::KEY_RIGHTALT.code()) {
        mods.push("alt".to_string());
    }
    if held.contains(&EvKey::KEY_LEFTSHIFT.code()) || held.contains(&EvKey::KEY_RIGHTSHIFT.code()) {
        mods.push("shift".to_string());
    }
    if held.contains(&EvKey::KEY_LEFTMETA.code()) || held.contains(&EvKey::KEY_RIGHTMETA.code()) {
        mods.push("super".to_string());
    }
    drop(held);

    *slot.latest.lock() = Some((name.to_string(), mods));
}
