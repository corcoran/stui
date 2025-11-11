//! Test for search race condition bug
//!
//! Bug: When user immediately searches after entering a folder, search shows blank results
//! on first try, but works on subsequent searches.
//!
//! Root Cause:
//! 1. App loads folder root from cache → breadcrumb.items has 35 items
//! 2. User immediately presses Ctrl-F to search
//! 3. apply_search_filter() calls cache.get_all_browse_items(folder_id, folder_sequence)
//! 4. If cache sequence doesn't match (stale cache, or not fully written), returns Ok(Vec::new())
//! 5. Code proceeds with 0 items instead of falling back to breadcrumb.items
//! 6. Result: 0 matches shown (blank screen)
//! 7. Second search works because cache is now populated/committed
//!
//! Fix: When cache returns empty vec but breadcrumb.items is non-empty, fall back to
//! filtering breadcrumb.items only (non-recursive search).

use stui::cache::CacheDb;
use stui::api::BrowseItem;

/// Test: get_all_browse_items returns empty when sequence doesn't match
#[test]
fn test_cache_returns_empty_on_sequence_mismatch() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let old_sequence = 100;
    let new_sequence = 101;

    // Simulate previous session: cache saved with old_sequence
    let items = vec![
        BrowseItem {
            name: "jeff-1.txt".to_string(),
            item_type: "file".to_string(),
            size: 1024,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "jeff-2.txt".to_string(),
            item_type: "file".to_string(),
            size: 2048,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "Movies".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    cache.save_browse_items(folder_id, None, &items, old_sequence)
        .expect("Failed to save items");

    // Verify cache has data with old sequence
    let cached = cache.get_browse_items(folder_id, None, old_sequence)
        .expect("Failed to get items");
    assert!(cached.is_some(), "Cache should exist with old sequence");
    assert_eq!(cached.unwrap().len(), 3, "Cache should have 3 items");

    // NEW SESSION: Folder sequence updated to new_sequence
    // User enters folder → load_root_level() would fetch from API (cache miss)
    // But breadcrumb.items gets populated with fresh data (35 items)

    // User immediately searches → apply_search_filter() calls get_all_browse_items
    let all_items = cache.get_all_browse_items(folder_id, new_sequence)
        .expect("get_all_browse_items should not error");

    // BUG: Returns empty vec (not an error) because sequence doesn't match
    assert!(
        all_items.is_empty(),
        "get_all_browse_items returns empty vec when sequence mismatch (this is the bug condition)"
    );

    // Meanwhile, breadcrumb.items would have 3 items (loaded from API)
    // The fix checks: if all_items.is_empty() && !breadcrumb.items.is_empty()
    // then fall back to filtering breadcrumb.items only
}

/// Test: get_all_browse_items returns empty when cache doesn't exist
#[test]
fn test_cache_returns_empty_when_no_cache() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let folder_sequence = 100;

    // Don't save anything to cache

    // User enters folder → gets data from API, populates breadcrumb.items
    // User immediately searches before cache is written
    let all_items = cache.get_all_browse_items(folder_id, folder_sequence)
        .expect("get_all_browse_items should not error");

    // Returns empty vec (cache doesn't exist yet)
    assert!(
        all_items.is_empty(),
        "get_all_browse_items returns empty vec when cache doesn't exist"
    );

    // The fix handles this case by falling back to breadcrumb.items
}

/// Test: get_all_browse_items returns items when cache is populated with correct sequence
#[test]
fn test_cache_returns_items_when_sequence_matches() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let folder_sequence = 100;

    let root_items = vec![
        BrowseItem {
            name: "jeff-1.txt".to_string(),
            item_type: "file".to_string(),
            size: 1024,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "Movies".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    let movies_items = vec![
        BrowseItem {
            name: "jeff-movie.mp4".to_string(),
            item_type: "file".to_string(),
            size: 1_000_000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Save both root and subdirectory
    cache.save_browse_items(folder_id, None, &root_items, folder_sequence)
        .expect("Failed to save root items");
    cache.save_browse_items(folder_id, Some("Movies/"), &movies_items, folder_sequence)
        .expect("Failed to save Movies/ items");

    // get_all_browse_items should return ALL items recursively
    let all_items = cache.get_all_browse_items(folder_id, folder_sequence)
        .expect("get_all_browse_items should not error");

    // Should return 3 items total (2 root + 1 subdirectory)
    assert_eq!(
        all_items.len(),
        3,
        "get_all_browse_items should return all cached items recursively"
    );

    // Verify paths are correct
    let paths: Vec<String> = all_items.iter().map(|(path, _)| path.clone()).collect();

    // Debug: print actual paths
    eprintln!("Actual paths: {:?}", paths);

    assert!(paths.contains(&"jeff-1.txt".to_string()), "Should include root item");
    assert!(paths.contains(&"Movies".to_string()), "Should include root directory");

    // Check for subdirectory item - the path format might be different
    let has_movie = paths.iter().any(|p| p.contains("jeff-movie"));
    assert!(has_movie, "Should include subdirectory item (actual paths: {:?})", paths);
}

/// Test: Verify search fallback logic with logic::search::filter_items
#[test]
fn test_search_fallback_filters_current_level() {
    // This tests the fallback path that the fix uses
    let items = vec![
        BrowseItem {
            name: "jeff-1.txt".to_string(),
            item_type: "file".to_string(),
            size: 1024,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "jeff-2.txt".to_string(),
            item_type: "file".to_string(),
            size: 2048,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "other.txt".to_string(),
            item_type: "file".to_string(),
            size: 512,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Search for "jeff"
    let filtered = stui::logic::search::filter_items(&items, "jeff", None);

    // Should match 2 items
    assert_eq!(filtered.len(), 2, "Should find 2 items matching 'jeff'");
    assert_eq!(filtered[0].name, "jeff-1.txt");
    assert_eq!(filtered[1].name, "jeff-2.txt");

    // Search with wildcard
    let filtered_wildcard = stui::logic::search::filter_items(&items, "*jeff*", None);
    assert_eq!(filtered_wildcard.len(), 2, "Wildcard search should also find 2 items");

    // Search with no matches
    let filtered_none = stui::logic::search::filter_items(&items, "nonexistent", None);
    assert_eq!(filtered_none.len(), 0, "Should return empty vec when no matches");
}

/// Test: Cache sequence cleanup when saving with new sequence
#[test]
fn test_cache_sequence_cleanup_on_save() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let old_sequence = 100;
    let new_sequence = 101;

    // Simulate old cache from previous session
    let old_root_items = vec![
        BrowseItem {
            name: "old-file.txt".to_string(),
            item_type: "file".to_string(),
            size: 1024,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    let old_subdir_items = vec![
        BrowseItem {
            name: "old-movie.mp4".to_string(),
            item_type: "file".to_string(),
            size: 1_000_000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    // Save old data with old sequence
    cache.save_browse_items(folder_id, None, &old_root_items, old_sequence)
        .expect("Failed to save old root");
    cache.save_browse_items(folder_id, Some("Movies/"), &old_subdir_items, old_sequence)
        .expect("Failed to save old subdir");

    // Verify old data exists
    let old_cache = cache.get_browse_items(folder_id, None, old_sequence)
        .expect("Failed to get old cache");
    assert!(old_cache.is_some(), "Old cache should exist");

    // NEW SESSION: Folder sequence updated to new_sequence
    // Save new root data with new sequence
    let new_root_items = vec![
        BrowseItem {
            name: "new-file.txt".to_string(),
            item_type: "file".to_string(),
            size: 2048,
            mod_time: "2025-01-02T00:00:00Z".to_string(),
        },
    ];

    cache.save_browse_items(folder_id, None, &new_root_items, new_sequence)
        .expect("Failed to save new root");

    // CRITICAL: Old subdirectory cache should be DELETED because sequence changed
    let old_subdir_after = cache.get_browse_items(folder_id, Some("Movies/"), old_sequence)
        .expect("Failed to query old subdir");
    assert!(
        old_subdir_after.is_none(),
        "Old subdirectory cache should be deleted when sequence changes"
    );

    // New root data should exist
    let new_root_after = cache.get_browse_items(folder_id, None, new_sequence)
        .expect("Failed to get new root");
    assert!(new_root_after.is_some(), "New root cache should exist");
    assert_eq!(new_root_after.unwrap()[0].name, "new-file.txt");

    // get_all_browse_items with new sequence should return only new data
    let all_items = cache.get_all_browse_items(folder_id, new_sequence)
        .expect("get_all_browse_items should not error");
    assert_eq!(all_items.len(), 1, "Should only have new root item");
    assert_eq!(all_items[0].1.name, "new-file.txt");
}
