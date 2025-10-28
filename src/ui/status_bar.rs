use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;
use crate::api::{Folder, FolderStatus, SyncState};
use crate::utils;

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
    // For breadcrumb level display
    breadcrumb_folder_label: Option<&str>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(&str, &str, Option<SyncState>, Option<bool>)>, // (name, type, sync_state, exists)
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
) {
    let status_line = if focus_level == 0 {
        // Show selected folder status
        if let Some(selected) = folders_state_selected {
            if let Some(folder) = folders.get(selected) {
                let folder_name = folder.label.as_ref().unwrap_or(&folder.id);
                if folder.paused {
                    format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                        format!("Folder: {}", folder_name),
                        "Paused",
                        "-",
                        "-",
                        "-"
                    )
                } else if let Some(status) = folder_statuses.get(&folder.id) {
                    let state_display = if status.state.is_empty() { "paused" } else { &status.state };
                    let in_sync = status.global_total_items.saturating_sub(status.need_total_items);
                    let items_display = format!("{}/{}", in_sync, status.global_total_items);

                    // Build status message considering both remote needs and local additions
                    let need_display = if status.receive_only_total_items > 0 {
                        // Has local additions
                        if status.need_total_items > 0 {
                            // Both local additions and remote needs
                            format!("↓{} ↑{} ({})",
                                status.need_total_items,
                                status.receive_only_total_items,
                                utils::format_bytes(status.need_bytes + status.receive_only_changed_bytes)
                            )
                        } else {
                            // Only local additions
                            format!("Local: {} items ({})",
                                status.receive_only_total_items,
                                utils::format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        // Only remote needs
                        format!("{} items ({}) ", status.need_total_items, utils::format_bytes(status.need_bytes))
                    } else {
                        "Up to date ".to_string()
                    };

                    format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                        format!("Folder: {}", folder_name),
                        state_display,
                        utils::format_bytes(status.global_bytes),
                        items_display,
                        need_display
                    )
                } else {
                    format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                        format!("Folder: {}", folder_name),
                        "Loading...",
                        "-",
                        "-",
                        "-"
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
        let sort_display = format!("Sort: {}{}",
            sort_mode,
            if sort_reverse { "↓" } else { "↑" }
        );
        metrics.push(sort_display);

        if let Some(load_time) = last_load_time_ms {
            metrics.push(format!("Load: {}ms", load_time));
        }

        if let Some(cache_hit) = cache_hit {
            metrics.push(format!("Cache: {}", if cache_hit { "HIT" } else { "MISS" }));
        }

        // Show selected item info if available
        if let Some((item_name, item_type, sync_state, exists)) = breadcrumb_selected_item {
            // Format name based on type: "dirname/" for directories, "filename" for files
            let formatted_name = match item_type {
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
    };

    // Parse status_line and color the labels (before colons)
    let status_spans: Vec<Span> = if status_line.is_empty() {
        vec![Span::raw("")]
    } else {
        let mut spans = vec![];
        // Check for both separators: " │ " (focus_level 0) and " | " (focus_level > 0)
        let parts: Vec<&str> = if status_line.contains(" │ ") {
            status_line.split(" │ ").collect()
        } else {
            status_line.split(" | ").collect()
        };

        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                // Use the appropriate separator
                if status_line.contains(" │ ") {
                    spans.push(Span::raw(" │ "));
                } else {
                    spans.push(Span::raw(" | "));
                }
            }

            // Check if this part is an ignored status (red text)
            let part_trimmed = part.trim();
            if part_trimmed == "Ignored" || part_trimmed == "Ignored, not deleted!" {
                spans.push(Span::styled(*part, Style::default().fg(Color::Red)));
            } else if let Some(colon_pos) = part.find(':') {
                // Split on first colon to separate label from value
                let label = &part[..=colon_pos];
                let value = &part[colon_pos + 1..];
                spans.push(Span::styled(label, Style::default().fg(Color::Yellow)));
                spans.push(Span::raw(value));
            } else {
                spans.push(Span::raw(*part));
            }
        }
        spans
    };

    let status_bar = Paragraph::new(Line::from(status_spans))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Gray));

    f.render_widget(status_bar, area);
}
