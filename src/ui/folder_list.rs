use super::icons::{FolderState, IconRenderer};
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
    icon_renderer: &IconRenderer,
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

            render_folder_card(
                folder,
                status,
                &card_state,
                icon_renderer,
                max_size_width,
                max_count_width,
            )
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

/// Map a card state to the icon state used by `IconRenderer`
fn card_state_to_folder_state(state: &FolderCardState) -> FolderState {
    match state {
        FolderCardState::Synced => FolderState::Synced,
        FolderCardState::OutOfSync { .. } => FolderState::OutOfSync,
        FolderCardState::Syncing { .. } => FolderState::Syncing,
        FolderCardState::Paused => FolderState::Paused,
        FolderCardState::Error => FolderState::Error,
        FolderCardState::Loading => FolderState::Loading,
    }
}

/// Render a single folder card as a multi-line ListItem
fn render_folder_card(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    icon_renderer: &IconRenderer,
    max_size_width: usize,
    max_count_width: usize,
) -> ListItem<'static> {
    let mut lines = Vec::new();

    // Line 1: Folder icon + sync status + name
    let display_name = folder.label.as_ref().unwrap_or(&folder.id);
    let mut line1 = icon_renderer.folder_with_status(card_state_to_folder_state(state));
    line1.push(Span::raw(" "));
    line1.push(Span::raw(display_name.to_string()));
    lines.push(Line::from(line1));

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::{FolderState, IconMode, IconTheme};
    use ratatui::{Terminal, backend::TestBackend};

    fn test_folder() -> Folder {
        Folder {
            id: "abc".to_string(),
            label: Some("Docs".to_string()),
            path: "/data/docs".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        }
    }

    fn render_card_text(mode: IconMode, state: &FolderCardState) -> String {
        let renderer = IconRenderer::new(mode, IconTheme::default());
        let item = render_folder_card(&test_folder(), None, state, &renderer, 3, 3);
        let backend = TestBackend::new(60, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(List::new(vec![item]), f.area()))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect()
    }

    // ========================================
    // CARD STATE MAPPING
    // ========================================

    #[test]
    fn test_card_state_to_folder_state_mapping() {
        let oos = FolderCardState::OutOfSync {
            remote_needed: 1,
            local_changes: 0,
        };
        let syncing = FolderCardState::Syncing {
            remote_needed: 1,
            local_changes: 0,
        };
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Synced),
            FolderState::Synced
        );
        assert_eq!(card_state_to_folder_state(&oos), FolderState::OutOfSync);
        assert_eq!(card_state_to_folder_state(&syncing), FolderState::Syncing);
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Paused),
            FolderState::Paused
        );
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Error),
            FolderState::Error
        );
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Loading),
            FolderState::Loading
        );
    }

    // ========================================
    // CARD ICON RENDERING
    // ========================================

    #[test]
    fn test_card_nerdfont_mode_has_no_emoji_folder() {
        let text = render_card_text(IconMode::NerdFont, &FolderCardState::Synced);
        assert!(!text.contains('📁'), "emoji folder leaked: {text:?}");
        assert!(!text.contains('📂'), "emoji folder leaked: {text:?}");
        assert!(
            text.contains('\u{F07C}'),
            "nerdfont glyph missing: {text:?}"
        );
    }

    #[test]
    fn test_card_emoji_mode_has_emoji_folder() {
        let text = render_card_text(IconMode::Emoji, &FolderCardState::Synced);
        assert!(text.contains('📂'), "emoji folder missing: {text:?}");
        assert!(!text.contains('\u{F07C}'));
    }
}
