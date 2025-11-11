//! Tests for filter index bug
//!
//! Bug: When search or out-of-sync filtering is active, `selected_index` is relative
//! to the filtered list (`display_items()`), but several action methods incorrectly
//! access the unfiltered list (`level.items[selected]`), causing actions to target
//! the wrong file.
//!
//! Example:
//! Full list: ["a.txt", "b.txt", "jeff-1.txt", "jeff-2.txt", "z.txt"]
//! Search for "jeff" → Filtered list: ["jeff-1.txt", "jeff-2.txt"]
//! User selects index 1 (visually "jeff-2.txt")
//! BUG: `level.items[1]` returns "b.txt" instead of "jeff-2.txt"
//!
//! Affected methods:
//! - delete_file() - src/app/file_ops.rs:198
//! - open_selected_item() - src/app/file_ops.rs:259
//! - copy_to_clipboard() - src/app/file_ops.rs:373
//! - toggle_ignore() - src/app/ignore.rs:39
//! - ignore_and_delete() - src/app/ignore.rs:221
//! - Delete confirmation handler - src/handlers/keyboard.rs:165

use stui::api::BrowseItem;
use stui::model::types::BreadcrumbLevel;

/// Helper: Create a breadcrumb level with full list and filtered subset
fn create_test_level_with_filter() -> BreadcrumbLevel {
    // Full unfiltered list: 5 files
    let all_items = vec![
        BrowseItem {
            name: "a.txt".to_string(),
            item_type: "file".to_string(),
            size: 100,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "b.txt".to_string(),
            item_type: "file".to_string(),
            size: 200,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "jeff-1.txt".to_string(),
            item_type: "file".to_string(),
            size: 300,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "jeff-2.txt".to_string(),
            item_type: "file".to_string(),
            size: 400,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "z.txt".to_string(),
            item_type: "file".to_string(),
            size: 500,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Filtered list: only files matching "jeff" (indices 2, 3 in full list)
    let filtered = vec![
        BrowseItem {
            name: "jeff-1.txt".to_string(),
            item_type: "file".to_string(),
            size: 300,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "jeff-2.txt".to_string(),
            item_type: "file".to_string(),
            size: 400,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    BreadcrumbLevel {
        folder_id: "test-folder".to_string(),
        folder_label: "Test Folder".to_string(),
        folder_path: "/syncthing/test".to_string(),
        items: all_items,
        filtered_items: Some(filtered),
        selected_index: None,
        prefix: None,
        file_sync_states: std::collections::HashMap::new(),
        ignored_exists: std::collections::HashMap::new(),
        translated_base_path: "/test/path".to_string(),
    }
}

/// Test: display_items() should return filtered list when filter is active
#[test]
fn test_display_items_returns_filtered_list() {
    let level = create_test_level_with_filter();

    let displayed = level.display_items();

    assert_eq!(displayed.len(), 2, "Should show 2 filtered items");
    assert_eq!(
        displayed[0].name, "jeff-1.txt",
        "First item should be jeff-1.txt"
    );
    assert_eq!(
        displayed[1].name, "jeff-2.txt",
        "Second item should be jeff-2.txt"
    );
}

/// Test: display_items() should return full list when no filter is active
#[test]
fn test_display_items_returns_full_list_when_no_filter() {
    let mut level = create_test_level_with_filter();
    level.filtered_items = None; // Remove filter

    let displayed = level.display_items();

    assert_eq!(displayed.len(), 5, "Should show all 5 items");
    assert_eq!(displayed[0].name, "a.txt", "First item should be a.txt");
}

/// Test: selected_item() should return item from filtered list
#[test]
fn test_selected_item_uses_filtered_list() {
    let mut level = create_test_level_with_filter();
    level.selected_index = Some(1); // Select index 1 in filtered list

    let selected = level.selected_item();

    assert!(selected.is_some(), "Should have a selected item");
    assert_eq!(
        selected.unwrap().name,
        "jeff-2.txt",
        "Index 1 in filtered list should be jeff-2.txt"
    );
}

/// Test: WRONG - Using level.items[selected_index] with filter active
#[test]
fn test_wrong_pattern_exposes_bug() {
    let mut level = create_test_level_with_filter();
    level.selected_index = Some(1); // User selects index 1 (visually "jeff-2.txt")

    // BUG: This is what the code currently does (WRONG)
    let selected_idx = level.selected_index.unwrap();
    let wrong_item = &level.items[selected_idx]; // Index 1 in FULL list = "b.txt"

    assert_eq!(
        wrong_item.name, "b.txt",
        "Bug: Using items[selected_index] returns wrong file!"
    );

    // CORRECT: This is what it should do
    let correct_item = level.display_items().get(selected_idx).unwrap();
    assert_eq!(
        correct_item.name, "jeff-2.txt",
        "Correct: Using display_items()[selected_index] returns right file"
    );
}

/// Test: Index 0 with filter should select first filtered item, not first full item
#[test]
fn test_index_zero_with_filter() {
    let mut level = create_test_level_with_filter();
    level.selected_index = Some(0); // Select first item in filtered view

    // WRONG pattern (what code currently does)
    let wrong_item = &level.items[0]; // Full list index 0 = "a.txt"
    assert_eq!(wrong_item.name, "a.txt", "Bug: items[0] = a.txt");

    // CORRECT pattern (what code should do)
    let correct_item = level.display_items().get(0).unwrap();
    assert_eq!(
        correct_item.name, "jeff-1.txt",
        "Correct: display_items()[0] = jeff-1.txt"
    );
}

/// Test: Out of bounds check - selected_index valid for filtered but not full list
#[test]
fn test_out_of_bounds_edge_case() {
    let mut level = create_test_level_with_filter();

    // Edge case: What if filtered list is longer than full list at that index?
    // Actually, filtered list is always subset, so this won't happen.
    // But test that filtered indices are always valid for filtered list
    level.selected_index = Some(1); // Valid for filtered (0,1)

    // This should always work (filtered list has 2 items)
    let item = level.display_items().get(1);
    assert!(item.is_some(), "Index 1 should be valid for filtered list");
}

/// Test: Delete confirmation should find item by name, not index
#[test]
fn test_delete_confirmation_by_name() {
    let mut level = create_test_level_with_filter();
    level.selected_index = Some(1); // Visually selecting "jeff-2.txt"

    // Get the item to delete (CORRECT way)
    let item_to_delete = level.display_items().get(1).unwrap();
    let item_name = item_to_delete.name.clone();

    // After deletion, we need to remove from BOTH lists (if applicable)
    // 1. Remove from full list by NAME
    level.items.retain(|item| item.name != item_name);

    // 2. Remove from filtered list by NAME (if exists)
    if let Some(ref mut filtered) = level.filtered_items {
        filtered.retain(|item| item.name != item_name);
    }

    // Verify: "jeff-2.txt" should be gone from both lists
    assert!(!level.items.iter().any(|item| item.name == "jeff-2.txt"));
    if let Some(ref filtered) = level.filtered_items {
        assert!(!filtered.iter().any(|item| item.name == "jeff-2.txt"));
    }

    // Verify: Other items should still exist
    assert_eq!(
        level.items.len(),
        4,
        "Should have 4 items left in full list"
    );
    assert_eq!(
        level.filtered_items.as_ref().unwrap().len(),
        1,
        "Should have 1 item left in filtered list"
    );
}

/// Test: Removing wrong item by index from full list (the bug!)
#[test]
fn test_delete_confirmation_bug_removes_wrong_item() {
    let mut level = create_test_level_with_filter();
    level.selected_index = Some(1); // User wants to delete "jeff-2.txt"

    // BUG: Current code does this (removes by index from full list)
    let idx = level.selected_index.unwrap();
    let wrong_item_name = level.items[idx].name.clone(); // This is "b.txt"!

    // Simulate buggy deletion (remove index 1 from full list)
    level.items.remove(idx);

    // Verify the bug: "b.txt" was removed (wrong!), "jeff-2.txt" still exists
    assert_eq!(
        wrong_item_name, "b.txt",
        "Bug removed b.txt instead of jeff-2.txt"
    );
    assert!(
        level.items.iter().any(|item| item.name == "jeff-2.txt"),
        "Bug: jeff-2.txt still exists (should have been deleted)"
    );
    assert!(
        !level.items.iter().any(|item| item.name == "b.txt"),
        "Bug: b.txt was deleted (should still exist)"
    );
}
