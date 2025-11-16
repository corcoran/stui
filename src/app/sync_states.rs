//! Sync state management and prefetching
//!
//! Methods for managing file/directory sync states:
//! - Updating directory states based on children
//! - Batch fetching sync states for visible items
//! - Prefetching subdirectory states
//! - Tracking ignored file existence

use crate::{App, SyncState, log_debug, logic, services};
use std::collections::HashMap;
use std::time::Duration;

impl App {
    pub(crate) fn update_directory_states(&mut self, level_idx: usize) {
        // Throttle: only run once every 2 seconds to prevent continuous cache queries
        if self.last_directory_update.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_directory_update = std::time::Instant::now();

        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return;
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];
        let mut dir_states: HashMap<String, SyncState> = HashMap::new();

        // Collect directory names first
        let directory_names: Vec<String> = level
            .items
            .iter()
            .filter(|item| item.item_type == "FILE_INFO_TYPE_DIRECTORY")
            .map(|item| item.name.clone())
            .collect();

        // For each directory, determine its state based on children
        for dir_name in &directory_names {
            // Get the directory's current direct state (from FileInfo)
            let direct_state = level.file_sync_states.get(dir_name).copied();

            // If the directory itself is RemoteOnly or Ignored, that takes precedence
            // (handled by aggregate_directory_state function)
            if matches!(
                direct_state,
                Some(SyncState::RemoteOnly) | Some(SyncState::Ignored)
            ) {
                dir_states.insert(dir_name.clone(), direct_state.unwrap());
                continue;
            }

            // Check if we have cached children states
            let dir_prefix = if let Some(ref prefix) = level.prefix {
                format!("{}{}/", prefix, dir_name)
            } else {
                format!("{}/", dir_name)
            };

            // Get folder sequence for cache validation
            let folder_sequence = self
                .model
                .syncthing
                .folder_statuses
                .get(&level.folder_id)
                .map(|status| status.sequence)
                .unwrap_or_else(|| {
                    log_debug(&format!(
                        "DEBUG [update_directory_states]: folder_id '{}' not found in folder_statuses, using sequence=0",
                        level.folder_id
                    ));
                    0
                });

            // Try to get cached browse items for this directory
            if let Ok(Some(children)) =
                self.cache
                    .get_browse_items(&level.folder_id, Some(&dir_prefix), folder_sequence)
            {
                // Collect all child states
                let child_states: Vec<SyncState> = children
                    .iter()
                    .filter_map(|child| {
                        let child_path = format!("{}{}", dir_prefix, child.name);
                        self.cache
                            .get_sync_state_unvalidated(&level.folder_id, &child_path)
                            .ok()
                            .flatten()
                    })
                    .collect();

                // Use pure function to determine aggregate state
                let aggregate_state =
                    logic::sync_states::aggregate_directory_state(direct_state, &child_states);
                dir_states.insert(dir_name.clone(), aggregate_state);
            } else {
                // No cached children, use direct state
                dir_states.insert(dir_name.clone(), direct_state.unwrap_or(SyncState::Synced));
            }
        }

        // Apply computed states
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            for (dir_name, state) in dir_states {
                let current = level.file_sync_states.get(&dir_name).copied();
                if current != Some(state) {
                    log_debug(&format!(
                        "DEBUG [update_directory_states]: Setting {} to {:?} (was {:?})",
                        dir_name, state, current
                    ));
                    level.file_sync_states.insert(dir_name, state);
                }
            }
        }
    }

    pub(crate) fn batch_fetch_visible_sync_states(&mut self, max_concurrent: usize) {
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return;
        }

        // Get items that need fetching (don't have sync state and aren't loading)
        let folder_id = self.model.navigation.breadcrumb_trail[level_idx]
            .folder_id
            .clone();
        let prefix = self.model.navigation.breadcrumb_trail[level_idx]
            .prefix
            .clone();

        let items_to_fetch: Vec<String> = self.model.navigation.breadcrumb_trail[level_idx]
            .items
            .iter()
            .filter(|item| {
                // Skip if already have sync state
                if self.model.navigation.breadcrumb_trail[level_idx]
                    .file_sync_states
                    .contains_key(&item.name)
                {
                    return false;
                }

                // Build file path and check if already loading
                let file_path = if let Some(ref prefix) = prefix {
                    format!("{}{}", prefix, item.name)
                } else {
                    item.name.clone()
                };
                let sync_key = format!("{}:{}", folder_id, file_path);

                !self
                    .model
                    .performance
                    .loading_sync_states
                    .contains(&sync_key)
            })
            .take(max_concurrent) // Limit how many we fetch at once
            .map(|item| item.name.clone())
            .collect();

        // Send non-blocking requests via channel
        for item_name in items_to_fetch {
            let file_path = if let Some(ref prefix) = prefix {
                format!("{}{}", prefix, item_name)
            } else {
                item_name.clone()
            };

            let sync_key = format!("{}:{}", folder_id, file_path);

            // Skip if already loading
            if self
                .model
                .performance
                .loading_sync_states
                .contains(&sync_key)
            {
                continue;
            }

            // Mark as loading
            self.model
                .performance
                .loading_sync_states
                .insert(sync_key.clone());

            log_debug(&format!(
                "DEBUG [batch_fetch]: Requesting file_path={} for folder={}",
                file_path, folder_id
            ));

            // Send non-blocking request via channel
            let _ = self.api_tx.send(services::api::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: file_path.clone(),
                priority: services::api::Priority::Medium,
            });
        }
    }

    // Recursively discover and fetch states for subdirectories when hovering over a directory
    // This ensures we have complete subdirectory information for deep trees
    pub(crate) fn prefetch_hovered_subdirectories(
        &mut self,
        max_depth: usize,
        max_dirs_per_frame: usize,
    ) {
        if !self.model.performance.prefetch_enabled {
            return;
        }

        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return;
        }

        // Only run if system isn't too busy
        let total_in_flight = self.model.performance.loading_browse.len()
            + self.model.performance.loading_sync_states.len();
        if total_in_flight > 15 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return;
        }

        // Get the currently selected/hovered item
        let selected_idx = self.model.navigation.breadcrumb_trail[level_idx].selected_index;
        if selected_idx.is_none() {
            return;
        }

        let selected_item = if let Some(item) = self.model.navigation.breadcrumb_trail[level_idx]
            .items
            .get(selected_idx.unwrap())
        {
            item.clone()
        } else {
            return;
        };

        // Only process if it's a directory
        if selected_item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
            return;
        }

        let folder_id = self.model.navigation.breadcrumb_trail[level_idx]
            .folder_id
            .clone();
        let prefix = self.model.navigation.breadcrumb_trail[level_idx]
            .prefix
            .clone();
        let folder_sequence = self
            .model
            .syncthing
            .folder_statuses
            .get(&folder_id)
            .map(|s| s.sequence)
            .unwrap_or(0);

        // Build path to hovered directory
        let hovered_dir_path = if let Some(ref prefix) = prefix {
            format!("{}{}/", prefix, selected_item.name)
        } else {
            format!("{}/", selected_item.name)
        };

        // Recursively discover subdirectories (non-blocking, uses cache only)
        let mut dirs_to_fetch = Vec::new();
        self.discover_subdirectories_sync(
            &folder_id,
            &hovered_dir_path,
            folder_sequence,
            0,
            max_depth,
            &mut dirs_to_fetch,
        );

        // Fetch states for discovered subdirectories (limit per frame)
        for dir_path in dirs_to_fetch.iter().take(max_dirs_per_frame) {
            let sync_key = format!("{}:{}", folder_id, dir_path);

            // Skip if already loading or cached
            if self
                .model
                .performance
                .loading_sync_states
                .contains(&sync_key)
            {
                continue;
            }

            // Check if already cached
            if let Ok(Some(_)) = self.cache.get_sync_state_unvalidated(&folder_id, dir_path) {
                continue;
            }

            // Mark as loading
            self.model
                .performance
                .loading_sync_states
                .insert(sync_key.clone());

            // Send non-blocking request for directory's sync state
            let _ = self.api_tx.send(services::api::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: dir_path.to_string(),
                priority: services::api::Priority::Low, // Low priority, it's speculative prefetch
            });
        }
    }

    // Helper to recursively discover subdirectories (browse only, no state fetching)
    // This is synchronous and only uses cached data - no blocking API calls
    pub(crate) fn discover_subdirectories_sync(
        &mut self,
        folder_id: &str,
        dir_path: &str,
        _folder_sequence: u64,
        current_depth: usize,
        max_depth: usize,
        result: &mut Vec<String>,
    ) {
        if current_depth >= max_depth {
            return;
        }

        let browse_key = format!("{}:{}", folder_id, dir_path);

        // Check if already discovered (prevent re-querying cache every frame)
        if self.model.performance.discovered_dirs.contains(&browse_key) {
            return;
        }

        // Check if already loading
        if self.model.performance.loading_browse.contains(&browse_key) {
            return;
        }

        // Try to get from cache first (accept any cached value, even if sequence is old)
        // For prefetch, we prioritize speed over perfect accuracy
        // Use sequence=0 to accept any cached value regardless of sequence number
        let items = if let Ok(Some(cached_items)) =
            self.cache.get_browse_items(folder_id, Some(dir_path), 0)
        {
            cached_items
        } else {
            // Not cached - request it non-blocking and mark as discovered to prevent re-requesting
            if !self.model.performance.loading_browse.contains(&browse_key) {
                self.model
                    .performance
                    .loading_browse
                    .insert(browse_key.clone());

                // Mark as discovered immediately to prevent repeated requests
                self.model
                    .performance
                    .discovered_dirs
                    .insert(browse_key.clone());

                // Send non-blocking browse request
                let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                    folder_id: folder_id.to_string(),
                    prefix: Some(dir_path.to_string()),
                    priority: services::api::Priority::Low,
                });
            }
            return; // Skip this iteration, will process when cached
        };

        // Mark this directory as discovered to prevent re-querying
        self.model.performance.discovered_dirs.insert(browse_key);

        // Add all subdirectories to result list
        for item in &items {
            if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                let subdir_path = format!("{}{}", dir_path, item.name);
                result.push(subdir_path.clone());

                // Recursively discover deeper (synchronous, no blocking)
                let nested_path = format!("{}/", subdir_path);
                self.discover_subdirectories_sync(
                    folder_id,
                    &nested_path,
                    _folder_sequence,
                    current_depth + 1,
                    max_depth,
                    result,
                );
            }
        }
    }

    // Fetch directory-level sync states for subdirectories (their own metadata, not children)
    // This is cheap and gives immediate feedback for navigation (ignored/deleted/out-of-sync dirs)
    pub(crate) fn fetch_directory_states(&mut self, max_concurrent: usize) {
        if !self.model.performance.prefetch_enabled {
            return;
        }

        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return;
        }

        // Only run if system isn't too busy
        let total_in_flight = self.model.performance.loading_browse.len()
            + self.model.performance.loading_sync_states.len();
        if total_in_flight > 10 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return;
        }

        let folder_id = self.model.navigation.breadcrumb_trail[level_idx]
            .folder_id
            .clone();
        let prefix = self.model.navigation.breadcrumb_trail[level_idx]
            .prefix
            .clone();

        // Find directories that don't have their own sync state cached
        let dirs_to_fetch: Vec<String> = self.model.navigation.breadcrumb_trail[level_idx]
            .items
            .iter()
            .filter(|item| {
                // Only process directories
                if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                    return false;
                }

                // Check if we already have this directory's state
                !self.model.navigation.breadcrumb_trail[level_idx]
                    .file_sync_states
                    .contains_key(&item.name)
            })
            .take(max_concurrent)
            .map(|item| item.name.clone())
            .collect();

        // Fetch directory metadata (not children) for each
        for dir_name in dirs_to_fetch {
            let dir_path = if let Some(ref prefix) = prefix {
                format!("{}{}", prefix, dir_name)
            } else {
                dir_name.clone()
            };

            let sync_key = format!("{}:{}", folder_id, dir_path);

            // Skip if already loading
            if self
                .model
                .performance
                .loading_sync_states
                .contains(&sync_key)
            {
                continue;
            }

            // Mark as loading
            self.model
                .performance
                .loading_sync_states
                .insert(sync_key.clone());

            // Send non-blocking request via API service
            // Response will be handled by handle_api_response
            let _ = self.api_tx.send(services::api::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: dir_path.clone(),
                priority: services::api::Priority::Medium,
            });
        }
    }

    pub(crate) fn fetch_selected_item_sync_state(&mut self) {
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx)
            && let Some(selected_idx) = level.selected_index
            && let Some(item) = level.items.get(selected_idx)
        {
            // Check if we already have the sync state cached
            if level.file_sync_states.contains_key(&item.name) {
                return;
            }

            // Build the file path for the API call
            let file_path = if let Some(ref prefix) = level.prefix {
                format!("{}{}", prefix, item.name)
            } else {
                item.name.clone()
            };

            // Create key for tracking in-flight operations
            let sync_key = format!("{}:{}", level.folder_id, file_path);

            // Skip if already loading
            if self
                .model
                .performance
                .loading_sync_states
                .contains(&sync_key)
            {
                return;
            }

            // Mark as loading
            self.model
                .performance
                .loading_sync_states
                .insert(sync_key.clone());

            // Send non-blocking request via API service
            // Response will be handled by handle_api_response
            let _ = self.api_tx.send(services::api::ApiRequest::GetFileInfo {
                folder_id: level.folder_id.clone(),
                file_path: file_path.clone(),
                priority: services::api::Priority::High, // High priority for selected item
            });
        }
    }

    /// Update ignored_exists status for a single file in a breadcrumb level
    pub(crate) fn update_ignored_exists_for_file(
        &mut self,
        level_idx: usize,
        file_name: &str,
        new_state: SyncState,
    ) {
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            if new_state == SyncState::Ignored {
                // File is now ignored - check if it exists
                // translated_base_path already includes the full path to this directory level
                let host_path = format!(
                    "{}/{}",
                    level.translated_base_path.trim_end_matches('/'),
                    file_name
                );
                let exists = std::path::Path::new(&host_path).exists();
                log_debug(&format!(
                    "DEBUG [update_ignored_exists_for_file]: file_name={} prefix={:?} translated_base_path={} host_path={} exists={}",
                    file_name, level.prefix, level.translated_base_path, host_path, exists
                ));
                level.ignored_exists.insert(file_name.to_string(), exists);
            } else {
                // File is no longer ignored - remove from ignored_exists
                level.ignored_exists.remove(file_name);
            }
        }
    }
}
