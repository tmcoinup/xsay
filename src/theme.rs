//! Design tokens extracted from the Figma Make reference (bundle hash
//! `965fafa0c4522141713b07c7f5f25d79349fedc9`). Keep this the single
//! source of truth — do not hard-code colors/radii in rendering code.

use eframe::egui::{Color32, Rounding};

/// Deepest page background (behind the settings window)
pub const BG_DEEPEST: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x12);
/// Panel / tab bar background
pub const BG_PANEL: Color32 = Color32::from_rgb(0x14, 0x14, 0x1A);
/// Settings window / card container
pub const BG_WINDOW: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x22);
/// Default card background
pub const BG_CARD: Color32 = Color32::from_rgb(0x26, 0x26, 0x2B);
/// Card hover / slightly brighter
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2E);
/// "Current" / selected card (green tint)
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x1E, 0x3C, 0x1E);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xEC);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA8);
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x60, 0x60, 0x68);

/// Primary accent (active tab, radio fill, links)
pub const ACCENT: Color32 = Color32::from_rgb(0x3D, 0xA5, 0xFF);
/// Success (status messages, OK toast)
pub const SUCCESS: Color32 = Color32::from_rgb(0x50, 0xDC, 0x50);
/// "当前使用" chip color
pub const CURRENT: Color32 = Color32::from_rgb(0x3A, 0x9A, 0x3A);

/// Recording indicator red
pub const REC: Color32 = Color32::from_rgb(0xC8, 0x32, 0x32);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x40, 0x40);
pub const DANGER_HOVER: Color32 = Color32::from_rgb(0xFF, 0x60, 0x60);
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xB4, 0x3C);

// --- Corner radii ---

pub fn radius_xs() -> Rounding {
    Rounding::same(2.0)
}
pub fn radius_sm() -> Rounding {
    Rounding::same(4.0)
}
pub fn radius_md() -> Rounding {
    Rounding::same(6.0)
}
/// Cards, frames
pub fn radius_lg() -> Rounding {
    Rounding::same(8.0)
}
/// Windows
pub fn radius_xl() -> Rounding {
    Rounding::same(10.0)
}
/// Hero elements
pub fn radius_xxl() -> Rounding {
    Rounding::same(12.0)
}

// --- Font sizes ---

pub const FONT_XS: f32 = 10.0;
pub const FONT_SM: f32 = 11.0;
pub const FONT_MD: f32 = 12.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_HEADING: f32 = 14.0;
pub const FONT_HERO: f32 = 20.0;
pub const FONT_H1: f32 = 28.0;
