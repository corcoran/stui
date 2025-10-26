use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::{collections::{HashMap, HashSet}, fs, io, time::{Duration, Instant}};

mod api;
mod cache;
mod config;

use api::{BrowseItem, Folder, FolderStatus, SyncState, SyncthingClient};
use cache::CacheDb;
use config::Config;

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[derive(Clone)]
struct BreadcrumbLevel {
    folder_id: String,
    folder_label: String,
    folder_path: String,  // Cache the folder's container path
    prefix: Option<String>,
    items: Vec<BrowseItem>,
    state: ListState,
    translated_base_path: String,  // Cached translated base path for this level
    file_sync_states: HashMap<String, SyncState>,  // Cache sync states by filename
}

struct App {
    client: SyncthingClient,
    cache: CacheDb,
    folders: Vec<Folder>,
    folders_state: ListState,
    folder_statuses: HashMap<String, FolderStatus>,
    statuses_loaded: bool,
    last_status_update: Instant,
    path_map: HashMap<String, String>,
    breadcrumb_trail: Vec<BreadcrumbLevel>,
    focus_level: usize, // 0 = folders, 1+ = breadcrumb levels
    should_quit: bool,
    // Track in-flight operations to prevent duplicate fetches
    loading_browse: std::collections::HashSet<String>, // Set of "folder_id:prefix" currently being loaded
    loading_sync_states: std::collections::HashSet<String>, // Set of "folder_id:path" currently being loaded
    prefetch_enabled: bool, // Flag to enable/disable prefetching when system is busy
    last_known_sequences: HashMap<String, u64>, // Track last known sequence per folder to detect changes
    // Confirmation prompt state
    confirm_revert: Option<(String, Vec<String>)>, // If Some, shows confirmation prompt for reverting (folder_id, changed_files)
    confirm_delete: Option<(String, String, bool)>, // If Some, shows confirmation prompt for deleting (host_path, display_name, is_dir)
    // Pattern selection menu for removing ignores
    pattern_selection: Option<(String, Vec<String>, ListState)>, // If Some, shows pattern selection menu (folder_id, patterns, selection_state)
}

impl App {
    fn translate_path(&self, folder: &Folder, relative_path: &str) -> String {
        // Get the full container path
        let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), relative_path);

        // Try to map container path to host path using path_map
        for (container_prefix, host_prefix) in &self.path_map {
            if container_path.starts_with(container_prefix) {
                let remainder = container_path.strip_prefix(container_prefix).unwrap_or("");
                return format!("{}{}", host_prefix.trim_end_matches('/'), remainder);
            }
        }

        // If no mapping found, return container path
        container_path
    }

    fn pattern_matches(&self, pattern: &str, file_path: &str) -> bool {
        // Syncthing ignore patterns are similar to .gitignore
        // Patterns starting with / are relative to folder root
        // Patterns without / match anywhere in the path

        let pattern = pattern.trim();

        // Exact match
        if pattern == file_path {
            return true;
        }

        // Pattern starts with / - match from root
        if let Some(pattern_without_slash) = pattern.strip_prefix('/') {
            // Exact match without leading slash
            if pattern_without_slash == file_path.trim_start_matches('/') {
                return true;
            }

            // Try glob matching
            if let Ok(pattern_obj) = glob::Pattern::new(pattern_without_slash) {
                if pattern_obj.matches(file_path.trim_start_matches('/')) {
                    return true;
                }
            }
        } else {
            // Pattern without / - match anywhere
            // Try matching the full path
            if let Ok(pattern_obj) = glob::Pattern::new(pattern) {
                if pattern_obj.matches(file_path.trim_start_matches('/')) {
                    return true;
                }

                // Also try matching just the filename
                if let Some(filename) = file_path.split('/').last() {
                    if pattern_obj.matches(filename) {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn find_matching_patterns(&self, patterns: &[String], file_path: &str) -> Vec<String> {
        patterns
            .iter()
            .filter(|p| self.pattern_matches(p, file_path))
            .cloned()
            .collect()
    }

    async fn new(config: Config) -> Result<Self> {
        let client = SyncthingClient::new(config.base_url.clone(), config.api_key.clone());
        let cache = CacheDb::new()?;
        let folders = client.get_folders().await?;

        let mut app = App {
            client,
            cache,
            folders,
            folders_state: ListState::default(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            last_status_update: Instant::now(),
            path_map: config.path_map,
            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            should_quit: false,
            loading_browse: HashSet::new(),
            loading_sync_states: HashSet::new(),
            prefetch_enabled: true,
            last_known_sequences: HashMap::new(),
            confirm_revert: None,
            confirm_delete: None,
            pattern_selection: None,
        };

        // Load folder statuses first (needed for cache validation)
        app.load_folder_statuses().await;

        if !app.folders.is_empty() {
            app.folders_state.select(Some(0));
            app.load_root_level().await?;
        }

        Ok(app)
    }

    async fn load_folder_statuses(&mut self) {
        for folder in &self.folders {
            // Try cache first - use it without validation on initial load
            if !self.statuses_loaded {
                if let Ok(Some(cached_status)) = self.cache.get_folder_status(&folder.id) {
                    self.folder_statuses.insert(folder.id.clone(), cached_status);
                    continue;
                }
            }

            // Cache miss or this is a refresh - fetch from API
            if let Ok(status) = self.client.get_folder_status(&folder.id).await {
                let sequence = status.sequence;

                // Check if sequence changed from last known value
                if let Some(&last_seq) = self.last_known_sequences.get(&folder.id) {
                    if last_seq != sequence {
                        // Sequence changed! Invalidate cached data for this folder
                        let _ = self.cache.invalidate_folder(&folder.id);

                        // Don't clear in-memory sync states immediately - let them be refreshed naturally
                        // This prevents flickering when toggling ignore states
                        // The background prefetching will update states as needed
                    }
                }

                // Update last known sequence
                self.last_known_sequences.insert(folder.id.clone(), sequence);

                // Save fresh status and use it
                let _ = self.cache.save_folder_status(&folder.id, &status, sequence);
                self.folder_statuses.insert(folder.id.clone(), status);
            }
        }
        self.statuses_loaded = true;
        self.last_status_update = Instant::now();
    }

    async fn check_and_update_statuses(&mut self) {
        // Auto-refresh every 5 seconds
        if self.last_status_update.elapsed() >= Duration::from_secs(5) {
            self.load_folder_statuses().await;
        }
    }


    fn load_sync_states_from_cache(&self, folder_id: &str, items: &[BrowseItem], prefix: Option<&str>) -> HashMap<String, SyncState> {
        let mut sync_states = HashMap::new();

        for item in items {
            // Build the file path
            let file_path = if let Some(prefix) = prefix {
                format!("{}{}", prefix, item.name)
            } else {
                item.name.clone()
            };

            // Load from cache without validation (will be validated on next fetch if needed)
            if let Ok(Some(state)) = self.cache.get_sync_state_unvalidated(folder_id, &file_path) {
                sync_states.insert(item.name.clone(), state);
            }
        }

        sync_states
    }


    async fn batch_fetch_visible_sync_states(&mut self, max_concurrent: usize) {
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        // Get items that need fetching (don't have sync state and aren't loading)
        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();
        let prefix = self.breadcrumb_trail[level_idx].prefix.clone();

        let items_to_fetch: Vec<String> = self.breadcrumb_trail[level_idx]
            .items
            .iter()
            .filter(|item| {
                // Skip if already have sync state
                if self.breadcrumb_trail[level_idx].file_sync_states.contains_key(&item.name) {
                    return false;
                }

                // Build file path and check if already loading
                let file_path = if let Some(ref prefix) = prefix {
                    format!("{}{}", prefix, item.name)
                } else {
                    item.name.clone()
                };
                let sync_key = format!("{}:{}", folder_id, file_path);

                !self.loading_sync_states.contains(&sync_key)
            })
            .take(max_concurrent) // Limit how many we fetch at once
            .map(|item| item.name.clone())
            .collect();

        // Spawn concurrent fetches
        for item_name in items_to_fetch {
            let file_path = if let Some(ref prefix) = prefix {
                format!("{}{}", prefix, item_name)
            } else {
                item_name.clone()
            };

            let sync_key = format!("{}:{}", folder_id, file_path);

            // Mark as loading
            self.loading_sync_states.insert(sync_key.clone());

            // Fetch file info from API
            match self.client.get_file_info(&folder_id, &file_path).await {
                Ok(file_details) => {
                    let file_sequence = file_details.local.as_ref()
                        .or(file_details.global.as_ref())
                        .map(|f| f.sequence)
                        .unwrap_or(0);

                    // Always determine fresh state from API response
                    // Don't use cache here since we're specifically refreshing
                    let state = file_details.determine_sync_state();
                    let _ = self.cache.save_sync_state(&folder_id, &file_path, state, file_sequence);

                    // Update the sync state in the current level
                    if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                        level.file_sync_states.insert(item_name.clone(), state);
                    }
                }
                Err(_e) => {
                    // Silently ignore errors
                }
            }

            // Done loading
            self.loading_sync_states.remove(&sync_key);
        }
    }

    // Recursively discover and fetch states for subdirectories when hovering over a directory
    // This ensures we have complete subdirectory information for deep trees
    async fn prefetch_hovered_subdirectories(&mut self, max_depth: usize, max_dirs_per_frame: usize) {
        if !self.prefetch_enabled {
            return;
        }

        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        // Only run if system isn't too busy
        let total_in_flight = self.loading_browse.len() + self.loading_sync_states.len();
        if total_in_flight > 15 {
            return;
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        // Get the currently selected/hovered item
        let selected_idx = self.breadcrumb_trail[level_idx].state.selected();
        if selected_idx.is_none() {
            return;
        }

        let selected_item = if let Some(item) = self.breadcrumb_trail[level_idx].items.get(selected_idx.unwrap()) {
            item.clone()
        } else {
            return;
        };

        // Only process if it's a directory
        if selected_item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
            return;
        }

        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();
        let prefix = self.breadcrumb_trail[level_idx].prefix.clone();
        let folder_sequence = self.folder_statuses
            .get(&folder_id)
            .map(|s| s.sequence)
            .unwrap_or(0);

        // Build path to hovered directory
        let hovered_dir_path = if let Some(ref prefix) = prefix {
            format!("{}{}/", prefix, selected_item.name)
        } else {
            format!("{}/", selected_item.name)
        };

        // Recursively discover subdirectories
        let mut dirs_to_fetch = Vec::new();
        self.discover_subdirectories_recursive(
            &folder_id,
            &hovered_dir_path,
            folder_sequence,
            0,
            max_depth,
            &mut dirs_to_fetch,
        ).await;

        // Fetch states for discovered subdirectories (limit per frame)
        for dir_path in dirs_to_fetch.iter().take(max_dirs_per_frame) {
            let sync_key = format!("{}:{}", folder_id, dir_path);

            // Skip if already loading or cached
            if self.loading_sync_states.contains(&sync_key) {
                continue;
            }

            // Check if already cached
            if let Ok(Some(_)) = self.cache.get_sync_state_unvalidated(&folder_id, dir_path) {
                continue;
            }

            // Mark as loading
            self.loading_sync_states.insert(sync_key.clone());

            // Fetch directory's own sync state
            match self.client.get_file_info(&folder_id, dir_path).await {
                Ok(file_details) => {
                    let file_sequence = file_details.local.as_ref()
                        .or(file_details.global.as_ref())
                        .map(|f| f.sequence)
                        .unwrap_or(0);

                    let state = file_details.determine_sync_state();
                    let _ = self.cache.save_sync_state(&folder_id, dir_path, state, file_sequence);
                }
                Err(_) => {}
            }

            self.loading_sync_states.remove(&sync_key);
        }
    }

    // Helper to recursively discover subdirectories (browse only, no state fetching)
    async fn discover_subdirectories_recursive(
        &mut self,
        folder_id: &str,
        dir_path: &str,
        folder_sequence: u64,
        current_depth: usize,
        max_depth: usize,
        result: &mut Vec<String>,
    ) {
        if current_depth >= max_depth {
            return;
        }

        let browse_key = format!("{}:{}", folder_id, dir_path);

        // Check if already loading
        if self.loading_browse.contains(&browse_key) {
            return;
        }

        // Try to get from cache first
        let items = if let Ok(Some(cached_items)) = self.cache.get_browse_items(folder_id, Some(dir_path), folder_sequence) {
            cached_items
        } else {
            // Not cached - fetch it
            self.loading_browse.insert(browse_key.clone());

            let items = match self.client.browse_folder(folder_id, Some(dir_path)).await {
                Ok(items) => {
                    // Cache the browse results
                    let _ = self.cache.save_browse_items(folder_id, Some(dir_path), &items, folder_sequence);
                    items
                }
                Err(_) => {
                    self.loading_browse.remove(&browse_key);
                    return;
                }
            };

            self.loading_browse.remove(&browse_key);
            items
        };

        // Add all subdirectories to result list
        for item in &items {
            if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                let subdir_path = format!("{}{}", dir_path, item.name);
                result.push(subdir_path.clone());

                // Recursively discover deeper
                let nested_path = format!("{}/", subdir_path);
                Box::pin(self.discover_subdirectories_recursive(
                    folder_id,
                    &nested_path,
                    folder_sequence,
                    current_depth + 1,
                    max_depth,
                    result,
                )).await;
            }
        }
    }

    // Fetch directory-level sync states for subdirectories (their own metadata, not children)
    // This is cheap and gives immediate feedback for navigation (ignored/deleted/out-of-sync dirs)
    async fn fetch_directory_states(&mut self, max_concurrent: usize) {
        if !self.prefetch_enabled {
            return;
        }

        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        // Only run if system isn't too busy
        let total_in_flight = self.loading_browse.len() + self.loading_sync_states.len();
        if total_in_flight > 10 {
            return;
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();
        let prefix = self.breadcrumb_trail[level_idx].prefix.clone();

        // Find directories that don't have their own sync state cached
        let dirs_to_fetch: Vec<String> = self.breadcrumb_trail[level_idx]
            .items
            .iter()
            .filter(|item| {
                // Only process directories
                if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                    return false;
                }

                // Check if we already have this directory's state
                self.breadcrumb_trail[level_idx].file_sync_states.get(&item.name).is_none()
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
            if self.loading_sync_states.contains(&sync_key) {
                continue;
            }

            // Mark as loading
            self.loading_sync_states.insert(sync_key.clone());

            // Fetch directory's own sync state (just metadata, not children)
            match self.client.get_file_info(&folder_id, &dir_path).await {
                Ok(file_details) => {
                    let file_sequence = file_details.local.as_ref()
                        .or(file_details.global.as_ref())
                        .map(|f| f.sequence)
                        .unwrap_or(0);

                    // Determine the directory's own sync state
                    let state = file_details.determine_sync_state();

                    // Cache it
                    let _ = self.cache.save_sync_state(&folder_id, &dir_path, state, file_sequence);

                    // Update in-memory state
                    if level_idx < self.breadcrumb_trail.len() {
                        self.breadcrumb_trail[level_idx].file_sync_states.insert(dir_name.clone(), state);
                    }
                }
                Err(_) => {
                    // Silently ignore errors
                }
            }

            // Done loading
            self.loading_sync_states.remove(&sync_key);
        }
    }

    async fn fetch_selected_item_sync_state(&mut self) {
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        let level_idx = self.focus_level - 1;
        if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
            if let Some(selected_idx) = level.state.selected() {
                if let Some(item) = level.items.get(selected_idx) {
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
                    if self.loading_sync_states.contains(&sync_key) {
                        return;
                    }

                    // Mark as loading
                    self.loading_sync_states.insert(sync_key.clone());

                    // Try to get file sequence for cache validation
                    // We need to fetch file info first to get the sequence
                    match self.client.get_file_info(&level.folder_id, &file_path).await {
                        Ok(file_details) => {
                            let file_sequence = file_details.local.as_ref()
                                .or(file_details.global.as_ref())
                                .map(|f| f.sequence)
                                .unwrap_or(0);

                            // Check cache first
                            let sync_state = if let Ok(Some(cached_state)) = self.cache.get_sync_state(&level.folder_id, &file_path, file_sequence) {
                                cached_state
                            } else {
                                // Cache miss - determine state and save
                                let state = file_details.determine_sync_state();
                                let _ = self.cache.save_sync_state(&level.folder_id, &file_path, state, file_sequence);
                                state
                            };

                            level.file_sync_states.insert(item.name.clone(), sync_state);
                        }
                        Err(_e) => {
                            // Silently ignore errors
                        }
                    }

                    // Done loading
                    self.loading_sync_states.remove(&sync_key);
                }
            }
        }
    }

    async fn load_root_level(&mut self) -> Result<()> {
        if let Some(selected) = self.folders_state.selected() {
            if let Some(folder) = self.folders.get(selected) {
                // Don't try to browse paused folders
                if folder.paused {
                    // Stay on folder list, don't enter the folder
                    return Ok(());
                }

                // Get folder sequence for cache validation
                let folder_sequence = self.folder_statuses
                    .get(&folder.id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                // Create key for tracking in-flight operations
                let browse_key = format!("{}:", folder.id); // Empty prefix for root

                // Try cache first
                let items = if let Ok(Some(cached_items)) = self.cache.get_browse_items(&folder.id, None, folder_sequence) {
                    cached_items
                } else if self.loading_browse.contains(&browse_key) {
                    // Already loading this path, skip to avoid duplicate work
                    return Ok(());
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());

                    // Cache miss - fetch from API
                    let items = self.client.browse_folder(&folder.id, None).await?;
                    let _ = self.cache.save_browse_items(&folder.id, None, &items, folder_sequence);

                    // Done loading
                    self.loading_browse.remove(&browse_key);

                    items
                };

                let mut state = ListState::default();
                if !items.is_empty() {
                    state.select(Some(0));
                }

                // Compute translated base path once
                let translated_base_path = self.translate_path(folder, "");

                // Load cached sync states for items
                let file_sync_states = self.load_sync_states_from_cache(&folder.id, &items, None);

                self.breadcrumb_trail = vec![BreadcrumbLevel {
                    folder_id: folder.id.clone(),
                    folder_label: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
                    folder_path: folder.path.clone(),
                    prefix: None,
                    items,
                    state,
                    translated_base_path,
                    file_sync_states,
                }];
                self.focus_level = 1;
            }
        }
        Ok(())
    }

    async fn enter_directory(&mut self) -> Result<()> {
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let current_level = &self.breadcrumb_trail[level_idx];
        if let Some(selected_idx) = current_level.state.selected() {
            if let Some(item) = current_level.items.get(selected_idx) {
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
                let folder_sequence = self.folder_statuses
                    .get(&folder_id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                // Create key for tracking in-flight operations
                let browse_key = format!("{}:{}", folder_id, new_prefix);

                // Try cache first
                let items = if let Ok(Some(cached_items)) = self.cache.get_browse_items(&folder_id, Some(&new_prefix), folder_sequence) {
                    cached_items
                } else if self.loading_browse.contains(&browse_key) {
                    // Already loading this path, skip to avoid duplicate work
                    return Ok(());
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());

                    // Cache miss - fetch from API
                    let items = self.client.browse_folder(&folder_id, Some(&new_prefix)).await?;
                    let _ = self.cache.save_browse_items(&folder_id, Some(&new_prefix), &items, folder_sequence);

                    // Done loading
                    self.loading_browse.remove(&browse_key);

                    items
                };

                let mut state = ListState::default();
                if !items.is_empty() {
                    state.select(Some(0));
                }

                // Compute translated base path once for this level
                let full_relative_path = new_prefix.trim_end_matches('/');
                let container_path = format!("{}/{}", folder_path.trim_end_matches('/'), full_relative_path);

                // Map to host path
                let translated_base_path = self.path_map.iter()
                    .find_map(|(container_prefix, host_prefix)| {
                        container_path.strip_prefix(container_prefix.as_str())
                            .map(|remainder| format!("{}{}", host_prefix.trim_end_matches('/'), remainder))
                    })
                    .unwrap_or(container_path);

                // Truncate breadcrumb trail to current level + 1
                self.breadcrumb_trail.truncate(level_idx + 1);

                // Load cached sync states for items
                let file_sync_states = self.load_sync_states_from_cache(&folder_id, &items, Some(&new_prefix));

                // Add new level
                self.breadcrumb_trail.push(BreadcrumbLevel {
                    folder_id,
                    folder_label,
                    folder_path,
                    prefix: Some(new_prefix),
                    items,
                    state,
                    translated_base_path,
                    file_sync_states,
                });

                self.focus_level += 1;
            }
        }

        Ok(())
    }

    fn go_back(&mut self) {
        if self.focus_level > 1 {
            self.breadcrumb_trail.pop();
            self.focus_level -= 1;
        } else if self.focus_level == 1 {
            self.focus_level = 0;
        }
    }

    fn next_item(&mut self) {
        if self.focus_level == 0 {
            // Navigate folders
            let i = match self.folders_state.selected() {
                Some(i) => {
                    if i >= self.folders.len() - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.folders_state.select(Some(i));
        } else {
            // Navigate current breadcrumb level
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => {
                        if i >= level.items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    fn previous_item(&mut self) {
        if self.focus_level == 0 {
            // Navigate folders
            let i = match self.folders_state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.folders.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.folders_state.select(Some(i));
        } else {
            // Navigate current breadcrumb level
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => {
                        if i == 0 {
                            level.items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    async fn rescan_selected_folder(&mut self) -> Result<()> {
        // Get the folder ID to rescan
        let folder_id = if self.focus_level == 0 {
            // On folder list - rescan selected folder
            if let Some(selected) = self.folders_state.selected() {
                if let Some(folder) = self.folders.get(selected) {
                    folder.id.clone()
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        } else {
            // In a breadcrumb level - rescan the current folder
            if !self.breadcrumb_trail.is_empty() {
                self.breadcrumb_trail[0].folder_id.clone()
            } else {
                return Ok(());
            }
        };

        // Trigger rescan via API
        self.client.rescan_folder(&folder_id).await?;

        // Immediately refresh folder statuses to pick up the sequence change
        self.load_folder_statuses().await;

        // Refresh sync states for current level if we're in a breadcrumb
        if self.focus_level > 0 {
            self.refresh_current_level_states_background();
        }

        Ok(())
    }

    async fn restore_selected_file(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();

        // Check if this is a receive-only folder with local changes
        if let Some(status) = self.folder_statuses.get(&folder_id) {
            if status.receive_only_total_items > 0 {
                // Receive-only folder with local changes - fetch the list of changed files
                let changed_files = self.client.get_local_changed_files(&folder_id).await
                    .unwrap_or_else(|_| Vec::new());

                // Show confirmation prompt with file list
                self.confirm_revert = Some((folder_id, changed_files));
                return Ok(());
            }
        }

        // Not receive-only or no local changes - just rescan
        self.client.rescan_folder(&folder_id).await?;
        self.load_folder_statuses().await;

        Ok(())
    }

    async fn delete_ignored_file(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.breadcrumb_trail[level_idx];

        // Get selected item
        let selected = match level.state.selected() {
            Some(idx) => idx,
            None => return Ok(()),
        };

        if selected >= level.items.len() {
            return Ok(());
        }

        let item = &level.items[selected];

        // Check if this is an ignored file
        let sync_state = level.file_sync_states.get(&item.name).copied().unwrap_or(SyncState::Unknown);
        if sync_state != SyncState::Ignored {
            return Ok(()); // Only delete ignored files
        }

        // Build the full host path
        let relative_path = if let Some(ref prefix) = level.prefix {
            format!("{}/{}", prefix, item.name)
        } else {
            item.name.clone()
        };
        let host_path = format!("{}/{}", level.translated_base_path.trim_end_matches('/'), relative_path);

        // Check if file exists on disk
        if !std::path::Path::new(&host_path).exists() {
            return Ok(()); // Nothing to delete
        }

        // Check if it's a directory
        let is_dir = std::path::Path::new(&host_path).is_dir();

        // Show confirmation prompt
        self.confirm_delete = Some((host_path, item.name.clone(), is_dir));

        Ok(())
    }

    async fn toggle_ignore(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();

        // Get selected item
        let selected = match level.state.selected() {
            Some(idx) => idx,
            None => return Ok(()),
        };

        if selected >= level.items.len() {
            return Ok(());
        }

        let item = &level.items[selected];
        let item_name = item.name.clone(); // Clone for later use
        let sync_state = level.file_sync_states.get(&item.name).copied().unwrap_or(SyncState::Unknown);

        // Build the relative path from folder root
        let relative_path = if let Some(ref prefix) = level.prefix {
            format!("{}/{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        // Get current ignore patterns
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;

        if sync_state == SyncState::Ignored {
            // File is ignored - find matching patterns and remove them
            let file_path = format!("/{}", relative_path);
            let matching_patterns = self.find_matching_patterns(&patterns, &file_path);

            if matching_patterns.is_empty() {
                return Ok(()); // No patterns match (shouldn't happen)
            }

            if matching_patterns.len() == 1 {
                // Only one pattern - remove it directly
                let pattern_to_remove = &matching_patterns[0];
                let updated_patterns: Vec<String> = patterns
                    .into_iter()
                    .filter(|p| p != pattern_to_remove)
                    .collect();

                self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;
                self.client.rescan_folder(&folder_id).await?;
                self.load_folder_statuses().await;

                // Refresh only this file's state
                self.refresh_single_file_state_background(&item_name);
            } else {
                // Multiple patterns match - show selection menu
                let mut selection_state = ListState::default();
                selection_state.select(Some(0));
                self.pattern_selection = Some((folder_id, matching_patterns, selection_state));
            }
        } else {
            // File is not ignored - add it to ignore
            let new_pattern = format!("/{}", relative_path);

            // Check if pattern already exists
            if patterns.contains(&new_pattern) {
                return Ok(());
            }

            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern);

            self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;
            self.client.rescan_folder(&folder_id).await?;
            self.load_folder_statuses().await;

            // Refresh only this file's state
            self.refresh_single_file_state_background(&item_name);
        }

        Ok(())
    }

    fn refresh_single_file_state_background(&mut self, file_name: &str) {
        // Clear just the specific file's state to force a refresh
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        // Build the file path to clear from cache
        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();
        let file_path = if let Some(ref prefix) = self.breadcrumb_trail[level_idx].prefix {
            format!("{}{}", prefix, file_name)
        } else {
            file_name.to_string()
        };

        // Clear the cached state so background fetch gets fresh data from API
        // Note: We can't easily delete from cache DB, but we can clear in-memory state
        // The background fetch will get fresh data and update the cache

        // Clear only the specific file's state
        // The background prefetching will naturally update it
        if level_idx < self.breadcrumb_trail.len() {
            self.breadcrumb_trail[level_idx].file_sync_states.remove(file_name);
        }
    }

    fn refresh_current_level_states_background(&mut self) {
        // Clear all states in the level to force a refresh
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return;
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        // Clear current states to force a refresh
        // The background prefetching will naturally update them
        if level_idx < self.breadcrumb_trail.len() {
            self.breadcrumb_trail[level_idx].file_sync_states.clear();
        }
    }

    async fn ignore_and_delete(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();

        // Get selected item
        let selected = match level.state.selected() {
            Some(idx) => idx,
            None => return Ok(()),
        };

        if selected >= level.items.len() {
            return Ok(());
        }

        let item = &level.items[selected];
        let item_name = item.name.clone(); // Clone for later use

        // Build paths
        let relative_path = if let Some(ref prefix) = level.prefix {
            format!("{}/{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        let host_path = format!("{}/{}", level.translated_base_path.trim_end_matches('/'), relative_path);

        // Check if file exists on disk
        if !std::path::Path::new(&host_path).exists() {
            // File doesn't exist, just add to ignore
            let patterns = self.client.get_ignore_patterns(&folder_id).await?;
            let new_pattern = format!("/{}", relative_path);

            if !patterns.contains(&new_pattern) {
                let mut updated_patterns = patterns;
                updated_patterns.insert(0, new_pattern);

                self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;
                self.client.rescan_folder(&folder_id).await?;
                self.load_folder_statuses().await;

                // Refresh only this file's state
                self.refresh_single_file_state_background(&item_name);
            }
            return Ok(());
        }

        // File exists - add to ignore first, then delete
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;
        let new_pattern = format!("/{}", relative_path);

        if !patterns.contains(&new_pattern) {
            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern);
            self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;
        }

        // Now delete the file
        let is_dir = std::path::Path::new(&host_path).is_dir();
        let delete_result = if is_dir {
            std::fs::remove_dir_all(&host_path)
        } else {
            std::fs::remove_file(&host_path)
        };

        if delete_result.is_ok() {
            // Trigger rescan after successful deletion
            self.client.rescan_folder(&folder_id).await?;
            self.load_folder_statuses().await;

            // Refresh only this file's state
            self.refresh_single_file_state_background(&item_name);
        }

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Handle confirmation prompts first
        if let Some((folder_id, _)) = &self.confirm_revert {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - revert the folder
                    let folder_id = folder_id.clone();
                    self.confirm_revert = None;
                    let _ = self.client.revert_folder(&folder_id).await;
                    self.load_folder_statuses().await;
                    return Ok(());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled
                    self.confirm_revert = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while prompt is showing
                    return Ok(());
                }
            }
        }

        // Handle delete confirmation prompt
        if let Some((host_path, _name, is_dir)) = &self.confirm_delete {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - delete the file/directory
                    let host_path = host_path.clone();
                    let is_dir = *is_dir;
                    self.confirm_delete = None;

                    // Perform deletion
                    let delete_result = if is_dir {
                        std::fs::remove_dir_all(&host_path)
                    } else {
                        std::fs::remove_file(&host_path)
                    };

                    if delete_result.is_ok() {
                        // Trigger rescan after successful deletion
                        let _ = self.rescan_selected_folder().await;
                    }
                    // TODO: Show error message if deletion fails

                    return Ok(());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled
                    self.confirm_delete = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while prompt is showing
                    return Ok(());
                }
            }
        }

        // Handle pattern selection menu
        if let Some((folder_id, patterns, state)) = &mut self.pattern_selection {
            match key.code {
                KeyCode::Up => {
                    let selected = state.selected().unwrap_or(0);
                    if selected > 0 {
                        state.select(Some(selected - 1));
                    }
                    return Ok(());
                }
                KeyCode::Down => {
                    let selected = state.selected().unwrap_or(0);
                    if selected < patterns.len() - 1 {
                        state.select(Some(selected + 1));
                    }
                    return Ok(());
                }
                KeyCode::Enter => {
                    // Remove the selected pattern
                    let selected = state.selected().unwrap_or(0);
                    if selected < patterns.len() {
                        let pattern_to_remove = patterns[selected].clone();
                        let folder_id = folder_id.clone();
                        self.pattern_selection = None;

                        // Get all patterns and remove the selected one
                        let all_patterns = self.client.get_ignore_patterns(&folder_id).await?;
                        let updated_patterns: Vec<String> = all_patterns
                            .into_iter()
                            .filter(|p| p != &pattern_to_remove)
                            .collect();

                        self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;
                        self.client.rescan_folder(&folder_id).await?;
                        self.load_folder_statuses().await;

                        // Refresh sync states for current level only
                        self.refresh_current_level_states_background();
                    }
                    return Ok(());
                }
                KeyCode::Esc => {
                    // Cancel
                    self.pattern_selection = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while menu is showing
                    return Ok(());
                }
            }
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => {
                // Rescan the selected/current folder
                let _ = self.rescan_selected_folder().await;
            }
            KeyCode::Char('R') => {
                // Restore selected file (if remote-only/deleted locally)
                let _ = self.restore_selected_file().await;
            }
            KeyCode::Char('d') => {
                // Delete ignored file from disk (with confirmation)
                let _ = self.delete_ignored_file().await;
            }
            KeyCode::Char('i') => {
                // Toggle ignore state (add or remove from .stignore)
                let _ = self.toggle_ignore().await;
            }
            KeyCode::Char('I') => {
                // Ignore file AND delete from disk
                let _ = self.ignore_and_delete().await;
            }
            KeyCode::Left | KeyCode::Backspace => {
                self.go_back();
            }
            KeyCode::Right | KeyCode::Enter => {
                if self.focus_level == 0 {
                    self.load_root_level().await?;
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Up => {
                self.previous_item();
            }
            KeyCode::Down => {
                self.next_item();
            }
            _ => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config_str = fs::read_to_string("config.yaml")?;
    let config: Config = serde_yaml::from_str(&config_str)?;

    // Initialize app
    let mut app = App::new(config).await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app with error handler
    let result = run_app(&mut terminal, &mut app).await;

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Return result after cleanup
    result
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let size = f.size();

            // Create main layout: content area + status bar at bottom
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),      // Content area
                    Constraint::Length(3),   // Status bar (3 lines: top border, text, bottom border)
                ])
                .split(size);

            let content_area = main_chunks[0];
            let status_area = main_chunks[1];

            // Calculate how many panes we need (folders + breadcrumb levels)
            let num_panes = 1 + app.breadcrumb_trail.len();

            // Determine visible panes based on terminal width
            let min_pane_width = 20;
            let max_visible_panes = (content_area.width / min_pane_width).max(2) as usize;

            // Calculate which panes to show (prioritize right side)
            let start_pane = if num_panes > max_visible_panes {
                num_panes - max_visible_panes
            } else {
                0
            };

            let visible_panes = num_panes.min(max_visible_panes);

            // Create equal-width constraints for visible panes
            let constraints: Vec<Constraint> = (0..visible_panes)
                .map(|_| Constraint::Ratio(1, visible_panes as u32))
                .collect();

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(content_area);

            let mut chunk_idx = 0;

            // Render folders pane if visible
            if start_pane == 0 {
                let folders_items: Vec<ListItem> = app
                    .folders
                    .iter()
                    .map(|folder| {
                        let display_name = folder.label.as_ref().unwrap_or(&folder.id);

                        // Determine folder icon based on status
                        let icon = if !app.statuses_loaded {
                            "📁🔍 " // Loading
                        } else if folder.paused {
                            "📁⏸  " // Paused
                        } else if let Some(status) = app.folder_statuses.get(&folder.id) {
                            if status.state == "" || status.state == "paused" {
                                "📁⏸  " // Paused (empty state means paused)
                            } else if status.state == "syncing" {
                                "📁🔄 " // Syncing
                            } else if status.need_total_items > 0 || status.receive_only_total_items > 0 {
                                "📁⚠️ " // Out of sync or has local additions
                            } else if status.state == "idle" {
                                "📁✅ " // Synced
                            } else if status.state.starts_with("sync") {
                                "📁🔄 " // Any sync variant
                            } else if status.state == "scanning" {
                                "📁🔍 " // Scanning
                            } else {
                                "📁❓ " // Unknown state
                            }
                        } else {
                            "📁❌ " // Error fetching status
                        };

                        ListItem::new(Span::raw(format!("{}{}", icon, display_name)))
                    })
                    .collect();

                let is_focused = app.focus_level == 0;
                let folders_list = List::new(folders_items)
                    .block(
                        Block::default()
                            .title("Folders")
                            .borders(Borders::ALL)
                            .border_style(if is_focused {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Gray)
                            }),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("> ");

                f.render_stateful_widget(folders_list, chunks[chunk_idx], &mut app.folders_state);
                chunk_idx += 1;
            }

            // Render breadcrumb levels
            for (idx, level) in app.breadcrumb_trail.iter_mut().enumerate() {
                if idx + 1 < start_pane {
                    continue; // Skip panes that are off-screen to the left
                }

                let items: Vec<ListItem> = level
                    .items
                    .iter()
                    .map(|item| {
                        // Use cached state directly (directories show their own metadata state, not aggregate)
                        let sync_state = level.file_sync_states.get(&item.name).copied().unwrap_or(SyncState::Unknown);

                        let icon = match sync_state {
                            SyncState::Synced => {
                                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                                    "📁✅ "
                                } else {
                                    "📄✅ "
                                }
                            }
                            SyncState::OutOfSync => {
                                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                                    "📁⚠️ "
                                } else {
                                    "📄⚠️ "
                                }
                            }
                            SyncState::LocalOnly => {
                                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                                    "📁💻 "
                                } else {
                                    "📄💻 "
                                }
                            }
                            SyncState::RemoteOnly => {
                                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                                    "📁☁️ "
                                } else {
                                    "📄☁️ "
                                }
                            }
                            SyncState::Ignored => {
                                // Check if file exists on disk
                                let relative_path = if let Some(ref prefix) = level.prefix {
                                    format!("{}/{}", prefix, item.name)
                                } else {
                                    item.name.clone()
                                };
                                let host_path = format!("{}/{}", level.translated_base_path.trim_end_matches('/'), relative_path);

                                if std::path::Path::new(&host_path).exists() {
                                    "🚫⚠️ "  // Alarming - ignored file exists on disk
                                } else {
                                    "🚫.. "  // Normal - ignored file doesn't exist
                                }
                            }
                            SyncState::Unknown => {
                                if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                                    "📁🔄 "
                                } else {
                                    "📄🔄 "
                                }
                            }
                        };
                        ListItem::new(Span::raw(format!("{}{}", icon, item.name)))
                    })
                    .collect();

                let title = if let Some(ref prefix) = level.prefix {
                    // Show last part of path
                    let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();
                    parts.last().map(|s| s.to_string()).unwrap_or_else(|| level.folder_label.clone())
                } else {
                    level.folder_label.clone()
                };

                let is_focused = app.focus_level == idx + 1;
                let list = List::new(items)
                    .block(
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_style(if is_focused {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Gray)
                            }),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("> ");

                if chunk_idx < chunks.len() {
                    f.render_stateful_widget(list, chunks[chunk_idx], &mut level.state);
                    chunk_idx += 1;
                }
            }

            // Render status bar at the bottom with columns
            let status_line = if app.focus_level == 0 {
                // Show selected folder status
                if let Some(selected) = app.folders_state.selected() {
                    if let Some(folder) = app.folders.get(selected) {
                        let folder_name = folder.label.as_ref().unwrap_or(&folder.id);
                        if folder.paused {
                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                "Paused",
                                "-",
                                "-",
                                "-"
                            )
                        } else if let Some(status) = app.folder_statuses.get(&folder.id) {
                            let state_display = if status.state.is_empty() { "paused" } else { &status.state };
                            let in_sync = status.global_total_items.saturating_sub(status.need_total_items);
                            let items_display = format!("{}/{}", in_sync, status.global_total_items);

                            // Build status message considering both remote needs and local additions
                            let need_display = if status.receive_only_total_items > 0 {
                                // Has local additions
                                if status.need_total_items > 0 {
                                    // Both local additions and remote needs
                                    format!("↓{} ↑{} ({})",
                                        status.need_total_items,
                                        status.receive_only_total_items,
                                        format_bytes(status.need_bytes + status.receive_only_changed_bytes)
                                    )
                                } else {
                                    // Only local additions
                                    format!("Local: {} items ({})",
                                        status.receive_only_total_items,
                                        format_bytes(status.receive_only_changed_bytes)
                                    )
                                }
                            } else if status.need_total_items > 0 {
                                // Only remote needs
                                format!("{} items ({}) ", status.need_total_items, format_bytes(status.need_bytes))
                            } else {
                                "Up to date ".to_string()
                            };

                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                state_display,
                                format_bytes(status.global_bytes),
                                items_display,
                                need_display
                            )
                        } else {
                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                "Loading...",
                                "-",
                                "-",
                                "-"
                            )
                        }
                    } else {
                        "No folder selected".to_string()
                    }
                } else {
                    "No folder selected".to_string()
                }
            } else {
                // Show current item in breadcrumb trail
                let level_idx = app.focus_level - 1;
                if let Some(level) = app.breadcrumb_trail.get(level_idx) {
                    if let Some(selected) = level.state.selected() {
                        if let Some(item) = level.items.get(selected) {
                            let item_type = match item.item_type.as_str() {
                                "FILE_INFO_TYPE_DIRECTORY" => "Directory",
                                "FILE_INFO_TYPE_FILE" => "File",
                                _ => "Item",
                            };

                            // Use cached translated base path and append item name
                            let full_path = format!("{}/{}",
                                level.translated_base_path.trim_end_matches('/'),
                                item.name
                            );

                            format!("{}: {}  |  Path: {}",
                                item_type,
                                item.name,
                                full_path
                            )
                        } else {
                            "No item selected".to_string()
                        }
                    } else {
                        "No item selected".to_string()
                    }
                } else {
                    "".to_string()
                }
            };

            let status_bar = Paragraph::new(Line::from(Span::raw(status_line)))
                .block(Block::default().borders(Borders::ALL).title("Status"))
                .style(Style::default().fg(Color::White));

            f.render_widget(status_bar, status_area);

            // Render confirmation prompt if active
            if let Some((_folder_id, changed_files)) = &app.confirm_revert {
                let file_list = changed_files.iter()
                    .take(5)
                    .map(|f| format!("  - {}", f))
                    .collect::<Vec<_>>()
                    .join("\n");

                let more_text = if changed_files.len() > 5 {
                    format!("\n  ... and {} more", changed_files.len() - 5)
                } else {
                    String::new()
                };

                let prompt_text = format!(
                    "Revert folder to restore deleted files?\n\n\
                    WARNING: This will remove {} local change(s):\n{}{}\n\n\
                    Continue? (y/n)",
                    changed_files.len(),
                    file_list,
                    more_text
                );

                // Center the prompt - adjust height based on number of files shown
                let area = f.size();
                let prompt_width = 60;
                let base_height = 10;
                let file_lines = changed_files.len().min(5);
                let prompt_height = base_height + file_lines as u16;
                let prompt_area = ratatui::layout::Rect {
                    x: (area.width.saturating_sub(prompt_width)) / 2,
                    y: (area.height.saturating_sub(prompt_height)) / 2,
                    width: prompt_width,
                    height: prompt_height,
                };

                let prompt = Paragraph::new(prompt_text)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm Revert")
                        .border_style(Style::default().fg(Color::Yellow)))
                    .style(Style::default().fg(Color::White).bg(Color::Black))
                    .wrap(ratatui::widgets::Wrap { trim: false });

                f.render_widget(ratatui::widgets::Clear, prompt_area);
                f.render_widget(prompt, prompt_area);
            }

            // Render delete confirmation prompt if active
            if let Some((_host_path, display_name, is_dir)) = &app.confirm_delete {
                let item_type = if *is_dir { "directory" } else { "file" };

                let prompt_text = format!(
                    "Delete {} from disk?\n\n\
                    {}: {}\n\n\
                    WARNING: This action cannot be undone!\n\n\
                    Continue? (y/n)",
                    item_type,
                    if *is_dir { "Directory" } else { "File" },
                    display_name
                );

                // Center the prompt
                let area = f.size();
                let prompt_width = 50;
                let prompt_height = 11;
                let prompt_area = ratatui::layout::Rect {
                    x: (area.width.saturating_sub(prompt_width)) / 2,
                    y: (area.height.saturating_sub(prompt_height)) / 2,
                    width: prompt_width,
                    height: prompt_height,
                };

                let prompt = Paragraph::new(prompt_text)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm Delete")
                        .border_style(Style::default().fg(Color::Red)))
                    .style(Style::default().fg(Color::White).bg(Color::Black))
                    .wrap(ratatui::widgets::Wrap { trim: false });

                f.render_widget(ratatui::widgets::Clear, prompt_area);
                f.render_widget(prompt, prompt_area);
            }

            // Render pattern selection menu if active
            if let Some((_folder_id, patterns, state)) = &mut app.pattern_selection {
                let menu_items: Vec<ListItem> = patterns
                    .iter()
                    .map(|pattern| {
                        ListItem::new(Span::raw(pattern.clone()))
                            .style(Style::default().fg(Color::White))
                    })
                    .collect();

                // Center the menu
                let area = f.size();
                let menu_width = 60;
                let menu_height = (patterns.len() as u16 + 6).min(20); // +6 for borders and instructions
                let menu_area = ratatui::layout::Rect {
                    x: (area.width.saturating_sub(menu_width)) / 2,
                    y: (area.height.saturating_sub(menu_height)) / 2,
                    width: menu_width,
                    height: menu_height,
                };

                let menu = List::new(menu_items)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .title("Select Pattern to Remove (↑↓ to navigate, Enter to remove, Esc to cancel)")
                        .border_style(Style::default().fg(Color::Yellow)))
                    .highlight_style(Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD))
                    .highlight_symbol("► ");

                f.render_widget(ratatui::widgets::Clear, menu_area);
                f.render_stateful_widget(menu, menu_area, state);
            }
        })?;

        if app.should_quit {
            break;
        }

        // Check for periodic status updates
        app.check_and_update_statuses().await;

        // HIGHEST PRIORITY: If hovering over a directory, recursively discover all subdirectories
        // and fetch their states (goes as deep as possible, depth=10, fetch 15 dirs per frame)
        app.prefetch_hovered_subdirectories(10, 15).await;

        // Fetch directory metadata states for visible directories in current level
        app.fetch_directory_states(10).await;

        // Fetch selected item specifically (high priority for user interaction)
        app.fetch_selected_item_sync_state().await;

        // LOWEST PRIORITY: Batch fetch file sync states for visible files
        app.batch_fetch_visible_sync_states(5).await;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await?;
            }
        }
    }

    Ok(())
}
