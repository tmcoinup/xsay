//! In-process action channel between the overlay / IPC server / settings UI
//! and the eframe `App::ui()` loop.
//!
//! Originally this module also drew a system-tray icon via the `tray-icon`
//! crate. On Linux that path required spawning a thread that called
//! `gtk::init()` and then `gtk::main()` — both of which GTK explicitly
//! requires to run on the *main* thread. With eframe's winit already
//! holding the main thread, the GTK thread and winit's X11 connection
//! raced for X global locks and intermittently deadlocked the process
//! before either viewport or tray icon could come up. Symptom from the
//! user side was xsay's daemon staying alive but no overlay window ever
//! mapping and the desktop's GNOME shell hanging until xsay was killed.
//!
//! The fix was to drop the tray icon entirely. Settings is opened by
//! clicking the overlay or sending `xsay show`. Quit is exposed via the
//! settings dialog's quit button and the `xsay quit` IPC command — see
//! settings_ui::render and ipc::dispatch.
//!
//! The public API of this module is kept stable so callers don't have to
//! change: `request_show_settings` / `request_quit` push into the same
//! channel, `poll_events` drains it from inside `App::ui()`.

use std::sync::{LazyLock, Mutex, mpsc};

pub enum TrayAction {
    ShowSettings,
    Quit,
}

static ACTIONS: LazyLock<(mpsc::Sender<TrayAction>, Mutex<mpsc::Receiver<TrayAction>>)> =
    LazyLock::new(|| {
        let (tx, rx) = mpsc::channel();
        (tx, Mutex::new(rx))
    });

pub fn request_show_settings() {
    let _ = ACTIONS.0.send(TrayAction::ShowSettings);
}

pub fn request_quit() {
    let _ = ACTIONS.0.send(TrayAction::Quit);
}

/// Drain pending actions; called once per frame from inside `App::ui()`.
pub fn poll_events() -> Vec<TrayAction> {
    let mut actions = Vec::new();
    let rx = ACTIONS.1.lock().expect("tray action receiver poisoned");
    while let Ok(action) = rx.try_recv() {
        actions.push(action);
    }
    actions
}

/// Kept as a no-op so callers (main.rs) don't need conditional compilation.
/// Previously this spawned a GTK main loop for the tray icon — that path
/// is gone (see module-level comment).
pub fn spawn_in_background() {}
