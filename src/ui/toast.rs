use crate::model::{Toast, ToastKind};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

fn toast_style(kind: ToastKind) -> (&'static str, Color) {
    match kind {
        ToastKind::Success => ("✓ ", Color::Green),
        ToastKind::Info => ("• ", Color::Cyan),
        ToastKind::Warning => ("! ", Color::Yellow),
        ToastKind::Error => ("✗ ", Color::Red),
    }
}

/// Render a toast notification (brief pop-up message)
pub fn render_toast(f: &mut Frame, area: Rect, toast: &Toast) {
    let message = toast.message.as_str();
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

    let (icon, color) = toast_style(toast.kind);
    let (icon_color, border_color) = (color, color);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Toast, ToastKind};
    use ratatui::{Terminal, backend::TestBackend};

    /// (icon cell text, icon fg, toast top-left border fg) for `toast` drawn on 80x10
    fn draw(toast: &Toast) -> (String, Color, Color) {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal.draw(|f| render_toast(f, f.area(), toast)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let row = 4; // toast at y=3, border row 3, text row 4
        let icon_x = (0..buf.area.width)
            .find(|&x| !buf[(x, row)].symbol().trim().is_empty() && buf[(x, row)].symbol() != "│")
            .expect("toast text row has content");
        let corner_x = (0..buf.area.width)
            .find(|&x| buf[(x, 3)].symbol() == "┌")
            .expect("toast border drawn");
        (
            buf[(icon_x, row)].symbol().to_string(),
            buf[(icon_x, row)].fg,
            buf[(corner_x, 3)].fg,
        )
    }

    fn toast(kind: ToastKind, message: &str) -> Toast {
        Toast {
            message: message.to_string(),
            kind,
            shown_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn test_toast_kinds_have_distinct_icon_and_color() {
        let cases = [
            (ToastKind::Success, "✓", Color::Green),
            (ToastKind::Info, "•", Color::Cyan),
            (ToastKind::Warning, "!", Color::Yellow),
            (ToastKind::Error, "✗", Color::Red),
        ];
        for (kind, icon, color) in cases {
            let (drawn_icon, icon_fg, border_fg) = draw(&toast(kind, "message"));
            assert_eq!(drawn_icon, icon, "{:?}", kind);
            assert_eq!(icon_fg, color, "{:?}", kind);
            assert_eq!(border_fg, color, "{:?}", kind);
        }
    }

    #[test]
    fn test_toast_style_comes_from_kind_not_message_text() {
        let (icon, _, _) = draw(&toast(ToastKind::Success, "Error: looks like a failure"));
        assert_eq!(icon, "✓");
        let (icon, _, _) = draw(&toast(ToastKind::Error, "Copied"));
        assert_eq!(icon, "✗");
    }
}
