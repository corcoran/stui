//! Shared types for the Model
//!
//! These types are used across multiple sub-models and represent
//! fundamental domain concepts.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use crate::api::{BrowseItem, FileDetails, SyncState};

/// Vim command state for tracking double-key commands like 'gg'
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimCommandState {
    None,
    WaitingForSecondG, // First 'g' pressed, waiting for second 'g'
}

/// Pattern selection menu state (for removing ignore patterns)
#[derive(Clone, Debug)]
pub struct PatternSelectionState {
    pub folder_id: String,
    pub item_name: String,
    pub patterns: Vec<String>,
    pub selected_index: Option<usize>,
}

/// Folder type selection menu state
#[derive(Clone, Debug)]
pub struct FolderTypeSelectionState {
    pub folder_id: String,
    pub folder_label: String,
    pub current_type: String,  // "sendonly", "sendreceive", "receiveonly"
    pub selected_index: usize, // 0=Send Only, 1=Send & Receive, 2=Receive Only
}

/// A single level in the breadcrumb trail
#[derive(Clone, Debug)]
pub struct BreadcrumbLevel {
    pub folder_id: String,
    pub folder_label: String,
    pub folder_path: String, // Container path for this folder
    pub prefix: Option<String>,
    pub items: Vec<BrowseItem>, // Source of truth (unfiltered)
    pub filtered_items: Option<Vec<BrowseItem>>, // Filtered view (if filter active)
    pub selected_index: Option<usize>,
    pub file_sync_states: HashMap<String, SyncState>,
    pub ignored_exists: HashMap<String, bool>,
    pub translated_base_path: String,
}

impl BreadcrumbLevel {
    /// Get the active display list (filtered if filter is active, otherwise all items)
    pub fn display_items(&self) -> &Vec<BrowseItem> {
        self.filtered_items.as_ref().unwrap_or(&self.items)
    }

    /// Get currently selected item (respects filtered items if active)
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        self.selected_index
            .and_then(|idx| self.display_items().get(idx))
    }

    /// Get sync state for a file/directory
    pub fn get_sync_state(&self, name: &str) -> Option<SyncState> {
        self.file_sync_states.get(name).copied()
    }

    /// Get relative path for an item
    pub fn relative_path(&self, item_name: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}/{}", prefix, item_name),
            None => item_name.to_string(),
        }
    }
}

/// Information about a pending ignore+delete operation
#[derive(Debug, Clone, PartialEq)]
pub struct PendingDeleteInfo {
    pub paths: HashSet<PathBuf>,
    pub initiated_at: Instant,
    pub rescan_triggered: bool,
}

/// File information popup state
/// Note: image_state removed - stays in Runtime (not cloneable)
#[derive(Clone, Debug)]
pub struct FileInfoPopupState {
    pub folder_id: String,
    pub file_path: String,
    pub browse_item: BrowseItem,
    pub file_details: Option<FileDetails>,
    pub file_content: Result<String, String>, // Ok(content) or Err(error message)
    pub exists_on_disk: bool,
    pub is_binary: bool,
    pub is_image: bool,
    pub scroll_offset: u16,
    // image_state moved to Runtime - ImagePreviewState is not Clone
}

/// Unified confirmation action enum
///
/// Replaces 4 separate Option<(...)> fields with single enum.
/// Each variant holds the data needed for that specific confirmation type.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    Revert {
        folder_id: String,
        changed_files: Vec<String>,
    },
    Delete {
        path: String,
        name: String,
        is_dir: bool,
    },
    IgnoreDelete {
        path: String,
        name: String,
        is_dir: bool,
    },
    PauseResume {
        folder_id: String,
        label: String,
        is_paused: bool,
    },
    Rescan {
        folder_id: String,
        folder_label: String,
    },
}

/// Folder sync breakdown - category counts for out-of-sync items
#[derive(Debug, Clone, Default)]
pub struct FolderSyncBreakdown {
    pub downloading: usize,
    pub queued: usize,
    pub remote_only: usize,
    pub modified: usize,
    pub local_only: usize,
}

/// Out-of-sync filter state for breadcrumb view
#[derive(Debug, Clone)]
pub struct OutOfSyncFilterState {
    pub origin_level: usize,
    pub last_refresh: SystemTime,
}

/// Out-of-sync summary modal state
#[derive(Debug, Clone)]
pub struct OutOfSyncSummaryState {
    pub selected_index: usize,
    pub breakdowns: HashMap<String, FolderSyncBreakdown>,
    pub loading: HashSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // CONFIRM ACTION
    // ========================================

    #[test]
    fn test_confirm_action_revert() {
        let action = ConfirmAction::Revert {
            folder_id: "test-folder".to_string(),
            changed_files: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };

        match action {
            ConfirmAction::Revert {
                folder_id,
                changed_files,
            } => {
                assert_eq!(folder_id, "test-folder");
                assert_eq!(changed_files.len(), 2);
            }
            _ => panic!("Expected Revert variant"),
        }
    }

    #[test]
    fn test_confirm_action_delete() {
        let action = ConfirmAction::Delete {
            path: "/path/to/file".to_string(),
            name: "file.txt".to_string(),
            is_dir: false,
        };

        match action {
            ConfirmAction::Delete { path, name, is_dir } => {
                assert_eq!(path, "/path/to/file");
                assert_eq!(name, "file.txt");
                assert!(!is_dir);
            }
            _ => panic!("Expected Delete variant"),
        }
    }

    #[test]
    fn test_confirm_action_ignore_delete() {
        let action = ConfirmAction::IgnoreDelete {
            path: "/path/to/dir".to_string(),
            name: "dir".to_string(),
            is_dir: true,
        };

        match action {
            ConfirmAction::IgnoreDelete { path, name, is_dir } => {
                assert_eq!(path, "/path/to/dir");
                assert_eq!(name, "dir");
                assert!(is_dir);
            }
            _ => panic!("Expected IgnoreDelete variant"),
        }
    }

    #[test]
    fn test_confirm_action_pause_resume() {
        let action = ConfirmAction::PauseResume {
            folder_id: "folder-123".to_string(),
            label: "My Folder".to_string(),
            is_paused: true,
        };

        match action {
            ConfirmAction::PauseResume {
                folder_id,
                label,
                is_paused,
            } => {
                assert_eq!(folder_id, "folder-123");
                assert_eq!(label, "My Folder");
                assert!(is_paused);
            }
            _ => panic!("Expected PauseResume variant"),
        }
    }

    #[test]
    fn test_confirm_action_is_cloneable() {
        let action = ConfirmAction::Delete {
            path: "/test".to_string(),
            name: "test".to_string(),
            is_dir: false,
        };
        let _cloned = action.clone();
    }

    #[test]
    fn test_confirm_action_equality() {
        let action1 = ConfirmAction::Revert {
            folder_id: "folder".to_string(),
            changed_files: vec!["file.txt".to_string()],
        };
        let action2 = ConfirmAction::Revert {
            folder_id: "folder".to_string(),
            changed_files: vec!["file.txt".to_string()],
        };
        let action3 = ConfirmAction::Delete {
            path: "/path".to_string(),
            name: "name".to_string(),
            is_dir: false,
        };

        assert_eq!(action1, action2);
        assert_ne!(action1, action3);
    }

    // ========================================
    // BREADCRUMB LEVEL
    // ========================================

    #[test]
    fn test_breadcrumb_level_filtered_items() {
        let mut level = BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![
                BrowseItem {
                    name: "file1.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 1024,
                },
                BrowseItem {
                    name: "file2.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 2048,
                },
            ],
            selected_index: None,
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
            filtered_items: None,
        };

        // Unfiltered - should show all items
        assert_eq!(level.items.len(), 2);
        assert_eq!(level.filtered_items, None);

        // Apply filter - keep only one item
        level.filtered_items = Some(vec![level.items[0].clone()]);

        // Original items unchanged
        assert_eq!(level.items.len(), 2);
        assert_eq!(level.filtered_items.as_ref().unwrap().len(), 1);

        // Clear filter
        level.filtered_items = None;
        assert_eq!(level.filtered_items, None);
    }

    #[test]
    fn test_selected_item_respects_filtered_items() {
        // Reproduces bug: dir1 (index 0), dir2 (index 1) in unfiltered list
        // Filter shows only dir2, making it visually at index 0
        // User selects index 0 expecting dir2, but gets dir1 instead
        let dir1 = BrowseItem {
            name: "dir1".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            mod_time: "2025-01-09T10:00:00Z".to_string(),
            size: 0,
        };
        let dir2 = BrowseItem {
            name: "dir2".to_string(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
            mod_time: "2025-01-09T10:00:00Z".to_string(),
            size: 0,
        };

        let level = BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![dir1.clone(), dir2.clone()], // Unfiltered: [dir1, dir2]
            filtered_items: Some(vec![dir2.clone()]), // Filtered: [dir2] only
            selected_index: Some(0), // Select index 0 (should be dir2 from filtered list)
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
        };

        // BUG: selected_item() returns items[0] = dir1 instead of filtered_items[0] = dir2
        let selected = level.selected_item();
        assert!(selected.is_some(), "Should have a selected item");
        assert_eq!(
            selected.unwrap().name,
            "dir2",
            "Should select dir2 from filtered list, not dir1 from unfiltered list"
        );
    }

    #[test]
    fn test_display_items_empty_filter_shows_empty_not_all() {
        // Bug: When search has zero matches, filtered_items = None shows ALL items
        // Expected: filtered_items = Some(vec![]) shows ZERO items
        let level = BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![
                BrowseItem {
                    name: "file1.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 1024,
                },
                BrowseItem {
                    name: "file2.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 2048,
                },
            ],
            selected_index: None,
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
            filtered_items: Some(vec![]), // Zero matches - should show empty list
        };

        // Should show empty list, not fall back to all items
        let displayed = level.display_items();
        assert_eq!(
            displayed.len(),
            0,
            "Zero search matches should show empty list, not all items"
        );
    }

    #[test]
    fn test_display_items_none_filter_shows_all() {
        // Baseline: filtered_items = None should show all items (no filter active)
        let level = BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![
                BrowseItem {
                    name: "file1.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 1024,
                },
                BrowseItem {
                    name: "file2.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    mod_time: "2025-01-09T10:00:00Z".to_string(),
                    size: 2048,
                },
            ],
            selected_index: None,
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
            filtered_items: None, // No filter active
        };

        // Should show all items
        let displayed = level.display_items();
        assert_eq!(displayed.len(), 2, "No filter (None) should show all items");
    }

    // ========================================
    // CONFIRM ACTION - RESCAN VARIANT
    // ========================================

    #[test]
    fn test_confirm_action_rescan() {
        let action = ConfirmAction::Rescan {
            folder_id: "folder-abc".to_string(),
            folder_label: "My Folder".to_string(),
        };

        match action {
            ConfirmAction::Rescan {
                folder_id,
                folder_label,
            } => {
                assert_eq!(folder_id, "folder-abc");
                assert_eq!(folder_label, "My Folder");
            }
            _ => panic!("Expected Rescan variant"),
        }
    }

    #[test]
    fn test_confirm_action_rescan_clone() {
        let action = ConfirmAction::Rescan {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
        };
        let cloned = action.clone();
        assert_eq!(action, cloned);
    }
}
