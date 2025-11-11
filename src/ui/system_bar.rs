use crate::api::SystemStatus;
use crate::model::syncthing::ConnectionState;
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

/// Render connection status span based on connection state
fn render_connection_status(state: &ConnectionState) -> Vec<Span<'_>> {
    match state {
        ConnectionState::Connected => {
            vec![
                Span::styled("🟢 Connected", Style::default().fg(Color::Green)),
                Span::raw(" | "),
            ]
        }
        ConnectionState::Connecting {
            attempt,
            next_retry_secs,
            ..
        } => {
            let text = if *attempt > 1 {
                format!(
                    "🟡 Connecting (attempt {}, next: {}s) ",
                    attempt, next_retry_secs
                )
            } else {
                "🟡 Connecting... ".to_string()
            };
            vec![
                Span::styled(text, Style::default().fg(Color::Yellow)),
                Span::raw("| "),
            ]
        }
        ConnectionState::Disconnected { message, .. } => {
            // Show raw error message for tech-savvy audience
            vec![
                Span::styled(format!("🔴 {} ", message), Style::default().fg(Color::Red)),
                Span::raw("| "),
            ]
        }
    }
}

/// Render the system info bar at the top of the screen
pub fn render_system_bar(
    f: &mut Frame,
    area: Rect,
    connection_state: &ConnectionState,
    system_status: Option<&SystemStatus>,
    device_name: Option<&str>,
    local_state_summary: (u64, u64, u64), // (files, dirs, bytes)
    last_transfer_rates: Option<(f64, f64)>, // (download, upload) in bytes/sec
) {
    let system_line = if let (true, Some(sys_status)) = (
        matches!(connection_state, ConnectionState::Connected),
        system_status,
    ) {
        // Only show full system info when connected AND have status
        let uptime_str = format_uptime(sys_status.uptime);
        let (total_files, total_dirs, total_bytes) = local_state_summary;

        let mut spans = render_connection_status(connection_state);
        spans.push(Span::raw(device_name.unwrap_or("Unknown")));
        spans.push(Span::raw(" | "));
        spans.push(Span::styled("Up:", Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(format!(" {}", uptime_str)));

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
        // No system status yet - show connection state and error
        let spans = match connection_state {
            ConnectionState::Disconnected { message, .. } => {
                // Show error message (no device name since we're not connected)
                vec![Span::styled(
                    format!("🔴 {}", message),
                    Style::default().fg(Color::Red),
                )]
            }
            ConnectionState::Connecting {
                attempt,
                last_error,
                next_retry_secs,
            } => {
                let mut spans = vec![];

                // Show connecting status
                let text = if *attempt > 1 {
                    format!(
                        "🟡 Connecting (attempt {}, next: {}s)",
                        attempt, next_retry_secs
                    )
                } else {
                    "🟡 Connecting...".to_string()
                };
                spans.push(Span::styled(text, Style::default().fg(Color::Yellow)));

                // Show last error if available
                if let Some(err) = last_error {
                    spans.push(Span::raw(" | "));
                    spans.push(Span::styled(
                        err.clone(),
                        Style::default().fg(Color::Yellow),
                    ));
                }

                spans
            }
            ConnectionState::Connected => {
                // Connected but no system status - still loading
                let mut spans = vec![Span::styled(
                    "🟢 Connected",
                    Style::default().fg(Color::Green),
                )];

                if let Some(name) = device_name {
                    spans.push(Span::raw(" | "));
                    spans.push(Span::raw(name));
                }

                spans.push(Span::raw(" | Loading..."));
                spans
            }
        };

        Line::from(spans)
    };

    let system_widget = Paragraph::new(system_line)
        .block(Block::default().borders(Borders::ALL).title("System"))
        .style(Style::default().fg(Color::Gray));

    f.render_widget(system_widget, area);
}
