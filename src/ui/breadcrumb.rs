use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use unicode_width::UnicodeWidthStr;
use crate::api::{BrowseItem, SyncState};
use super::icons::IconRenderer;

/// Display mode for file info (timestamp and/or size)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Off,              // No timestamp or size
    TimestampOnly,    // Show timestamp only
    TimestampAndSize, // Show both size and timestamp
}

/// Format ISO timestamp into human-readable string (e.g., "2025-10-26 20:58")
fn format_timestamp(timestamp: &str) -> String {
    if timestamp.is_empty() {
        return String::new();
    }

    // Try to parse and format
    // Input format: "2025-10-26T20:58:21.580021398Z"
    // Output format: "2025-10-26 20:58"
    if let Some(datetime_part) = timestamp.split('T').next() {
        if let Some(time_part) = timestamp.split('T').nth(1) {
            let time = time_part.split(':').take(2).collect::<Vec<_>>().join(":");
            return format!("{} {}", datetime_part, time);
        }
        return datetime_part.to_string();
    }

    timestamp.to_string()
}

/// Format human-readable size (4-character alignment, e.g., "1.2K", "5.3M", " 128G")
fn format_human_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if size == 0 {
        return "   0".to_string();
    } else if size < KB {
        return format!("{:>4}", size);
    } else if size < MB {
        let kb = size as f64 / KB as f64;
        if kb < 10.0 {
            return format!("{:.1}K", kb);
        } else {
            return format!("{:>3}K", (size / KB));
        }
    } else if size < GB {
        let mb = size as f64 / MB as f64;
        if mb < 10.0 {
            return format!("{:.1}M", mb);
        } else {
            return format!("{:>3}M", (size / MB));
        }
    } else if size < TB {
        let gb = size as f64 / GB as f64;
        if gb < 10.0 {
            return format!("{:.1}G", gb);
        } else {
            return format!("{:>3}G", (size / GB));
        }
    } else {
        let tb = size as f64 / TB as f64;
        if tb < 10.0 {
            return format!("{:.1}T", tb);
        } else {
            return format!("{:>3}T", (size / TB));
        }
    }
}

/// Build a list item with icon, name, and optional timestamp/size info
fn build_list_item<'a>(
    item: &'a BrowseItem,
    icon_spans: Vec<Span<'a>>,
    panel_width: u16,
    display_mode: DisplayMode,
    add_spacing: bool,
) -> ListItem<'a> {
    let is_directory = item.item_type == "FILE_INFO_TYPE_DIRECTORY";

    // Add spacing prefix if requested (for non-focused breadcrumbs)
    let spacing_prefix = if add_spacing {
        vec![Span::raw("  ")]  // Two spaces to match "> " width
    } else {
        vec![]
    };

    // If no display mode, just show icon + name
    if display_mode == DisplayMode::Off || item.mod_time.is_empty() {
        let mut line_spans = spacing_prefix;
        line_spans.extend(icon_spans);
        line_spans.push(Span::raw(&item.name));
        return ListItem::new(Line::from(line_spans));
    }

    // Build display with timestamp and/or size
    let full_timestamp = format_timestamp(&item.mod_time);
    let icon_string: String = icon_spans.iter().map(|s| s.content.as_ref()).collect();
    let icon_and_name = format!("{}{}", icon_string, item.name);

    // Calculate available space: panel_width - borders(2) - highlight(2) - padding(2)
    let available_width = panel_width.saturating_sub(6) as usize;
    let spacing = 2; // Minimum spacing between name and info

    // Use unicode width for proper emoji handling
    let name_width = icon_and_name.width();

    // Determine what to show based on display mode (omit size for directories)
    let info_string = match display_mode {
        DisplayMode::TimestampOnly => full_timestamp.clone(),
        DisplayMode::TimestampAndSize => {
            if is_directory {
                // Directories: show only timestamp
                full_timestamp.clone()
            } else {
                // Files: show size + timestamp
                let human_size = format_human_size(item.size);
                format!("{} {}", human_size, full_timestamp)
            }
        },
        DisplayMode::Off => String::new(),
    };

    let info_width = info_string.width();

    // If everything fits, show it all with styled info
    if name_width + spacing + info_width <= available_width {
        let padding = available_width - name_width - info_width;
        let mut line_spans = spacing_prefix.clone();
        line_spans.extend(icon_spans);
        line_spans.push(Span::raw(&item.name));
        line_spans.push(Span::raw(" ".repeat(padding)));
        line_spans.push(Span::styled(info_string, Style::default().fg(Color::Rgb(120, 120, 120))));
        return ListItem::new(Line::from(line_spans));
    }

    // Truncate info to make room for name
    let space_left = available_width.saturating_sub(name_width + spacing);

    if space_left >= 5 {
        // Show truncated info (prioritize time over date)
        let truncated_info = if display_mode == DisplayMode::TimestampAndSize && !is_directory {
            // Files with size: Try "1.2K HH:MM" (10 chars) or just "HH:MM" (5 chars)
            if space_left >= 10 && full_timestamp.len() >= 16 {
                // Show size + time only
                let human_size = format_human_size(item.size);
                format!("{} {}", human_size, &full_timestamp[11..16])
            } else if space_left >= 5 && full_timestamp.len() >= 16 {
                // Show just time
                full_timestamp[11..16].to_string()
            } else {
                String::new()
            }
        } else {
            // TimestampOnly OR directory: progressively truncate timestamp
            if space_left >= 16 {
                full_timestamp
            } else if space_left >= 10 && full_timestamp.len() >= 16 {
                // Show "MM-DD HH:MM" (10 chars)
                full_timestamp[5..16].to_string()
            } else if space_left >= 5 && full_timestamp.len() >= 16 {
                // Show just time "HH:MM" (5 chars)
                full_timestamp[11..16].to_string()
            } else {
                String::new()
            }
        };

        if !truncated_info.is_empty() {
            let info_width = truncated_info.width();
            let padding = available_width - name_width - info_width;
            let mut line_spans = spacing_prefix.clone();
            line_spans.extend(icon_spans);
            line_spans.push(Span::raw(&item.name));
            line_spans.push(Span::raw(" ".repeat(padding)));
            line_spans.push(Span::styled(truncated_info, Style::default().fg(Color::Rgb(120, 120, 120))));
            return ListItem::new(Line::from(line_spans));
        }
    }

    // Not enough room for info, just show name
    let mut line_spans = spacing_prefix;
    line_spans.extend(icon_spans);
    line_spans.push(Span::raw(&item.name));
    ListItem::new(Line::from(line_spans))
}

/// Render a single breadcrumb level panel
pub fn render_breadcrumb_panel(
    f: &mut Frame,
    area: Rect,
    items: &[BrowseItem],
    file_sync_states: &std::collections::HashMap<String, SyncState>,
    state: &mut ratatui::widgets::ListState,
    title: &str,
    is_focused: bool,
    is_parent_selected: bool,
    display_mode: DisplayMode,
    icon_renderer: &IconRenderer,
    translated_base_path: &str,
    prefix: Option<&str>,
) {
    let panel_width = area.width;

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            // Use cached state directly (directories show their own metadata state, not aggregate)
            let sync_state = file_sync_states.get(&item.name).copied().unwrap_or(SyncState::Unknown);

            // Build icon as spans (for coloring)
            let is_directory = item.item_type == "FILE_INFO_TYPE_DIRECTORY";
            let icon_spans: Vec<Span> = if sync_state == SyncState::Ignored {
                // Check if ignored file exists on disk (only for ignored files, typically very few)
                let relative_path = if let Some(prefix_val) = prefix {
                    format!("{}/{}", prefix_val, item.name)
                } else {
                    item.name.clone()
                };
                let host_path = format!("{}/{}", translated_base_path.trim_end_matches('/'), relative_path);
                let exists = std::path::Path::new(&host_path).exists();
                icon_renderer.ignored_item(is_directory, exists)
            } else {
                icon_renderer.item_with_sync_state(is_directory, sync_state)
            };

            build_list_item(
                item,
                icon_spans,
                panel_width,
                display_mode,
                !is_focused && !is_parent_selected,  // Add spacing when neither focused nor parent selected
            )
        })
        .collect();

    // Build list widget with conditional styling
    let border_color = if is_focused {
        Color::Cyan
    } else if is_parent_selected {
        Color::Blue  // Distinct color for parent selection
    } else {
        Color::Gray
    };

    let mut list = List::new(list_items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

    // Add highlight when focused (with arrow) or parent selected (without arrow)
    if is_focused {
        list = list
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("> ");
        f.render_stateful_widget(list, area, state);
    } else if is_parent_selected {
        list = list
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("  ");  // Two spaces to maintain alignment
        f.render_stateful_widget(list, area, state);
    } else {
        let mut empty_state = ratatui::widgets::ListState::default();
        f.render_stateful_widget(list, area, &mut empty_state);
    }
}
