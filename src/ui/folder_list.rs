use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::collections::HashMap;
use crate::api::{Folder, FolderStatus, SystemStatus};
use super::icons::{FolderState, IconRenderer};

/// Format uptime seconds into human-readable string (e.g., "3d 15h", "15h 44m", "44m 30s")
fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
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

/// Format transfer rate (bytes/sec) into human-readable string
fn format_transfer_rate(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes_per_sec < KB {
        format!("{:.0}B/s", bytes_per_sec)
    } else if bytes_per_sec < MB {
        format!("{:.1}K/s", bytes_per_sec / KB)
    } else if bytes_per_sec < GB {
        format!("{:.1}M/s", bytes_per_sec / MB)
    } else {
        format!("{:.2}G/s", bytes_per_sec / GB)
    }
}

/// Render the folder list panel with device status bar at bottom
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
    // Device/system status
    system_status: Option<&SystemStatus>,
    device_name: Option<&str>,
    local_state_summary: (u64, u64, u64), // (files, dirs, bytes)
    last_transfer_rates: Option<(f64, f64)>, // (download, upload) in bytes/sec
) {
    // Split folders pane into folders list (top) + device status bar (bottom)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),      // Folders list content
            Constraint::Length(3),   // Device status bar (3 lines: top border, text, bottom border)
        ])
        .split(area);

    // Render Device Status Bar
    let device_status_line = if let Some(sys_status) = system_status {
        let uptime_str = format_uptime(sys_status.uptime);
        let (total_files, total_dirs, total_bytes) = local_state_summary;

        let mut spans = vec![
            Span::raw(device_name.unwrap_or("Unknown")),
            Span::raw(" | "),
            Span::styled("Up:", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" {}", uptime_str)),
        ];

        // Add local state (use trimmed size to avoid padding)
        let size_str = format_human_size(total_bytes).trim().to_string();
        spans.push(Span::raw(" | "));
        spans.push(Span::styled("Local:", Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(format!(" {} files, {} dirs, {}", total_files, total_dirs, size_str)));

        // Add rates if available (display pre-calculated rates)
        if let Some((in_rate, out_rate)) = last_transfer_rates {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("↓", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format_transfer_rate(in_rate)));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("↑", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format_transfer_rate(out_rate)));
        }

        Line::from(spans)
    } else {
        Line::from(Span::raw("Device: Loading..."))
    };

    let device_status_widget = Paragraph::new(device_status_line)
        .block(Block::default().borders(Borders::ALL).title("System"))
        .style(Style::default().fg(Color::Gray));

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
                let elapsed = timestamp.elapsed().unwrap_or(std::time::Duration::from_secs(0));
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
                        Style::default().fg(Color::Rgb(150, 150, 150)) // Medium gray visible on both dark gray and black backgrounds
                    ))
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

    f.render_stateful_widget(folders_list, chunks[0], folders_state);

    // Render Device Status Bar at bottom
    f.render_widget(device_status_widget, chunks[1]);
}
