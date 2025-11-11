//! Tests for browse cache invalidation
//!
//! Bug: When RemoteIndexUpdated event fires with dir_path="" (entire folder changed),
//! invalidate_directory() only deletes root cache (prefix=""), leaving subdirectory
//! caches (like prefix="Messages/") with stale data.
//!
//! Example:
//! 1. Cache has: prefix="" and prefix="Messages/" with 3 items
//! 2. Remote deletes Messages/test/foo
//! 3. RemoteIndexUpdated fires → invalidate_directory(folder, "")
//! 4. BUG: Only prefix="" deleted, Messages/ cache still has 3 items
//! 5. User navigates to Messages/ → cache HIT with stale data (should be 2 items)

use stui::cache::CacheDb;
use stui::api::BrowseItem;

/// Test: invalidate_directory with empty dir_path should clear ALL prefixes
#[test]
fn test_invalidate_empty_dir_clears_all_prefixes() {
    // Create temporary cache
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let folder_sequence = 100;

    // Populate cache with multiple prefixes
    let root_items = vec![
        BrowseItem {
            name: "Messages".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    let messages_items = vec![
        BrowseItem {
            name: "file1.txt".to_string(),
            item_type: "file".to_string(),
            size: 100,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "file2.txt".to_string(),
            item_type: "file".to_string(),
            size: 200,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "test".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    // Save to cache
    cache.save_browse_items(folder_id, None, &root_items, folder_sequence)
        .expect("Failed to save root items");
    cache.save_browse_items(folder_id, Some("Messages/"), &messages_items, folder_sequence)
        .expect("Failed to save Messages/ items");

    // Verify cache has data
    let root_cached = cache.get_browse_items(folder_id, None, folder_sequence)
        .expect("Failed to get root items");
    assert!(root_cached.is_some(), "Root cache should exist before invalidation");

    let messages_cached = cache.get_browse_items(folder_id, Some("Messages/"), folder_sequence)
        .expect("Failed to get Messages/ items");
    assert!(messages_cached.is_some(), "Messages/ cache should exist before invalidation");

    // Invalidate entire folder (empty dir_path)
    cache.invalidate_directory(folder_id, "")
        .expect("Failed to invalidate directory");

    // CRITICAL: After invalidation, ALL prefixes should be cleared
    let root_after = cache.get_browse_items(folder_id, None, folder_sequence)
        .expect("Failed to get root items after invalidation");
    assert!(root_after.is_none(), "Root cache should be cleared after invalidate_directory with empty dir_path");

    let messages_after = cache.get_browse_items(folder_id, Some("Messages/"), folder_sequence)
        .expect("Failed to get Messages/ items after invalidation");
    assert!(
        messages_after.is_none(),
        "Messages/ cache should be cleared after invalidate_directory with empty dir_path (THIS IS THE BUG!)"
    );
}

/// Test: invalidate_directory with specific dir_path should only clear that prefix
#[test]
fn test_invalidate_specific_dir_only_clears_that_prefix() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let folder_sequence = 100;

    let root_items = vec![
        BrowseItem {
            name: "Messages".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    let messages_items = vec![
        BrowseItem {
            name: "file1.txt".to_string(),
            item_type: "file".to_string(),
            size: 100,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    cache.save_browse_items(folder_id, None, &root_items, folder_sequence)
        .expect("Failed to save root items");
    cache.save_browse_items(folder_id, Some("Messages/"), &messages_items, folder_sequence)
        .expect("Failed to save Messages/ items");

    // Invalidate only Messages/ directory
    cache.invalidate_directory(folder_id, "Messages/")
        .expect("Failed to invalidate Messages/");

    // Root should still exist
    let root_after = cache.get_browse_items(folder_id, None, folder_sequence)
        .expect("Failed to get root items");
    assert!(root_after.is_some(), "Root cache should NOT be cleared when invalidating Messages/");

    // Messages/ should be cleared
    let messages_after = cache.get_browse_items(folder_id, Some("Messages/"), folder_sequence)
        .expect("Failed to get Messages/");
    assert!(messages_after.is_none(), "Messages/ cache should be cleared after invalidating Messages/");
}

/// Test: Real-world scenario - RemoteIndexUpdated with deleted subdirectory file
#[test]
fn test_remote_file_deletion_clears_cache() {
    let cache = CacheDb::new_in_memory().expect("Failed to create cache");

    let folder_id = "test-folder";
    let old_sequence = 6087;
    let new_sequence = 6088;

    // Initial state: Messages/ has 3 items
    let messages_items_before = vec![
        BrowseItem {
            name: "VID_20210328_020959.mp4".to_string(),
            item_type: "file".to_string(),
            size: 1000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "VID_20211016_145355.mpeg".to_string(),
            item_type: "file".to_string(),
            size: 2000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "test".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            size: 0,
            mod_time: String::new(),
        },
    ];

    cache.save_browse_items(folder_id, Some("Messages/"), &messages_items_before, old_sequence)
        .expect("Failed to save initial Messages/ items");

    // Simulate: Remote deletes Messages/test/foo
    // This triggers RemoteIndexUpdated with dir_path="" (we don't know what changed)
    cache.invalidate_directory(folder_id, "")
        .expect("Failed to invalidate on RemoteIndexUpdated");

    // After invalidation, cache query with NEW sequence should return None (cache miss)
    let messages_after_invalidation = cache.get_browse_items(folder_id, Some("Messages/"), new_sequence)
        .expect("Failed to query cache");

    assert!(
        messages_after_invalidation.is_none(),
        "Cache should be invalidated after RemoteIndexUpdated, forcing fresh API call"
    );

    // Simulate: Fresh API call returns updated list (2 items, test/ directory removed)
    let messages_items_after = vec![
        BrowseItem {
            name: "VID_20210328_020959.mp4".to_string(),
            item_type: "file".to_string(),
            size: 1000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
        BrowseItem {
            name: "VID_20211016_145355.mpeg".to_string(),
            item_type: "file".to_string(),
            size: 2000,
            mod_time: "2025-01-01T00:00:00Z".to_string(),
        },
    ];

    cache.save_browse_items(folder_id, Some("Messages/"), &messages_items_after, new_sequence)
        .expect("Failed to save updated Messages/ items");

    // Verify correct data is now cached
    let final_cached = cache.get_browse_items(folder_id, Some("Messages/"), new_sequence)
        .expect("Failed to get final cached items")
        .expect("Cache should have fresh data");

    assert_eq!(final_cached.len(), 2, "Should have 2 items (test/ directory removed)");
}
