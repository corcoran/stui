use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;
use crate::api::{Folder, FolderStatus};

/// Format bytes into human-readable string (e.g., "1.2 KB", "5.3 MB")
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
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
    // For breadcrumb level display
    breadcrumb_folder_label: Option<&str>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(&str, &str)>, // (name, type)
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
                                format_bytes(status.need_bytes + status.receive_only_changed_bytes)
                            )
                        } else {
                            // Only local additions
                            format!("Local: {} items ({})",
                                status.receive_only_total_items,
                                format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        // Only remote needs
                        format!("{} items ({}) ", status.need_total_items, format_bytes(status.need_bytes))
                    } else {
                        "Up to date ".to_string()
                    };

                    format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                        format!("Folder: {}", folder_name),
                        state_display,
                        format_bytes(status.global_bytes),
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
        if let Some((item_name, item_type)) = breadcrumb_selected_item {
            let type_display = match item_type {
                "FILE_INFO_TYPE_DIRECTORY" => "Dir",
                "FILE_INFO_TYPE_FILE" => "File",
                _ => "Item",
            };
            metrics.push(format!("Selected: {} ({})", item_name, type_display));
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
            // Split on first colon to separate label from value
            if let Some(colon_pos) = part.find(':') {
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
