//! System-tray icon over the StatusNotifierItem D-Bus protocol (via the
//! pure-Rust [`ksni`] crate). Replaces the old `tray-icon` + GTK setup
//! that deadlocked on launch — `ksni` runs its event loop on its own
//! thread via zbus, with no GTK and no main-thread requirement, so it
//! can't race eframe's winit X11 connection the way GTK did.
//!
//! The protocol still requires the desktop to host an SNI implementation:
//!   * KDE / Plasma, Cinnamon, Xfce, MATE: native support, just works.
//!   * GNOME: needs the `gnome-shell-extension-appindicator` extension
//!     (Ubuntu ships it preinstalled). Without it the daemon still runs;
//!     the icon just won't display, but settings stays reachable via the
//!     overlay click and `xsay show` IPC.
//!
//! The old in-process action channel is preserved so callers (overlay,
//! ipc) don't need to change.

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

#[cfg(target_os = "linux")]
pub fn spawn_in_background() {
    std::thread::spawn(|| {
        use ksni::blocking::TrayMethods;
        match XsayTray.spawn() {
            Ok(handle) => {
                log::info!("Tray icon (StatusNotifierItem) ready");
                // Hold the handle for the lifetime of the daemon. Dropping
                // it tears the icon down; parking the worker keeps both
                // the handle and the ksni event loop alive.
                std::mem::forget(handle);
                std::thread::park();
            }
            Err(e) => {
                log::warn!(
                    "Tray icon unavailable: {} — GNOME needs the AppIndicator \
                     extension; settings is still reachable via overlay click \
                     or `xsay show`.",
                    e
                );
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_in_background() {
    // macOS / Windows tray would use a different stack (NSStatusItem /
    // Shell_NotifyIcon). Not wired up; the same overlay click + IPC
    // commands cover the daily flow on those platforms too.
}

#[cfg(target_os = "linux")]
struct XsayTray;

#[cfg(target_os = "linux")]
impl ksni::Tray for XsayTray {
    fn id(&self) -> String {
        // Stable D-Bus object path across launches so the panel can
        // remember position, etc.
        "xsay".into()
    }

    fn title(&self) -> String {
        "xsay 语音输入".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
            // assets/tray-32.rgba was hand-baked as 32×32 RGBA at build
            // time; ksni wants ARGB so we rotate each pixel one byte.
            const SIZE: u32 = 32;
            const RGBA: &[u8] = include_bytes!("../assets/tray-32.rgba");
            assert_eq!(RGBA.len(), (SIZE * SIZE * 4) as usize);
            let mut argb = RGBA.to_vec();
            for pixel in argb.chunks_exact_mut(4) {
                pixel.rotate_right(1);
            }
            ksni::Icon {
                width: SIZE as i32,
                height: SIZE as i32,
                data: argb,
            }
        });
        vec![ICON.clone()]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click on the tray icon: open settings, matching what
        // happens when the user clicks the overlay disk.
        request_show_settings();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "打开设置".into(),
                activate: Box::new(|_| request_show_settings()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出 xsay".into(),
                activate: Box::new(|_| request_quit()),
                ..Default::default()
            }
            .into(),
        ]
    }
}
