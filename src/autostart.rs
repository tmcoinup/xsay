//! Cross-platform autostart management.
//!
//! Linux: `~/.config/autostart/xsay.desktop`
//! macOS: `~/Library/LaunchAgents/com.xsay.plist`
//! Windows: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\xsay.bat`

use std::path::PathBuf;

pub fn is_enabled() -> bool {
    autostart_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn enable() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法获取自身路径: {}", e))?;
    let path = autostart_path().ok_or_else(|| "无法定位自启动目录".to_string())?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let content = render_entry(&exe);
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

pub fn disable() -> Result<(), String> {
    let path = autostart_path().ok_or_else(|| "无法定位自启动目录".to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("删除失败: {}", e))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn autostart_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join("xsay.desktop"))
}

#[cfg(target_os = "macos")]
fn autostart_path() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join("Library")
            .join("LaunchAgents")
            .join("com.xsay.plist"),
    )
}

#[cfg(target_os = "windows")]
fn autostart_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("xsay.bat"),
    )
}

#[cfg(target_os = "linux")]
fn render_entry(exe: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=xsay\n\
         Comment=AI Voice Input Tool\n\
         Exec={}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         Categories=Utility;\n",
        exe.display()
    )
}

#[cfg(target_os = "macos")]
fn render_entry(exe: &std::path::Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key><string>com.xsay</string>\n\
           <key>ProgramArguments</key><array><string>{}</string></array>\n\
           <key>RunAtLoad</key><true/>\n\
           <key>KeepAlive</key><false/>\n\
         </dict>\n\
         </plist>\n",
        exe.display()
    )
}

#[cfg(target_os = "windows")]
fn render_entry(exe: &std::path::Path) -> String {
    format!("@echo off\r\nstart \"\" \"{}\"\r\n", exe.display())
}
