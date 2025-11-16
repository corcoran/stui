//! Folder card formatting logic
//!
//! Pure functions for calculating folder card states and formatting card data

use crate::api::{Folder, FolderStatus};

/// Card state enum for visual rendering
#[derive(Debug, Clone, PartialEq)]
pub enum FolderCardState {
    /// Fully synced, no pending changes
    Synced,
    /// Out of sync (remote_needed files, local_changes files)
    OutOfSync {
        remote_needed: u64,
        local_changes: u64,
    },
    /// Currently syncing (remote_needed files, local_changes files)
    Syncing {
        remote_needed: u64,
        local_changes: u64,
    },
    /// Folder is paused
    Paused,
    /// Folder has errors
    Error,
    /// Status not yet loaded
    Loading,
}

/// Calculate the card state for a folder
pub fn calculate_folder_card_state(
    folder: &Folder,
    status: Option<&FolderStatus>,
) -> FolderCardState {
    if folder.paused {
        return FolderCardState::Paused;
    }

    let Some(status) = status else {
        return FolderCardState::Loading;
    };

    if status.errors > 0 {
        return FolderCardState::Error;
    }

    let remote_needed = status.need_total_items;
    let local_changes = status.receive_only_total_items;

    if status.state == "syncing" || status.state == "sync-preparing" {
        return FolderCardState::Syncing {
            remote_needed,
            local_changes,
        };
    }

    if remote_needed > 0 || local_changes > 0 {
        FolderCardState::OutOfSync {
            remote_needed,
            local_changes,
        }
    } else {
        FolderCardState::Synced
    }
}

/// Format folder type to user-friendly string
pub fn format_folder_type(folder_type: &str) -> String {
    match folder_type {
        "sendonly" => "Send Only".to_string(),
        "sendreceive" => "Send & Receive".to_string(),
        "receiveonly" => "Receive Only".to_string(),
        _ => folder_type.to_string(),
    }
}

/// Format byte size to human-readable
pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, units[unit_index])
    }
}

/// Format file count to human-readable
pub fn format_file_count(count: u64) -> String {
    if count == 0 {
        return "0 files".to_string();
    }

    if count < 1000 {
        format!("{} files", count)
    } else if count < 1_000_000 {
        format!("{:.1}K files", count as f64 / 1000.0)
    } else {
        format!("{:.1}M files", count as f64 / 1_000_000.0)
    }
}

/// Format status message for the card
pub fn format_status_message(state: &FolderCardState) -> String {
    match state {
        FolderCardState::Synced => "Up to date".to_string(),
        FolderCardState::OutOfSync { .. } => {
            // Don't show count here - it's shown in detail on line 3
            "Out of sync".to_string()
        }
        FolderCardState::Syncing { .. } => {
            // Don't show count here - it's shown in detail on line 3
            "Syncing...".to_string()
        }
        FolderCardState::Paused => "Paused".to_string(),
        FolderCardState::Error => "Error".to_string(),
        FolderCardState::Loading => "Loading...".to_string(),
    }
}

/// Format out-of-sync details with arrows
/// For receive-only folders, local changes are shown as "modified locally" since they can't be uploaded
pub fn format_out_of_sync_details(
    remote_needed: u64,
    local_changes: u64,
    need_bytes: u64,
    folder_type: &str,
) -> Option<String> {
    let mut parts = Vec::new();

    if remote_needed > 0 {
        let size_str = format_size(need_bytes);
        parts.push(format!("↓ {} files ({})", remote_needed, size_str));
    }

    if local_changes > 0 {
        // For receive-only folders, local changes can't be uploaded
        // They represent files modified locally that conflict with receive-only mode
        if folder_type == "receiveonly" {
            let count_str = if local_changes == 1 {
                "1 file modified".to_string()
            } else {
                format!("{} files modified", local_changes)
            };
            parts.push(format!("✎ {} locally", count_str));
        } else {
            // For sendreceive and sendonly, show as upload
            parts.push(format!("↑ {} files", local_changes));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Calculate card height in lines
#[allow(dead_code)]
pub fn calculate_card_height(state: &FolderCardState) -> u16 {
    match state {
        FolderCardState::OutOfSync {
            remote_needed,
            local_changes,
        }
        | FolderCardState::Syncing {
            remote_needed,
            local_changes,
        } => {
            if *remote_needed > 0 || *local_changes > 0 {
                4 // Title + info + details + spacing
            } else {
                3
            }
        }
        _ => 3, // Title + info + spacing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // CARD STATE CALCULATION
    // ========================================

    #[test]
    fn test_calculate_folder_card_state_paused() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: true,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let state = calculate_folder_card_state(&folder, None);
        assert_eq!(state, FolderCardState::Paused);
    }

    #[test]
    fn test_calculate_folder_card_state_loading() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let state = calculate_folder_card_state(&folder, None);
        assert_eq!(state, FolderCardState::Loading);
    }

    #[test]
    fn test_calculate_folder_card_state_synced() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let status = FolderStatus {
            state: "idle".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
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
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(state, FolderCardState::Synced);
    }

    #[test]
    fn test_calculate_folder_card_state_out_of_sync() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let mut status = FolderStatus {
            state: "idle".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 1024,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 5,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::OutOfSync {
                remote_needed: 5,
                local_changes: 0
            }
        );

        // Test with local changes
        status.need_total_items = 0;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::OutOfSync {
                remote_needed: 0,
                local_changes: 3
            }
        );
    }

    #[test]
    fn test_calculate_folder_card_state_syncing_with_counts() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        // Test syncing with remote needed files
        let mut status = FolderStatus {
            state: "syncing".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 1024,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 5,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 0
            }
        );

        // Test syncing with local changes
        status.need_total_items = 0;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 0,
                local_changes: 3
            }
        );

        // Test syncing with both
        status.need_total_items = 5;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 3
            }
        );
    }

    #[test]
    fn test_calculate_folder_card_state_sync_preparing_with_counts() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let status = FolderStatus {
            state: "sync-preparing".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 2048,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 8,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 2,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 8,
                local_changes: 2
            }
        );
    }

    // ========================================
    // FORMATTING FUNCTIONS
    // ========================================

    #[test]
    fn test_format_folder_type() {
        assert_eq!(format_folder_type("sendonly"), "Send Only");
        assert_eq!(format_folder_type("sendreceive"), "Send & Receive");
        assert_eq!(format_folder_type("receiveonly"), "Receive Only");
        assert_eq!(format_folder_type("unknown"), "unknown");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_file_count() {
        assert_eq!(format_file_count(0), "0 files");
        assert_eq!(format_file_count(1), "1 files");
        assert_eq!(format_file_count(500), "500 files");
        assert_eq!(format_file_count(1500), "1.5K files");
        assert_eq!(format_file_count(1_500_000), "1.5M files");
    }

    #[test]
    fn test_format_status_message() {
        assert_eq!(
            format_status_message(&FolderCardState::Synced),
            "Up to date"
        );
        assert_eq!(format_status_message(&FolderCardState::Paused), "Paused");
        assert_eq!(format_status_message(&FolderCardState::Error), "Error");
        assert_eq!(
            format_status_message(&FolderCardState::Loading),
            "Loading..."
        );

        // Out of sync - no count (shown on line 3)
        assert_eq!(
            format_status_message(&FolderCardState::OutOfSync {
                remote_needed: 1,
                local_changes: 0
            }),
            "Out of sync"
        );
        assert_eq!(
            format_status_message(&FolderCardState::OutOfSync {
                remote_needed: 5,
                local_changes: 3
            }),
            "Out of sync"
        );

        // Syncing - no count (shown on line 3)
        assert_eq!(
            format_status_message(&FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 0
            }),
            "Syncing..."
        );
        assert_eq!(
            format_status_message(&FolderCardState::Syncing {
                remote_needed: 0,
                local_changes: 3
            }),
            "Syncing..."
        );
        assert_eq!(
            format_status_message(&FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 3
            }),
            "Syncing..."
        );
        assert_eq!(
            format_status_message(&FolderCardState::Syncing {
                remote_needed: 1,
                local_changes: 0
            }),
            "Syncing..."
        );
    }

    #[test]
    fn test_format_out_of_sync_details() {
        assert_eq!(format_out_of_sync_details(0, 0, 0, "sendreceive"), None);

        // Send & Receive: remote needed
        assert_eq!(
            format_out_of_sync_details(5, 0, 1024, "sendreceive"),
            Some("↓ 5 files (1.0 KB)".to_string())
        );

        // Send & Receive: local changes (uploading)
        assert_eq!(
            format_out_of_sync_details(0, 3, 0, "sendreceive"),
            Some("↑ 3 files".to_string())
        );

        // Send & Receive: both
        assert_eq!(
            format_out_of_sync_details(5, 3, 2048, "sendreceive"),
            Some("↓ 5 files (2.0 KB), ↑ 3 files".to_string())
        );

        // Receive Only: local changes (modified locally, need revert)
        assert_eq!(
            format_out_of_sync_details(0, 3, 0, "receiveonly"),
            Some("✎ 3 files modified locally".to_string())
        );

        // Receive Only: both remote needed and local changes
        assert_eq!(
            format_out_of_sync_details(5, 3, 2048, "receiveonly"),
            Some("↓ 5 files (2.0 KB), ✎ 3 files modified locally".to_string())
        );

        // Send Only: should not have local changes (only remote needed)
        assert_eq!(
            format_out_of_sync_details(5, 0, 1024, "sendonly"),
            Some("↓ 5 files (1.0 KB)".to_string())
        );
    }

    #[test]
    fn test_calculate_card_height() {
        assert_eq!(calculate_card_height(&FolderCardState::Synced), 3);
        assert_eq!(calculate_card_height(&FolderCardState::Paused), 3);
        assert_eq!(
            calculate_card_height(&FolderCardState::OutOfSync {
                remote_needed: 5,
                local_changes: 0
            }),
            4
        );
        assert_eq!(
            calculate_card_height(&FolderCardState::OutOfSync {
                remote_needed: 0,
                local_changes: 3
            }),
            4
        );
        assert_eq!(
            calculate_card_height(&FolderCardState::OutOfSync {
                remote_needed: 0,
                local_changes: 0
            }),
            3
        );

        // Test Syncing state with counts
        assert_eq!(
            calculate_card_height(&FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 0
            }),
            4
        );
        assert_eq!(
            calculate_card_height(&FolderCardState::Syncing {
                remote_needed: 0,
                local_changes: 3
            }),
            4
        );
        assert_eq!(
            calculate_card_height(&FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 3
            }),
            4
        );
        assert_eq!(
            calculate_card_height(&FolderCardState::Syncing {
                remote_needed: 0,
                local_changes: 0
            }),
            3
        );
    }
}
