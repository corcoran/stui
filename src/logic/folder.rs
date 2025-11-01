//! Folder validation and business logic
//!
//! Pure functions for folder-related validations and calculations.

use crate::api::FolderStatus;

/// Check if a folder has local changes that can be reverted
///
/// For receive-only folders, returns true if there are local changes
/// that differ from the remote state.
///
/// # Arguments
/// * `status` - Optional folder status from Syncthing API
///
/// # Returns
/// `true` if the folder has local changes (receive_only_total_items > 0)
pub fn has_local_changes(status: Option<&FolderStatus>) -> bool {
    status
        .map(|s| s.receive_only_total_items > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_status(receive_only_items: u64) -> FolderStatus {
        FolderStatus {
            state: "idle".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 0,
            global_symlinks: 0,
            global_total_items: 0,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 0,
            local_symlinks: 0,
            local_total_items: 0,
            need_bytes: 0,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 0,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: receive_only_items,
        }
    }

    #[test]
    fn test_has_local_changes_with_changes() {
        let status = create_test_status(5);
        assert!(has_local_changes(Some(&status)));
    }

    #[test]
    fn test_has_local_changes_without_changes() {
        let status = create_test_status(0);
        assert!(!has_local_changes(Some(&status)));
    }

    #[test]
    fn test_has_local_changes_none() {
        assert!(!has_local_changes(None));
    }
}
