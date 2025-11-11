//! Layout calculation logic
//!
//! Pure functions for calculating UI layout dimensions and constraints.

/// Information about which panes are visible in the breadcrumb navigation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisiblePaneRange {
    /// Index of first visible pane (0 = folders list, 1+ = breadcrumb levels)
    pub start_pane: usize,
    /// Number of panes that can fit on screen
    pub visible_panes: usize,
    /// Whether the folders list pane is visible
    pub folders_visible: bool,
}

/// Calculate which panes should be visible based on available width
///
/// When there are more panes than can fit on screen, this function determines
/// which subset to show, prioritizing the rightmost (current) panes.
///
/// # Arguments
/// * `content_width` - Available horizontal space in terminal cells
/// * `num_breadcrumb_levels` - Number of breadcrumb navigation levels
///
/// # Returns
/// `VisiblePaneRange` indicating which panes to display
///
/// # Layout Strategy
/// - Each pane needs minimum 20 cells width
/// - Total panes = 1 (folders) + breadcrumb_levels
/// - At least 2 panes shown when possible
/// - Prioritize rightmost (current) breadcrumbs when scrolling
///
/// # Examples
/// ```
/// use stui::logic::layout::calculate_visible_pane_range;
///
/// // Wide terminal: 100 cells, 3 breadcrumb levels = 4 total panes
/// // Min width = 20, max panes = 100/20 = 5, all 4 panes fit
/// let range = calculate_visible_pane_range(100, 3);
/// assert_eq!(range.start_pane, 0);
/// assert_eq!(range.visible_panes, 4);
/// assert!(range.folders_visible);
///
/// // Narrow terminal: 60 cells, 5 breadcrumb levels = 6 total panes
/// // Max visible = 60/20 = 3 panes, show last 3 (hide folders + 2 breadcrumbs)
/// let range = calculate_visible_pane_range(60, 5);
/// assert_eq!(range.start_pane, 3);  // Skip first 3 panes
/// assert_eq!(range.visible_panes, 3);
/// assert!(!range.folders_visible);  // Folders pane hidden
/// ```
pub fn calculate_visible_pane_range(
    content_width: u16,
    num_breadcrumb_levels: usize,
) -> VisiblePaneRange {
    const MIN_PANE_WIDTH: u16 = 20;

    // Calculate how many panes we need (folders + breadcrumb levels)
    let num_panes = 1 + num_breadcrumb_levels;

    // Determine visible panes based on terminal width
    let max_visible_panes = (content_width / MIN_PANE_WIDTH).max(2) as usize;

    // Calculate which panes to show (prioritize right side)
    let start_pane = num_panes.saturating_sub(max_visible_panes);

    let visible_panes = num_panes.min(max_visible_panes);
    let folders_visible = start_pane == 0;

    VisiblePaneRange {
        start_pane,
        visible_panes,
        folders_visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_visible_pane_range_all_fit() {
        // Wide terminal: all panes fit
        let range = calculate_visible_pane_range(100, 3);
        assert_eq!(range.start_pane, 0);
        assert_eq!(range.visible_panes, 4); // 1 folder + 3 breadcrumbs
        assert!(range.folders_visible);
    }

    #[test]
    fn test_calculate_visible_pane_range_narrow_terminal() {
        // Narrow terminal: can only show 3 panes, hide leftmost
        let range = calculate_visible_pane_range(60, 5);
        assert_eq!(range.start_pane, 3); // Skip first 3 panes
        assert_eq!(range.visible_panes, 3);
        assert!(!range.folders_visible); // Folders pane hidden
    }

    #[test]
    fn test_calculate_visible_pane_range_exact_fit() {
        // Exactly enough space for all panes
        let range = calculate_visible_pane_range(80, 3);
        assert_eq!(range.start_pane, 0);
        assert_eq!(range.visible_panes, 4);
        assert!(range.folders_visible);
    }

    #[test]
    fn test_calculate_visible_pane_range_minimum_width() {
        // Very narrow: minimum 2 panes (enforced by max())
        let range = calculate_visible_pane_range(30, 5);
        assert_eq!(range.start_pane, 4); // Skip first 4 panes
        assert_eq!(range.visible_panes, 2);
        assert!(!range.folders_visible);
    }

    #[test]
    fn test_calculate_visible_pane_range_no_breadcrumbs() {
        // Only folders pane, no breadcrumbs
        let range = calculate_visible_pane_range(100, 0);
        assert_eq!(range.start_pane, 0);
        assert_eq!(range.visible_panes, 1);
        assert!(range.folders_visible);
    }

    #[test]
    fn test_calculate_visible_pane_range_one_breadcrumb() {
        // Folders + 1 breadcrumb = 2 panes
        let range = calculate_visible_pane_range(50, 1);
        assert_eq!(range.start_pane, 0);
        assert_eq!(range.visible_panes, 2);
        assert!(range.folders_visible);
    }
}
