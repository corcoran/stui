use crate::api::{Folder, FolderStatus, SyncState};
use crate::ui::icons::FolderState;
use crate::utils;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;

/// Map Syncthing API state to FolderState enum and user-friendly label
///
/// Special case: When receive_only_items > 0, returns "Local Additions"
/// state regardless of API state (matches Syncthing web UI behavior).
///
/// # Arguments
/// * `api_state` - Raw state string from Syncthing API
/// * `receive_only_items` - Number of local additions in receive-only folder
///
/// # Returns
/// Tuple of (FolderState enum, display label)
fn map_folder_state(api_state: &str, receive_only_items: u64) -> (FolderState, &'static str) {
    // Special case: Local Additions takes precedence
    if receive_only_items > 0 {
        return (FolderState::LocalOnly, "Local Additions");
    }

    match api_state {
        "idle" => (FolderState::Synced, "Idle"),
        "scanning" => (FolderState::Scanning, "Scanning"),
        "syncing" => (FolderState::Syncing, "Syncing"),
        "preparing" => (FolderState::Syncing, "Preparing"),
        "waiting-to-scan" => (FolderState::Scanning, "Waiting to Scan"),
        "outofsync" => (FolderState::OutOfSync, "Out of Sync"),
        "error" => (FolderState::Error, "Error"),
        "stopped" => (FolderState::Paused, "Stopped"),
        "paused" => (FolderState::Paused, "Paused"),
        "unshared" => (FolderState::Unknown, "Unshared"),
        _ => (FolderState::Unknown, "Unknown"), // Fallback for unknown states
    }
}

/// Build the status bar paragraph (reusable for both rendering and height calculation)
pub fn build_status_paragraph(
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> Paragraph<'static> {
    let status_line = build_status_line(
        focus_level,
        folders,
        folder_statuses,
        folders_state_selected,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        sort_mode,
        sort_reverse,
        last_load_time_ms,
        cache_hit,
        pending_operations_count,
    );

    // Parse status_line and color the labels (before colons)
    let status_spans: Vec<Span> = if status_line.is_empty() {
        vec![Span::raw("")]
    } else {
        let mut spans = vec![];
        // Split on " | " separator (now used by both folder and breadcrumb views)
        let parts: Vec<&str> = status_line.split(" | ").collect();

        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::raw(" | "));
            }

            // Check if this part is an ignored status (red text)
            let part_trimmed = part.trim();
            if part_trimmed == "Ignored" || part_trimmed == "Ignored, not deleted!" {
                spans.push(Span::styled(
                    part.to_string(),
                    Style::default().fg(Color::Red),
                ));
            } else if let Some(colon_pos) = part.find(':') {
                // Split on first colon to separate label from value
                let label = &part[..=colon_pos];
                let value = &part[colon_pos + 1..];
                spans.push(Span::styled(
                    label.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::raw(value.to_string()));
            } else {
                spans.push(Span::raw(part.to_string()));
            }
        }
        spans
    };

    Paragraph::new(vec![Line::from(status_spans)])
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false })
}

/// Build the status line string (extracted for reuse)
fn build_status_line(
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> String {
    if focus_level == 0 {
        // Show selected folder status
        if let Some(selected) = folders_state_selected {
            if let Some(folder) = folders.get(selected) {
                let folder_name = folder.label.as_ref().unwrap_or(&folder.id);

                // Convert folder type to user-friendly display
                let type_display = match folder.folder_type.as_str() {
                    "sendonly" => "Send Only",
                    "sendreceive" => "Send & Receive",
                    "receiveonly" => "Receive Only",
                    _ => &folder.folder_type,
                };

                if folder.paused {
                    format!(
                        "Folder: {} | {} | Paused",
                        folder_name,
                        type_display
                    )
                } else if let Some(status) = folder_statuses.get(&folder.id) {
                    let state_display = if status.state.is_empty() {
                        "paused"
                    } else {
                        &status.state
                    };
                    let in_sync = status
                        .global_total_items
                        .saturating_sub(status.need_total_items);
                    let items_display = format!("{}/{}", in_sync, status.global_total_items);

                    // Build status message considering both remote needs and local additions
                    let need_display = if status.receive_only_total_items > 0 {
                        // Has local additions
                        if status.need_total_items > 0 {
                            // Both local additions and remote needs
                            format!(
                                "↓{} ↑{} ({})",
                                status.need_total_items,
                                status.receive_only_total_items,
                                utils::format_bytes(
                                    status.need_bytes + status.receive_only_changed_bytes
                                )
                            )
                        } else {
                            // Only local additions
                            format!(
                                "Local: {} items ({})",
                                status.receive_only_total_items,
                                utils::format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        // Only remote needs
                        format!(
                            "{} items ({}) ",
                            status.need_total_items,
                            utils::format_bytes(status.need_bytes)
                        )
                    } else {
                        "Up to date ".to_string()
                    };

                    format!(
                        "Folder: {} | {} | {} | {} | {} | {}",
                        folder_name,
                        type_display,
                        state_display,
                        utils::format_bytes(status.global_bytes),
                        items_display,
                        need_display
                    )
                } else {
                    format!(
                        "Folder: {} | {} | Loading...",
                        folder_name,
                        type_display
                    )
                }
            } else {
                "No folder selected".to_string()
            }
        } else {
            "No folder selected".to_string()
        }
    } else {
        // Show current directory performance metrics
        let mut metrics = Vec::new();

        if let Some(folder_label) = breadcrumb_folder_label {
            metrics.push(format!("Folder: {}", folder_label));
        }

        if let Some(item_count) = breadcrumb_item_count {
            metrics.push(format!("{} items", item_count));
        }

        // Show sort mode
        let sort_display = format!(
            "Sort: {}{}",
            sort_mode,
            if sort_reverse { "↓" } else { "↑" }
        );
        metrics.push(sort_display);

        // Show pending operations count if any
        if pending_operations_count > 0 {
            metrics.push(format!("⏳ {} deletions processing", pending_operations_count));
        }

        if let Some(load_time) = last_load_time_ms {
            metrics.push(format!("Load: {}ms", load_time));
        }

        if let Some(cache_hit) = cache_hit {
            metrics.push(format!("Cache: {}", if cache_hit { "HIT" } else { "MISS" }));
        }

        // Show selected item info if available
        if let Some((item_name, item_type, sync_state, exists)) = breadcrumb_selected_item {
            // Format name based on type: "dirname/" for directories, "filename" for files
            let formatted_name = match item_type.as_str() {
                "FILE_INFO_TYPE_DIRECTORY" => format!("{}/", item_name),
                _ => item_name.to_string(),
            };
            metrics.push(format!("Selected: {}", formatted_name));

            // Add ignored status if applicable
            if sync_state == Some(SyncState::Ignored) {
                if let Some(exists_val) = exists {
                    let ignored_status = if exists_val {
                        "Ignored, not deleted!"
                    } else {
                        "Ignored"
                    };
                    metrics.push(ignored_status.to_string());
                }
            }
        }

        metrics.join(" | ")
    }
}

/// Render the bottom status bar
/// - When focus_level == 0: Shows folder status (state, size, sync progress)
/// - When focus_level > 0: Shows directory metrics (items, sort mode, load time, cache hit)
pub fn render_status_bar(
    f: &mut Frame,
    area: Rect,
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) {
    let status_bar = build_status_paragraph(
        focus_level,
        folders,
        folder_statuses,
        folders_state_selected,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        sort_mode,
        sort_reverse,
        last_load_time_ms,
        cache_hit,
        pending_operations_count,
    );
    f.render_widget(status_bar, area);
}

/// Calculate required height for status bar based on terminal width and content
pub fn calculate_status_height(
    terminal_width: u16,
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> u16 {
    // Build status line WITHOUT block borders for accurate line counting
    let status_line = build_status_line(
        focus_level,
        folders,
        folder_statuses,
        folders_state_selected,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        sort_mode,
        sort_reverse,
        last_load_time_ms,
        cache_hit,
        pending_operations_count,
    );

    // Parse status_line and color the labels (same as in build_status_paragraph)
    let status_spans: Vec<Span> = if status_line.is_empty() {
        vec![Span::raw("")]
    } else {
        let mut spans = vec![];
        let parts: Vec<&str> = status_line.split(" | ").collect();

        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::raw(" | "));
            }

            let part_trimmed = part.trim();
            if part_trimmed == "Ignored" || part_trimmed == "Ignored, not deleted!" {
                spans.push(Span::styled(
                    part.to_string(),
                    Style::default().fg(Color::Red),
                ));
            } else if let Some(colon_pos) = part.find(':') {
                let label = &part[..=colon_pos];
                let value = &part[colon_pos + 1..];
                spans.push(Span::styled(
                    label.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::raw(value.to_string()));
            } else {
                spans.push(Span::raw(part.to_string()));
            }
        }
        spans
    };

    // Create paragraph WITHOUT block for accurate line counting
    let paragraph_for_counting = Paragraph::new(vec![Line::from(status_spans)])
        .wrap(Wrap { trim: false });

    // Calculate available width (subtract left + right borders)
    let available_width = terminal_width.saturating_sub(2);

    // Get exact line count for wrapped text
    let line_count = paragraph_for_counting.line_count(available_width);

    // Add top + bottom borders, ensure minimum of 3
    (line_count as u16).saturating_add(2).max(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::FolderState;

    #[test]
    fn test_map_folder_state_idle() {
        let (state, label) = map_folder_state("idle", 0);
        assert_eq!(state, FolderState::Synced);
        assert_eq!(label, "Idle");
    }

    #[test]
    fn test_map_folder_state_scanning() {
        let (state, label) = map_folder_state("scanning", 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Scanning");
    }

    #[test]
    fn test_map_folder_state_syncing() {
        let (state, label) = map_folder_state("syncing", 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Syncing");
    }

    #[test]
    fn test_map_folder_state_preparing() {
        let (state, label) = map_folder_state("preparing", 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Preparing");
    }

    #[test]
    fn test_map_folder_state_waiting() {
        let (state, label) = map_folder_state("waiting-to-scan", 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Waiting to Scan");
    }

    #[test]
    fn test_map_folder_state_outofsync() {
        let (state, label) = map_folder_state("outofsync", 0);
        assert_eq!(state, FolderState::OutOfSync);
        assert_eq!(label, "Out of Sync");
    }

    #[test]
    fn test_map_folder_state_error() {
        let (state, label) = map_folder_state("error", 0);
        assert_eq!(state, FolderState::Error);
        assert_eq!(label, "Error");
    }

    #[test]
    fn test_map_folder_state_stopped() {
        let (state, label) = map_folder_state("stopped", 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Stopped");
    }

    #[test]
    fn test_map_folder_state_paused() {
        let (state, label) = map_folder_state("paused", 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Paused");
    }

    #[test]
    fn test_map_folder_state_unshared() {
        let (state, label) = map_folder_state("unshared", 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "Unshared");
    }

    #[test]
    fn test_map_folder_state_local_additions() {
        // Local additions takes precedence over API state
        let (state, label) = map_folder_state("idle", 5);
        assert_eq!(state, FolderState::LocalOnly);
        assert_eq!(label, "Local Additions");
    }

    #[test]
    fn test_map_folder_state_unknown() {
        // Fallback for unknown API states
        let (state, label) = map_folder_state("unknown-state", 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "Unknown");
    }
}
