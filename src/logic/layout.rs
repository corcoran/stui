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

/// Rect inside `area` that an image of `dims` pixels fills when scaled to fit and
/// centered, given the terminal's cell size in pixels.
pub fn fit_image_rect(
    area: ratatui::layout::Rect,
    dims: Option<(u32, u32)>,
    font: Option<(u16, u16)>,
) -> ratatui::layout::Rect {
    let (Some((img_w, img_h)), Some((font_w, font_h))) = (dims, font) else {
        return area;
    };

    let desired_width = ((img_w as f32) / (font_w as f32)).ceil() as u16;
    let desired_height = ((img_h as f32) / (font_h as f32)).ceil() as u16;

    let (fit_width, fit_height) = if desired_width <= area.width && desired_height <= area.height {
        (desired_width, desired_height)
    } else {
        let desired_aspect = desired_width as f32 / desired_height as f32;
        let area_aspect = area.width as f32 / area.height as f32;
        if desired_aspect > area_aspect {
            let width = area.width;
            (width, (width as f32 / desired_aspect) as u16)
        } else {
            let height = area.height;
            ((height as f32 * desired_aspect) as u16, height)
        }
    };

    ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(fit_width) / 2,
        y: area.y + area.height.saturating_sub(fit_height) / 2,
        width: fit_width,
        height: fit_height,
    }
}

/// Rows `text` occupies when word-wrapped to `width` columns.
pub fn wrapped_line_count(text: &ratatui::text::Text, width: u16) -> usize {
    if width == 0 {
        return text.lines.len();
    }
    ratatui::widgets::Paragraph::new(text.clone())
        .wrap(ratatui::widgets::Wrap { trim: false })
        .line_count(width)
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

    // ========================================
    // IMAGE FIT
    // ========================================

    use ratatui::layout::Rect;

    const INNER: Rect = Rect {
        x: 10,
        y: 5,
        width: 80,
        height: 40,
    };

    #[test]
    fn test_fit_full_width_landscape_fills_width() {
        // 1600x1200 at 10x20 px cells wants 160x60 cells: width-constrained
        let r = fit_image_rect(INNER, Some((1600, 1200)), Some((10, 20)));
        assert_eq!((r.width, r.height), (80, 30));
        assert_eq!((r.x, r.y), (10, 5 + 5));
    }

    #[test]
    fn test_fit_portrait_is_height_constrained() {
        let r = fit_image_rect(INNER, Some((1200, 1600)), Some((10, 20)));
        assert_eq!(r.height, 40);
        assert_eq!(r.y, INNER.y);
    }

    #[test]
    fn test_fit_small_image_keeps_desired_size_and_centers() {
        // 200x100 px at 10x20 wants 20x5 cells
        let r = fit_image_rect(INNER, Some((200, 100)), Some((10, 20)));
        assert_eq!((r.width, r.height), (20, 5));
        assert_eq!(r.x, 10 + (80 - 20) / 2);
    }

    #[test]
    fn test_fit_without_dimensions_uses_area() {
        assert_eq!(fit_image_rect(INNER, None, Some((10, 20))), INNER);
        assert_eq!(fit_image_rect(INNER, Some((10, 10)), None), INNER);
    }

    // ========================================
    // WRAPPED LINE COUNT
    // ========================================

    use ratatui::text::Text;

    #[test]
    fn test_wrapped_line_count_counts_wrapped_rows() {
        let text = Text::from("x".repeat(100));
        assert_eq!(wrapped_line_count(&text, 40), 3);
    }

    #[test]
    fn test_wrapped_line_count_short_lines_unchanged() {
        let text = Text::from("one\ntwo\nthree");
        assert_eq!(wrapped_line_count(&text, 40), 3);
    }

    #[test]
    fn test_wrapped_line_count_zero_width_falls_back_to_lines() {
        let text = Text::from("one\ntwo");
        assert_eq!(wrapped_line_count(&text, 0), 2);
    }
}
