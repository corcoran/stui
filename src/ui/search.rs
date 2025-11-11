//! Search Input UI
//!
//! Renders the search input box with query, match count, and blinking cursor.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render search input box above legend
///
/// # Arguments
/// - `f`: Ratatui frame
/// - `area`: Rectangular area to render in
/// - `query`: Current search query
/// - `active`: Whether input is actively receiving keystrokes
/// - `match_count`: Number of matches found (None if not calculated)
/// - `vim_mode`: Whether vim keybindings are enabled
pub fn render_search_input(
    f: &mut Frame,
    area: Rect,
    query: &str,
    active: bool,
    match_count: Option<usize>,
    vim_mode: bool,
) {
    // Build title with match count
    let title = if active {
        match match_count {
            Some(count) => format!(" Search ({} matches) - Esc to cancel ", count),
            None => " Search - Esc to cancel ".to_string(),
        }
    } else if !query.is_empty() {
        // Search accepted (Enter pressed) - show match count
        match match_count {
            Some(count) => format!(" Search ({} matches) - Esc to clear ", count),
            None => " Search - Esc to clear ".to_string(),
        }
    } else {
        // No search query - show trigger key
        let search_key = if vim_mode { "/" } else { "Ctrl-F" };
        format!(" Search ({}) ", search_key)
    };

    let border_color = if active { Color::Cyan } else { Color::Gray };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(border_color));

    // Build input line with cursor
    let cursor_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::SLOW_BLINK);

    let input_line = if active {
        Line::from(vec![
            Span::raw("Match: "),
            Span::raw(query),
            Span::styled("█", cursor_style), // Blinking cursor
        ])
    } else {
        Line::from(vec![Span::styled(
            format!("Match: {}", query),
            Style::default().fg(Color::Gray),
        )])
    };

    let paragraph = Paragraph::new(vec![input_line])
        .block(block)
        .style(Style::default());

    f.render_widget(paragraph, area);
}
