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

    // Still try Ctrl+V — works for XWayland apps (terminals, older GTK/Qt,
    // VS Code stable, etc.). Errors are now logged instead of swallowed so
    // Wayland-only failures surface in the log.
    match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => {
            if let Err(e) = enigo.key(Key::Control, Direction::Press) {
                log::warn!("enigo: Ctrl press failed ({}); likely a native Wayland window", e);
            }
            std::thread::sleep(Duration::from_millis(10));
            if let Err(e) = enigo.key(Key::Unicode('v'), Direction::Click) {
                log::warn!("enigo: V click failed ({}); likely a native Wayland window", e);
            }
            std::thread::sleep(Duration::from_millis(10));
            let _ = enigo.key(Key::Control, Direction::Release);
        }
        Err(e) => {
            log::warn!("Failed to create enigo (paste will rely on manual Ctrl+V): {}", e);
        }
    }

    if wayland {
        // Send paste keys directly via /dev/uinput. We keep a single
        // virtual keyboard alive for the lifetime of the process —
        // recreating one per paste (what `ydotool` does without its
        // daemon) has a 50-150ms setup window during which emitted events
        // are silently dropped. Alive-device has zero-latency emits.
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
        // Small delay then restore clipboard so the user's prior copy
        // (URL, etc.) isn't stomped.
        std::thread::sleep(Duration::from_millis(100));
        if let Some(prev) = prev_text {
            let _ = clipboard.set_text(prev);
        }
    }
}

/// Persistent /dev/uinput virtual keyboard for Wayland-native paste.
/// Created lazily on first use and kept alive for the process lifetime —
/// per-paste re-creation (as `ydotool` without its daemon does) has a
/// 50-150ms enumeration window during which emits are silently dropped,
/// which is why subprocess-based paste unreliably lands.
#[cfg(target_os = "linux")]
mod uinput_paste {
    use evdev::{
        uinput::{VirtualDevice, VirtualDeviceBuilder},
        AttributeSet, KeyCode, KeyEvent,
    };
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

    fn create() -> std::io::Result<VirtualDevice> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_LEFTCTRL);
        keys.insert(KeyCode::KEY_LEFTSHIFT);
        keys.insert(KeyCode::KEY_V);
        let dev = VirtualDeviceBuilder::new()?
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
