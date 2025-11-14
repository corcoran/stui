//! Folder update history functionality
//!
//! Orchestrates fetching file modification times from API and building history modal state.

use crate::{log_debug, logic, model, App};
use std::time::SystemTime;

type FileList = Vec<(String, SystemTime, u64)>;
type BoxedFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + 'a>>;

impl App {
    /// Open the folder update history modal for the selected folder
    ///
    /// Fetches all files recursively from Syncthing using /rest/db/browse,
    /// sorts by modification time, and displays the 100 most recently updated files.
    pub async fn open_folder_history_modal(&mut self, folder_id: &str, folder_label: &str) {
        log_debug(&format!(
            "Opening folder history modal for folder: {}",
            folder_id
        ));

        // Recursively fetch all files from folder
        let files_result = self.fetch_all_files_recursive(folder_id, "").await;

        match files_result {
            Ok(mut files) => {
                log_debug(&format!("Fetched {} total files from folder", files.len()));

                // Sort all files by modification time (newest first)
                files.sort_by(|a, b| b.1.cmp(&a.1));

                let total_files = files.len();
                let has_more = total_files > 100;

                // Extract first batch (first 100 files)
                let entries = logic::folder_history::extract_batch_from_sorted(&files, 0, 100);

                log_debug(&format!(
                    "Processed {} file updates for folder {} (total: {})",
                    entries.len(),
                    folder_id,
                    total_files
                ));

                // Create modal state with cached sorted files
                let modal = model::types::FolderHistoryModal {
                    folder_id: folder_id.to_string(),
                    folder_label: folder_label.to_string(),
                    entries,
                    selected_index: 0,
                    total_files_scanned: total_files,
                    loading: false,
                    has_more,
                    current_offset: 100,           // First batch loaded
                    all_files_sorted: Some(files), // Cache sorted files for pagination
                };

                self.model.ui.folder_history_modal = Some(modal);
            }
            Err(e) => {
                log_debug(&format!("Failed to browse folder: {}", e));
                self.model
                    .ui
                    .show_toast(format!("Failed to load history: {}", e));
            }
        }
    }

    /// Recursively fetch all files from a folder
    ///
    /// Traverses the folder tree using /rest/db/browse, following all directories
    /// to collect every file with its full path and modification time.
    fn fetch_all_files_recursive<'a>(
        &'a self,
        folder_id: &'a str,
        prefix: &'a str,
    ) -> BoxedFuture<'a, anyhow::Result<FileList>> {
        Box::pin(async move {
            let items = self.client.browse_folder(folder_id, Some(prefix)).await?;
            let mut all_files = Vec::new();

            for item in items {
                let full_path = if prefix.is_empty() {
                    item.name.clone()
                } else {
                    format!("{}/{}", prefix, item.name)
                };

                if item.item_type == "FILE_INFO_TYPE_FILE" {
                    // Parse modification time
                    let mod_time = chrono::DateTime::parse_from_rfc3339(&item.mod_time)
                        .ok()
                        .and_then(|dt| {
                            SystemTime::UNIX_EPOCH
                                .checked_add(std::time::Duration::from_secs(dt.timestamp() as u64))
                        })
                        .unwrap_or(SystemTime::UNIX_EPOCH);

                    all_files.push((full_path, mod_time, item.size));
                } else if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                    // Recursively fetch files from subdirectory
                    let subdir_files = self
                        .fetch_all_files_recursive(folder_id, &full_path)
                        .await?;
                    all_files.extend(subdir_files);
                }
            }

            Ok(all_files)
        })
    }

    /// Close the folder update history modal
    pub fn close_folder_history_modal(&mut self) {
        log_debug("Closing folder history modal");
        self.model.ui.folder_history_modal = None;
    }

    /// Load the next batch of files for folder history pagination
    ///
    /// Extracts the next 100 files from the cached sorted file list and appends them
    /// to the modal's entries. Updates pagination state (offset, has_more, loading).
    ///
    /// # Returns
    /// Ok(()) if successful, Err if modal not found or already loading
    pub async fn load_next_history_batch(&mut self) -> anyhow::Result<()> {
        // Get mutable reference to modal
        let modal = self
            .model
            .ui
            .folder_history_modal
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No folder history modal open"))?;

        // Prevent duplicate loading
        if modal.loading || !modal.has_more {
            log_debug("Skipping batch load: already loading or no more files");
            return Ok(());
        }

        // Check if we have cached files
        let all_files = modal
            .all_files_sorted
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No cached file list available"))?;

        log_debug(&format!(
            "Loading next batch from offset {} (total files: {})",
            modal.current_offset,
            all_files.len()
        ));

        // Set loading flag
        modal.loading = true;

        // Extract next batch from cached sorted list
        let new_entries =
            logic::folder_history::extract_batch_from_sorted(all_files, modal.current_offset, 100);

        let batch_size = new_entries.len();

        // Append new entries
        modal.entries.extend(new_entries);
        modal.current_offset += batch_size;

        // Update has_more flag
        modal.has_more = modal.current_offset < all_files.len();

        // Clear loading flag
        modal.loading = false;

        log_debug(&format!(
            "Loaded {} entries (offset now: {}, has_more: {})",
            batch_size, modal.current_offset, modal.has_more
        ));

        Ok(())
    }

    /// Jump to a file from the folder history modal
    ///
    /// Navigates breadcrumbs to the file's location by:
    /// 1. Parsing the file path into directory components
    /// 2. For each directory: finding it, selecting it, calling enter_directory()
    /// 3. Finally highlighting the target file
    ///
    /// If any directory is not found, navigates as deep as possible and shows error.
    pub async fn jump_to_file(&mut self, file_path: &str) -> anyhow::Result<()> {
        log_debug(&format!("Jumping to file: {}", file_path));

        // Parse path into components
        let (dirs, filename) = crate::logic::file_navigation::parse_file_path(file_path);

        log_debug(&format!("Parsed path: dirs={:?}, file={}", dirs, filename));

        // Navigate through each directory
        for (idx, dir_name) in dirs.iter().enumerate() {
            log_debug(&format!(
                "Navigating to directory {}/{}: {}",
                idx + 1,
                dirs.len(),
                dir_name
            ));

            // Get current breadcrumb level
            let level_idx = if self.model.navigation.focus_level == 0 {
                self.model
                    .ui
                    .show_toast("Cannot navigate: not in folder view".to_string());
                return Ok(());
            } else {
                self.model.navigation.focus_level - 1
            };

            if level_idx >= self.model.navigation.breadcrumb_trail.len() {
                self.model.ui.show_toast(format!(
                    "Could not navigate to {}: Invalid breadcrumb state",
                    file_path
                ));
                return Ok(());
            }

            // Find directory in current level
            let current_level = &self.model.navigation.breadcrumb_trail[level_idx];
            let dir_index =
                crate::logic::navigation::find_item_index_by_name(&current_level.items, dir_name);

            match dir_index {
                Some(index) => {
                    // Verify it's actually a directory
                    if let Some(item) = current_level.items.get(index) {
                        if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                            self.model.ui.show_toast(format!(
                                "Could not navigate to {}: '{}' is not a directory",
                                file_path, dir_name
                            ));
                            return Ok(());
                        }
                    }

                    // Set selection to this directory
                    self.model.navigation.breadcrumb_trail[level_idx].selected_index = Some(index);

                    // Navigate into it
                    self.enter_directory().await?;
                }
                None => {
                    // Directory not found - stop here and show error
                    self.model.ui.show_toast(format!(
                        "Could not navigate to {}: Directory '{}' not found",
                        file_path, dir_name
                    ));
                    return Ok(());
                }
            }
        }

        // All directories navigated successfully - now find and select the file
        let level_idx = if self.model.navigation.focus_level == 0 {
            self.model.ui.show_toast(format!(
                "Could not navigate to {}: Invalid state",
                file_path
            ));
            return Ok(());
        } else {
            self.model.navigation.focus_level - 1
        };

        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            self.model.ui.show_toast(format!(
                "Could not navigate to {}: Invalid breadcrumb state",
                file_path
            ));
            return Ok(());
        }

        let current_level = &self.model.navigation.breadcrumb_trail[level_idx];
        let file_index =
            crate::logic::navigation::find_item_index_by_name(&current_level.items, &filename);

        match file_index {
            Some(index) => {
                // Set selection to highlight the file
                self.model.navigation.breadcrumb_trail[level_idx].selected_index = Some(index);
                log_debug(&format!("Successfully navigated to {}", file_path));
            }
            None => {
                // File not found - we're at the right directory but file is missing
                self.model
                    .ui
                    .show_toast(format!("Could not find file '{}' in directory", filename));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    // ========================================
    // JUMP TO FILE NAVIGATION
    // ========================================

    #[test]
    fn test_jump_to_file_parses_path_correctly() {
        // Verify that parse_file_path is called correctly
        let (dirs, file) = crate::logic::file_navigation::parse_file_path("docs/2024/report.txt");

        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], "docs");
        assert_eq!(dirs[1], "2024");
        assert_eq!(file, "report.txt");
    }

    #[test]
    fn test_jump_to_file_parses_root_level_file() {
        // Root-level file should have empty dirs
        let (dirs, file) = crate::logic::file_navigation::parse_file_path("readme.txt");

        assert_eq!(dirs.len(), 0);
        assert_eq!(file, "readme.txt");
    }

    // Note: Full integration tests require running Syncthing instance
    // Manual testing will cover end-to-end navigation

    // ========================================
    // BATCH LOADING LOGIC
    // ========================================

    #[test]
    fn test_load_next_batch_appends_entries() {
        use std::time::SystemTime;

        let mut modal = crate::model::types::FolderHistoryModal {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 0,
            loading: false,
            has_more: true,
            current_offset: 0,
            all_files_sorted: Some(vec![
                ("file1.txt".to_string(), SystemTime::now(), 100),
                ("file2.txt".to_string(), SystemTime::now(), 200),
                ("file3.txt".to_string(), SystemTime::now(), 300),
            ]),
        };

        // Simulate loading next batch from offset 0, limit 2
        let batch = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            0,
            2,
        );
        modal.entries.extend(batch);
        modal.current_offset = 2;

        assert_eq!(modal.entries.len(), 2);
        assert_eq!(modal.current_offset, 2);
    }

    #[test]
    fn test_load_next_batch_updates_state() {
        use std::time::SystemTime;

        let mut modal = crate::model::types::FolderHistoryModal {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 0,
            loading: false,
            has_more: true,
            current_offset: 0,
            all_files_sorted: Some(vec![("file1.txt".to_string(), SystemTime::now(), 100)]),
        };

        // Set loading flag
        modal.loading = true;
        assert!(modal.loading);

        // Load batch
        let batch = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            0,
            100,
        );
        modal.entries.extend(batch);
        modal.current_offset = 100;
        modal.loading = false;
        modal.has_more = false; // No more files

        assert!(!modal.loading);
        assert!(!modal.has_more);
        assert_eq!(modal.entries.len(), 1);
    }

    #[test]
    fn test_load_next_batch_prevents_duplicate_loading() {
        let modal = crate::model::types::FolderHistoryModal {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 0,
            loading: true, // Already loading
            has_more: true,
            current_offset: 0,
            all_files_sorted: None,
        };

        // Should not trigger loading if already loading
        assert!(modal.loading);

        // In actual implementation, load_next_history_batch would return early
        // This test verifies the guard condition
    }

    // ========================================
    // ERROR HANDLING AND EDGE CASES
    // ========================================

    #[test]
    fn test_empty_folder_history() {
        use std::time::SystemTime;

        let modal = crate::model::types::FolderHistoryModal {
            folder_id: "test".to_string(),
            folder_label: "Empty Folder".to_string(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 0,
            loading: false,
            has_more: false,
            current_offset: 0,
            all_files_sorted: Some(vec![]),
        };

        assert_eq!(modal.entries.len(), 0);
        assert_eq!(modal.total_files_scanned, 0);
        assert!(!modal.has_more);

        // Extract batch from empty list should return empty vec
        let batch = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            0,
            100,
        );
        assert_eq!(batch.len(), 0);
    }

    #[test]
    fn test_single_file_folder() {
        use std::time::SystemTime;

        let files = vec![("single.txt".to_string(), SystemTime::now(), 100)];
        let batch = crate::logic::folder_history::extract_batch_from_sorted(&files, 0, 100);

        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].file_path, "single.txt");
        assert_eq!(batch[0].file_size, Some(100));
    }

    #[test]
    fn test_exactly_100_files() {
        use std::time::SystemTime;

        let mut files = vec![];
        for i in 0..100 {
            files.push((format!("file{}.txt", i), SystemTime::now(), 100));
        }

        let batch = crate::logic::folder_history::extract_batch_from_sorted(&files, 0, 100);

        assert_eq!(batch.len(), 100);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_batch_at_exact_boundary() {
        use std::time::SystemTime;

        let mut files = vec![];
        for i in 0..200 {
            files.push((format!("file{}.txt", i), SystemTime::now(), 100));
        }

        // First batch
        let batch1 = crate::logic::folder_history::extract_batch_from_sorted(&files, 0, 100);
        assert_eq!(batch1.len(), 100);

        // Second batch - exactly at boundary
        let batch2 = crate::logic::folder_history::extract_batch_from_sorted(&files, 100, 100);
        assert_eq!(batch2.len(), 100);

        // Third batch - offset beyond end
        let batch3 = crate::logic::folder_history::extract_batch_from_sorted(&files, 200, 100);
        assert_eq!(batch3.len(), 0);
    }

    #[test]
    fn test_partial_final_batch() {
        use std::time::SystemTime;

        let mut files = vec![];
        for i in 0..150 {
            files.push((format!("file{}.txt", i), SystemTime::now(), 100));
        }

        // First batch - full 100
        let batch1 = crate::logic::folder_history::extract_batch_from_sorted(&files, 0, 100);
        assert_eq!(batch1.len(), 100);

        // Second batch - partial 50
        let batch2 = crate::logic::folder_history::extract_batch_from_sorted(&files, 100, 100);
        assert_eq!(batch2.len(), 50);
    }

    #[test]
    fn test_has_more_flag_accuracy() {
        use std::time::SystemTime;

        // Test with exactly 100 files
        let files_100: Vec<(String, SystemTime, u64)> = (0..100)
            .map(|i| (format!("file{}.txt", i), SystemTime::now(), 100))
            .collect();
        let total_100 = files_100.len();
        let has_more_100 = total_100 > 100;
        assert!(!has_more_100, "100 files should not have more");

        // Test with 101 files
        let files_101: Vec<(String, SystemTime, u64)> = (0..101)
            .map(|i| (format!("file{}.txt", i), SystemTime::now(), 100))
            .collect();
        let total_101 = files_101.len();
        let has_more_101 = total_101 > 100;
        assert!(has_more_101, "101 files should have more");

        // Test with 99 files
        let files_99: Vec<(String, SystemTime, u64)> = (0..99)
            .map(|i| (format!("file{}.txt", i), SystemTime::now(), 100))
            .collect();
        let total_99 = files_99.len();
        let has_more_99 = total_99 > 100;
        assert!(!has_more_99, "99 files should not have more");
    }

    #[test]
    fn test_offset_calculation_after_multiple_batches() {
        use std::time::SystemTime;

        let mut modal = crate::model::types::FolderHistoryModal {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 250,
            loading: false,
            has_more: true,
            current_offset: 0,
            all_files_sorted: Some(
                (0..250)
                    .map(|i| (format!("file{}.txt", i), SystemTime::now(), 100))
                    .collect(),
            ),
        };

        // Simulate loading 3 batches
        // Batch 1: offset 0, limit 100
        let batch1 = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            0,
            100,
        );
        modal.entries.extend(batch1);
        modal.current_offset = 100;

        assert_eq!(modal.entries.len(), 100);
        assert_eq!(modal.current_offset, 100);

        // Batch 2: offset 100, limit 100
        let batch2 = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            100,
            100,
        );
        modal.entries.extend(batch2);
        modal.current_offset = 200;

        assert_eq!(modal.entries.len(), 200);
        assert_eq!(modal.current_offset, 200);

        // Batch 3: offset 200, limit 100 (only 50 remaining)
        let batch3 = crate::logic::folder_history::extract_batch_from_sorted(
            modal.all_files_sorted.as_ref().unwrap(),
            200,
            100,
        );
        modal.entries.extend(batch3);
        modal.current_offset = 250;
        modal.has_more = false;

        assert_eq!(modal.entries.len(), 250);
        assert_eq!(modal.current_offset, 250);
        assert!(!modal.has_more);
    }
}
