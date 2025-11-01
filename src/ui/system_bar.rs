use crate::api::SystemStatus;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Format uptime seconds into human-readable string (e.g., "3d 15h", "15h 44m", "44m 30s")
fn format_uptime(seconds: u64) -> String {
    crate::logic::formatting::format_uptime(seconds)
}

/// Format human-readable size (4-character alignment, e.g., "1.2K", "5.3M", " 128G")
fn format_human_size(size: u64) -> String {
    crate::logic::formatting::format_human_size(size)
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

/// Render the system info bar at the top of the screen
pub fn render_system_bar(
    f: &mut Frame,
    area: Rect,
    system_status: Option<&SystemStatus>,
    device_name: Option<&str>,
    local_state_summary: (u64, u64, u64), // (files, dirs, bytes)
    last_transfer_rates: Option<(f64, f64)>, // (download, upload) in bytes/sec
) {
    let system_line = if let Some(sys_status) = system_status {
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
        spans.push(Span::raw(format!(
            " {} files, {} dirs, {}",
            total_files, total_dirs, size_str
        )));

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

    let system_widget = Paragraph::new(system_line)
        .block(Block::default().borders(Borders::ALL).title("System"))
        .style(Style::default().fg(Color::Gray));

    f.render_widget(system_widget, area);
}
