//! Install a CJK font into egui so Chinese text renders instead of tofu.
//!
//! We load a system font at runtime (keeps the binary small). If no CJK font
//! is found, we log a warning but don't fail — the app still works with the
//! default Latin font, Chinese just won't render.

use eframe::egui;
use std::path::{Path, PathBuf};

/// Call from inside the eframe creator closure to register a CJK font family.
pub fn install(ctx: &egui::Context) {
    let Some((path, index)) = find_system_cjk_font() else {
        log::warn!("No CJK font found on system; Chinese text may show as ▯");
        return;
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Failed to read font {}: {}", path.display(), e);
            return;
        }
    };

    let mut fonts = egui::FontDefinitions::default();

    let font_data = egui::FontData {
        font: std::borrow::Cow::Owned(bytes),
        index,
        tweak: egui::FontTweak::default(),
    };
    fonts.font_data.insert("cjk".to_owned(), font_data);

    // Insert CJK as fallback behind the Latin default — default font handles
    // ASCII nicely, CJK picks up everything else. Proportional first position
    // would mean CJK style for ASCII too, which looks worse.
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.push("cjk".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("cjk".to_owned());
    }

    ctx.set_fonts(fonts);
    log::info!(
        "Loaded CJK font: {} (index {})",
        path.display(),
        index
    );
}

/// Returns (path, ttc_face_index). Face index is 0 for .ttf/.otf, or a specific
/// face inside a .ttc collection (e.g. Noto Sans CJK-Regular.ttc index 2 = SC).
#[cfg(target_os = "linux")]
fn find_system_cjk_font() -> Option<(PathBuf, u32)> {
    // .ttc files: pick the SC (Simplified Chinese) face when available.
    // Noto Sans CJK-Regular.ttc ordering: 0=JP, 1=KR, 2=SC, 3=TC, 4=HK
    let ttc_candidates: &[&str] = &[
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
    ];
    for candidate in ttc_candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some((p, 2)); // SC
        }
    }

    // wqy is a .ttc but indexing differs; index 0 (wqy-microhei) is fine for zh.
    let wqy_ttc: &[&str] = &[
        "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/wqy-zenhei/wqy-zenhei.ttc",
    ];
    for candidate in wqy_ttc {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some((p, 0));
        }
    }

    // Single-face files
    let single: &[&str] = &[
        "/usr/share/fonts/truetype/arphic/uming.ttc",
        "/usr/share/fonts/opentype/source-han-sans/SourceHanSansSC-Regular.otf",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    for candidate in single {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some((p, 0));
        }
    }

    // Last resort: fontconfig via fc-match command
    fc_match_cjk()
}

#[cfg(target_os = "macos")]
fn find_system_cjk_font() -> Option<(PathBuf, u32)> {
    let candidates: &[(&str, u32)] = &[
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
        ("/System/Library/Fonts/STHeiti Light.ttc", 0),
        ("/Library/Fonts/Songti.ttc", 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ];
    for (path, idx) in candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some((p, *idx));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn find_system_cjk_font() -> Option<(PathBuf, u32)> {
    let windir = std::env::var_os("WINDIR")?;
    let fonts_dir = Path::new(&windir).join("Fonts");
    let candidates: &[(&str, u32)] = &[
        ("msyh.ttc", 0),   // Microsoft YaHei
        ("msyh.ttf", 0),
        ("simsun.ttc", 0), // SimSun
        ("simsun.ttf", 0),
        ("simhei.ttf", 0),
        ("Deng.ttf", 0),
    ];
    for (name, idx) in candidates {
        let p = fonts_dir.join(name);
        if p.exists() {
            return Some((p, *idx));
        }
    }
    None
}

/// Ask fontconfig for any installed font that covers CJK. Runs `fc-match`
/// synchronously; only used when hard-coded paths miss.
#[cfg(target_os = "linux")]
fn fc_match_cjk() -> Option<(PathBuf, u32)> {
    let out = std::process::Command::new("fc-match")
        .arg("-f")
        .arg("%{file}")
        .arg("sans:lang=zh")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let p = PathBuf::from(path.trim());
    if p.exists() {
        // fc-match gives a specific face; index 0 is a safe default.
        Some((p, 0))
    } else {
        None
    }
}

// Suppress unused-import warning on non-linux builds where the helper isn't compiled.
#[allow(dead_code)]
fn _path_unused(_: &Path) {}
