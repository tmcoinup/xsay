//! 历史记录标签页：展示 ~/.cache/xsay/history.jsonl 的最近条目。

use super::SettingsState;
use crate::theme::{self, Icon};
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    // Reload cache only when dirty (first entry, after clear, tab re-entry).
    // Reading the JSONL file every frame was blocking the UI thread under
    // Whisper CPU load and causing "not responding" dialogs.
    if state.history_dirty {
        state.history_cache = crate::history::load_recent(200);
        state.history_dirty = false;
    }
    let entries_len = state.history_cache.len();

    // Header row: title on the left, refresh + clear-all actions on the right.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("最近 {} 条识别结果", entries_len))
                .color(crate::theme::TEXT_PRIMARY)
                .strong()
                .size(crate::theme::FONT_HEADING),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = entries_len > 0;
            let color = if enabled {
                crate::theme::DANGER_HOVER
            } else {
                crate::theme::TEXT_DISABLED
            };
            let resp = theme::icon_link_button(ui, Icon::Trash, "清空", color);
            if enabled && resp.clicked() {
                if let Err(e) = crate::history::clear() {
                    state.set_status(
                        format!("清空失败：{}", e),
                        crate::theme::DANGER_HOVER,
                    );
                } else {
                    state.set_status("历史已清空", crate::theme::SUCCESS);
                }
                state.history_dirty = true;
            }
            if theme::icon_link_button(ui, Icon::Refresh, "刷新", crate::theme::ACCENT)
                .clicked()
            {
                state.history_dirty = true;
            }
        });
    });
    ui.add_space(10.0);

    if entries_len == 0 {
        // Empty state inside a card so it doesn't look like a forgotten page.
        egui::Frame::new()
            .fill(crate::theme::BG_CARD)
            .corner_radius(crate::theme::radius_lg())
            .inner_margin(egui::Margin::symmetric(16, 20))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("暂无历史记录")
                            .color(crate::theme::TEXT_PRIMARY)
                            .size(crate::theme::FONT_BODY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("识别出的文本会自动保存到这里")
                            .color(crate::theme::TEXT_SECONDARY)
                            .size(crate::theme::FONT_SM),
                    );
                });
            });
        return;
    }

    // Render the cache by index so we don't need to clone the whole vec
    // per frame. Copy clicks are rare, so the per-click `text.clone()` is
    // fine; we can't call `state.set_status` inside the loop though (would
    // alias the cache borrow), so defer it.
    let mut copied: Option<String> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        for i in 0..entries_len {
            let entry = &state.history_cache[i];
            if let Some(text) = render_entry_card(ui, entry) {
                copied = Some(text);
            }
        }
    });
    if let Some(text) = copied {
        ui.ctx().copy_text(text);
        state.set_status("已复制到剪贴板", crate::theme::SUCCESS);
    }

    if let Some((msg, color, _)) = &state.status_msg {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(msg)
                .color(*color)
                .size(crate::theme::FONT_SM),
        );
    }
}

/// Returns `Some(text)` if the user clicked Copy on this entry; caller
/// applies the clipboard + status effect outside the cache borrow.
fn render_entry_card(
    ui: &mut egui::Ui,
    entry: &crate::history::HistoryEntry,
) -> Option<String> {
    let mut copied: Option<String> = None;
    egui::Frame::new()
        .fill(crate::theme::BG_CARD)
        .corner_radius(crate::theme::radius_lg())
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(crate::history::format_timestamp(entry.timestamp))
                        .color(crate::theme::TEXT_SECONDARY)
                        .monospace()
                        .size(crate::theme::FONT_SM),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::icon_link_button(ui, Icon::Check, "复制", crate::theme::ACCENT)
                        .clicked()
                    {
                        copied = Some(entry.text.clone());
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&entry.text)
                    .color(crate::theme::TEXT_PRIMARY)
                    .size(crate::theme::FONT_BODY),
            );
        });
    copied
}
