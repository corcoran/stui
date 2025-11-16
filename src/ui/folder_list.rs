use super::icons::IconRenderer;
use crate::api::{Folder, FolderStatus};
use crate::logic::folder_card::{
    FolderCardState, calculate_folder_card_state, format_file_count, format_folder_type,
    format_out_of_sync_details, format_size, format_status_message,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::collections::HashMap;

/// Render folder cards with inline stats and status
#[allow(clippy::too_many_arguments)]
pub fn render_folder_list(
    f: &mut Frame,
    area: Rect,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    _statuses_loaded: bool,
    folders_state: &mut ListState,
    is_focused: bool,
    _icon_renderer: &IconRenderer,
    _last_folder_updates: &HashMap<String, (std::time::SystemTime, String)>,
) {
    // Calculate title with folder counts
    let title = calculate_folder_list_title(folders, folder_statuses);

    // Calculate maximum column widths for alignment
    let max_size_width = folders
        .iter()
        .map(|f| {
            folder_statuses
                .get(&f.id)
                .map(|s| format_size(s.global_bytes).len())
                .unwrap_or(3) // "..." length
        })
        .max()
        .unwrap_or(8);

    let max_count_width = folders
        .iter()
        .map(|f| {
            folder_statuses
                .get(&f.id)
                .map(|s| format_file_count(s.global_files).len())
                .unwrap_or(3) // "..." length
        })
        .max()
        .unwrap_or(11);

    // Render cards as multi-line ListItems
    let folder_items: Vec<ListItem> = folders
        .iter()
        .map(|folder| {
            let status = folder_statuses.get(&folder.id);
            let card_state = calculate_folder_card_state(folder, status);

            render_folder_card(folder, status, &card_state, max_size_width, max_count_width)
        })
        .collect();

    let folders_list = List::new(folder_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if is_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(folders_list, area, folders_state);
}

/// Calculate folder list title with counts
fn calculate_folder_list_title(
    folders: &[Folder],
    statuses: &HashMap<String, FolderStatus>,
) -> String {
    let total = folders.len();
    let synced = folders
        .iter()
        .filter(|f| {
            if f.paused {
                return false;
            }
            if let Some(status) = statuses.get(&f.id) {
                status.state == "idle"
                    && status.need_total_items == 0
                    && status.receive_only_total_items == 0
            } else {
                false
            }
        })
        .count();

    let syncing = folders
        .iter()
        .filter(|f| {
            if let Some(status) = statuses.get(&f.id) {
                status.state == "syncing" || status.state == "sync-preparing"
            } else {
                false
            }
        })
        .count();

    let paused = folders.iter().filter(|f| f.paused).count();

    let mut parts = vec![format!("{} total", total)];
    if synced > 0 {
        parts.push(format!("{} synced", synced));
    }
    if syncing > 0 {
        parts.push(format!("{} syncing", syncing));
    }
    if paused > 0 {
        parts.push(format!("{} paused", paused));
    }

    format!(" Folders ({}) ", parts.join(", "))
}

/// Render a single folder card as a multi-line ListItem
fn render_folder_card(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    max_size_width: usize,
    max_count_width: usize,
) -> ListItem<'static> {
    let mut lines = Vec::new();

    // Line 1: Folder icon + sync status + name
    let display_name = folder.label.as_ref().unwrap_or(&folder.id);
    let icon = match state {
        FolderCardState::Synced => "✅",
        FolderCardState::OutOfSync { .. } => "⚠️",
        FolderCardState::Syncing { .. } => "🔄",
        FolderCardState::Paused => "⏸",
        FolderCardState::Error => "❌",
        FolderCardState::Loading => "⏳",
    };

    lines.push(Line::from(vec![
        Span::raw("📁"),
        Span::raw(icon),
        Span::raw(" "),
        Span::raw(display_name.to_string()),
    ]));

    // Line 2: Type | Size | File Count | Status
    // Use fixed width for Type (14) and dynamic widths for Size/Count based on actual data
    let folder_type_str = format_folder_type(&folder.folder_type);
    let size_str = status
        .map(|s| format_size(s.global_bytes))
        .unwrap_or_else(|| "...".to_string());
    let file_count_str = status
        .map(|s| format_file_count(s.global_files))
        .unwrap_or_else(|| "...".to_string());
    let status_msg = format_status_message(state);

    lines.push(Line::from(format!(
        "     {:<14} │ {:>width_size$} │ {:>width_count$} │ {}",
        folder_type_str,
        size_str,
        file_count_str,
        status_msg,
        width_size = max_size_width,
        width_count = max_count_width
    )));

    // Line 3 (optional): Out-of-sync details (for both OutOfSync and Syncing states)
    match state {
        FolderCardState::OutOfSync {
            remote_needed,
            local_changes,
        }
        | FolderCardState::Syncing {
            remote_needed,
            local_changes,
        } => {
            if let Some(status) = status
                && let Some(details) = format_out_of_sync_details(
                    *remote_needed,
                    *local_changes,
                    status.need_bytes,
                    &folder.folder_type,
                )
            {
                lines.push(Line::from(format!("     {}", details)));
            }
        }
        _ => {}
    }

    // Line 4: Blank separator between folders
    lines.push(Line::from(""));

    ListItem::new(lines)
}
