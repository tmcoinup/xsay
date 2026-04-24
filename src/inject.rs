use crate::config::InjectionConfig;
use arboard::Clipboard;
use crossbeam_channel::{Receiver, Sender};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub enum InjectCmd {
    Type(String),
}

/// True if we're running under a Wayland session. enigo's Ctrl+V synthesis
/// routes through X11 XTEST, which reaches XWayland apps but NOT native
/// Wayland apps — the paste keystroke silently disappears. In that case we
/// rely on the user pressing Ctrl+V manually and keep the clipboard intact.
fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE")
            .map(|s| s == "wayland")
            .unwrap_or(false)
}

pub fn run_inject_thread(
    inject_rx: Receiver<InjectCmd>,
    done_tx: Sender<()>,
    shared_config: Arc<Mutex<InjectionConfig>>,
) {
    loop {
        let cmd = match inject_rx.recv() {
            Ok(c) => c,
            Err(_) => break,
        };

        match cmd {
            InjectCmd::Type(text) => {
                if text.is_empty() {
                    let _ = done_tx.send(());
                    continue;
                }
                log::debug!("Injecting text: {:?}", text);

                let cfg = shared_config.lock().clone();
                match cfg.method.as_str() {
                    "type" => inject_via_keystrokes(&text),
                    _ => inject_via_clipboard(&text, cfg.clipboard_delay_ms, &cfg.paste_shortcut),
                }

                let _ = done_tx.send(());
            }
        }
    }
}

fn inject_via_clipboard(text: &str, delay_ms: u64, paste_shortcut: &str) {
    // Save current clipboard content to restore later (only on X11 — on
    // Wayland the synthetic Ctrl+V doesn't reach native apps, so restoring
    // would wipe the transcription from the clipboard before the user can
    // paste it manually).
    let wayland = is_wayland();
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to open clipboard: {}", e);
            return;
        }
    };

    let prev_text = if wayland { None } else { clipboard.get_text().ok() };

    if let Err(e) = clipboard.set_text(text) {
        log::error!("Failed to set clipboard: {}", e);
        return;
    }

    // Brief delay so clipboard contents settle
    std::thread::sleep(Duration::from_millis(delay_ms));

    if wayland {
        // Wayland-only path: skip enigo (X11 XTEST) entirely and go
        // straight through /dev/uinput. Sending both would double-paste
        // in XWayland apps (terminals) because BOTH the X11 synthetic
        // keypress AND the uinput kernel-level keypress get delivered
        // to the focused app — that's where "启动识别会重复" was coming
        // from. uinput reaches every app regardless of protocol.
        let pasted = uinput_paste::send_paste(paste_shortcut);
        if !pasted {
            let preview = preview_for_notification(text);
            notify(
                "xsay 识别完成（请按 Ctrl+V 粘贴）",
                &format!("{}\n自动粘贴失败（见 /tmp/xsay.log）", preview),
            );
        }
        // Leave the clipboard set to the transcription — do not restore,
        // so manual Ctrl+V still works even if uinput succeeded.
    } else {
        // X11 path: enigo's XTEST synthesis is the natural choice (no
        // uinput permission dependency, instantaneous). Errors are
        // logged rather than swallowed so unusual X11 setups surface.
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => {
                if let Err(e) = enigo.key(Key::Control, Direction::Press) {
                    log::warn!("enigo: Ctrl press failed ({})", e);
                }
                std::thread::sleep(Duration::from_millis(10));
                if let Err(e) = enigo.key(Key::Unicode('v'), Direction::Click) {
                    log::warn!("enigo: V click failed ({})", e);
                }
                std::thread::sleep(Duration::from_millis(10));
                let _ = enigo.key(Key::Control, Direction::Release);
            }
            Err(e) => {
                log::warn!(
                    "Failed to create enigo (paste will rely on manual Ctrl+V): {}",
                    e
                );
            }
        }

        // X11-only clipboard restore. On Wayland we intentionally leave
        // the transcription on the clipboard so manual Ctrl+V still
        // works if auto-paste failed.
        std::thread::sleep(Duration::from_millis(100));
        if let Some(prev) = prev_text {
            let _ = clipboard.set_text(prev);
        }
    }
}

/// Persistent /dev/uinput virtual keyboard for Wayland-native paste AND
/// for synthesizing hotkey-release events after hotkey_evdev grabs a
/// physical keyboard (see hotkey_evdev::run_device_loop). Created lazily
/// on first use and kept alive for the process lifetime — per-use
/// re-creation (as `ydotool` without its daemon does) has a 50-150ms
/// enumeration window during which emits are silently dropped, which
/// is why subprocess-based paste unreliably lands.
///
/// Module is pub(crate) so hotkey_evdev can call send_release() to
/// cancel the compositor's held-key state when we EVIOCGRAB a device
/// mid-chord.
#[cfg(target_os = "linux")]
pub(crate) mod uinput_paste {
    use evdev::{AttributeSet, KeyCode, KeyEvent, uinput::VirtualDevice};
    use parking_lot::Mutex;
    use std::sync::LazyLock;

    static DEVICE: LazyLock<Mutex<Option<VirtualDevice>>> = LazyLock::new(|| {
        let dev = match create() {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!(
                    "uinput virtual keyboard unavailable ({}); auto-paste \
                     disabled, user must Ctrl+V manually. Check /dev/uinput \
                     permissions + input group membership.",
                    e
                );
                None
            }
        };
        Mutex::new(dev)
    });

    /// Register a broad key set so the device can synthesize release
    /// events for arbitrary user-chosen hotkeys (modifiers + A-Z +
    /// 0-9 + F1-F12 + common control keys). A uinput device can only
    /// emit keys declared at build time; anything not in this set
    /// would silently no-op when released via send_release.
    fn register_keys() -> AttributeSet<KeyCode> {
        let mut keys = AttributeSet::<KeyCode>::new();
        // Paste shortcut keys
        keys.insert(KeyCode::KEY_LEFTCTRL);
        keys.insert(KeyCode::KEY_RIGHTCTRL);
        keys.insert(KeyCode::KEY_LEFTSHIFT);
        keys.insert(KeyCode::KEY_RIGHTSHIFT);
        keys.insert(KeyCode::KEY_LEFTALT);
        keys.insert(KeyCode::KEY_RIGHTALT);
        keys.insert(KeyCode::KEY_LEFTMETA);
        keys.insert(KeyCode::KEY_RIGHTMETA);
        // Letters A-Z
        for c in b'A'..=b'Z' {
            if let Some(k) = letter_to_key(c as char) {
                keys.insert(k);
            }
        }
        // Digits 0-9
        keys.insert(KeyCode::KEY_0);
        keys.insert(KeyCode::KEY_1);
        keys.insert(KeyCode::KEY_2);
        keys.insert(KeyCode::KEY_3);
        keys.insert(KeyCode::KEY_4);
        keys.insert(KeyCode::KEY_5);
        keys.insert(KeyCode::KEY_6);
        keys.insert(KeyCode::KEY_7);
        keys.insert(KeyCode::KEY_8);
        keys.insert(KeyCode::KEY_9);
        // Function keys
        keys.insert(KeyCode::KEY_F1);
        keys.insert(KeyCode::KEY_F2);
        keys.insert(KeyCode::KEY_F3);
        keys.insert(KeyCode::KEY_F4);
        keys.insert(KeyCode::KEY_F5);
        keys.insert(KeyCode::KEY_F6);
        keys.insert(KeyCode::KEY_F7);
        keys.insert(KeyCode::KEY_F8);
        keys.insert(KeyCode::KEY_F9);
        keys.insert(KeyCode::KEY_F10);
        keys.insert(KeyCode::KEY_F11);
        keys.insert(KeyCode::KEY_F12);
        // Common non-letter hotkey keys
        keys.insert(KeyCode::KEY_SPACE);
        keys.insert(KeyCode::KEY_ENTER);
        keys.insert(KeyCode::KEY_ESC);
        keys.insert(KeyCode::KEY_TAB);
        keys.insert(KeyCode::KEY_CAPSLOCK);
        keys.insert(KeyCode::KEY_HOME);
        keys.insert(KeyCode::KEY_END);
        keys.insert(KeyCode::KEY_PAGEUP);
        keys.insert(KeyCode::KEY_PAGEDOWN);
        keys.insert(KeyCode::KEY_INSERT);
        keys.insert(KeyCode::KEY_DELETE);
        keys
    }

    fn letter_to_key(c: char) -> Option<KeyCode> {
        match c {
            'A' => Some(KeyCode::KEY_A),
            'B' => Some(KeyCode::KEY_B),
            'C' => Some(KeyCode::KEY_C),
            'D' => Some(KeyCode::KEY_D),
            'E' => Some(KeyCode::KEY_E),
            'F' => Some(KeyCode::KEY_F),
            'G' => Some(KeyCode::KEY_G),
            'H' => Some(KeyCode::KEY_H),
            'I' => Some(KeyCode::KEY_I),
            'J' => Some(KeyCode::KEY_J),
            'K' => Some(KeyCode::KEY_K),
            'L' => Some(KeyCode::KEY_L),
            'M' => Some(KeyCode::KEY_M),
            'N' => Some(KeyCode::KEY_N),
            'O' => Some(KeyCode::KEY_O),
            'P' => Some(KeyCode::KEY_P),
            'Q' => Some(KeyCode::KEY_Q),
            'R' => Some(KeyCode::KEY_R),
            'S' => Some(KeyCode::KEY_S),
            'T' => Some(KeyCode::KEY_T),
            'U' => Some(KeyCode::KEY_U),
            'V' => Some(KeyCode::KEY_V),
            'W' => Some(KeyCode::KEY_W),
            'X' => Some(KeyCode::KEY_X),
            'Y' => Some(KeyCode::KEY_Y),
            'Z' => Some(KeyCode::KEY_Z),
            _ => None,
        }
    }

    fn create() -> std::io::Result<VirtualDevice> {
        let keys = register_keys();
        let dev = VirtualDevice::builder()?
            .name("xsay-virtual-kbd")
            .with_keys(&keys)?
            .build()?;
        // Udev + libinput + compositor + xserver enumerate new devices
        // asynchronously. Sleep once here on creation so the first
        // send_paste() below isn't racing device registration.
        std::thread::sleep(std::time::Duration::from_millis(200));
        log::info!("uinput virtual keyboard created for auto-paste");
        Ok(dev)
    }

    /// Emit KEY_UP events for a set of keycodes. Used by hotkey_evdev
    /// right after it grabs a physical keyboard — the compositor has
    /// already seen the physical KEY_DOWN that triggered our hotkey,
    /// so without a matching KEY_UP it auto-repeats (typing "xxxxx"
    /// into the focused app for as long as the hotkey is held). A
    /// synthesized release via uinput looks to the compositor like the
    /// user let go, stopping the repeat.
    pub fn send_release(codes: &[evdev::KeyCode]) {
        let mut guard = DEVICE.lock();
        if guard.is_none() {
            *guard = create().ok();
        }
        let Some(dev) = guard.as_mut() else {
            return;
        };
        let events: Vec<_> = codes
            .iter()
            .map(|&k| *KeyEvent::new(k, 0))
            .collect();
        if let Err(e) = dev.emit(&events) {
            log::warn!("uinput: synthetic release failed: {}", e);
        }
    }

    /// Emit a paste key sequence. `shortcut` picks the modifier set:
    ///   "ctrl-v"       → Ctrl+V        (GUI text fields)
    ///   "ctrl-shift-v" → Ctrl+Shift+V  (terminals / CLI)
    ///   "both"         → Ctrl+V then Ctrl+Shift+V, 20ms apart
    pub fn send_paste(shortcut: &str) -> bool {
        let mut guard = DEVICE.lock();
        if guard.is_none() {
            // Retry creation — permission issues at startup may have
            // resolved (e.g. user logged out and back in for input group).
            *guard = create().ok();
        }
        let Some(dev) = guard.as_mut() else {
            return false;
        };
        let (a, b) = split_shortcut(shortcut);
        let ok_a = emit(dev, a);
        if let Some(second) = b {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let ok_b = emit(dev, second);
            return ok_a || ok_b;
        }
        ok_a
    }

    fn split_shortcut(s: &str) -> (&'static [(KeyCode, i32)], Option<&'static [(KeyCode, i32)]>) {
        match s {
            "ctrl-shift-v" => (CTRL_SHIFT_V, None),
            "both" => (CTRL_V, Some(CTRL_SHIFT_V)),
            _ => (CTRL_V, None),
        }
    }

    const CTRL_V: &[(KeyCode, i32)] = &[
        (KeyCode::KEY_LEFTCTRL, 1),
        (KeyCode::KEY_V, 1),
        (KeyCode::KEY_V, 0),
        (KeyCode::KEY_LEFTCTRL, 0),
    ];
    const CTRL_SHIFT_V: &[(KeyCode, i32)] = &[
        (KeyCode::KEY_LEFTCTRL, 1),
        (KeyCode::KEY_LEFTSHIFT, 1),
        (KeyCode::KEY_V, 1),
        (KeyCode::KEY_V, 0),
        (KeyCode::KEY_LEFTSHIFT, 0),
        (KeyCode::KEY_LEFTCTRL, 0),
    ];

    fn emit(dev: &mut VirtualDevice, seq: &[(KeyCode, i32)]) -> bool {
        let events: Vec<_> = seq
            .iter()
            .map(|&(k, v)| *KeyEvent::new(k, v))
            .collect();
        match dev.emit(&events) {
            Ok(()) => {
                log::debug!("uinput: {} keys sent", events.len());
                true
            }
            Err(e) => {
                log::warn!("uinput emit failed: {}", e);
                false
            }
        }
    }
}

// Non-Linux stub so the module path compiles everywhere. Auto-paste
// outside Linux goes through enigo (Ctrl+V) on its native paths.
#[cfg(not(target_os = "linux"))]
mod uinput_paste {
    pub fn send_paste(_shortcut: &str) -> bool {
        false
    }
}

fn inject_via_keystrokes(text: &str) {
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to create enigo: {}", e);
            return;
        }
    };

    if let Err(e) = enigo.text(text) {
        log::error!("Failed to type text: {}", e);
        // Fall back to clipboard with the "both" policy so the user gets
        // auto-paste in whatever target they're focused on.
        inject_via_clipboard(text, 80, "both");
    }
}

/// Shell out to `notify-send`. Done via subprocess so we don't pull in a
/// dbus dependency for this single notification path. Failures are
/// non-fatal — worst case the clipboard is set but the user doesn't see a
/// toast; they can still check the overlay / history.
fn notify(title: &str, body: &str) {
    let result = std::process::Command::new("notify-send")
        .arg("-a")
        .arg("xsay")
        .arg("-t")
        .arg("4000")
        .arg(title)
        .arg(body)
        .status();
    match result {
        Ok(s) if s.success() => {}
        Ok(s) => log::debug!("notify-send exited {}", s),
        Err(e) => log::debug!(
            "notify-send unavailable ({}); install libnotify-bin for paste toasts",
            e
        ),
    }
}

/// Truncate long transcripts for the notification body. Notifications have
/// limited screen real estate and a 500-char line would wrap forever.
fn preview_for_notification(text: &str) -> String {
    const MAX: usize = 80;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX).collect();
    out.push('…');
    out
}
