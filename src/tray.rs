use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

const ID_SHOW_SETTINGS: &str = "xsay.show_settings";
const ID_QUIT: &str = "xsay.quit";

pub enum TrayAction {
    ShowSettings,
    Quit,
}

/// Holds the tray so it stays alive; drop this to remove the icon.
pub struct TrayHandle {
    #[allow(dead_code)]
    tray: TrayIcon,
}

/// Spawn the tray on a dedicated GTK thread (Linux requirement).
/// Returns once the tray has attempted to initialize; failures are logged.
#[cfg(target_os = "linux")]
pub fn spawn_in_background() {
    std::thread::spawn(|| {
        if let Err(e) = gtk::init() {
            log::warn!("Tray disabled: GTK init failed: {:?}", e);
            return;
        }
        let _handle = match build_inner() {
            Ok(h) => h,
            Err(e) => {
                log::warn!("Tray disabled: {}", e);
                eprintln!(
                    "⚠ 系统托盘不可用：{}。GNOME 需要 AppIndicator 扩展才能看到图标。",
                    e
                );
                return;
            }
        };
        log::info!("Tray icon ready");
        // Run the GTK main loop so menu clicks get dispatched.
        // This blocks the thread until the process exits.
        gtk::main();
    });
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_in_background() {
    // On macOS/Windows, tray can be built from any thread.
    std::thread::spawn(|| match build_inner() {
        Ok(_handle) => {
            log::info!("Tray icon ready");
            // Keep the thread alive so the handle is not dropped.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
        Err(e) => log::warn!("Tray disabled: {}", e),
    });
}

fn build_inner() -> Result<TrayHandle, String> {
    let menu = Menu::new();
    let show_item = MenuItem::with_id(
        MenuId::new(ID_SHOW_SETTINGS),
        "⚙  打开设置",
        true,
        None,
    );
    let quit_item = MenuItem::with_id(MenuId::new(ID_QUIT), "退出 xsay", true, None);

    menu.append(&show_item).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&quit_item).map_err(|e| e.to_string())?;

    let icon = make_icon();

    let tray = TrayIconBuilder::new()
        .with_tooltip("xsay 语音输入")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(TrayHandle { tray })
}

/// Drain menu events; returns actions the app should take this frame.
pub fn poll_events() -> Vec<TrayAction> {
    let mut actions = Vec::new();
    while let Ok(ev) = MenuEvent::receiver().try_recv() {
        let id = ev.id.as_ref();
        if id == ID_SHOW_SETTINGS {
            actions.push(TrayAction::ShowSettings);
        } else if id == ID_QUIT {
            actions.push(TrayAction::Quit);
        }
    }
    actions
}

fn make_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    // Draw a simple microphone icon: red circle + white mic body + stand
    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = ((y * SIZE + x) * 4) as usize;
            let dx = x as i32 - 16;
            let dy = y as i32 - 14;
            let r2 = dx * dx + dy * dy;

            let in_circle = r2 <= 121; // radius ~11
            let in_mic = dx.abs() <= 3 && dy >= -8 && dy <= 3;
            let in_stand = dx == 0 && dy >= 3 && dy <= 9;
            let in_base = dy == 10 && dx.abs() <= 5;

            if in_mic || in_stand || in_base {
                // White foreground
                rgba[idx] = 255;
                rgba[idx + 1] = 255;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = 255;
            } else if in_circle {
                // Red background
                rgba[idx] = 200;
                rgba[idx + 1] = 50;
                rgba[idx + 2] = 50;
                rgba[idx + 3] = 230;
            }
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("failed to build tray icon")
}
