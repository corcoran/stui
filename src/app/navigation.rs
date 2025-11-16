//! Navigation orchestration methods
//!
//! Methods for traversing the folder hierarchy:
//! - Loading root level and entering directories
//! - Going back to parent directories
//! - Moving selection up/down/page/jump

use crate::{App, SyncState, log_debug, logic, model, services};
use anyhow::Result;
use std::time::Instant;

impl App {
    pub(crate) async fn load_root_level(&mut self, preview_only: bool) -> Result<()> {
        if let Some(selected) = self.model.navigation.folders_state_selection
            && let Some(folder) = self.model.syncthing.folders.get(selected).cloned()
        {
            // Don't try to browse paused folders
            if folder.paused {
                // Stay on folder list, don't enter the folder
                return Ok(());
            }

            // Start timing
            let start = Instant::now();

            // Get folder sequence for cache validation
            let folder_sequence = self
                .model
                .syncthing
                .folder_statuses
                .get(&folder.id)
                .map(|s| s.sequence)
                .unwrap_or(0);

            log_debug(&format!(
                "DEBUG [load_root_level]: folder={} using sequence={}",
                folder.id, folder_sequence
            ));

            // Create key for tracking in-flight operations
            let browse_key = format!("{}:", folder.id); // Empty prefix for root

            // Remove from loading_browse set if it's there (cleanup from previous attempts)
            self.model.performance.loading_browse.remove(&browse_key);

            // Try cache first
            let (items, local_items) = if let Ok(Some(cached_items)) =
                self.cache
                    .get_browse_items(&folder.id, None, folder_sequence)
            {
                self.model.performance.cache_hit = Some(true);
                let mut items = cached_items;
                // Merge local files even from cache
                let local_items = self
                    .merge_local_only_files(&folder.id, &mut items, None)
                    .await;
                (items, local_items)
            } else {
                // Mark as loading
                self.model
                    .performance
                    .loading_browse
                    .insert(browse_key.clone());

                // Cache miss - fetch from API
                self.model.performance.cache_hit = Some(false);
                match self.client.browse_folder(&folder.id, None).await {
                    Ok(mut items) => {
                        // Merge local-only files from receive-only folders
                        let local_items = self
                            .merge_local_only_files(&folder.id, &mut items, None)
                            .await;

                        if let Err(e) =
                            self.cache
                                .save_browse_items(&folder.id, None, &items, folder_sequence)
                        {
                            log_debug(&format!("ERROR saving root cache: {}", e));
                        }

                        // Done loading
                        self.model.performance.loading_browse.remove(&browse_key);

                        (items, local_items)
                    }
                    Err(e) => {
                        self.model.performance.loading_browse.remove(&browse_key);
                        log_debug(&format!(
                            "Failed to browse folder root: {}",
                            crate::logic::errors::format_error_message(&e)
                        ));
                        return Ok(());
                    }
                }
            };

            // Record load time
            self.model.performance.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

            // Compute translated base path once
            let translated_base_path =
                logic::path::translate_path(&folder.path, "", &self.path_map);

            // Load cached sync states for items
            let mut file_sync_states = self.load_sync_states_from_cache(&folder.id, &items, None);

            // Mark local-only items with LocalOnly sync state and save to cache
            for local_item_name in &local_items {
                file_sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                // Save to cache so it persists
                let _ = self.cache.save_sync_state(
                    &folder.id,
                    local_item_name,
                    SyncState::LocalOnly,
                    0,
                );
            }

            // Check which ignored files exist on disk (one-time check, not per-frame)
            // Root level: no parent to check
            let ignored_exists = logic::sync_states::check_ignored_existence(
                &items,
                &file_sync_states,
                &translated_base_path,
                None,
            );

            self.model.navigation.breadcrumb_trail = vec![model::BreadcrumbLevel {
                folder_id: folder.id.clone(),
                folder_label: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
                folder_path: folder.path.clone(),
                prefix: None,
                items,
                selected_index: None, // sort_current_level will set selection
                translated_base_path,
                file_sync_states,
                ignored_exists,
                filtered_items: None,
            }];

            // Only change focus if not in preview mode
            if !preview_only {
                self.model.navigation.focus_level = 1;
            }

            // Apply initial sorting
            self.sort_current_level();
        }
        Ok(())
    }

    pub(crate) async fn enter_directory(&mut self) -> Result<()> {
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        // Start timing
        let start = Instant::now();

        let current_level = &self.model.navigation.breadcrumb_trail[level_idx];
        if let Some(selected_idx) = current_level.selected_index
            && let Some(item) = current_level.display_items().get(selected_idx)
        {
            // Only enter if it's a directory
            if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                return Ok(());
            }

            let folder_id = current_level.folder_id.clone();
            let folder_label = current_level.folder_label.clone();
            let folder_path = current_level.folder_path.clone();

            // Build new prefix
            let new_prefix = if let Some(ref prefix) = current_level.prefix {
                format!("{}{}/", prefix, item.name)
            } else {
                format!("{}/", item.name)
            };

            // Get folder sequence for cache validation
            let folder_sequence = self
                .model
                .syncthing
                .folder_statuses
                .get(&folder_id)
                .map(|s| s.sequence)
                .unwrap_or(0);

            // Create key for tracking in-flight operations
            let browse_key = format!("{}:{}", folder_id, new_prefix);

            // Remove from loading_browse set if it's there (cleanup from previous attempts)
            self.model.performance.loading_browse.remove(&browse_key);

            // Try cache first
            let (items, local_items) = if let Ok(Some(cached_items)) =
                self.cache
                    .get_browse_items(&folder_id, Some(&new_prefix), folder_sequence)
            {
                self.model.performance.cache_hit = Some(true);
                let mut items = cached_items;
                // Merge local files even from cache
                let local_items = self
                    .merge_local_only_files(&folder_id, &mut items, Some(&new_prefix))
                    .await;
                (items, local_items)
            } else {
                // Mark as loading
                self.model
                    .performance
                    .loading_browse
                    .insert(browse_key.clone());
                self.model.performance.cache_hit = Some(false);

                // Cache miss - fetch from API (BLOCKING)
                match self
                    .client
                    .browse_folder(&folder_id, Some(&new_prefix))
                    .await
                {
                    Ok(mut items) => {
                        // Merge local-only files from receive-only folders
                        let local_items = self
                            .merge_local_only_files(&folder_id, &mut items, Some(&new_prefix))
                            .await;

                        let _ = self.cache.save_browse_items(
                            &folder_id,
                            Some(&new_prefix),
                            &items,
                            folder_sequence,
                        );

                        // Done loading
                        self.model.performance.loading_browse.remove(&browse_key);

                        (items, local_items)
                    }
                    Err(e) => {
                        self.model.ui.show_toast(format!(
                            "Unable to browse: {}",
                            crate::logic::errors::format_error_message(&e)
                        ));
                        self.model.performance.loading_browse.remove(&browse_key);
                        return Ok(());
                    }
                }
            };

            // Record load time
            self.model.performance.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

            // Compute translated base path once for this level
            let full_relative_path = new_prefix.trim_end_matches('/');
            let container_path = format!(
                "{}/{}",
                folder_path.trim_end_matches('/'),
                full_relative_path
            );

            // Map to host path
            let translated_base_path = self
                .path_map
                .iter()
                .find_map(|(container_prefix, host_prefix)| {
                    container_path
                        .strip_prefix(container_prefix.as_str())
                        .map(|remainder| {
                            format!("{}{}", host_prefix.trim_end_matches('/'), remainder)
                        })
                })
                .unwrap_or(container_path);

            // Truncate breadcrumb trail to current level + 1
            self.model
                .navigation
                .breadcrumb_trail
                .truncate(level_idx + 1);

            // Load cached sync states for items
            let mut file_sync_states =
                self.load_sync_states_from_cache(&folder_id, &items, Some(&new_prefix));
            log_debug(&format!(
                "DEBUG [enter_directory]: Loaded {} cached states for new level with prefix={}",
                file_sync_states.len(),
                new_prefix
            ));

            // Mark local-only items with LocalOnly sync state and save to cache
            for local_item_name in &local_items {
                file_sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                // Save to cache so it persists
                let file_path = format!("{}{}", new_prefix, local_item_name);
                let _ = self
                    .cache
                    .save_sync_state(&folder_id, &file_path, SyncState::LocalOnly, 0);
            }

            // Check if we're inside an ignored directory (check all ancestors) - if so, mark all children as ignored
            // This handles the case where you ignore a directory and immediately drill into it
            // Ancestor checking removed - FileInfo API will provide correct states

            // Check which ignored files exist on disk (one-time check, not per-frame)
            // Determine if parent directory exists (optimization for ignored directories)
            let parent_exists = Some(std::path::Path::new(&translated_base_path).exists());
            let ignored_exists = logic::sync_states::check_ignored_existence(
                &items,
                &file_sync_states,
                &translated_base_path,
                parent_exists,
            );

            // Add new level
            self.model
                .navigation
                .breadcrumb_trail
                .push(model::BreadcrumbLevel {
                    folder_id,
                    folder_label,
                    folder_path,
                    prefix: Some(new_prefix),
                    items,
                    selected_index: None, // sort_current_level will set selection
                    translated_base_path,
                    file_sync_states,
                    ignored_exists,
                    filtered_items: None,
                });

            self.model.navigation.focus_level += 1;

            // Apply initial sorting
            self.sort_current_level();

            // Apply search filter if search is active
            if !self.model.ui.search_query.is_empty() {
                self.apply_search_filter();
            }

            // Apply out-of-sync filter if active
            if self.model.ui.out_of_sync_filter.is_some() {
                self.apply_out_of_sync_filter();
            }
        }

        Ok(())
    }

    /// Enter a folder from folder view (focus_level == 0)
    ///
    /// Creates the first breadcrumb level for the folder.
    /// This is the programmatic equivalent of selecting a folder and pressing Enter.
    pub async fn enter_folder(&mut self, folder_id: &str) -> anyhow::Result<()> {
        use std::time::Instant;

        log_debug(&format!("Entering folder: {}", folder_id));

        // Find folder
        let folder = self
            .model
            .syncthing
            .folders
            .iter()
            .find(|f| f.id == folder_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Folder not found: {}", folder_id))?;

        // Update selection in folder list
        if let Some(idx) = self
            .model
            .syncthing
            .folders
            .iter()
            .position(|f| f.id == folder_id)
        {
            self.model.navigation.folders_state_selection = Some(idx);
        }

        let folder_id = folder.id.clone();
        let folder_label = folder.label.clone().unwrap_or_else(|| folder.id.clone());
        let folder_path = folder.path;

        // Start timing
        let start = Instant::now();

        // Get folder sequence for cache validation
        let folder_sequence = self
            .model
            .syncthing
            .folder_statuses
            .get(&folder_id)
            .map(|s| s.sequence)
            .unwrap_or(0);

        // Create key for tracking in-flight operations
        let browse_key = format!("{}:", folder_id);

        // Remove from loading_browse set if it's there
        self.model.performance.loading_browse.remove(&browse_key);

        // Try cache first
        let (items, local_items) = if let Ok(Some(cached_items)) =
            self.cache
                .get_browse_items(&folder_id, None, folder_sequence)
        {
            self.model.performance.cache_hit = Some(true);
            let mut items = cached_items;
            let local_items = self
                .merge_local_only_files(&folder_id, &mut items, None)
                .await;
            (items, local_items)
        } else {
            // Mark as loading
            self.model
                .performance
                .loading_browse
                .insert(browse_key.clone());
            self.model.performance.cache_hit = Some(false);

            // Cache miss - fetch from API
            match self.client.browse_folder(&folder_id, None).await {
                Ok(mut items) => {
                    let local_items = self
                        .merge_local_only_files(&folder_id, &mut items, None)
                        .await;

                    let _ = self
                        .cache
                        .save_browse_items(&folder_id, None, &items, folder_sequence);

                    self.model.performance.loading_browse.remove(&browse_key);

                    (items, local_items)
                }
                Err(e) => {
                    self.model.ui.show_toast(format!("Unable to browse: {}", e));
                    self.model.performance.loading_browse.remove(&browse_key);
                    return Err(e);
                }
            }
        };

        // Record load time
        self.model.performance.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

        // Load cached sync states for items
        let mut file_sync_states = self.load_sync_states_from_cache(&folder_id, &items, None);

        // Mark local-only items with LocalOnly sync state
        for local_item_name in &local_items {
            file_sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
            let _ =
                self.cache
                    .save_sync_state(&folder_id, local_item_name, SyncState::LocalOnly, 0);
        }

        // Check which ignored files exist on disk
        let parent_exists = Some(std::path::Path::new(&folder_path).exists());
        let ignored_exists = logic::sync_states::check_ignored_existence(
            &items,
            &file_sync_states,
            &folder_path,
            parent_exists,
        );

        // Clear any existing breadcrumb trail and create first breadcrumb level
        self.model.navigation.breadcrumb_trail.clear();
        self.model
            .navigation
            .breadcrumb_trail
            .push(model::BreadcrumbLevel {
                folder_id,
                folder_label,
                folder_path: folder_path.clone(),
                prefix: None,
                items,
                selected_index: None,
                translated_base_path: folder_path,
                file_sync_states,
                ignored_exists,
                filtered_items: None,
            });

        self.model.navigation.focus_level = 1;

        // Apply initial sorting
        self.sort_current_level();

        // Apply search filter if search is active
        if !self.model.ui.search_query.is_empty() {
            self.apply_search_filter();
        }

        // Apply out-of-sync filter if active
        if self.model.ui.out_of_sync_filter.is_some() {
            self.apply_out_of_sync_filter();
        }

        Ok(())
    }

    pub(crate) fn go_back(&mut self) {
        // Only clear search if backing out past the level where it was initiated
        let should_clear_search = if let Some(origin_level) = self.model.ui.search_origin_level {
            // Clear search if we're backing out of the origin level or below it
            self.model.navigation.focus_level <= origin_level
        } else {
            false
        };

        if should_clear_search {
            self.clear_search(None); // No toast, contextual clearing
        }

        if self.model.navigation.focus_level > 1 {
            // Backing out to a parent breadcrumb - refresh it if search was cleared
            if should_clear_search && self.model.navigation.focus_level >= 2 {
                let parent_idx = self.model.navigation.focus_level - 2;
                if let Some(parent_level) = self.model.navigation.breadcrumb_trail.get(parent_idx) {
                    let folder_id = parent_level.folder_id.clone();
                    let prefix = parent_level.prefix.clone();

                    let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                        folder_id,
                        prefix,
                        priority: services::api::Priority::High,
                    });
                }
            }

            self.model.navigation.breadcrumb_trail.pop();
            self.model.navigation.focus_level -= 1;
        } else if self.model.navigation.focus_level == 1 {
            // Going back to folder view - refresh root directory if search was cleared
            if should_clear_search
                && let Some(root_level) = self.model.navigation.breadcrumb_trail.first()
            {
                let folder_id = root_level.folder_id.clone();
                let prefix = root_level.prefix.clone();

                let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                    folder_id,
                    prefix,
                    priority: services::api::Priority::High,
                });
            }

            // Clear out-of-sync filter when backing out to folder list
            if self.model.ui.out_of_sync_filter.is_some() {
                self.clear_out_of_sync_filter(false, None); // Don't preserve (leaving breadcrumbs), no toast
            }

            self.model.navigation.focus_level = 0;
        }
    }

    /// Navigate with a custom update function
    ///
    /// This helper unifies all navigation operations (next, prev, jump, page).
    /// It handles the focus_level branching and auto-preview loading for folder view.
    ///
    /// The update_fn receives (current_selection, list_length) and returns new_selection.
    async fn navigate_with<F>(&mut self, update_fn: F)
    where
        F: Fn(Option<usize>, usize) -> Option<usize>,
    {
        if self.model.navigation.focus_level == 0 {
            // Folder view navigation
            let len = self.model.syncthing.folders.len();
            if len > 0 {
                self.model.navigation.folders_state_selection =
                    update_fn(self.model.navigation.folders_state_selection, len);
                // Auto-load the selected folder's root directory as preview (don't change focus)
                let _ = self.load_root_level(true).await;
            }
        } else {
            // Breadcrumb level navigation
            let level_idx = self.model.navigation.focus_level - 1;
            if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                let len = level.display_items().len();
                if len > 0 {
                    level.selected_index = update_fn(level.selected_index, len);
                }
            }
        }
    }

    pub(crate) async fn next_item(&mut self) {
        self.navigate_with(logic::navigation::next_selection).await;
    }

    pub(crate) async fn previous_item(&mut self) {
        self.navigate_with(logic::navigation::prev_selection).await;
    }

    pub(crate) async fn jump_to_first(&mut self) {
        self.navigate_with(|_, _| Some(0)).await;
    }

    pub(crate) async fn jump_to_last(&mut self) {
        self.navigate_with(|_, len| Some(len - 1)).await;
    }

    pub(crate) async fn page_down(&mut self, page_size: usize) {
        self.navigate_with(|sel, len| Some(sel.map_or(0, |i| (i + page_size).min(len - 1))))
            .await;
    }

    pub(crate) async fn page_up(&mut self, page_size: usize) {
        self.navigate_with(|sel, _| Some(sel.map_or(0, |i| i.saturating_sub(page_size))))
            .await;
    }

    pub(crate) async fn half_page_down(&mut self, visible_height: usize) {
        self.page_down(visible_height / 2).await;
    }

    pub(crate) async fn half_page_up(&mut self, visible_height: usize) {
        self.page_up(visible_height / 2).await;
    }
}
