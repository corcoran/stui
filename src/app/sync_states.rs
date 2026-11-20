//! Sync state management and prefetching
//!
//! Methods for managing file/directory sync states:
//! - Updating directory states based on children
//! - Batch fetching sync states for visible items
//! - Prefetching subdirectory states
//! - Tracking ignored file existence

use crate::api::FolderStatus;
use crate::model::syncthing::{NeedFetch, SyncthingModel};
use crate::{App, SyncState, log_debug, logic, services};
use std::collections::HashMap;
use std::time::Instant;

/// Everything the need-index derivation needs to know about `folder_id`.
pub(crate) fn derive_context<'a>(
    syncthing: &'a SyncthingModel,
    folder_id: &str,
) -> logic::sync_states::DeriveContext<'a> {
    logic::sync_states::DeriveContext {
        index: syncthing.need_index.get(folder_id),
        status: syncthing.folder_statuses.get(folder_id),
        paused: syncthing.get_folder(folder_id).is_some_and(|f| f.paused),
    }
}

impl App {
    /// Queue a /rest/db/need fetch for `folder_id`. Responses to earlier requests are still
    /// accepted while newer than the index, unless `discard_pending` is set.
    pub(crate) fn request_folder_need(
        &mut self,
        folder_id: &str,
        sequence: u64,
        discard_pending: bool,
    ) {
        let at = Instant::now();
        let mut fetch = NeedFetch::new(at, sequence);
        if !discard_pending && let Some(prev) = self.model.syncthing.need_fetches.get(folder_id) {
            fetch.floor = prev.floor;
        }
        self.model
            .syncthing
            .need_fetches
            .insert(folder_id.to_string(), fetch);
        let _ = self.api_tx.send(services::api::ApiRequest::GetFolderNeed {
            folder_id: folder_id.to_string(),
            sequence,
            requested_at: at,
        });
    }

    /// After a folder status arrives: refetch the need index and local changes when they are
    /// stale, and drop the syncing set when nothing can be downloading. Folders with no open
    /// breadcrumb level are only fetched while syncing; `ensure_need_index` covers opening them.
    ///
    /// `prev_state` is the folder's state before this status. `status` must already be stored
    /// in `folder_statuses`, or re-derived states would use the old one.
    pub(crate) fn schedule_need_index(
        &mut self,
        folder_id: &str,
        prev_state: Option<&str>,
        status: &FolderStatus,
    ) {
        let syncthing = &mut self.model.syncthing;
        let paused =
            status.state == "paused" || syncthing.get_folder(folder_id).is_some_and(|f| f.paused);
        if paused {
            // Also invalidates any outstanding fetch, so a late response can't resurrect state
            syncthing.syncing_files.remove(folder_id);
            syncthing.need_fetches.remove(folder_id);
            return;
        }

        let folder_open = self
            .model
            .navigation
            .breadcrumb_trail
            .iter()
            .any(|level| level.folder_id == folder_id);
        let syncthing = &mut self.model.syncthing;
        let plan = logic::sync_states::need_fetch_plan(
            prev_state,
            &status.state,
            syncthing.need_fetches.get(folder_id).copied(),
            status.sequence,
            Instant::now(),
        );
        let wanted = logic::sync_states::need_index_wanted(folder_open, &status.state);
        let fetch = plan.fetch && wanted;
        if plan.clear_syncing {
            syncthing.syncing_files.remove(folder_id);
        }
        if plan.fetch && !wanted {
            // Stale and not worth fetching while closed: forget the outstanding request (late
            // responses are dropped and nothing looks pending) and the need list, so opening
            // the folder fetches afresh instead of deriving from a mid-sync list
            syncthing.need_fetches.remove(folder_id);
            if let Some(index) = syncthing.need_index.get_mut(folder_id) {
                logic::sync_states::clear_index_need(index);
            }
        } else if plan.discard_pending
            && !fetch
            && let Some(fetch) = syncthing.need_fetches.get_mut(folder_id)
        {
            fetch.floor = Instant::now();
        }

        let sequence = status.sequence;
        let mut local_changes_cleared = false;
        if status.receive_only_total_items == 0 {
            syncthing.local_changed_requested.remove(folder_id);
            let index = syncthing
                .need_index
                .entry(folder_id.to_string())
                .or_default();
            if index.local_changed_sequence != Some(sequence) || !index.local_changed.is_empty() {
                local_changes_cleared = !index.local_changed.is_empty();
                logic::sync_states::set_index_local_changed(index, &[], sequence);
            }
        } else if folder_open {
            self.request_local_changed_if_due(folder_id, sequence);
        }

        if fetch {
            self.request_folder_need(folder_id, sequence, plan.discard_pending);
        }
        if local_changes_cleared {
            self.refresh_derived_states(folder_id);
        }
    }

    /// Request the folder's receive-only local changes for `sequence` unless already requested
    /// or backing off after a failure.
    pub(crate) fn request_local_changed_if_due(&mut self, folder_id: &str, sequence: u64) {
        let syncthing = &mut self.model.syncthing;
        if !logic::sync_states::local_changed_fetch_due(
            syncthing.local_changed_requested.get(folder_id).copied(),
            syncthing.local_changed_failed_at.get(folder_id).copied(),
            sequence,
            Instant::now(),
        ) {
            return;
        }
        syncthing.local_changed_failed_at.remove(folder_id);
        syncthing
            .local_changed_requested
            .insert(folder_id.to_string(), sequence);
        let _ = self
            .api_tx
            .send(services::api::ApiRequest::GetLocalChangedIndex {
                folder_id: folder_id.to_string(),
                sequence,
            });
    }

    /// Periodic directory-state update from the main loop (throttled to every 2s). Covers
    /// every open level, so parents of a changed directory catch up even when not focused.
    pub(crate) fn tick_directory_states(&mut self) {
        if self.last_directory_update.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        self.last_directory_update = Instant::now();
        for idx in 0..self.model.navigation.breadcrumb_trail.len() {
            self.update_directory_states(idx);
        }
    }

    /// Drop memoized child states that a cache write or invalidation made stale: those of
    /// `path`'s ancestor directories, or all of the folder's when `path` is None.
    pub(crate) fn invalidate_dir_child_states(&mut self, folder_id: &str, path: Option<&str>) {
        self.dir_child_states.retain(|(folder, dir_prefix), _| {
            folder != folder_id || path.is_some_and(|p| !p.starts_with(dir_prefix.as_str()))
        });
    }

    /// Fetch the previewed folder's need index once the folder-list selection has rested.
    pub(crate) fn ensure_preview_need_index_if_idle(&mut self) {
        if !self.model.performance.is_idle() {
            return;
        }
        let Some(folder_id) = self.pending_preview_need.take() else {
            return;
        };
        let still_previewed = self
            .model
            .navigation
            .breadcrumb_trail
            .first()
            .is_some_and(|level| level.folder_id == folder_id);
        if still_previewed {
            self.ensure_need_index(&folder_id);
        }
    }

    /// Re-derive states on every open breadcrumb level of `folder_id` (preview included).
    pub(crate) fn refresh_derived_states(&mut self, folder_id: &str) {
        let level_indices: Vec<usize> = self
            .model
            .navigation
            .breadcrumb_trail
            .iter()
            .enumerate()
            .filter(|(_, level)| level.folder_id == folder_id)
            .map(|(idx, _)| idx)
            .collect();
        for idx in level_indices {
            let ctx = derive_context(&self.model.syncthing, folder_id);
            let level = &mut self.model.navigation.breadcrumb_trail[idx];
            logic::sync_states::apply_derived_states(
                &level.items,
                level.prefix.as_deref(),
                &mut level.file_sync_states,
                &level.direct_states,
                &ctx,
            );
            let filtered = level.filtered_items.is_some();
            self.update_directory_states(idx);
            if !filtered {
                self.sort_level(idx);
            }
        }
    }

    /// Queue FileInfo for the visible items of a just-opened level that have no state.
    ///
    /// Runs at navigation time so states load without waiting for the idle fetchers.
    pub(crate) fn request_level_file_info(
        &mut self,
        level_idx: usize,
        cap: usize,
        priority: services::api::Priority,
    ) {
        let Some(level) = self.model.navigation.breadcrumb_trail.get(level_idx) else {
            return;
        };
        let folder_id = level.folder_id.clone();
        let paths = logic::sync_states::file_info_paths_to_request(
            level.display_items(),
            &level.file_sync_states,
            level.prefix.as_deref(),
            level.selected_index,
            cap,
        );

        let mut queued = 0;
        for file_path in paths {
            let sync_key = format!("{}:{}", folder_id, file_path);
            if !self.model.performance.loading_sync_states.insert(sync_key) {
                continue;
            }
            let _ = self.api_tx.send(services::api::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path,
                priority,
            });
            queued += 1;
        }
        log_debug(&format!(
            "DEBUG [request_level_file_info]: level={} folder={} queued={}",
            level_idx, folder_id, queued
        ));
    }

    /// Set each directory's shown state on a level from its own state (`direct_states`): the
    /// derived state when the need index has one, else the aggregate of cached child states
    /// (visited directories), else its own state. Its own RemoteOnly or Ignored always wins.
    ///
    /// Child states are read from the cache at most once per `DIR_CHILD_STATES_TTL` per
    /// directory; the derived and own-state paths are always immediate.
    pub(crate) fn update_directory_states(&mut self, level_idx: usize) {
        let ttl = logic::sync_states::DIR_CHILD_STATES_TTL;
        self.dir_child_states
            .retain(|_, (read_at, _)| read_at.elapsed() < ttl);
        let Some(level) = self.model.navigation.breadcrumb_trail.get(level_idx) else {
            return;
        };
        let ctx = derive_context(&self.model.syncthing, &level.folder_id);
        let folder_sequence = ctx.status.map(|status| status.sequence).unwrap_or(0);
        let mut dir_states: HashMap<String, SyncState> = HashMap::new();

        for item in level
            .items
            .iter()
            .filter(|item| item.item_type == "FILE_INFO_TYPE_DIRECTORY")
        {
            let dir_name = &item.name;
            let shown = level.file_sync_states.get(dir_name).copied();
            let own = logic::sync_states::directory_own_state(
                level.direct_states.get(dir_name).copied(),
                shown,
            );

            let dir_path =
                logic::sync_states::breadcrumb_item_path(level.prefix.as_deref(), dir_name);
            let derived = logic::sync_states::derived_state(&dir_path, true, &ctx, own);
            if derived.is_some() || matches!(own, Some(SyncState::RemoteOnly | SyncState::Ignored))
            {
                if let Some(state) = logic::sync_states::resolve_directory_state(own, derived) {
                    dir_states.insert(dir_name.clone(), state);
                }
                continue;
            }

            let dir_prefix = format!("{}/", dir_path);
            let key = (level.folder_id.clone(), dir_prefix);
            let now = Instant::now();
            let child_states = match self.dir_child_states.get(&key) {
                Some((read_at, states))
                    if now.saturating_duration_since(*read_at)
                        < logic::sync_states::DIR_CHILD_STATES_TTL =>
                {
                    states.clone()
                }
                _ => {
                    let states = self.cached_child_states(&key.0, &key.1, folder_sequence);
                    self.dir_child_states.insert(key, (now, states.clone()));
                    states
                }
            };
            if let Some(child_states) = child_states {
                let aggregate_state =
                    logic::sync_states::aggregate_directory_state(own, &child_states);
                dir_states.insert(dir_name.clone(), aggregate_state);
                continue;
            }
            if let Some(own) = own {
                dir_states.insert(dir_name.clone(), own);
            }
        }

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

    /// Cached states of every child of `dir_prefix`, or None if the directory's listing isn't
    /// cached or any child has no state (an aggregate would then be a guess).
    fn cached_child_states(
        &self,
        folder_id: &str,
        dir_prefix: &str,
        folder_sequence: u64,
    ) -> Option<Vec<SyncState>> {
        let children = self
            .cache
            .get_browse_items(folder_id, Some(dir_prefix), folder_sequence)
            .ok()
            .flatten()?;
        children
            .iter()
            .map(|child| {
                let child_path = format!("{}{}", dir_prefix, child.name);
                self.cache
                    .get_sync_state_unvalidated(folder_id, &child_path)
                    .ok()
                    .flatten()
            })
            .collect()
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
