use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Render a toast notification (brief pop-up message)
pub fn render_toast(f: &mut Frame, area: Rect, message: &str) {
    // Calculate toast dimensions - allow for longer messages
    let max_width = (area.width as usize).min(80); // Max 80 chars wide
    let toast_width = (message.len() + 6).min(max_width) as u16;
    let toast_height = 4; // Slightly taller for better appearance

    let toast_x = (area.width.saturating_sub(toast_width)) / 2;
    let toast_y = 3; // Near the top but not too close

    let toast_area = Rect {
        x: area.x + toast_x,
        y: area.y + toast_y,
        width: toast_width,
        height: toast_height,
    };

    // Clear the area first to prevent background bleed-through
    f.render_widget(Clear, toast_area);

    // Detect error messages and use different styling
    let is_error = message.starts_with("Error:");
    let (icon, icon_color, border_color) = if is_error {
        ("✗ ", Color::Red, Color::Red) // Red theme for errors
    } else {
        ("✓ ", Color::Green, Color::Green) // Green theme for success
    };

    // Create styled toast with icon
    let toast_line = Line::from(vec![
        Span::styled(
            icon,
            Style::default().fg(icon_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(message, Style::default()),
    ]);

    let toast_block = Block::default().borders(Borders::ALL).border_style(
        Style::default()
            .fg(border_color)
            .add_modifier(Modifier::BOLD),
    );

    let toast_text = Paragraph::new(vec![toast_line])
        .block(toast_block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false });

    f.render_widget(toast_text, toast_area);
}
