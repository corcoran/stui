//! Sync State Logic
//!
//! This module contains logic for sync state management and transitions.

use crate::api::{BrowseItem, SyncState};
use std::collections::HashMap;

/// Get the priority of a sync state for sorting
///
/// Lower number = higher priority (displayed first)
///
/// Priority order:
/// 1. OutOfSync (⚠️) - Most important
/// 2. Syncing (🔄) - Active operation
/// 3. RemoteOnly (☁️)
/// 4. LocalOnly (💻)
/// 5. Ignored (🚫)
/// 6. Unknown (❓)
/// 7. Synced (✅) - Least important
pub fn sync_state_priority(state: SyncState) -> u8 {
    match state {
        SyncState::OutOfSync => 0,  // ⚠️ Most important
        SyncState::Syncing => 1,    // 🔄 Active operation
        SyncState::RemoteOnly => 2, // ☁️
        SyncState::LocalOnly => 3,  // 💻
        SyncState::Ignored => 4,    // 🚫
        SyncState::Unknown => 5,    // ❓
        SyncState::Synced => 6,     // ✅ Least important
    }
}

/// Check which ignored files exist on the filesystem
///
/// For all items with Ignored sync state, checks if the corresponding file/directory
/// exists on disk. This is used to distinguish between ignored files that still exist
/// (⚠️ warning icon) vs. those that have been deleted (🚫 ban icon).
///
/// # Arguments
/// * `items` - List of browse items to check
/// * `file_sync_states` - Current sync states for items
/// * `translated_base_path` - The host path to the directory
/// * `parent_exists` - Optional optimization: if parent doesn't exist, skip checks
///
/// # Returns
/// HashMap mapping item names to whether they exist on disk (only for ignored items)
pub fn check_ignored_existence(
    items: &[BrowseItem],
    file_sync_states: &HashMap<String, SyncState>,
    translated_base_path: &str,
    parent_exists: Option<bool>,
) -> HashMap<String, bool> {
    let mut ignored_exists = HashMap::new();

    for item in items {
        if let Some(SyncState::Ignored) = file_sync_states.get(&item.name) {
            // Optimization: If parent directory doesn't exist, children can't either
            if parent_exists == Some(false) {
                ignored_exists.insert(item.name.clone(), false);
                continue;
            }

            // Check filesystem for this item
            let host_path = format!(
                "{}/{}",
                translated_base_path.trim_end_matches('/'),
                item.name
            );
            let exists = std::path::Path::new(&host_path).exists();
            ignored_exists.insert(item.name.clone(), exists);
        }
    }

    ignored_exists
}

/// Aggregate directory sync state based on direct state and children states
///
/// Determines a directory's overall sync state by considering:
/// 1. The directory's own direct state (from FileInfo)
/// 2. The states of all its children
///
/// # Priority Rules
/// - If directory itself is RemoteOnly or Ignored → use that (takes precedence)
/// - Otherwise, use highest priority child state:
///   - Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced
/// - If all children synced → use directory's direct state
///
/// # Arguments
/// * `direct_state` - The directory's own sync state (None = default to Synced)
/// * `child_states` - Slice of all children's sync states
///
/// # Returns
/// The aggregated sync state for the directory
pub fn aggregate_directory_state(
    direct_state: Option<SyncState>,
    child_states: &[SyncState],
) -> SyncState {
    let direct = direct_state.unwrap_or(SyncState::Synced);

    // If directory itself is RemoteOnly or Ignored, that takes precedence
    if matches!(direct, SyncState::RemoteOnly | SyncState::Ignored) {
        return direct;
    }

    // Collect child state flags
    let mut has_syncing = false;
    let mut has_remote_only = false;
    let mut has_out_of_sync = false;
    let mut has_local_only = false;

    for state in child_states {
        match state {
            SyncState::Syncing => has_syncing = true,
            SyncState::RemoteOnly => has_remote_only = true,
            SyncState::OutOfSync => has_out_of_sync = true,
            SyncState::LocalOnly => has_local_only = true,
            _ => {}
        }
    }

    // Priority order: Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced
    if has_syncing {
        SyncState::Syncing
    } else if has_remote_only {
        SyncState::RemoteOnly
    } else if has_out_of_sync {
        SyncState::OutOfSync
    } else if has_local_only {
        SyncState::LocalOnly
    } else {
        // All children synced, use directory's direct state
        direct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state_priority_order() {
        // OutOfSync has highest priority (lowest number)
        assert_eq!(sync_state_priority(SyncState::OutOfSync), 0);
        assert_eq!(sync_state_priority(SyncState::Syncing), 1);
        assert_eq!(sync_state_priority(SyncState::RemoteOnly), 2);
        assert_eq!(sync_state_priority(SyncState::LocalOnly), 3);
        assert_eq!(sync_state_priority(SyncState::Ignored), 4);
        assert_eq!(sync_state_priority(SyncState::Unknown), 5);

        // Synced has lowest priority (highest number)
        assert_eq!(sync_state_priority(SyncState::Synced), 6);
    }

    #[test]
    fn test_priority_ordering() {
        // OutOfSync should come before Synced
        assert!(sync_state_priority(SyncState::OutOfSync) < sync_state_priority(SyncState::Synced));

        // Syncing should come before RemoteOnly
        assert!(sync_state_priority(SyncState::Syncing) < sync_state_priority(SyncState::RemoteOnly));

        // Unknown should come before Synced
        assert!(sync_state_priority(SyncState::Unknown) < sync_state_priority(SyncState::Synced));
    }

    #[test]
    fn test_check_ignored_existence_empty_items() {
        let items: Vec<BrowseItem> = vec![];
        let file_sync_states = HashMap::new();

        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_check_ignored_existence_no_ignored_items() {
        let items = vec![
            BrowseItem {
                name: "file1.txt".to_string(),
                size: 100,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
        ];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("file1.txt".to_string(), SyncState::Synced);

        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_check_ignored_existence_parent_not_exists() {
        let items = vec![
            BrowseItem {
                name: "ignored.txt".to_string(),
                size: 100,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
        ];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("ignored.txt".to_string(), SyncState::Ignored);

        // Parent doesn't exist - should optimize and return false without checking
        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", Some(false));
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("ignored.txt"), Some(&false));
    }

    #[test]
    fn test_check_ignored_existence_mixed_states() {
        let items = vec![
            BrowseItem {
                name: "synced.txt".to_string(),
                size: 100,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
            BrowseItem {
                name: "ignored.txt".to_string(),
                size: 200,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
        ];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("synced.txt".to_string(), SyncState::Synced);
        file_sync_states.insert("ignored.txt".to_string(), SyncState::Ignored);

        // Only ignored items should be in the result
        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("ignored.txt"));
        assert!(!result.contains_key("synced.txt"));
    }
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    #[test]
    fn test_aggregate_directory_state_all_synced() {
        // When all children are synced, use direct state
        let direct_state = Some(SyncState::Synced);
        let child_states = vec![SyncState::Synced, SyncState::Synced];

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Synced);
    }

    #[test]
    fn test_aggregate_directory_state_one_syncing() {
        // Syncing has highest priority
        let direct_state = Some(SyncState::Synced);
        let child_states = vec![
            SyncState::Synced,
            SyncState::Syncing,
            SyncState::OutOfSync,
        ];

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Syncing);
    }

    #[test]
    fn test_aggregate_directory_state_mixed_priorities() {
        // RemoteOnly > OutOfSync > LocalOnly
        let direct_state = Some(SyncState::Synced);

        // RemoteOnly wins
        let child_states1 = vec![
            SyncState::RemoteOnly,
            SyncState::OutOfSync,
            SyncState::LocalOnly,
        ];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states1),
            SyncState::RemoteOnly
        );

        // OutOfSync wins (no RemoteOnly)
        let child_states2 = vec![SyncState::OutOfSync, SyncState::LocalOnly];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states2),
            SyncState::OutOfSync
        );

        // LocalOnly wins (no higher priority)
        let child_states3 = vec![SyncState::LocalOnly, SyncState::Synced];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states3),
            SyncState::LocalOnly
        );
    }

    #[test]
    fn test_aggregate_directory_state_remote_only_direct() {
        // Directory itself is RemoteOnly - takes precedence
        let direct_state = Some(SyncState::RemoteOnly);
        let child_states = vec![SyncState::Synced]; // Doesn't matter

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::RemoteOnly);
    }

    #[test]
    fn test_aggregate_directory_state_ignored_direct() {
        // Directory itself is Ignored - takes precedence
        let direct_state = Some(SyncState::Ignored);
        let child_states = vec![SyncState::Syncing]; // Doesn't matter

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Ignored);
    }

}
