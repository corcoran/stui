use crate::api::{Folder, FolderStatus, SyncState};
use crate::ui::icons::{FolderState, IconRenderer};
use crate::utils;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;

/// Map SyncState to user-friendly label for file/directory display
fn map_sync_state_label(sync_state: SyncState) -> &'static str {
    match sync_state {
        SyncState::Synced => "Synced",
        SyncState::OutOfSync => "Out of Sync",
        SyncState::LocalOnly => "Local Only",
        SyncState::RemoteOnly => "Remote Only",
        SyncState::Ignored => "Ignored",
        SyncState::Syncing => "Syncing",
        SyncState::Unknown => "Unknown",
    }
}

/// Map Syncthing API state to FolderState enum and user-friendly label
///
/// Special cases:
/// - When receive_only_items > 0, returns "Local Additions" (matches Syncthing web UI)
/// - When need_total_items > 0 and state is "idle", returns "Out of Sync" (matches web UI)
///
/// # Arguments
/// * `api_state` - Raw state string from Syncthing API
/// * `receive_only_items` - Number of local additions in receive-only folder
/// * `need_total_items` - Number of items that need syncing
///
/// # Returns
/// Tuple of (FolderState enum, display label)
pub fn map_folder_state(
    api_state: &str,
    receive_only_items: u64,
    need_total_items: u64,
) -> (FolderState, &'static str) {
    // Special case: Local Additions takes precedence
    if receive_only_items > 0 {
        return (FolderState::LocalOnly, "Local Additions");
    }

    // Special case: Idle but has items to sync = Out of Sync (matches web UI behavior)
    if api_state == "idle" && need_total_items > 0 {
        return (FolderState::OutOfSync, "Out of Sync");
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
    icon_renderer: &IconRenderer,
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
        icon_renderer,
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
    icon_renderer: &IconRenderer,
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
                    // Get API state (empty means paused)
                    let api_state = if status.state.is_empty() {
                        "paused"
                    } else {
                        &status.state
                    };

                    // Map to FolderState enum and get display label
                    let (folder_state, state_label) = map_folder_state(
                        api_state,
                        status.receive_only_total_items,
                        status.need_total_items,
                    );

                    // Render state icon + label
                    let state_icon = icon_renderer.folder_with_status(folder_state);
                    let state_display = format!(
                        "{}{}",
                        state_icon.iter()
                            .map(|s| s.content.as_ref())
                            .collect::<Vec<_>>()
                            .join(""),
                        state_label
                    );

                    // Calculate sync metrics
                    let in_sync = status
                        .global_total_items
                        .saturating_sub(status.need_total_items);
                    let items_display = format!("{}/{} items", in_sync, status.global_total_items);

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
                                "{} items ({})",
                                status.receive_only_total_items,
                                utils::format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        // Only remote needs
                        format!(
                            "{} items ({})",
                            status.need_total_items,
                            utils::format_bytes(status.need_bytes)
                        )
                    } else {
                        "Up to date".to_string()
                    };

                    // NEW FIELD ORDER: name | state | need_display | type | bytes | items
                    format!(
                        "Folder: {} | {} | {} | {} | {} | {}",
                        folder_name,
                        state_display,
                        need_display,
                        type_display,
                        utils::format_bytes(status.global_bytes),
                        items_display,
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
            // Add state display with icon if sync state is available
            if let Some(state) = sync_state {
                let is_dir = item_type.as_str() == "FILE_INFO_TYPE_DIRECTORY";

                // For ignored items, check existence to choose correct icon
                let state_icon = if state == SyncState::Ignored {
                    if let Some(exists_val) = exists {
                        icon_renderer.ignored_item(is_dir, exists_val)
                    } else {
                        // Default to warning icon if exists is None
                        icon_renderer.ignored_item(is_dir, true)
                    }
                } else {
                    icon_renderer.item_with_sync_state(is_dir, state)
                };

                let state_label = map_sync_state_label(state);
                let state_display = format!(
                    "{}{}",
                    state_icon.iter()
                        .map(|s| s.content.as_ref())
                        .collect::<Vec<_>>()
                        .join(""),
                    state_label
                );
                metrics.push(state_display);
            }

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
    icon_renderer: &IconRenderer,
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
        icon_renderer,
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
    icon_renderer: &IconRenderer,
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
        icon_renderer,
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
        let (state, label) = map_folder_state("idle", 0, 0);
        assert_eq!(state, FolderState::Synced);
        assert_eq!(label, "Idle");
    }

    #[test]
    fn test_map_folder_state_idle_with_needs() {
        // Idle with items to sync = Out of Sync (matches web UI)
        let (state, label) = map_folder_state("idle", 0, 1);
        assert_eq!(state, FolderState::OutOfSync);
        assert_eq!(label, "Out of Sync");
    }

    #[test]
    fn test_map_folder_state_scanning() {
        let (state, label) = map_folder_state("scanning", 0, 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Scanning");
    }

    #[test]
    fn test_map_folder_state_syncing() {
        let (state, label) = map_folder_state("syncing", 0, 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Syncing");
    }

    #[test]
    fn test_map_folder_state_preparing() {
        let (state, label) = map_folder_state("preparing", 0, 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Preparing");
    }

    #[test]
    fn test_map_folder_state_waiting() {
        let (state, label) = map_folder_state("waiting-to-scan", 0, 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Waiting to Scan");
    }

    #[test]
    fn test_map_folder_state_outofsync() {
        let (state, label) = map_folder_state("outofsync", 0, 0);
        assert_eq!(state, FolderState::OutOfSync);
        assert_eq!(label, "Out of Sync");
    }

    #[test]
    fn test_map_folder_state_error() {
        let (state, label) = map_folder_state("error", 0, 0);
        assert_eq!(state, FolderState::Error);
        assert_eq!(label, "Error");
    }

    #[test]
    fn test_map_folder_state_stopped() {
        let (state, label) = map_folder_state("stopped", 0, 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Stopped");
    }

    #[test]
    fn test_map_folder_state_paused() {
        let (state, label) = map_folder_state("paused", 0, 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Paused");
    }

    #[test]
    fn test_map_folder_state_unshared() {
        let (state, label) = map_folder_state("unshared", 0, 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "Unshared");
    }

    #[test]
    fn test_map_folder_state_local_additions() {
        // Local additions takes precedence over API state
        let (state, label) = map_folder_state("idle", 5, 0);
        assert_eq!(state, FolderState::LocalOnly);
        assert_eq!(label, "Local Additions");
    }

    #[test]
    fn test_map_folder_state_local_additions_with_needs() {
        // Local additions takes precedence over need_total_items
        let (state, label) = map_folder_state("idle", 5, 10);
        assert_eq!(state, FolderState::LocalOnly);
        assert_eq!(label, "Local Additions");
    }

    #[test]
    fn test_map_folder_state_unknown() {
        // Fallback for unknown API states
        let (state, label) = map_folder_state("unknown-state", 0, 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "Unknown");
    }

    // Tests for file/directory state display in breadcrumb view
    #[test]
    fn test_file_state_display_synced() {
        // Test: File with Synced state shows icon before "Selected:"
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1, // breadcrumb view
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::Synced), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄✅ Synced"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_file_state_display_out_of_sync() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::OutOfSync), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄⚠️ Out of Sync"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_file_state_display_local_only() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::LocalOnly), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄💻 Local Only"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_file_state_display_remote_only() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::RemoteOnly), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄☁️ Remote Only"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_file_state_display_syncing() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::Syncing), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄🔄 Syncing"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_dir_state_display_synced() {
        // Test: Directory with Synced state shows folder icon
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("subdir".to_string(), "FILE_INFO_TYPE_DIRECTORY".to_string(), Some(SyncState::Synced), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📁✅ Synced"));
        assert!(status.contains("Selected: subdir/"));
    }

    #[test]
    fn test_file_state_display_ignored_exists() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::Ignored), Some(true))),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄🔇 Ignored"));
        assert!(status.contains("Selected: test.txt"));
        assert!(status.contains("Ignored, not deleted!"));
    }

    #[test]
    fn test_file_state_display_ignored_deleted() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::Ignored), Some(false))),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄🚫 Ignored"));
        assert!(status.contains("Selected: test.txt"));
        assert!(status.contains("Ignored")); // Should have single "Ignored" (not "Ignored, not deleted!")
        assert!(!status.contains("Ignored, not deleted!"));
    }

    #[test]
    fn test_file_state_display_unknown() {
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), Some(SyncState::Unknown), None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(status.contains("📄❓ Unknown"));
        assert!(status.contains("Selected: test.txt"));
    }

    #[test]
    fn test_file_state_display_none() {
        // Test: No sync state (None) should not show state display
        let icon_renderer = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let status = build_status_line(
            &icon_renderer,
            1,
            &[],
            &HashMap::new(),
            None,
            Some("TestFolder".to_string()),
            Some(5),
            Some(("test.txt".to_string(), "file".to_string(), None, None)),
            "A-Z",
            false,
            None,
            None,
            0,
        );
        assert!(!status.contains("Synced"));
        assert!(!status.contains("Out of Sync"));
        assert!(status.contains("Selected: test.txt"));
    }
}
