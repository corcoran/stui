use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::api::Folder;
use crate::model::types::OutOfSyncSummaryState;
use crate::ui::icons::IconRenderer;

pub fn render_out_of_sync_summary(
    f: &mut Frame,
    area: Rect,
    folders: &[Folder],
    summary_state: &OutOfSyncSummaryState,
    _icon_renderer: &IconRenderer,
) {
    // Create centered modal (60% width, auto height)
    let modal_width = (area.width as f32 * 0.6) as u16;
    let modal_height = (folders.len() as u16 * 3) + 4; // 3 lines per folder + borders

    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width.min(area.width),
        height: modal_height.min(area.height),
    };

    // Build folder items
    let items: Vec<ListItem> = folders
        .iter()
        .map(|folder| {
            let display_name = folder.label.as_ref().unwrap_or(&folder.id);

            // Get breakdown for this folder
            let breakdown = summary_state.breakdowns.get(&folder.id);
            let is_loading = summary_state.loading.contains(&folder.id);

            let mut lines = vec![
                Line::from(vec![
                    Span::raw("📂 "),
                    Span::styled(display_name, Style::default().add_modifier(Modifier::BOLD)),
                ]),
            ];

            if is_loading {
                lines.push(Line::from(Span::styled(
                    "   Loading...",
                    Style::default().fg(Color::Gray),
                )));
            } else if let Some(b) = breakdown {
                let total = b.downloading + b.queued + b.remote_only + b.modified + b.local_only;

                if total == 0 {
                    lines.push(Line::from(Span::styled(
                        "   ✅ All synced",
                        Style::default().fg(Color::Green),
                    )));
                } else {
                    let mut status_parts = Vec::new();

                    if b.downloading > 0 {
                        status_parts.push(format!("🔄 Downloading: {}", b.downloading));
                    }
                    if b.queued > 0 {
                        status_parts.push(format!("⏳ Queued: {}", b.queued));
                    }
                    if b.local_only > 0 {
                        status_parts.push(format!("💻 Local: {}", b.local_only));
                    }
                    if b.remote_only > 0 {
                        status_parts.push(format!("☁️ Remote: {}", b.remote_only));
                    }
                    if b.modified > 0 {
                        status_parts.push(format!("⚠️ Modified: {}", b.modified));
                    }

                    lines.push(Line::from(Span::styled(
                        format!("   {}", status_parts.join("  ")),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Out-of-Sync Summary")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    // Clear background behind modal first
    f.render_widget(Clear, modal_area);
    // Then render the modal
    f.render_widget(list, modal_area);
}
