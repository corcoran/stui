//! Search and out-of-sync filter functionality
//!
//! This module handles two mutually exclusive filtering features:
//! - **Search**: Recursive search across folder hierarchies with wildcard support
//! - **Out-of-sync Filter**: Shows only files that need syncing or have local changes
//!
//! Both filters work on breadcrumb navigation and share common patterns:
//! - Apply filter to current/all levels
//! - Preserve selection when switching
//! - Mutual exclusion (activating one clears the other)

use crate::{api, log_debug, logic, model, services, App};

impl App {
    // ============================================================================
    // HELPER METHODS
    // ============================================================================

    /// Restore selection in filtered items by name, or default to first item
    fn restore_selection_in_filtered_items(
        level: &mut model::BreadcrumbLevel,
        selected_name: Option<String>,
    ) {
        level.selected_index = if level.display_items().is_empty() {
            None
        } else if let Some(name) = selected_name {
            logic::navigation::find_item_index_by_name(level.display_items(), &name).or(Some(0))
        } else {
            Some(0)
        };
    }

    // ============================================================================
    // SEARCH FUNCTIONALITY
    // ============================================================================

    /// Prefetch subdirectories for search to populate cache recursively
    ///
    /// When user searches, this queues browse requests for all subdirectories
    /// in the folder so that search can find matches at any depth.
    pub(crate) fn prefetch_subdirectories_for_search(
        &mut self,
        folder_id: &str,
        prefix: Option<&str>,
    ) {
        let folder_sequence = self
            .model
            .syncthing
            .folder_statuses
            .get(folder_id)
            .map(|status| status.sequence)
            .unwrap_or(0);

        // Check cache for items at this prefix
        if let Ok(Some(items)) = self
            .cache
            .get_browse_items(folder_id, prefix, folder_sequence)
        {
            // Queue browse requests for all subdirectories
            for item in items {
                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                    let subdir_prefix = if let Some(p) = prefix {
                        format!("{}{}/", p, item.name)
                    } else {
                        format!("{}/", item.name)
                    };

                    // Check if we've already queued this directory
                    let prefetch_key = format!("{}:{}", folder_id, subdir_prefix);
                    if self
                        .model
                        .performance
                        .discovered_dirs
                        .contains(&prefetch_key)
                    {
                        continue; // Already queued, skip
                    }

                    // Mark as queued BEFORE sending request
                    self.model.performance.discovered_dirs.insert(prefetch_key);

                    // Queue the browse request (low priority)
                    let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                        folder_id: folder_id.to_string(),
                        prefix: Some(subdir_prefix),
                        priority: services::api::Priority::Low,
                    });
                }
            }
        }
    }

    /// Apply search filter to current breadcrumb level
    ///
    /// Searches ALL cached items in the folder recursively and filters the current
    /// level to show items that either match or have descendants that match.
    pub(crate) fn apply_search_filter(&mut self) {
        // Don't filter folder list (only breadcrumbs)
        if self.model.navigation.focus_level == 0 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            let folder_id = level.folder_id.clone();
            let prefix = level.prefix.clone();
            let query = self.model.ui.search_query.clone();

            // Save currently selected item name BEFORE filtering to restore after
            let selected_name = level.selected_item().map(|item| item.name.clone());

            // If query is empty, clear filter
            if query.is_empty() {
                level.filtered_items = None;
                return;
            }

            // Minimum 2 characters required for search
            if query.len() < 2 {
                return;
            }

            // Get folder sequence for cache validation
            let folder_sequence = self
                .model
                .syncthing
                .folder_statuses
                .get(&folder_id)
                .map(|status| status.sequence)
                .unwrap_or(0);

            // Get ALL cached items in the folder recursively
            let all_items = match self.cache.get_all_browse_items(&folder_id, folder_sequence) {
                Ok(items) => {
                    // BUG FIX: If cache returns 0 items but level.items has items,
                    // this means cache isn't fully populated yet (sequence mismatch or pending write).
                    // Fall back to filtering current level only.
                    if items.is_empty() && !level.items.is_empty() {
                        log_debug(&format!(
                            "DEBUG [search]: Cache empty but level has {} items - falling back to current-level-only search",
                            level.items.len()
                        ));
                        let filtered =
                            logic::search::filter_items(&level.items, &query, prefix.as_deref());
                        level.filtered_items = Some(filtered);
                        Self::restore_selection_in_filtered_items(level, selected_name);
                        return;
                    }

                    items
                }
                Err(e) => {
                    log_debug(&format!("Failed to get cached items for search: {:?}", e));
                    // Fallback to simple filtering of current level.items only
                    let filtered =
                        logic::search::filter_items(&level.items, &query, prefix.as_deref());
                    level.filtered_items = Some(filtered);
                    Self::restore_selection_in_filtered_items(level, selected_name);
                    return;
                }
            };

            // Use level.items as source (already sorted correctly)
            let current_items = level.items.clone();

            // Build current path for comparison
            let current_path = prefix.as_deref().unwrap_or("");

            // Filter current level items: show items that match OR have matching descendants
            let filtered_items: Vec<api::BrowseItem> = current_items
                .into_iter()
                .filter(|item| {
                    let item_path = if current_path.is_empty() {
                        item.name.clone()
                    } else {
                        // current_path already ends with /, so just append
                        format!("{}{}", current_path, item.name)
                    };

                    // Check if item itself matches
                    if logic::search::search_matches(&query, &item_path) {
                        return true;
                    }

                    // For directories, check if any descendant (at any depth) matches
                    if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                        let descendant_prefix = format!("{}/", item_path);

                        // Check if ANY file/folder inside this directory tree matches the query
                        all_items.iter().any(|(full_path, _)| {
                            full_path.starts_with(&descendant_prefix)
                                && logic::search::search_matches(&query, full_path)
                        })
                    } else {
                        false
                    }
                })
                .collect();

            // Store filtered results (non-destructive)
            // IMPORTANT: Use Some(vec![]) for zero matches, not None
            // None = "no filter active, show all items"
            // Some(vec![]) = "filter active, zero matches found"
            level.filtered_items = Some(filtered_items);
            Self::restore_selection_in_filtered_items(level, selected_name);
        }
    }

    /// Enter search mode (handles mutual exclusion with filter)
    pub(crate) fn enter_search_mode(&mut self) {
        // Only works in breadcrumb view
        if self.model.navigation.focus_level == 0 {
            return;
        }

        // Clear filter if active (mutual exclusion)
        if self.model.ui.out_of_sync_filter.is_some() {
            self.clear_out_of_sync_filter(true, Some("Filter cleared - search active"));
        }

        // Activate search mode
        self.model.ui.search_mode = true;
        self.model.ui.search_query.clear();
        self.model.ui.search_origin_level = Some(self.model.navigation.focus_level);
    }

    /// Clear search state and filtered items
    ///
    /// # Arguments
    /// * `show_toast` - Optional toast message to display
    pub(crate) fn clear_search(&mut self, show_toast: Option<&str>) {
        self.model.ui.search_query.clear();
        self.model.ui.search_mode = false;
        self.model.ui.search_origin_level = None;
        self.model.performance.discovered_dirs.clear();

        // Clear filtered items from all breadcrumb levels
        for level in &mut self.model.navigation.breadcrumb_trail {
            level.filtered_items = None;
        }

        if let Some(msg) = show_toast {
            self.model.ui.show_toast(msg.to_string());
        }
    }

    // ============================================================================
    // OUT-OF-SYNC FILTER FUNCTIONALITY
    // ============================================================================

    /// Apply out-of-sync filter to current breadcrumb level
    ///
    /// Shows only files/directories that:
    /// - Need to be downloaded from remote (out-of-sync)
    /// - Have local changes (receive-only folders)
    pub(crate) fn apply_out_of_sync_filter(&mut self) {
        // Don't filter folder list (only breadcrumbs)
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return;
        }

        // Get folder_id from first breadcrumb level (all levels are in same folder)
        let folder_id = self.model.navigation.breadcrumb_trail[0].folder_id.clone();

        // Get out-of-sync items from cache (once for entire folder)
        let out_of_sync_items = match self.cache.get_out_of_sync_items(&folder_id) {
            Ok(items) => items,
            Err(e) => {
                log_debug(&format!("Cache query failed for out-of-sync items: {}", e));
                for level in &mut self.model.navigation.breadcrumb_trail {
                    level.filtered_items = None;
                }
                return;
            }
        };

        // Get local changed items from cache (once for entire folder)
        let local_changed_items = match self.cache.get_local_changed_items(&folder_id) {
            Ok(items) => items.into_iter().collect::<std::collections::HashSet<_>>(),
            Err(_) => std::collections::HashSet::new(),
        };

        // If no out-of-sync items and no local changed items in cache, handle based on current filter state
        if out_of_sync_items.is_empty() && local_changed_items.is_empty() {
            let any_filtered = self
                .model
                .navigation
                .breadcrumb_trail
                .iter()
                .any(|l| l.filtered_items.is_some());

            if any_filtered {
                // Filter already active but cache is empty (likely just invalidated)
                // Queue fresh NeededFiles request to refresh cache
                let _ = self.api_tx.send(services::api::ApiRequest::GetNeededFiles {
                    folder_id: folder_id.clone(),
                    page: None,
                    perpage: Some(1000),
                });

                // Also queue GetLocalChanged for receive-only folders
                let is_receive_only = self
                    .model
                    .syncthing
                    .folders
                    .iter()
                    .find(|f| f.id == folder_id)
                    .map(|f| f.folder_type == "receiveonly")
                    .unwrap_or(false);

                if is_receive_only {
                    let _ = self
                        .api_tx
                        .send(services::api::ApiRequest::GetLocalChanged {
                            folder_id: folder_id.clone(),
                        });
                }

                // Keep existing filtered_items until fresh data arrives
                return;
            } else {
                // No out-of-sync items - don't activate filter
                return;
            }
        }

        // Apply filter to ALL breadcrumb levels
        let num_levels = self.model.navigation.breadcrumb_trail.len();
        for level_idx in 0..num_levels {
            // Extract data from level (to avoid borrowing issues)
            let (prefix, current_items, selected_name) = {
                let level = &self.model.navigation.breadcrumb_trail[level_idx];

                // Save currently selected item name BEFORE filtering (for current level only)
                let selected_name = if level_idx == self.model.navigation.focus_level - 1 {
                    level.selected_item().map(|item| item.name.clone())
                } else {
                    None
                };

                // Use level.items as source (already sorted correctly)
                (level.prefix.clone(), level.items.clone(), selected_name)
            };

            // Build current path for comparison
            let current_path = prefix.clone().unwrap_or_default();

            // Filter items: keep only those in out_of_sync_items map
            let mut filtered: Vec<api::BrowseItem> = Vec::new();

            for item in &current_items {
                let full_path = if current_path.is_empty() {
                    item.name.clone()
                } else {
                    format!("{}{}", current_path, item.name)
                };

                // Check if this item (or any child if directory) is out of sync OR local changed
                let is_out_of_sync = if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                    // For directories: check if any child is out of sync OR local changed
                    let dir_prefix = if full_path.ends_with('/') {
                        full_path.clone()
                    } else {
                        format!("{}/", full_path)
                    };

                    out_of_sync_items
                        .keys()
                        .any(|path| path.starts_with(&dir_prefix) || path == &full_path)
                        || local_changed_items
                            .iter()
                            .any(|path| path.starts_with(&dir_prefix) || path == &full_path)
                } else {
                    // For files: direct match in either remote or local
                    out_of_sync_items.contains_key(&full_path)
                        || local_changed_items.contains(&full_path)
                };

                if is_out_of_sync {
                    filtered.push(item.clone());
                }
            }

            // Update level with filtered items
            let level = &mut self.model.navigation.breadcrumb_trail[level_idx];

            if filtered.is_empty() {
                level.filtered_items = None;
            } else {
                level.filtered_items = Some(filtered);
            }

            // Restore selection to same item name if possible (only for current level)
            if level_idx == self.model.navigation.focus_level - 1 {
                if level.filtered_items.is_some() {
                    // Filter is active - try to restore previous selection
                    level.selected_index = if let Some(name) = selected_name {
                        logic::navigation::find_item_index_by_name(level.display_items(), &name)
                            .or(Some(0)) // If not found, default to first item
                    } else {
                        Some(0) // No previous selection, default to first item
                    };
                }
                // If no filter (filtered_items = None), keep existing selection
            }
        }
    }

    /// Toggle out-of-sync filter in breadcrumb view
    pub fn toggle_out_of_sync_filter(&mut self) {
        // Only works in breadcrumb view
        if self.model.navigation.focus_level == 0 {
            return;
        }

        // If filter is already active, clear it (regardless of what level we're on)
        if self.model.ui.out_of_sync_filter.is_some() {
            self.clear_out_of_sync_filter(true, None); // Preserve selection, no toast (user toggling off)
            return;
        }

        // Clear any stale filter from a different folder/level
        self.clear_out_of_sync_filter(false, None); // Don't preserve (stale context), no toast

        // Get current folder info
        let level_idx = self.model.navigation.focus_level - 1;
        let (folder_id, _prefix) = match self.model.navigation.breadcrumb_trail.get(level_idx) {
            Some(level) => (level.folder_id.clone(), level.prefix.clone()),
            None => return,
        };

        // Check folder status
        let has_out_of_sync = self
            .model
            .syncthing
            .folder_statuses
            .get(&folder_id)
            .map(|status| status.need_total_items > 0 || status.receive_only_total_items > 0)
            .unwrap_or(false);

        if !has_out_of_sync {
            // No out-of-sync items, show toast
            self.model.ui.show_toast("All files synced!".to_string());
            return;
        }

        // Check if we have cached out-of-sync data
        let has_cached_data = self
            .cache
            .get_out_of_sync_items(&folder_id)
            .map(|items| !items.is_empty())
            .unwrap_or(false);

        // Check if we have cached local changed data
        let has_local_cached = self
            .cache
            .get_local_changed_items(&folder_id)
            .map(|items| !items.is_empty())
            .unwrap_or(false);

        // Determine if this is a receive-only folder
        let is_receive_only = self
            .model
            .syncthing
            .folders
            .iter()
            .find(|f| f.id == folder_id)
            .map(|f| f.folder_type == "receiveonly")
            .unwrap_or(false);

        // Queue requests for missing data
        if !has_cached_data {
            // Queue GetNeededFiles request
            let _ = self.api_tx.send(services::api::ApiRequest::GetNeededFiles {
                folder_id: folder_id.clone(),
                page: None,
                perpage: Some(1000), // Get all items
            });
        }

        // Also check for local changes if folder is receive-only
        if is_receive_only && !has_local_cached {
            let _ = self
                .api_tx
                .send(services::api::ApiRequest::GetLocalChanged {
                    folder_id: folder_id.clone(),
                });
        }

        // If we're missing any required data, show loading toast and return
        if !has_cached_data || (is_receive_only && !has_local_cached) {
            // Show loading toast and return - filter will activate when data arrives
            self.model
                .ui
                .show_toast("Loading out-of-sync files...".to_string());
            return;
        }

        // Activate filter (only when data is already cached)
        self.model.ui.out_of_sync_filter = Some(model::types::OutOfSyncFilterState {
            origin_level: self.model.navigation.focus_level,
            last_refresh: std::time::SystemTime::now(),
        });

        // Apply current sort mode first
        self.sort_all_levels();

        // Then apply filter (filtered_items will reflect sorted order)
        self.apply_out_of_sync_filter();
    }

    /// Clear out-of-sync filter state and filtered items
    ///
    /// # Arguments
    /// * `preserve_selection` - Whether to keep cursor on same item by name
    /// * `show_toast` - Optional toast message to display
    pub(crate) fn clear_out_of_sync_filter(
        &mut self,
        preserve_selection: bool,
        show_toast: Option<&str>,
    ) {
        self.model.ui.out_of_sync_filter = None;

        if preserve_selection {
            // Clear filtered items while preserving selection by name
            for level in &mut self.model.navigation.breadcrumb_trail {
                let selected_name = level
                    .selected_index
                    .and_then(|idx| level.display_items().get(idx))
                    .map(|item| item.name.clone());

                level.filtered_items = None;

                if let Some(name) = selected_name {
                    level.selected_index =
                        logic::navigation::find_item_index_by_name(&level.items, &name);
                }
            }
        } else {
            // Simple clear without preserving selection
            for level in &mut self.model.navigation.breadcrumb_trail {
                level.filtered_items = None;
            }
        }

        if let Some(msg) = show_toast {
            self.model.ui.show_toast(msg.to_string());
        }
    }

    /// Activate out-of-sync filter (handles mutual exclusion with search)
    pub(crate) fn activate_out_of_sync_filter(&mut self) {
        // Only works in breadcrumb view
        if self.model.navigation.focus_level == 0 {
            return;
        }

        // Clear search if active (mutual exclusion)
        if !self.model.ui.search_query.is_empty() || self.model.ui.search_mode {
            self.clear_search(Some("Search cleared - filter active"));
        }

        // Toggle the filter
        self.toggle_out_of_sync_filter();
    }
}
