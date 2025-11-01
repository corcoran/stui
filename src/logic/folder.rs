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

/// Check if file deletion is allowed given current navigation state
///
/// File deletion only works when viewing breadcrumb contents (not the folder list).
/// This requires both being in a breadcrumb level (focus_level > 0) and having
/// a valid navigation trail.
///
/// # Arguments
/// * `focus_level` - Current navigation focus level (0 = folder list, >0 = breadcrumb)
/// * `breadcrumb_trail_empty` - Whether the breadcrumb navigation trail is empty
///
/// # Returns
/// `true` if file deletion is allowed (in breadcrumb view with valid trail)
///
/// # Examples
/// ```
/// use synctui::logic::folder::can_delete_file;
///
/// // Can delete: in breadcrumb view with trail
/// assert!(can_delete_file(1, false));
///
/// // Cannot delete: in folder list view
/// assert!(!can_delete_file(0, false));
///
/// // Cannot delete: no navigation trail
/// assert!(!can_delete_file(1, true));
/// ```
pub fn can_delete_file(focus_level: usize, breadcrumb_trail_empty: bool) -> bool {
    focus_level > 0 && !breadcrumb_trail_empty
}

/// Check if the Restore button should be shown in the hotkey legend
///
/// Restore is only available when viewing breadcrumb contents (not the folder list)
/// and the folder has local changes that can be reverted. This applies to receive-only
/// folders that have local modifications.
///
/// # Arguments
/// * `focus_level` - Current navigation focus level (0 = folder list, >0 = breadcrumb)
/// * `folder_status` - Optional folder status from Syncthing API
///
/// # Returns
/// `true` if the Restore button should be shown (in breadcrumb view with local changes)
///
/// # Examples
/// ```
/// use synctui::logic::folder::should_show_restore_button;
/// use synctui::api::FolderStatus;
///
/// // Show restore: breadcrumb view + has changes
/// let status = FolderStatus { receive_only_total_items: 5, ..Default::default() };
/// assert!(should_show_restore_button(1, Some(&status)));
///
/// // Don't show: in folder list view
/// assert!(!should_show_restore_button(0, Some(&status)));
///
/// // Don't show: no local changes
/// let status = FolderStatus { receive_only_total_items: 0, ..Default::default() };
/// assert!(!should_show_restore_button(1, Some(&status)));
/// ```
pub fn should_show_restore_button(
    focus_level: usize,
    folder_status: Option<&FolderStatus>,
) -> bool {
    focus_level > 0 && has_local_changes(folder_status)
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

    #[test]
    fn test_can_delete_file_in_breadcrumb_view() {
        // Can delete when in breadcrumb view (focus_level > 0) with valid trail
        assert!(can_delete_file(1, false));
        assert!(can_delete_file(2, false));
        assert!(can_delete_file(5, false));
    }

    #[test]
    fn test_can_delete_file_in_folder_list() {
        // Cannot delete in folder list view (focus_level == 0)
        assert!(!can_delete_file(0, false));
    }

    #[test]
    fn test_can_delete_file_no_breadcrumb_trail() {
        // Cannot delete without valid breadcrumb trail
        assert!(!can_delete_file(1, true));
        assert!(!can_delete_file(2, true));
    }

    #[test]
    fn test_can_delete_file_folder_list_no_trail() {
        // Cannot delete in folder list even with no trail
        assert!(!can_delete_file(0, true));
    }

    #[test]
    fn test_should_show_restore_button_breadcrumb_with_changes() {
        // Show restore: breadcrumb view + has changes
        let status = create_test_status(5);
        assert!(should_show_restore_button(1, Some(&status)));
        assert!(should_show_restore_button(2, Some(&status)));
    }

    #[test]
    fn test_should_show_restore_button_folder_list() {
        // Don't show: in folder list view (even with changes)
        let status = create_test_status(5);
        assert!(!should_show_restore_button(0, Some(&status)));
    }

    #[test]
    fn test_should_show_restore_button_no_changes() {
        // Don't show: no local changes
        let status = create_test_status(0);
        assert!(!should_show_restore_button(1, Some(&status)));
        assert!(!should_show_restore_button(2, Some(&status)));
    }

    #[test]
    fn test_should_show_restore_button_no_status() {
        // Don't show: no folder status
        assert!(!should_show_restore_button(0, None));
        assert!(!should_show_restore_button(1, None));
    }
}
