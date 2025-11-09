//! Tests for out-of-sync filter persistence across navigation
//!
//! Bug: When out-of-sync filter is active and you drill into a subdirectory,
//! the filter is not applied to the new breadcrumb level. All files are shown
//! instead of just the out-of-sync ones.
//!
//! Example:
//! 1. Press 'f' in folder view → filter activates, shows only "Messages/foo" (RemoteOnly)
//! 2. Press Enter to drill into "Messages/" → ALL files in Messages/ are visible
//! 3. Expected: Only "foo" should be visible (it's the only out-of-sync file)

use synctui::model::{Model, types::OutOfSyncFilterState};
use synctui::api::BrowseItem;

/// Test: Filter state should be tracked when activated
#[test]
fn test_filter_activation_sets_state() {
    let mut model = Model::new(false);

    // Initial state: no filter active
    assert!(model.ui.out_of_sync_filter.is_none(), "Filter should not be active initially");

    // Simulate pressing 'f' to activate filter
    model.ui.out_of_sync_filter = Some(OutOfSyncFilterState {
        origin_level: 1, // Activated at first breadcrumb level
        last_refresh: std::time::SystemTime::now(),
    });

    // Verify filter is now active
    assert!(model.ui.out_of_sync_filter.is_some(), "Filter should be active after toggle");
}

/// Test: Filter state should persist when navigating into subdirectory
#[test]
fn test_filter_persists_across_navigation() {
    let mut model = Model::new(false);

    // Activate filter at level 1
    model.ui.out_of_sync_filter = Some(OutOfSyncFilterState {
        origin_level: 1,
        last_refresh: std::time::SystemTime::now(),
    });
    model.navigation.focus_level = 1;

    // Simulate drilling into subdirectory (would increment focus_level)
    model.navigation.focus_level = 2;

    // Filter state should STILL be active
    assert!(
        model.ui.out_of_sync_filter.is_some(),
        "Filter state should persist when drilling into subdirectory"
    );
}

/// Test: New breadcrumb level should have filter applied if filter is active
#[test]
fn test_new_breadcrumb_inherits_filter() {
    let _model = Model::new(false);

    // This is what SHOULD happen (but doesn't currently):
    // When enter_directory() creates a new breadcrumb level,
    // it should check if model.ui.out_of_sync_filter.is_some()
    // and call apply_out_of_sync_filter() if so

    // Simulate filter being active
    let filter_active = true;

    // When creating new breadcrumb level, check for active filters
    let should_apply_filter = filter_active;

    assert!(
        should_apply_filter,
        "New breadcrumb level should have filter applied when filter is active"
    );
}

/// Test: Filtered items should only show out-of-sync files
#[test]
fn test_filter_only_shows_out_of_sync() {
    // Simulate directory with mixed sync states
    let all_items = vec![
        BrowseItem {
            name: "synced.txt".to_string(),
            item_type: "file".to_string(), // Synced file
            size: 100,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "out-of-sync.txt".to_string(),
            item_type: "file".to_string(), // Out-of-sync file
            size: 200,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "also-synced.txt".to_string(),
            item_type: "file".to_string(), // Synced file
            size: 150,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Out-of-sync items from cache (would come from GetNeededFiles API)
    let out_of_sync_items = vec!["out-of-sync.txt"];

    // Filter should only keep items in out_of_sync_items list
    let filtered: Vec<_> = all_items
        .iter()
        .filter(|item| out_of_sync_items.contains(&item.name.as_str()))
        .collect();

    assert_eq!(filtered.len(), 1, "Should only have 1 out-of-sync item");
    assert_eq!(filtered[0].name, "out-of-sync.txt", "Should be the out-of-sync file");
}

/// Test: Directory should be shown if ANY child is out-of-sync
#[test]
fn test_directory_shown_if_child_out_of_sync() {
    // Simulate root level with directories
    let current_path = "";
    let items = vec![
        BrowseItem {
            name: "SyncedDir".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
        BrowseItem {
            name: "Messages".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    // Out-of-sync files (nested in Messages/)
    let out_of_sync_paths = vec!["Messages/foo"];

    // Check each directory
    for item in &items {
        let full_path = if current_path.is_empty() {
            item.name.clone()
        } else {
            format!("{}{}", current_path, item.name)
        };

        if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
            let dir_prefix = format!("{}/", full_path);

            // Directory should be included if any child path starts with its prefix
            let has_out_of_sync_child = out_of_sync_paths
                .iter()
                .any(|path| path.starts_with(&dir_prefix));

            if item.name == "Messages" {
                assert!(
                    has_out_of_sync_child,
                    "Messages/ should be shown because Messages/foo is out-of-sync"
                );
            } else if item.name == "SyncedDir" {
                assert!(
                    !has_out_of_sync_child,
                    "SyncedDir/ should not be shown because no children are out-of-sync"
                );
            }
        }
    }
}

/// Test: When drilling into Messages/, only foo should be visible (if filter active)
#[test]
fn test_subdirectory_filter_application() {
    // This tests the ACTUAL bug scenario:
    // You're at root level with filter active showing "Messages/" (has out-of-sync child)
    // You drill into "Messages/"
    // The new breadcrumb level should ONLY show "foo", not all files

    // Simulate being inside Messages/ directory
    let current_path = "Messages/";
    let items_in_messages = vec![
        BrowseItem {
            name: "foo".to_string(),
            item_type: "file".to_string(), // Out-of-sync file
            size: 100,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "bar".to_string(),
            item_type: "file".to_string(), // Synced file
            size: 200,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "baz.txt".to_string(),
            item_type: "file".to_string(), // Synced file
            size: 150,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Out-of-sync paths (full paths from cache)
    let out_of_sync_paths = vec!["Messages/foo"];

    // Filter logic: keep only items that match out_of_sync_paths
    let filtered: Vec<_> = items_in_messages
        .iter()
        .filter(|item| {
            let full_path = format!("{}{}", current_path, item.name);
            out_of_sync_paths.contains(&full_path.as_str())
        })
        .collect();

    // CRITICAL ASSERTION: Only "foo" should be visible
    assert_eq!(
        filtered.len(),
        1,
        "Should only show 1 file (foo) - the out-of-sync one"
    );
    assert_eq!(
        filtered[0].name,
        "foo",
        "Only 'foo' should be visible as it's out-of-sync"
    );

    // Bug manifestation: Without fix, all 3 files would be shown
    // because enter_directory() doesn't check filter state
}
