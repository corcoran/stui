use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout information for rendering
pub struct LayoutInfo {
    /// Bottom status bar area
    pub status_area: Rect,
    /// Folders pane area (if visible)
    pub folders_area: Option<Rect>,
    /// Breadcrumb pane areas
    pub breadcrumb_areas: Vec<Rect>,
    /// Hotkey legend area (if visible)
    pub legend_area: Option<Rect>,
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
    // Create main layout: content area + status bar at bottom
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),      // Content area
            Constraint::Length(3),   // Status bar (3 lines: top border, text, bottom border)
        ])
        .split(terminal_size);

    let content_area = main_chunks[0];
    let status_area = main_chunks[1];

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

    // Split breadcrumb areas vertically if needed for legend
    let (folders_area, breadcrumb_areas, legend_area) = if has_breadcrumbs && folders_visible && chunks.len() > 1 {
        // Split all chunks except the first (folders) to make room for legend
        let breadcrumb_area = Rect {
            x: chunks[1].x,
            y: chunks[1].y,
            width: content_area.width - chunks[0].width,
            height: content_area.height,
        };

        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),      // Breadcrumb panes area
                Constraint::Length(3),   // Legend area (3 lines)
            ])
            .split(breadcrumb_area);

        // Create new chunks for breadcrumb panels (folders already takes first chunk)
        // Give more space to the rightmost (current) breadcrumb
        let num_breadcrumbs = visible_panes - 1;
        let breadcrumb_constraints: Vec<Constraint> = if num_breadcrumbs == 1 {
            vec![Constraint::Percentage(100)]
        } else if num_breadcrumbs == 2 {
            // 40% for parent, 60% for current
            vec![Constraint::Percentage(40), Constraint::Percentage(60)]
        } else {
            // For 3+ breadcrumbs: current gets 50%, rest split the remaining 50%
            let mut c = Vec::new();
            let parent_panes = num_breadcrumbs - 1;
            for _ in 0..parent_panes {
                c.push(Constraint::Ratio(1, parent_panes as u32 * 2));
            }
            c.push(Constraint::Percentage(50));
            c
        };

        let bc = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(breadcrumb_constraints)
            .split(split[0]);

        (Some(chunks[0]), bc.to_vec(), Some(split[1]))
    } else if has_breadcrumbs && !folders_visible {
        // No folders visible - split entire area
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),      // Panes area
                Constraint::Length(3),   // Legend area (3 lines)
            ])
            .split(content_area);

        // Give more space to the rightmost (current) breadcrumb
        let breadcrumb_constraints: Vec<Constraint> = if visible_panes == 1 {
            vec![Constraint::Percentage(100)]
        } else if visible_panes == 2 {
            // 40% for parent, 60% for current
            vec![Constraint::Percentage(40), Constraint::Percentage(60)]
        } else {
            // For 3+ breadcrumbs: current gets 50%, rest split the remaining 50%
            let mut c = Vec::new();
            let parent_panes = visible_panes - 1;
            for _ in 0..parent_panes {
                c.push(Constraint::Ratio(1, parent_panes as u32 * 2));
            }
            c.push(Constraint::Percentage(50));
            c
        };

        let bc = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(breadcrumb_constraints)
            .split(split[0]);

        (None, bc.to_vec(), Some(split[1]))
    } else if folders_visible {
        // Folders visible but no breadcrumbs or no legend needed
        (Some(chunks[0]), Vec::new(), None)
    } else {
        // No folders, use all chunks for breadcrumbs
        (None, chunks.to_vec(), None)
    };

    LayoutInfo {
        status_area,
        folders_area,
        breadcrumb_areas,
        legend_area,
        start_pane,
        folders_visible,
    }
}
