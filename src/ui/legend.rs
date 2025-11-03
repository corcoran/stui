use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Build hotkey spans (extracted for testability)
fn build_hotkey_spans(
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    search_mode: bool,
    has_search_query: bool,
) -> Vec<Span<'static>> {
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
            Span::styled("o", Style::default().fg(Color::Yellow)),
            Span::raw(":Open Syncthing Web UI  "),
        ]);
    }

    // Actions that only apply to breadcrumbs (focus_level > 0), not folders
    if focus_level > 0 {
        hotkey_spans.extend(vec![
            Span::styled("c", Style::default().fg(Color::Yellow)),
            Span::raw(":Copy path  "),
        ]);

        // Search key - contextual based on search state
        if search_mode {
            // Actively typing in search input
            hotkey_spans.extend(vec![
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(":Exit Search  "),
            ]);
        } else if has_search_query {
            // Search accepted (Enter pressed), showing filtered results
            hotkey_spans.extend(vec![
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(":Clear Search  "),
            ]);
        } else {
            // No active search, show trigger key
            let search_key = if vim_mode { "/" } else { "^F" };
            hotkey_spans.extend(vec![
                Span::styled(search_key, Style::default().fg(Color::Yellow)),
                Span::raw(":Search  "),
            ]);
        }

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

    hotkey_spans
}

/// Build the legend paragraph (reusable for both rendering and height calculation)
pub fn build_legend_paragraph(
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    search_mode: bool,
    has_search_query: bool,
) -> Paragraph<'static> {
    let hotkey_spans = build_hotkey_spans(
        vim_mode,
        focus_level,
        can_restore,
        has_open_command,
        search_mode,
        has_search_query,
    );
    let hotkey_line = Line::from(hotkey_spans);

    Paragraph::new(vec![hotkey_line])
        .block(Block::default().borders(Borders::ALL).title("Hotkeys"))
        .style(Style::default().fg(Color::Gray))
        .wrap(ratatui::widgets::Wrap { trim: false })
}

/// Render the hotkey legend (dynamically changes based on vim mode and focus level)
pub fn render_legend(
    f: &mut Frame,
    area: Rect,
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    search_mode: bool,
    has_search_query: bool,
) {
    let legend = build_legend_paragraph(
        vim_mode,
        focus_level,
        can_restore,
        has_open_command,
        search_mode,
        has_search_query,
    );
    f.render_widget(legend, area);
}

/// Calculate required height for legend based on terminal width and content
pub fn calculate_legend_height(
    terminal_width: u16,
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    search_mode: bool,
    has_search_query: bool,
) -> u16 {
    // Build paragraph WITHOUT block borders for accurate line counting
    // (line_count() doesn't account for borders correctly when block is attached)
    let hotkey_spans = build_hotkey_spans(
        vim_mode,
        focus_level,
        can_restore,
        has_open_command,
        search_mode,
        has_search_query,
    );
    let hotkey_line = Line::from(hotkey_spans);

    // Create paragraph WITHOUT block for accurate line counting
    let paragraph_for_counting = Paragraph::new(vec![hotkey_line])
        .wrap(ratatui::widgets::Wrap { trim: false });

    // Calculate available width (subtract left + right borders)
    let available_width = terminal_width.saturating_sub(2);

    // Get exact line count for wrapped text
    let line_count = paragraph_for_counting.line_count(available_width);

    // Add top + bottom borders, ensure minimum of 3
    (line_count as u16).saturating_add(2).max(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to convert spans to plain text for assertions
    fn spans_to_text(spans: &[Span]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn test_legend_shows_open_web_ui_in_folder_view() {
        // In folder view (focus_level == 0), legend should show "o:Open Syncthing Web UI"
        let spans = build_hotkey_spans(
            false, // vim_mode
            0,     // focus_level (folder view)
            false, // can_restore
            true,  // has_open_command
            false, // search_mode
            false, // has_search_query
        );

        let text = spans_to_text(&spans);
        assert!(
            text.contains("o") && text.contains("Open Syncthing Web UI"),
            "Legend in folder view should contain 'o:Open Syncthing Web UI', got: {}",
            text
        );
    }

    #[test]
    fn test_legend_shows_open_in_breadcrumb_view_with_command() {
        // In breadcrumb view (focus_level > 0) with open_command, legend should show "o:Open"
        let spans = build_hotkey_spans(
            false, // vim_mode
            1,     // focus_level (breadcrumb view)
            false, // can_restore
            true,  // has_open_command
            false, // search_mode
            false, // has_search_query
        );

        let text = spans_to_text(&spans);
        assert!(
            text.contains("o") && text.contains("Open"),
            "Legend in breadcrumb view should contain 'o:Open', got: {}",
            text
        );
        // Should NOT contain "Open Syncthing Web UI" - that's folder view only
        assert!(
            !text.contains("Open Syncthing Web UI"),
            "Legend in breadcrumb view should not contain 'Open Syncthing Web UI', got: {}",
            text
        );
    }

    #[test]
    fn test_legend_hides_open_in_breadcrumb_view_without_command() {
        // In breadcrumb view without open_command, legend should NOT show 'o' key
        let spans = build_hotkey_spans(
            false, // vim_mode
            1,     // focus_level (breadcrumb view)
            false, // can_restore
            false, // has_open_command (no command configured)
            false, // search_mode
            false, // has_search_query
        );

        let text = spans_to_text(&spans);

        // Should still have other keys like 'c' for copy, 'i' for ignore, etc.
        assert!(text.contains("c"), "Legend should still contain other keys");

        // But should NOT contain "o:Open" since open_command is not configured
        // Note: We need to be careful here - 'o' might appear in other text like "Info"
        // So we check for the pattern "o:" which is how hotkeys are displayed
        let has_o_key = text.split_whitespace().any(|word| word.starts_with("o:"));
        assert!(
            !has_o_key,
            "Legend in breadcrumb view without open_command should not show 'o' key, got: {}",
            text
        );
    }

    #[test]
    fn test_legend_always_shows_open_web_ui_in_folder_view_even_without_command() {
        // In folder view, "o:Open Syncthing Web UI" should ALWAYS be shown (for discoverability)
        // even if open_command is not configured (will show error toast when pressed)
        let spans = build_hotkey_spans(
            false, // vim_mode
            0,     // focus_level (folder view)
            false, // can_restore
            false, // has_open_command (no command configured)
            false, // search_mode
            false, // has_search_query
        );

        let text = spans_to_text(&spans);
        assert!(
            text.contains("o") && text.contains("Open Syncthing Web UI"),
            "Legend in folder view should ALWAYS show 'o:Open Syncthing Web UI' for discoverability, got: {}",
            text
        );
    }

    #[test]
    fn test_legend_context_aware_behavior() {
        // Test that 'o' key behavior changes based on focus_level

        // Folder view (focus_level == 0)
        let folder_spans = build_hotkey_spans(false, 0, false, true, false, false);
        let folder_text = spans_to_text(&folder_spans);

        // Breadcrumb view (focus_level > 0)
        let breadcrumb_spans = build_hotkey_spans(false, 1, false, true, false, false);
        let breadcrumb_text = spans_to_text(&breadcrumb_spans);

        // Verify they're different
        assert!(
            folder_text.contains("Open Syncthing Web UI"),
            "Folder view should mention 'Open Syncthing Web UI'"
        );
        assert!(
            !breadcrumb_text.contains("Open Syncthing Web UI"),
            "Breadcrumb view should not mention 'Open Syncthing Web UI'"
        );
        assert!(
            breadcrumb_text.contains("Copy path"),
            "Breadcrumb view should have 'Copy path' option"
        );
    }
}
