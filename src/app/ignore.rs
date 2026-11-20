//! Ignore pattern management
//!
//! Methods for managing .stignore patterns:
//! - Toggle ignore state (add/remove patterns)
//! - Ignore and delete (immediate action)

use crate::{App, SyncState, log_debug, logic, model, services};
use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const UNIGNORE_FOLLOW_UP_DELAY: Duration = Duration::from_secs(2);

/// What `toggle_ignore` did, so the key handler can give feedback.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IgnoreToggle {
    Unchanged,
    Ignored,
    /// Holds the item name.
    Unignored(String),
    /// Several patterns match; the pattern selection menu is open.
    ChoosePattern,
}

impl App {
    pub(crate) async fn toggle_ignore(&mut self) -> Result<IgnoreToggle> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return Ok(IgnoreToggle::Unchanged);
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(IgnoreToggle::Unchanged);
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();
        let prefix = level.prefix.clone();

        // Get selected item (respects filtered view if active)
        let selected_idx = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(IgnoreToggle::Unchanged),
        };

        let item = match level.display_items().get(selected_idx) {
            Some(item) => item,
            None => return Ok(IgnoreToggle::Unchanged),
        };
        let item_name = item.name.clone(); // Clone for later use
        let sync_state = level
            .file_sync_states
            .get(&item.name)
            .copied()
            .unwrap_or(SyncState::Unknown);

        // Build the relative path from folder root
        let relative_path = if let Some(ref prefix) = prefix {
            format!("{}{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        // Get current ignore patterns
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;

        if sync_state == SyncState::Ignored {
            // File is ignored - check if un-ignore is allowed (not pending deletion)
            // Build host path to check against pending deletes
            let level = &self.model.navigation.breadcrumb_trail[level_idx];
            let host_path = format!(
                "{}/{}",
                level.translated_base_path.trim_end_matches('/'),
                item.name
            );
            let path_buf = PathBuf::from(&host_path);

            // Check if this path or any parent is pending deletion
            if let Some(pending_path) = self.is_path_or_parent_pending(&folder_id, &path_buf) {
                let message = format!(
                    "Cannot un-ignore: deletion in progress for {}",
                    pending_path.display()
                );
                self.model.ui.toast_message = Some((message, Instant::now()));
                log_debug(&format!(
                    "Blocked un-ignore: path {:?} is pending deletion",
                    pending_path
                ));
                return Ok(IgnoreToggle::Unchanged);
            }

            // File is ignored - find matching patterns and remove them
            let file_path = format!("/{}", relative_path);
            let matching_patterns = logic::ignore::find_matching_patterns(&patterns, &file_path);

            if matching_patterns.is_empty() {
                return Ok(IgnoreToggle::Unchanged); // No patterns match (shouldn't happen)
            }

            if matching_patterns.len() == 1 {
                // Only one pattern - remove it directly
                let pattern_to_remove = &matching_patterns[0];
                let updated_patterns: Vec<String> = patterns
                    .into_iter()
                    .filter(|p| p != pattern_to_remove)
                    .collect();

                self.client
                    .set_ignore_patterns(&folder_id, updated_patterns)
                    .await?;

                let file_path_for_api = if let Some(ref prefix) = prefix {
                    format!("{}/{}", prefix.trim_matches('/'), item_name)
                } else {
                    item_name.clone()
                };
                self.finish_unignore(&folder_id, file_path_for_api);
                return Ok(IgnoreToggle::Unignored(item_name));
            } else {
                // Multiple patterns match - show selection menu
                self.model.ui.pattern_selection = Some(model::PatternSelectionState {
                    folder_id,
                    item_name,
                    patterns: matching_patterns,
                    selected_index: Some(0),
                });
                return Ok(IgnoreToggle::ChoosePattern);
            }
        } else {
            // File is not ignored - add it to ignore
            let new_pattern = format!("/{}", relative_path);

            // Check if pattern already exists
            if patterns.contains(&new_pattern) {
                return Ok(IgnoreToggle::Unchanged);
            }

            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern);

            self.client
                .set_ignore_patterns(&folder_id, updated_patterns)
                .await?;

            // Immediately mark as ignored in UI
            if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                level
                    .file_sync_states
                    .insert(item_name.clone(), SyncState::Ignored);

                // Update ignored_exists (file is now ignored) - do it inline to avoid borrow issues
                // translated_base_path already includes the full path to this directory level
                let host_path = format!(
                    "{}/{}",
                    level.translated_base_path.trim_end_matches('/'),
                    item_name
                );
                let exists = std::path::Path::new(&host_path).exists();
                level.ignored_exists.insert(item_name.clone(), exists);

                // Update cache immediately so browse refresh doesn't overwrite with stale data
                let _ =
                    self.cache
                        .save_sync_state(&folder_id, &relative_path, SyncState::Ignored, 0);
            }

            // Trigger rescan in background
            let client = self.client.clone();
            let folder_id_clone = folder_id.clone();
            tokio::spawn(async move {
                let _ = client.rescan_folder(&folder_id_clone).await;
            });
        }

        Ok(IgnoreToggle::Ignored)
    }

    /// Rescan after an unignore and refresh the item's state.
    ///
    /// The item keeps its Ignored state and icon until a FileInfo response replaces them.
    /// `POST /rest/db/scan` returns once the scan has finished, so the first FileInfo
    /// reflects the new ignore patterns. The second one catches flags the puller updates
    /// after the scan (an unignored file that only exists remotely).
    pub(crate) fn finish_unignore(&self, folder_id: &str, file_path: String) {
        log_debug(&format!(
            "DEBUG [finish_unignore]: folder={} path={}",
            folder_id, file_path
        ));

        let client = self.client.clone();
        let folder_id = folder_id.to_string();
        let api_tx = self.api_tx.clone();

        tokio::spawn(async move {
            // Give Syncthing a moment to apply the new .stignore before scanning
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            let _ = client.rescan_folder(&folder_id).await;

            let request = || services::api::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: file_path.clone(),
                priority: services::api::Priority::High,
            };
            let _ = api_tx.send(request());
            tokio::time::sleep(UNIGNORE_FOLLOW_UP_DELAY).await;
            let _ = api_tx.send(request());
        });
    }

    pub(crate) async fn ignore_and_delete(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();

        // Get selected item (respects filtered view if active)
        let selected_idx = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(()),
        };

        let item = match level.display_items().get(selected_idx) {
            Some(item) => item,
            None => return Ok(()),
        };
        let item_name = item.name.clone(); // Clone for later use

        // Build paths
        let relative_path = if let Some(ref prefix) = level.prefix {
            format!("{}{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        // Note: translated_base_path already includes full directory path (with prefix),
        // so we only append the item name (not relative_path which duplicates the prefix)
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

        // Check if file exists on disk
        if !std::path::Path::new(&host_path).exists() {
            // File doesn't exist, just add to ignore (no need to track pending)
            let patterns = self.client.get_ignore_patterns(&folder_id).await?;
            let new_pattern = format!("/{}", relative_path);

            if !patterns.contains(&new_pattern) {
                let mut updated_patterns = patterns;
                updated_patterns.insert(0, new_pattern);

                self.client
                    .set_ignore_patterns(&folder_id, updated_patterns)
                    .await?;

                if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                    level
                        .file_sync_states
                        .insert(item_name.clone(), SyncState::Ignored);

                    // Update ignored_exists (file is now ignored)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Ignored);
                }

                // Trigger rescan in background
                let client = self.client.clone();
                let folder_id_clone = folder_id.clone();
                tokio::spawn(async move {
                    let _ = client.rescan_folder(&folder_id_clone).await;
                });
            }
            return Ok(());
        }

        // File exists - add to ignore first, then delete
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;
        let new_pattern = format!("/{}", relative_path);

        if !patterns.contains(&new_pattern) {
            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern);
            self.client
                .set_ignore_patterns(&folder_id, updated_patterns)
                .await?;
        }

        // Register this path as pending deletion BEFORE we delete
        // This prevents un-ignore until Syncthing processes the change
        let path_buf = PathBuf::from(&host_path);
        self.add_pending_delete(folder_id.clone(), path_buf.clone());

        // Now delete the file
        let is_dir = std::path::Path::new(&host_path).is_dir();
        let delete_result = if is_dir {
            std::fs::remove_dir_all(&host_path)
        } else {
            std::fs::remove_file(&host_path)
        };

        match delete_result {
            Ok(()) => {
                // Verify file is actually gone
                if std::path::Path::new(&host_path).exists() {
                    log_debug(&format!(
                        "Warning: File still exists after deletion: {}",
                        host_path
                    ));
                    // Keep in pending set for safety
                } else {
                    log_debug(&format!("Successfully deleted file: {}", host_path));
                }

                if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                    level
                        .file_sync_states
                        .insert(item_name.clone(), SyncState::Ignored);

                    // Update ignored_exists (file is now ignored and deleted)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Ignored);
                }

                // Mark that rescan has been triggered for this folder
                if let Some(pending_info) = self
                    .model
                    .performance
                    .pending_ignore_deletes
                    .get_mut(&folder_id)
                {
                    pending_info.rescan_triggered = true;
                }

                // Trigger rescan in background
                let client = self.client.clone();
                tokio::spawn(async move {
                    let _ = client.rescan_folder(&folder_id).await;
                });
            }
            Err(e) => {
                // Deletion failed - remove from pending immediately
                log_debug(&format!("Failed to delete file: {} - {}", host_path, e));
                self.remove_pending_delete(&folder_id, &path_buf);
                return Err(e.into());
            }
        }

        Ok(())
    }
}
