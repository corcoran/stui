//! Folder update history modal rendering
//!
//! Displays scrollable list of recent file updates with timestamps, icons, and sizes.

use crate::logic::formatting::format_human_size;
use crate::model::types::FolderHistoryModal;
use crate::ui::icons::IconRenderer;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame,
};

/// Render the folder update history modal
///
/// Shows a centered modal with scrollable list of file updates.
pub fn render_folder_history_modal(
    f: &mut Frame,
    area: Rect,
    modal_state: &FolderHistoryModal,
    _icon_renderer: &IconRenderer,
) {
    // Calculate centered modal dimensions (80% width, 80% height)
    let modal_width = (area.width as f32 * 0.8) as u16;
    let modal_height = (area.height as f32 * 0.8) as u16;

    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width.min(area.width),
        height: modal_height.min(area.height),
    };

    // Calculate available width for items (modal width - borders - padding)
    let item_width = modal_width.saturating_sub(4) as usize;

    // Calculate number width for right-aligned indices (based on total count)
    let total_count = modal_state.total_files_scanned;
    let number_width = total_count.to_string().len();

    // Build list items from history entries (match breadcrumb style exactly)
    let items: Vec<ListItem> = modal_state
        .entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            // Format right-aligned number (1-indexed)
            let number_str = format!("{:>width$}", idx + 1, width = number_width);

            // Format timestamp
            let timestamp = format_timestamp(&entry.timestamp);

            // Format file size (match breadcrumb format: "size timestamp")
            let size_str = entry
                .file_size
                .map(format_human_size)
                .unwrap_or_else(|| "    ".to_string()); // 4-space placeholder for alignment

            // Build info string (size + timestamp, like breadcrumbs)
            let info_str = format!("{} {}", size_str, timestamp);

            // Calculate padding to right-align info (like breadcrumbs)
            // Account for number + separator in width calculation
            use unicode_width::UnicodeWidthStr;
            let number_and_sep_width = number_width + 2; // number + "  " (double space)
            let name_width = entry.file_path.width();
            let info_width = info_str.width();
            let spacing = 2; // Minimum spacing

            let available_for_content = item_width.saturating_sub(number_and_sep_width);
            let padding = if name_width + spacing + info_width <= available_for_content {
                available_for_content.saturating_sub(name_width + info_width)
            } else {
                spacing
            };

            // Build line spans with number prefix and right-aligned info
            let spans = vec![
                Span::styled(number_str, Style::default().fg(Color::Rgb(120, 120, 120))), // Match date/size color
                Span::raw("  "), // Double space after number
                Span::styled(&entry.file_path, Style::default().fg(Color::White)),
                Span::raw(" ".repeat(padding)),
                Span::styled(
                    info_str,
                    Style::default().fg(Color::Rgb(120, 120, 120)), // Match breadcrumb gray
                ),
            ];

            let line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();

    // Create title with entry count and loading state
    let title = if modal_state.loading {
        format!(
            " {} - Update History (Loading... {}/?) ",
            modal_state.folder_label,
            modal_state.entries.len()
        )
    } else if modal_state.has_more {
        format!(
            " {} - Update History (Showing {} files, more available) ",
            modal_state.folder_label,
            modal_state.entries.len()
        )
    } else {
        format!(
            " {} - Update History (All {} files) ",
            modal_state.folder_label, modal_state.total_files_scanned
        )
    };

    // Build list widget with proper highlighting
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    // Create stateful list state for scrolling
    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(modal_state.selected_index));

    // Clear background and render modal with stateful widget
    f.render_widget(Clear, modal_area);
    f.render_stateful_widget(list, modal_area, &mut list_state);

    // Render scrollbar if content exceeds viewport
    if modal_state.entries.len() > (modal_height.saturating_sub(2)) as usize {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state =
            ScrollbarState::new(modal_state.entries.len()).position(modal_state.selected_index);

        f.render_stateful_widget(
            scrollbar,
            modal_area.inner(ratatui::layout::Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut scrollbar_state,
        );
    }
}

/// Format timestamp for display (YYYY-MM-DD HH:MM:SS)
fn format_timestamp(timestamp: &std::time::SystemTime) -> String {
    use chrono::{DateTime, Utc};
    use std::time::UNIX_EPOCH;

    let duration = timestamp.duration_since(UNIX_EPOCH).unwrap_or_default();
    let datetime: DateTime<Utc> = DateTime::from_timestamp(duration.as_secs() as i64, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());

    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}
