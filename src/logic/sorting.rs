//! Sorting comparison logic
//!
//! Pure functions for comparing browse items across different sort modes.

use crate::api::{BrowseItem, SyncState};
use crate::SortMode;
use std::cmp::Ordering;
use std::collections::HashMap;

/// Compare two browse items according to the given sort mode
///
/// # Arguments
/// * `a` - First item
/// * `b` - Second item
/// * `sort_mode` - Which attribute to sort by
/// * `reverse` - Whether to reverse the ordering
/// * `sync_states` - Map of item names to sync states (for VisualIndicator mode)
///
/// # Returns
/// Ordering indicating relative position (Less, Equal, Greater)
///
/// # Sort Rules
/// - Directories always come before files
/// - Within same type (dir/file), apply sort mode
/// - Alphabetical tie-breaking for VisualIndicator, LastModified, FileSize
pub fn compare_browse_items(
    a: &BrowseItem,
    b: &BrowseItem,
    sort_mode: SortMode,
    reverse: bool,
    sync_states: &HashMap<String, SyncState>,
) -> Ordering {
    // Always prioritize directories first
    let a_is_dir = a.item_type == "FILE_INFO_TYPE_DIRECTORY";
    let b_is_dir = b.item_type == "FILE_INFO_TYPE_DIRECTORY";

    if a_is_dir != b_is_dir {
        return if a_is_dir {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    let result = match sort_mode {
        SortMode::VisualIndicator => {
            // Sort by sync state priority
            let a_state = sync_states
                .get(&a.name)
                .copied()
                .unwrap_or(SyncState::Unknown);
            let b_state = sync_states
                .get(&b.name)
                .copied()
                .unwrap_or(SyncState::Unknown);

            let a_priority = crate::logic::sync_states::sync_state_priority(a_state);
            let b_priority = crate::logic::sync_states::sync_state_priority(b_state);

            a_priority
                .cmp(&b_priority)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
        SortMode::Alphabetical => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        SortMode::LastModified => {
            // Reverse order for modified time (newest first)
            b.mod_time
                .cmp(&a.mod_time)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
        SortMode::FileSize => {
            // Reverse order for size (largest first)
            b.size
                .cmp(&a.size)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
    };

    if reverse {
        result.reverse()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(name: &str, size: u64, mod_time: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size,
            mod_time: mod_time.to_string(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        }
    }

    fn make_dir(name: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: "2023-01-01T00:00:00Z".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
        }
    }

    #[test]
    fn test_compare_directories_before_files() {
        let dir = make_dir("dir");
        let file = make_file("file", 100, "2023-01-01T00:00:00Z");
        let states = HashMap::new();

        // Directory always comes first, regardless of sort mode or reverse
        assert_eq!(
            compare_browse_items(&dir, &file, SortMode::Alphabetical, false, &states),
            Ordering::Less
        );
        assert_eq!(
            compare_browse_items(&file, &dir, SortMode::Alphabetical, false, &states),
            Ordering::Greater
        );
        assert_eq!(
            compare_browse_items(&dir, &file, SortMode::Alphabetical, true, &states),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_alphabetical_mode() {
        let a = make_file("apple.txt", 100, "2023-01-01T00:00:00Z");
        let b = make_file("banana.txt", 100, "2023-01-01T00:00:00Z");
        let states = HashMap::new();

        // Case-insensitive alphabetical
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::Alphabetical, false, &states),
            Ordering::Less
        );
        assert_eq!(
            compare_browse_items(&b, &a, SortMode::Alphabetical, false, &states),
            Ordering::Greater
        );
    }

    #[test]
    fn test_compare_alphabetical_reverse() {
        let a = make_file("apple.txt", 100, "2023-01-01T00:00:00Z");
        let b = make_file("banana.txt", 100, "2023-01-01T00:00:00Z");
        let states = HashMap::new();

        // Reverse mode flips ordering
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::Alphabetical, true, &states),
            Ordering::Greater
        );
        assert_eq!(
            compare_browse_items(&b, &a, SortMode::Alphabetical, true, &states),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_visual_indicator_mode() {
        let a = make_file("a.txt", 100, "2023-01-01T00:00:00Z");
        let b = make_file("b.txt", 100, "2023-01-01T00:00:00Z");

        let mut states = HashMap::new();
        states.insert("a.txt".to_string(), SyncState::OutOfSync); // Priority 0
        states.insert("b.txt".to_string(), SyncState::Synced); // Priority 6

        // OutOfSync (0) < Synced (6)
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::VisualIndicator, false, &states),
            Ordering::Less
        );

        // Reverse mode flips
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::VisualIndicator, true, &states),
            Ordering::Greater
        );
    }

    #[test]
    fn test_compare_last_modified_mode() {
        let older = make_file("old.txt", 100, "2023-01-01T00:00:00Z");
        let newer = make_file("new.txt", 100, "2023-12-31T23:59:59Z");
        let states = HashMap::new();

        // Newer first (reverse chronological)
        assert_eq!(
            compare_browse_items(&older, &newer, SortMode::LastModified, false, &states),
            Ordering::Greater
        );
        assert_eq!(
            compare_browse_items(&newer, &older, SortMode::LastModified, false, &states),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_file_size_mode() {
        let small = make_file("small.txt", 100, "2023-01-01T00:00:00Z");
        let large = make_file("large.txt", 10000, "2023-01-01T00:00:00Z");
        let states = HashMap::new();

        // Larger first
        assert_eq!(
            compare_browse_items(&small, &large, SortMode::FileSize, false, &states),
            Ordering::Greater
        );
        assert_eq!(
            compare_browse_items(&large, &small, SortMode::FileSize, false, &states),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_tie_breaking_with_name() {
        let a = make_file("a.txt", 100, "2023-01-01T00:00:00Z");
        let b = make_file("b.txt", 100, "2023-01-01T00:00:00Z");

        let mut states = HashMap::new();
        states.insert("a.txt".to_string(), SyncState::Synced);
        states.insert("b.txt".to_string(), SyncState::Synced);

        // Same state, same size, same time → alphabetical tie-breaking
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::VisualIndicator, false, &states),
            Ordering::Less
        );
        assert_eq!(
            compare_browse_items(&a, &b, SortMode::FileSize, false, &states),
            Ordering::Less
        );
    }
}
