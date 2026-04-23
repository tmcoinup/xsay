//! 历史记录标签页：展示 ~/.cache/xsay/history.jsonl 的最近条目。

use super::SettingsState;
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut SettingsState) {
    let entries = crate::history::load_recent(200);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("最近 {} 条识别结果", entries.len()))
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = !entries.is_empty();
            if ui
                .add_enabled(enabled, egui::Button::new("🗑 清空"))
                .clicked()
            {
                if let Err(e) = crate::history::clear() {
                    state.status_msg = Some((
                        format!("✗ 清空失败: {}", e),
                        egui::Color32::from_rgb(255, 120, 120),
                    ));
                } else {
                    state.status_msg = Some((
                        "✓ 历史已清空".to_string(),
                        egui::Color32::from_rgb(80, 200, 80),
                    ));
                }
            }
        });
    });
    ui.add_space(4.0);

    if entries.is_empty() {
        ui.label(
            egui::RichText::new("暂无历史记录。识别出的文本会自动保存到这里。")
                .weak(),
        );
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        for entry in &entries {
            egui::Frame::none()
                .fill(ui.visuals().extreme_bg_color)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .rounding(egui::Rounding::same(4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(crate::history::format_timestamp(
                                entry.timestamp,
                            ))
                            .monospace()
                            .small()
                            .weak(),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.small_button("📋 复制").clicked() {
                                    ui.output_mut(|o| {
                                        o.copied_text = entry.text.clone();
                                    });
                                    state.status_msg = Some((
                                        "✓ 已复制到剪贴板".to_string(),
                                        egui::Color32::from_rgb(80, 200, 80),
                                    ));
                                }
                            },
                        );
                    });
                    ui.label(&entry.text);
                });
        }
    });

    if let Some((msg, color)) = &state.status_msg {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(msg).color(*color).small());
    }
}
