use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout information for rendering
pub struct LayoutInfo {
    /// Top system info bar area
    pub system_area: Rect,
    /// Folders pane area (if visible)
    pub folders_area: Option<Rect>,
    /// Breadcrumb pane areas
    pub breadcrumb_areas: Vec<Rect>,
    /// Search input area (if visible)
    pub search_area: Option<Rect>,
    /// Hotkey legend area (full width)
    pub legend_area: Option<Rect>,
    /// Bottom status bar area
    pub status_area: Rect,
    /// Index of first visible pane (for scrolling)
    pub start_pane: usize,
    /// Whether folders pane is visible
    pub folders_visible: bool,
}

/// Calculate the screen layout for all UI components
pub fn calculate_layout(
    terminal_size: Rect,
    num_breadcrumb_levels: usize,
    has_breadcrumbs: bool,
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    status_height: u16,
    search_visible: bool,
) -> LayoutInfo {
    // Calculate dynamic legend height based on terminal width and content
    // Note: We use search_visible for both parameters since we need visibility for layout
    let legend_height = super::legend::calculate_legend_height(
        terminal_size.width,
        vim_mode,
        focus_level,
        can_restore,
        has_open_command,
        search_visible, // search_mode (approximation for layout)
        search_visible, // has_search_query (approximation for layout)
    );

    let search_height = if search_visible { 3 } else { 0 };

    // Create main layout: system bar (top) + content area + search + legend + status bar (bottom)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),            // System info bar (3 lines: top border, text, bottom border)
            Constraint::Min(3),               // Content area (folders + breadcrumbs)
            Constraint::Length(search_height), // Search input (3 lines when visible, 0 when hidden)
            Constraint::Length(legend_height), // Legend area (dynamic height, exact fit for wrapped content)
            Constraint::Length(status_height), // Status bar (dynamic height, exact fit for wrapped content)
        ])
        .split(terminal_size);

    let system_area = main_chunks[0];
    let content_area = main_chunks[1];
    let search_area = if search_visible {
        Some(main_chunks[2])
    } else {
        None
    };
    let legend_area = main_chunks[3];
    let status_area = main_chunks[4];

    // Calculate which panes should be visible based on available width
    let pane_range = crate::logic::layout::calculate_visible_pane_range(
        content_area.width,
        num_breadcrumb_levels,
    );
    let start_pane = pane_range.start_pane;
    let visible_panes = pane_range.visible_panes;
    let folders_visible = pane_range.folders_visible;

    // Create horizontal split for all panes
    // Give more space to the rightmost (current) pane
    let constraints: Vec<Constraint> = if visible_panes == 1 {
        vec![Constraint::Percentage(100)]
    } else if visible_panes == 2 {
        // 40% for parent, 60% for current
        vec![Constraint::Percentage(40), Constraint::Percentage(60)]
    } else {
        // For 3+ panes: current gets 50%, rest split the remaining 50%
        let mut c = Vec::new();
        let parent_panes = visible_panes - 1;
        for _ in 0..parent_panes {
            c.push(Constraint::Ratio(1, parent_panes as u32 * 2)); // Each parent gets 1/(2*parent_panes)
        }
        c.push(Constraint::Percentage(50)); // Current gets 50%
        c
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(content_area);

    // Determine which areas to use for folders and breadcrumbs
    let (folders_area, breadcrumb_areas) = if has_breadcrumbs && folders_visible && chunks.len() > 1
    {
        // First chunk is folders, rest are breadcrumbs
        let bc: Vec<Rect> = chunks[1..].to_vec();
        (Some(chunks[0]), bc)
    } else if has_breadcrumbs && !folders_visible {
        // All chunks are breadcrumbs
        (None, chunks.to_vec())
    } else if folders_visible {
        // Only folders visible
        (Some(chunks[0]), Vec::new())
    } else {
        // No folders, use all chunks for breadcrumbs
        (None, chunks.to_vec())
    };

    LayoutInfo {
        system_area,
        folders_area,
        breadcrumb_areas,
        search_area,
        legend_area: Some(legend_area), // Always show legend
        status_area,
        start_pane,
        folders_visible,
    }
}
