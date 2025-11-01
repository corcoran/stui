//! Navigation selection logic
//!
//! Pure functions for calculating navigation selection indices with wrapping behavior.

/// Calculate the next selection index with wrapping
///
/// Advances the selection to the next item in the list. If at the end,
/// wraps around to the beginning. If no item is selected, selects the first item.
///
/// # Arguments
/// * `current` - Current selection index (None if no selection)
/// * `list_len` - Total number of items in the list
///
/// # Returns
/// * `Some(index)` - The next selection index
/// * `None` - If the list is empty
///
/// # Examples
/// ```
/// use synctui::logic::navigation::next_selection;
///
/// // Empty list
/// assert_eq!(next_selection(None, 0), None);
///
/// // Normal progression
/// assert_eq!(next_selection(None, 3), Some(0));
/// assert_eq!(next_selection(Some(0), 3), Some(1));
/// assert_eq!(next_selection(Some(1), 3), Some(2));
///
/// // Wrapping at end
/// assert_eq!(next_selection(Some(2), 3), Some(0));
/// ```
pub fn next_selection(current: Option<usize>, list_len: usize) -> Option<usize> {
    if list_len == 0 {
        return None;
    }

    Some(match current {
        Some(i) if i >= list_len - 1 => 0, // Wrap to start
        Some(i) => i + 1,
        None => 0,
    })
}

/// Calculate the previous selection index with wrapping
///
/// Moves the selection to the previous item in the list. If at the beginning,
/// wraps around to the end. If no item is selected, selects the last item.
///
/// # Arguments
/// * `current` - Current selection index (None if no selection)
/// * `list_len` - Total number of items in the list
///
/// # Returns
/// * `Some(index)` - The previous selection index
/// * `None` - If the list is empty
///
/// # Examples
/// ```
/// use synctui::logic::navigation::prev_selection;
///
/// // Empty list
/// assert_eq!(prev_selection(None, 0), None);
///
/// // Normal progression
/// assert_eq!(prev_selection(Some(2), 3), Some(1));
/// assert_eq!(prev_selection(Some(1), 3), Some(0));
///
/// // Wrapping at beginning
/// assert_eq!(prev_selection(Some(0), 3), Some(2));
/// assert_eq!(prev_selection(None, 3), Some(2));
/// ```
pub fn prev_selection(current: Option<usize>, list_len: usize) -> Option<usize> {
    if list_len == 0 {
        return None;
    }

    Some(match current {
        Some(0) | None => list_len - 1, // Wrap to end
        Some(i) => i - 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_selection_empty_list() {
        // Empty list should return None
        assert_eq!(next_selection(None, 0), None);
        assert_eq!(next_selection(Some(0), 0), None);
        assert_eq!(next_selection(Some(5), 0), None);
    }

    #[test]
    fn test_next_selection_no_current() {
        // No current selection should select first item
        assert_eq!(next_selection(None, 3), Some(0));
        assert_eq!(next_selection(None, 1), Some(0));
        assert_eq!(next_selection(None, 10), Some(0));
    }

    #[test]
    fn test_next_selection_normal_progression() {
        // Normal forward progression
        assert_eq!(next_selection(Some(0), 3), Some(1));
        assert_eq!(next_selection(Some(1), 3), Some(2));
        assert_eq!(next_selection(Some(0), 5), Some(1));
        assert_eq!(next_selection(Some(3), 5), Some(4));
    }

    #[test]
    fn test_next_selection_wrapping() {
        // Wrap to start when at end
        assert_eq!(next_selection(Some(2), 3), Some(0));
        assert_eq!(next_selection(Some(4), 5), Some(0));
        assert_eq!(next_selection(Some(0), 1), Some(0)); // Single item wraps to itself
    }

    #[test]
    fn test_prev_selection_empty_list() {
        // Empty list should return None
        assert_eq!(prev_selection(None, 0), None);
        assert_eq!(prev_selection(Some(0), 0), None);
        assert_eq!(prev_selection(Some(5), 0), None);
    }

    #[test]
    fn test_prev_selection_no_current() {
        // No current selection should select last item
        assert_eq!(prev_selection(None, 3), Some(2));
        assert_eq!(prev_selection(None, 1), Some(0));
        assert_eq!(prev_selection(None, 10), Some(9));
    }

    #[test]
    fn test_prev_selection_normal_progression() {
        // Normal backward progression
        assert_eq!(prev_selection(Some(2), 3), Some(1));
        assert_eq!(prev_selection(Some(1), 3), Some(0));
        assert_eq!(prev_selection(Some(4), 5), Some(3));
        assert_eq!(prev_selection(Some(1), 5), Some(0));
    }

    #[test]
    fn test_prev_selection_wrapping() {
        // Wrap to end when at beginning
        assert_eq!(prev_selection(Some(0), 3), Some(2));
        assert_eq!(prev_selection(Some(0), 5), Some(4));
        assert_eq!(prev_selection(Some(0), 1), Some(0)); // Single item wraps to itself
    }

    #[test]
    fn test_selection_single_item() {
        // Single item list behavior
        assert_eq!(next_selection(None, 1), Some(0));
        assert_eq!(next_selection(Some(0), 1), Some(0));
        assert_eq!(prev_selection(None, 1), Some(0));
        assert_eq!(prev_selection(Some(0), 1), Some(0));
    }

    #[test]
    fn test_selection_out_of_bounds() {
        // Should handle out-of-bounds indices gracefully
        assert_eq!(next_selection(Some(10), 3), Some(0)); // Way past end wraps
        assert_eq!(prev_selection(Some(10), 3), Some(9)); // Way past end goes back one
    }
}
