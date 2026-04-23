//! 历史记录标签页：展示 ~/.cache/xsay/history.jsonl 的最近条目。

use super::SettingsState;
use crate::theme::{self, Icon};
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    let entries = crate::history::load_recent(200);

    // Header row: title on the left, clear-all action on the right.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("最近 {} 条识别结果", entries.len()))
                .color(crate::theme::TEXT_PRIMARY)
                .strong()
                .size(crate::theme::FONT_HEADING),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = !entries.is_empty();
            let color = if enabled {
                crate::theme::DANGER_HOVER
            } else {
                crate::theme::TEXT_DISABLED
            };
            let resp = theme::icon_link_button(ui, Icon::Trash, "清空", color);
            if enabled && resp.clicked() {
                if let Err(e) = crate::history::clear() {
                    state.status_msg = Some((
                        format!("清空失败：{}", e),
                        crate::theme::DANGER_HOVER,
                    ));
                } else {
                    state.status_msg = Some((
                        "历史已清空".to_string(),
                        crate::theme::SUCCESS,
                    ));
                }
            }
        });
    });
    ui.add_space(10.0);

    if entries.is_empty() {
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

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        for entry in &entries {
            render_entry_card(ui, entry, state);
        }
    });

    if let Some((msg, color)) = &state.status_msg {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(msg)
                .color(*color)
                .size(crate::theme::FONT_SM),
        );
    }
}

fn render_entry_card(
    ui: &mut egui::Ui,
    entry: &crate::history::HistoryEntry,
    state: &mut SettingsState,
) {
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
                        ui.ctx().copy_text(entry.text.clone());
                        state.status_msg =
                            Some(("已复制到剪贴板".to_string(), crate::theme::SUCCESS));
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
}
