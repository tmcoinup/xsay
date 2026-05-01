//! Common types shared by all global-hotkey backends (evdev on Linux,
//! rdev on macOS / Windows). The backend implementations live in
//! `hotkey_evdev` / `hotkey_rdev` and are picked at compile time so we
//! don't pay for X11/XRecord on Linux at all.

use parking_lot::Mutex;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Debug, Clone)]
pub enum AppEvent {
    HotkeyPressed,
    HotkeyReleased,
    EscapePressed,
}

/// Writable slot used by the settings UI to capture the next pressed key.
///
/// When `active = true`, the active hotkey backend (evdev / rdev) writes
/// the next key press into `latest` *instead of* firing the normal recording
/// logic. The UI polls `latest` each frame and applies it.
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

/// Which backend ended up running, surfaced to the settings UI for the
/// status banner. Variants are kept platform-agnostic on purpose — UI
/// branches on the variant, not on the OS.
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    /// No in-process global listener. Recording is triggered through IPC
    /// (`xsay toggle` / `press` / `release`) bound to a desktop shortcut.
    SystemShortcutOnly,
    /// Linux: direct /dev/input/event* monitoring via evdev. Works under
    /// both X11 and Wayland and bypasses the X server entirely, so it
    /// can't deadlock with mutter the way XRecord-based backends do.
    Evdev { devices: usize },
    /// Linux: evdev was tried but failed (typically because the user
    /// isn't in the `input` group). The daemon falls back to
    /// `SystemShortcutOnly` at runtime; this variant exists so the
    /// settings UI can render the precise error.
    EvdevUnavailable { reason: String },
    /// macOS / Windows: rdev's native key-tap APIs. Not used on Linux —
    /// rdev's Linux path goes through XRecord, which we explicitly
    /// dropped because it caused mutter freezes. The variant is kept on
    /// every platform so the settings UI's match stays exhaustive.
    #[allow(dead_code)]
    Rdev,
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
