use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout information for rendering
pub struct LayoutInfo {
    /// Top system info bar area
    pub system_area: Rect,
    /// Folders pane area (if visible)
    pub folders_area: Option<Rect>,
    /// Breadcrumb pane areas
    pub breadcrumb_areas: Vec<Rect>,
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
) -> LayoutInfo {
    // Create main layout: system bar (top) + content area + legend + status bar (bottom)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // System info bar (3 lines: top border, text, bottom border)
            Constraint::Min(3),      // Content area (folders + breadcrumbs)
            Constraint::Length(3),   // Legend area (3 lines: top border, text, bottom border)
            Constraint::Length(3),   // Status bar (3 lines: top border, text, bottom border)
        ])
        .split(terminal_size);

    let system_area = main_chunks[0];
    let content_area = main_chunks[1];
    let legend_area = main_chunks[2];
    let status_area = main_chunks[3];

    // Calculate how many panes we need (folders + breadcrumb levels)
    let num_panes = 1 + num_breadcrumb_levels;

    // Determine visible panes based on terminal width
    let min_pane_width = 20;
    let max_visible_panes = (content_area.width / min_pane_width).max(2) as usize;

    // Calculate which panes to show (prioritize right side)
    let start_pane = if num_panes > max_visible_panes {
        num_panes - max_visible_panes
    } else {
        0
    };

    let visible_panes = num_panes.min(max_visible_panes);
    let folders_visible = start_pane == 0;

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
    let (folders_area, breadcrumb_areas) = if has_breadcrumbs && folders_visible && chunks.len() > 1 {
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
        legend_area: Some(legend_area), // Always show legend
        status_area,
        start_pane,
        folders_visible,
    }
}
