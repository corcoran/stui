use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the hotkey legend (dynamically changes based on vim mode and focus level)
pub fn render_legend(
    f: &mut Frame,
    area: Rect,
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
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

    // Folder-specific actions - only in folder view (focus_level == 0)
    if focus_level == 0 {
        hotkey_spans.extend(vec![
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(":Change Type  "),
            Span::styled("p", Style::default().fg(Color::Yellow)),
            Span::raw(":Pause/Resume  "),
        ]);
    }

    // Actions that only apply to breadcrumbs (focus_level > 0), not folders
    if focus_level > 0 {
        hotkey_spans.extend(vec![
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(":Copy path  "),
        ]);
        hotkey_spans.extend(vec![
            Span::styled("s", Style::default().fg(Color::Yellow)),
            Span::raw(":Sort  "),
            Span::styled("S", Style::default().fg(Color::Yellow)),
            Span::raw(":Reverse  "),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            Span::raw(":Info  "),
            Span::styled("?", Style::default().fg(Color::Yellow)),
            Span::raw(":File Info  "),
        ]);

        // Open - only show if open_command is configured
        if has_open_command {
            hotkey_spans.extend(vec![
                Span::styled("o", Style::default().fg(Color::Yellow)),
                Span::raw(":Open  "),
            ]);
        }

        hotkey_spans.extend(vec![
            Span::styled("i", Style::default().fg(Color::Yellow)),
            Span::raw(":Ignore  "),
            Span::styled("I", Style::default().fg(Color::Yellow)),
            Span::raw(":Ign+Del  "),
            Span::styled("d", Style::default().fg(Color::Yellow)),
            Span::raw(":Delete  "),
        ]);
    }

    // Rescan - available in both folder list and breadcrumbs
    hotkey_spans.extend(vec![
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(":Rescan  "),
    ]);

    // Restore - only show when there are local changes to restore
    if can_restore {
        hotkey_spans.extend(vec![
            Span::styled("R", Style::default().fg(Color::Yellow)),
            Span::raw(":Restore  "),
        ]);
    }

    // Quit - always available
    hotkey_spans.extend(vec![
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
