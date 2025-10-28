use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the hotkey legend (dynamically changes based on vim mode)
pub fn render_legend(
    f: &mut Frame,
    area: Rect,
    vim_mode: bool,
) {
    // Build a single line with all hotkeys that will wrap automatically
    let mut hotkey_spans = vec![];

    // Navigation keys (different for vim mode)
    if vim_mode {
        hotkey_spans.extend(vec![
            Span::styled("hjkl", Style::default().fg(Color::Yellow)),
            Span::raw(":Nav  "),
            Span::styled("gg/G", Style::default().fg(Color::Yellow)),
            Span::raw(":First/Last  "),
            Span::styled("^d/^u", Style::default().fg(Color::Yellow)),
            Span::raw(":½Page  "),
            Span::styled("^f/^b", Style::default().fg(Color::Yellow)),
            Span::raw(":FullPage  "),
        ]);
    } else {
        hotkey_spans.extend(vec![
            Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
            Span::raw(":Nav  "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(":Open  "),
            Span::styled("←", Style::default().fg(Color::Yellow)),
            Span::raw(":Back  "),
        ]);
    }

    // Common actions (same for both modes)
    hotkey_spans.extend(vec![
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::raw(":Sort  "),
        Span::styled("S", Style::default().fg(Color::Yellow)),
        Span::raw(":Reverse  "),
        Span::styled("t", Style::default().fg(Color::Yellow)),
        Span::raw(":Info  "),
        Span::styled("i", Style::default().fg(Color::Yellow)),
        Span::raw(":Ignore  "),
        Span::styled("I", Style::default().fg(Color::Yellow)),
        Span::raw(":Ign+Del  "),
        Span::styled("d", Style::default().fg(Color::Yellow)),
        Span::raw(":Delete  "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(":Rescan  "),
        Span::styled("R", Style::default().fg(Color::Yellow)),
        Span::raw(":Restore  "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(":Quit"),
    ]);

    let hotkey_line = Line::from(hotkey_spans);

    let legend = Paragraph::new(vec![hotkey_line])
        .block(Block::default().borders(Borders::ALL).title("Hotkeys"))
        .style(Style::default().fg(Color::Gray))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(legend, area);
}
