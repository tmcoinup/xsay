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

/// Tray icon is our brand logo rendered to 32×32 at build time and
/// included as raw RGBA. Previously we hand-drew a red circle + white
/// mic inline, which:
///   1. didn't match the application brand, and
///   2. showed a persistent red dot that users mistook for a recording
///      indicator (stacking with the overlay's actual recording badge
///      gave the impression of two red dots on screen).
/// The brand PNG itself has a transparent background so GNOME/KDE
/// compositors blend it cleanly with any panel color.
fn make_icon() -> Icon {
    const SIZE: u32 = 32;
    const RGBA: &[u8] = include_bytes!("../assets/tray-32.rgba");
    debug_assert_eq!(RGBA.len(), (SIZE * SIZE * 4) as usize);
    Icon::from_rgba(RGBA.to_vec(), SIZE, SIZE).expect("failed to build tray icon")
}
