use super::icons::{FolderState, IconRenderer};
use crate::api::{Folder, FolderStatus};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};
use std::collections::HashMap;

/// Render the folder list panel
pub fn render_folder_list(
    f: &mut Frame,
    area: Rect,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    statuses_loaded: bool,
    folders_state: &mut ListState,
    is_focused: bool,
    icon_renderer: &IconRenderer,
    last_folder_updates: &HashMap<String, (std::time::SystemTime, String)>,
) {
    // Render Folders List
    let folders_items: Vec<ListItem> = folders
        .iter()
        .map(|folder| {
            let display_name = folder.label.as_ref().unwrap_or(&folder.id);

            // Determine folder state
            let folder_state = if !statuses_loaded {
                FolderState::Loading
            } else if folder.paused {
                FolderState::Paused
            } else if let Some(status) = folder_statuses.get(&folder.id) {
                if status.state == "" || status.state == "paused" {
                    FolderState::Paused
                } else if status.state == "syncing" {
                    FolderState::Syncing
                } else if status.need_total_items > 0 || status.receive_only_total_items > 0 {
                    FolderState::OutOfSync
                } else if status.state == "idle" {
                    FolderState::Synced
                } else if status.state.starts_with("sync") {
                    FolderState::Syncing
                } else if status.state == "scanning" {
                    FolderState::Scanning
                } else {
                    FolderState::Unknown
                }
            } else {
                FolderState::Error
            };

            // Build the folder display with optional last update info
            let mut icon_spans = icon_renderer.folder_with_status(folder_state);
            icon_spans.push(Span::raw(display_name));
            let folder_line = Line::from(icon_spans);

            // Add last update info if available
            if let Some((timestamp, last_file)) = last_folder_updates.get(&folder.id) {
                // Calculate time since last update
                let elapsed = timestamp
                    .elapsed()
                    .unwrap_or(std::time::Duration::from_secs(0));
                let time_str = if elapsed.as_secs() < 60 {
                    format!("{}s ago", elapsed.as_secs())
                } else if elapsed.as_secs() < 3600 {
                    format!("{}m ago", elapsed.as_secs() / 60)
                } else if elapsed.as_secs() < 86400 {
                    format!("{}h ago", elapsed.as_secs() / 3600)
                } else {
                    format!("{}d ago", elapsed.as_secs() / 86400)
                };

                // Truncate filename if too long
                let max_file_len = 40;
                let file_display = if last_file.len() > max_file_len {
                    format!("...{}", &last_file[last_file.len() - max_file_len..])
                } else {
                    last_file.clone()
                };

                // Multi-line item with update info
                ListItem::new(vec![
                    folder_line,
                    Line::from(Span::styled(
                        format!("  ↳ {} - {}", time_str, file_display),
                        Style::default().fg(Color::Rgb(150, 150, 150)), // Medium gray visible on both dark gray and black backgrounds
                    )),
                ])
            } else {
                // Single-line item without update info
                ListItem::new(folder_line)
            }
        })
        .collect();

    let folders_list = List::new(folders_items)
        .block(
            Block::default()
                .title("Folders")
                .borders(Borders::ALL)
                .border_style(if is_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Gray)
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(folders_list, area, folders_state);
}
