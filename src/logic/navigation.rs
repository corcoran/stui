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
/// use stui::logic::navigation::next_selection;
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
/// use stui::logic::navigation::prev_selection;
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

/// Find the index of an item in a list by its name
///
/// Searches for an item with the given name and returns its index.
/// Used for preserving selection after sorting or filtering operations.
///
/// # Arguments
/// * `items` - Slice of browse items to search
/// * `name` - Name to search for (case-sensitive)
///
/// # Returns
/// * `Some(index)` - Index of the item if found
/// * `None` - If item not found or list is empty
///
/// # Examples
/// ```
/// use stui::logic::navigation::find_item_index_by_name;
/// use stui::api::BrowseItem;
///
/// let items = vec![
///     BrowseItem {
///         name: "a.txt".to_string(),
///         size: 0,
///         mod_time: "2023-01-01T00:00:00Z".to_string(),
///         item_type: "FILE_INFO_TYPE_FILE".to_string(),
///     },
///     BrowseItem {
///         name: "b.txt".to_string(),
///         size: 0,
///         mod_time: "2023-01-01T00:00:00Z".to_string(),
///         item_type: "FILE_INFO_TYPE_FILE".to_string(),
///     },
/// ];
///
/// assert_eq!(find_item_index_by_name(&items, "b.txt"), Some(1));
/// assert_eq!(find_item_index_by_name(&items, "z.txt"), None);
/// ```
pub fn find_item_index_by_name(items: &[crate::api::BrowseItem], name: &str) -> Option<usize> {
    items.iter().position(|item| item.name == name)
}

/// Index of the nearest file after `from` (or before it when `forward` is false),
/// skipping directories. `None` when there is no such file.
pub fn adjacent_file_index(
    items: &[crate::api::BrowseItem],
    from: usize,
    forward: bool,
) -> Option<usize> {
    let is_file = |i: &usize| items[*i].item_type != "FILE_INFO_TYPE_DIRECTORY";
    if forward {
        (from.saturating_add(1)..items.len()).find(is_file)
    } else {
        (0..from.min(items.len())).rev().find(is_file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // NEXT SELECTION
    // ========================================

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

    // ========================================
    // PREV SELECTION
    // ========================================

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

    // ========================================
    // SELECTION EDGE CASES
    // ========================================

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

#[cfg(test)]
mod find_item_tests {
    use super::*;
    use crate::api::BrowseItem;

    fn make_item(name: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: "2023-01-01T00:00:00Z".to_string(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        }
    }

    // ========================================
    // FIND ITEM BY NAME
    // ========================================

    #[test]
    fn test_find_item_index_by_name_found() {
        let items = vec![make_item("a.txt"), make_item("b.txt"), make_item("c.txt")];

        assert_eq!(find_item_index_by_name(&items, "a.txt"), Some(0));
        assert_eq!(find_item_index_by_name(&items, "b.txt"), Some(1));
        assert_eq!(find_item_index_by_name(&items, "c.txt"), Some(2));
    }

    #[test]
    fn test_find_item_index_by_name_not_found() {
        let items = vec![make_item("a.txt"), make_item("b.txt")];

        assert_eq!(find_item_index_by_name(&items, "z.txt"), None);
        assert_eq!(find_item_index_by_name(&items, ""), None);
    }

    #[test]
    fn test_find_item_index_by_name_empty_list() {
        let items: Vec<BrowseItem> = vec![];

        assert_eq!(find_item_index_by_name(&items, "any.txt"), None);
    }

    #[test]
    fn test_find_item_index_by_name_case_sensitive() {
        let items = vec![make_item("File.txt"), make_item("file.txt")];

        // Should be case-sensitive
        assert_eq!(find_item_index_by_name(&items, "File.txt"), Some(0));
        assert_eq!(find_item_index_by_name(&items, "file.txt"), Some(1));
        assert_eq!(find_item_index_by_name(&items, "FILE.TXT"), None);
    }

    // ========================================
    // ADJACENT FILE
    // ========================================

    fn make_dir(name: &str) -> BrowseItem {
        BrowseItem {
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            ..make_item(name)
        }
    }

    #[test]
    fn test_adjacent_file_next_and_previous() {
        let items = vec![make_item("a"), make_item("b"), make_item("c")];
        assert_eq!(adjacent_file_index(&items, 1, true), Some(2));
        assert_eq!(adjacent_file_index(&items, 1, false), Some(0));
    }

    #[test]
    fn test_adjacent_file_skips_directories() {
        let items = vec![
            make_item("a"),
            make_dir("d1"),
            make_dir("d2"),
            make_item("b"),
        ];
        assert_eq!(adjacent_file_index(&items, 0, true), Some(3));
        assert_eq!(adjacent_file_index(&items, 3, false), Some(0));
    }

    #[test]
    fn test_adjacent_file_stops_at_ends() {
        let items = vec![make_item("a"), make_item("b")];
        assert_eq!(adjacent_file_index(&items, 1, true), None);
        assert_eq!(adjacent_file_index(&items, 0, false), None);
    }

    #[test]
    fn test_adjacent_file_none_when_only_directories_remain() {
        let items = vec![make_dir("d0"), make_item("a"), make_dir("d1")];
        assert_eq!(adjacent_file_index(&items, 1, true), None);
        assert_eq!(adjacent_file_index(&items, 1, false), None);
    }

    #[test]
    fn test_adjacent_file_out_of_range_start() {
        let items = vec![make_item("a"), make_item("b")];
        assert_eq!(adjacent_file_index(&items, 5, true), None);
        assert_eq!(adjacent_file_index(&items, 5, false), Some(1));
        assert_eq!(adjacent_file_index(&[], 0, true), None);
    }
}
