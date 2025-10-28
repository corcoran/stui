use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

/// Render the revert confirmation dialog (for restoring deleted files in receive-only folders)
pub fn render_revert_confirmation(
    f: &mut Frame,
    changed_files: &[String],
) {
    let file_list = changed_files.iter()
        .take(5)
        .map(|file| format!("  - {}", file))
        .collect::<Vec<_>>()
        .join("\n");

    let more_text = if changed_files.len() > 5 {
        format!("\n  ... and {} more", changed_files.len() - 5)
    } else {
        String::new()
    };

    let prompt_text = format!(
        "Revert folder to restore deleted files?\n\n\
        WARNING: This will remove {} local change(s):\n{}{}\n\n\
        Continue? (y/n)",
        changed_files.len(),
        file_list,
        more_text
    );

    // Center the prompt - adjust height based on number of files shown
    let area = f.size();
    let prompt_width = 60;
    let base_height = 10;
    let file_lines = changed_files.len().min(5);
    let prompt_height = base_height + file_lines as u16;
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

    let prompt = Paragraph::new(prompt_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Confirm Revert")
            .border_style(Style::default().fg(Color::Yellow)))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(prompt, prompt_area);
}

/// Render the delete confirmation dialog
pub fn render_delete_confirmation(
    f: &mut Frame,
    display_name: &str,
    is_dir: bool,
) {
    let item_type = if is_dir { "directory" } else { "file" };

    let prompt_text = format!(
        "Delete {} from disk?\n\n\
        {}: {}\n\n\
        WARNING: This action cannot be undone!\n\n\
        Continue? (y/n)",
        item_type,
        if is_dir { "Directory" } else { "File" },
        display_name
    );

    // Center the prompt
    let area = f.size();
    let prompt_width = 50;
    let prompt_height = 11;
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

    let prompt = Paragraph::new(prompt_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Confirm Delete")
            .border_style(Style::default().fg(Color::Red)))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(prompt, prompt_area);
}

/// Render the pattern selection menu (for removing ignore patterns)
pub fn render_pattern_selection(
    f: &mut Frame,
    patterns: &[String],
    state: &mut ListState,
) {
    let menu_items: Vec<ListItem> = patterns
        .iter()
        .map(|pattern| {
            ListItem::new(Span::raw(pattern.clone()))
                .style(Style::default().fg(Color::White))
        })
        .collect();

    // Center the menu
    let area = f.size();
    let menu_width = 60;
    let menu_height = (patterns.len() as u16 + 6).min(20); // +6 for borders and instructions
    let menu_area = Rect {
        x: (area.width.saturating_sub(menu_width)) / 2,
        y: (area.height.saturating_sub(menu_height)) / 2,
        width: menu_width,
        height: menu_height,
    };

    let menu = List::new(menu_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Select Pattern to Remove (↑↓ to navigate, Enter to remove, Esc to cancel)")
            .border_style(Style::default().fg(Color::Yellow)))
        .highlight_style(Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD))
        .highlight_symbol("► ");

    f.render_widget(ratatui::widgets::Clear, menu_area);
    f.render_stateful_widget(menu, menu_area, state);
}
