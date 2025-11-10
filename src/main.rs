use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// Syncthing TUI Manager
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Enable debug logging to /tmp/synctui-debug.log
    #[arg(short, long)]
    debug: bool,

    /// Enable vim keybindings (hjkl, ^D/U, ^F/B, gg/G)
    #[arg(long)]
    vim: bool,

    /// Path to config file (default: platform-specific, see docs)
    #[arg(short, long)]
    config: Option<String>,
}

// Global flag for debug mode
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

mod api;
mod app;
mod cache;
mod config;
mod handlers;
mod logic;
mod model;
mod services;
mod ui;
mod utils;

use api::{
    BrowseItem, Folder, SyncState,
    SyncthingClient,
};
use cache::CacheDb;
use config::Config;
use synctui::{DisplayMode, SortMode};
use ui::icons::{IconMode, IconRenderer, IconTheme};

fn log_debug(msg: &str) {
    // Only log if debug mode is enabled
    if !DEBUG_MODE.load(Ordering::Relaxed) {
        return;
    }

    use std::fs::OpenOptions;
    use std::io::Write;
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/synctui-debug.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

#[derive(Clone, Debug)]
pub struct ImageMetadata {
    pub dimensions: Option<(u32, u32)>,
    pub format: Option<String>,
    pub file_size: u64,
}

pub enum ImagePreviewState {
    Loading,
    Ready {
        protocol: ratatui_image::protocol::StatefulProtocol,
        metadata: ImageMetadata,
    },
    Failed {
        metadata: ImageMetadata,
    },
}

impl std::fmt::Debug for ImagePreviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImagePreviewState::Loading => write!(f, "ImagePreviewState::Loading"),
            ImagePreviewState::Ready { metadata, .. } => {
                f.debug_struct("ImagePreviewState::Ready")
                    .field("metadata", metadata)
                    .field("protocol", &"<StatefulProtocol>")
                    .finish()
            }
            ImagePreviewState::Failed { metadata } => {
                f.debug_struct("ImagePreviewState::Failed")
                    .field("metadata", metadata)
                    .finish()
            }
        }
    }
}

pub struct App {
    pub model: model::Model,

    client: SyncthingClient,
    cache: CacheDb,
    api_tx: tokio::sync::mpsc::UnboundedSender<services::api::ApiRequest>,
    api_rx: tokio::sync::mpsc::UnboundedReceiver<services::api::ApiResponse>,
    invalidation_rx: tokio::sync::mpsc::UnboundedReceiver<services::events::CacheInvalidation>,
    event_id_rx: tokio::sync::mpsc::UnboundedReceiver<u64>,
    icon_renderer: IconRenderer,
    image_picker: Option<ratatui_image::picker::Picker>,
    image_update_tx: tokio::sync::mpsc::UnboundedSender<(String, ImagePreviewState)>,
    image_update_rx: tokio::sync::mpsc::UnboundedReceiver<(String, ImagePreviewState)>,

    path_map: HashMap<String, String>,
    open_command: Option<String>,
    clipboard_command: Option<String>,
    base_url: String,

    last_status_update: Instant,
    last_system_status_update: Instant,
    last_connection_stats_fetch: Instant,
    last_directory_update: Instant,
    last_db_flush: Instant,
    last_reconnect_attempt: Instant,
    reconnect_delay: Duration,

    pending_sync_state_writes: Vec<(String, String, SyncState, u64)>,

    /// Maps file paths to their image preview states
    image_state_map: std::collections::HashMap<String, ImagePreviewState>,
}

impl App {

    /// Check if a path or any of its parent directories are pending deletion
    /// Returns Some(pending_path) if blocked, None if allowed
    fn is_path_or_parent_pending(&self, folder_id: &str, path: &PathBuf) -> Option<PathBuf> {
        if let Some(pending_info) = self.model.performance.pending_ignore_deletes.get(folder_id) {
            logic::path::is_path_or_parent_in_set(&pending_info.paths, path)
        } else {
            None
        }
    }

    /// Add a path to pending deletions for a folder
    fn add_pending_delete(&mut self, folder_id: String, path: PathBuf) {
        let pending_info = self.model.performance.pending_ignore_deletes
            .entry(folder_id)
            .or_insert_with(|| model::PendingDeleteInfo {
                paths: HashSet::new(),
                initiated_at: Instant::now(),
                rescan_triggered: false,
            });

        pending_info.paths.insert(path);
        log_debug(&format!("Added pending delete: {:?}", pending_info.paths));
    }

    /// Remove a path from pending deletions after verification
    fn remove_pending_delete(&mut self, folder_id: &str, path: &PathBuf) {
        if let Some(pending_info) = self.model.performance.pending_ignore_deletes.get_mut(folder_id) {
            pending_info.paths.remove(path);
            log_debug(&format!("Removed pending delete: {:?}, remaining: {:?}", path, pending_info.paths));

            // Clean up empty folder entry
            if pending_info.paths.is_empty() {
                self.model.performance.pending_ignore_deletes.remove(folder_id);
                log_debug(&format!("Cleared pending deletes for folder: {}", folder_id));
            }
        }
    }

    /// Clean up stale pending deletes (older than 60 seconds)
    fn cleanup_stale_pending_deletes(&mut self) {
        let now = Instant::now();

        self.model.performance.pending_ignore_deletes.retain(|folder_id, info| {
            if logic::performance::should_cleanup_stale_pending(info.initiated_at, now) {
                log_debug(&format!("Cleaning up stale pending deletes for folder: {}", folder_id));
                false // Remove this entry
            } else {
                true // Keep this entry
            }
        });
    }

    /// Verify and cleanup completed pending deletes
    /// Removes paths that:
    /// 1. Have rescan_triggered = true
    /// 2. Are older than 5 seconds (buffer for Syncthing to process)
    /// 3. Files are verified gone from disk
    fn verify_and_cleanup_pending_deletes(&mut self) {
        let now = Instant::now();

        for (_folder_id, info) in self.model.performance.pending_ignore_deletes.iter_mut() {
            // Only check if ready for verification
            if !logic::performance::should_verify_pending(
                info.initiated_at,
                now,
                info.rescan_triggered,
            ) {
                continue;
            }

            // Check each path and remove if verified gone
            info.paths.retain(|path| {
                if path.exists() {
                    log_debug(&format!("Pending delete still exists: {:?}", path));
                    true // Keep in pending set
                } else {
                    log_debug(&format!("Pending delete verified gone: {:?}", path));
                    false // Remove from pending set
                }
            });
        }

        // Clean up folders with no pending paths
        self.model.performance.pending_ignore_deletes.retain(|folder_id, info| {
            if info.paths.is_empty() {
                log_debug(&format!("All pending deletes completed for folder: {}", folder_id));
                false
            } else {
                true
            }
        });
    }

    pub fn get_local_state_summary(&self) -> (u64, u64, u64) {
        logic::folder::calculate_local_state_summary(&self.model.syncthing.folder_statuses)
    }

    /// Flush pending database writes in a single transaction
    fn flush_pending_db_writes(&mut self) {
        if self.pending_sync_state_writes.is_empty() {
            return;
        }

        const ABSOLUTE_MAX_BATCH: usize = 100;

        if self.pending_sync_state_writes.len() > ABSOLUTE_MAX_BATCH {
            log_debug(&format!(
                "Warning: Large batch size {}, flushing in chunks",
                self.pending_sync_state_writes.len()
            ));

            // Flush in chunks if extremely large
            for chunk in self.pending_sync_state_writes.chunks(ABSOLUTE_MAX_BATCH) {
                if let Err(e) = self.cache.save_sync_states_batch(chunk) {
                    log_debug(&format!("Failed to flush sync state chunk: {}", e));
                }
            }
            self.pending_sync_state_writes.clear();
        } else {
            // Normal flush path
            if let Err(e) = self.cache.save_sync_states_batch(&self.pending_sync_state_writes) {
                log_debug(&format!("Failed to flush sync state batch: {}", e));
            }
            self.pending_sync_state_writes.clear();
        }

        self.last_db_flush = Instant::now();
    }

    /// Check if we should flush pending writes based on batch size or time
    fn should_flush_db(&self) -> bool {
        logic::performance::should_flush_batch(
            self.pending_sync_state_writes.len(),
            self.last_db_flush.elapsed(),
        )
    }


    async fn new(config: Config, config_path: String) -> Result<Self> {
        let client = SyncthingClient::new(config.base_url.clone(), config.api_key.clone());
        let cache = CacheDb::new()?;

        // Try to fetch folders from API, fall back to cache on error
        let (folders, initial_connection_state) = match client.get_folders().await {
            Ok(folders) => {
                let _ = cache.save_folders(&folders);
                (folders, model::syncthing::ConnectionState::Connected)
            }
            Err(e) => {
                log_debug(&format!("Failed to fetch folders from API: {}", e));
                let cached_folders = cache.get_all_folders().unwrap_or_else(|cache_err| {
                    log_debug(&format!("Failed to load folders from cache: {}", cache_err));
                    vec![]
                });

                if cached_folders.is_empty() {
                    (
                        vec![],
                        model::syncthing::ConnectionState::Disconnected {
                            error_type: logic::errors::classify_error(&e),
                            message: logic::errors::format_error_message(&e),
                        },
                    )
                } else {
                    log_debug(&format!("Using {} cached folders, will auto-retry", cached_folders.len()));
                    (
                        cached_folders,
                        model::syncthing::ConnectionState::Connecting {
                            attempt: 1,
                            last_error: Some(e.to_string()),
                            next_retry_secs: 5,
                        },
                    )
                }
            }
        };

        let devices = client.get_devices().await.unwrap_or_default();

        // Spawn API service worker
        let (api_tx, api_rx) = services::api::spawn_api_service(client.clone());

        // Get last event ID from cache
        let last_event_id = cache.get_last_event_id().unwrap_or(0);

        // Create channels for event listener
        let (invalidation_tx, invalidation_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_id_tx, event_id_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create channel for image updates
        let (image_update_tx, image_update_rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn event listener
        services::events::spawn_event_listener(
            config.base_url.clone(),
            config.api_key.clone(),
            last_event_id,
            invalidation_tx,
            event_id_tx,
        );

        // Parse icon mode from config
        let icon_mode = match config.icon_mode.to_lowercase().as_str() {
            "emoji" => IconMode::Emoji,
            "nerdfont" | "nerd" | "nf" => IconMode::NerdFont,
            _ => IconMode::NerdFont, // Default to nerd font
        };
        let icon_renderer = IconRenderer::new(icon_mode, IconTheme::default());

        // Initialize image preview protocol picker
        let (image_picker, image_font_size) = if config.image_preview_enabled {
            // Get picker with terminal dimensions
            let mut picker = match ratatui_image::picker::Picker::from_query_stdio() {
                Ok(p) => p,
                Err(e) => {
                    log_debug(&format!("Image preview: Failed to detect terminal: {}", e));
                    ratatui_image::picker::Picker::from_fontsize((8, 16)) // Fallback font size
                }
            };

            // Store font size for centering calculations
            let font_size = picker.font_size();
            log_debug(&format!("Image font size: {}x{}", font_size.0, font_size.1));

            // Apply protocol from config
            match config.image_protocol.to_lowercase().as_str() {
                "auto" => {
                    // Protocol already auto-detected by from_query_stdio()
                    log_debug("Image preview: Auto-detected protocol");
                }
                "iterm2" => {
                    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
                    log_debug("Image preview: Using iTerm2 protocol");
                }
                "kitty" => {
                    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
                    log_debug("Image preview: Using Kitty protocol");
                }
                "sixel" => {
                    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Sixel);
                    log_debug("Image preview: Using Sixel protocol");
                }
                "halfblocks" => {
                    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Halfblocks);
                    log_debug("Image preview: Using Halfblocks protocol");
                }
                unknown => {
                    // Protocol already auto-detected, just log the warning
                    log_debug(&format!(
                        "Image preview: Unknown protocol '{}', using auto-detect",
                        unknown
                    ));
                }
            }

            (Some(picker), Some(font_size))
        } else {
            log_debug("Image preview disabled in config");
            (None, None)
        };

        // Initialize pure Model with appropriate defaults
        let mut model = model::Model::new(config.vim_mode);
        model.ui.display_mode = DisplayMode::TimestampAndSize; // Start with most info
        model.ui.sort_mode = SortMode::Alphabetical;
        model.syncthing.folders = folders.clone();
        model.syncthing.devices = devices;
        model.syncthing.connection_state = initial_connection_state.clone();
        model.ui.config_path = config_path;
        model.ui.image_font_size = image_font_size;

        // Show setup help if no folders and disconnected
        if folders.is_empty() && matches!(initial_connection_state, model::syncthing::ConnectionState::Disconnected { .. }) {
            model.ui.show_setup_help = true;
        }

        let mut app = App {
            model,
            client,
            cache,
            api_tx,
            api_rx,
            invalidation_rx,
            event_id_rx,
            icon_renderer,
            image_picker,
            image_update_tx,
            image_update_rx,
            path_map: config.path_map,
            open_command: config.open_command,
            clipboard_command: config.clipboard_command,
            base_url: config.base_url,
            last_status_update: Instant::now(),
            last_system_status_update: Instant::now(),
            last_connection_stats_fetch: Instant::now(),
            last_directory_update: Instant::now(),
            last_db_flush: Instant::now(),
            last_reconnect_attempt: Instant::now(),
            reconnect_delay: Duration::from_secs(5), // Start with 5s
            pending_sync_state_writes: Vec::new(),
            image_state_map: HashMap::new(),
        };

        // Load folder statuses first (needed for cache validation)
        app.load_folder_statuses().await;

        // Load cached device name (if available) to avoid "Unknown" flash
        if let Ok(Some(cached_name)) = app.cache.get_device_name() {
            app.model.syncthing.device_name = Some(cached_name);
        }

        // Initialize system status and connection stats
        if let Ok(device_name) = app.client.get_device_name().await {
            app.model.syncthing.device_name = Some(device_name.clone());
            // Cache device name for next startup
            let _ = app.cache.save_device_name(&device_name);
        }

        if let Ok(sys_status) = app.client.get_system_status().await {
            app.model.syncthing.system_status = Some(sys_status);
        }

        if let Ok(conn_stats) = app.client.get_connection_stats().await {
            app.model.syncthing.last_connection_stats = Some((conn_stats.clone(), Instant::now()));
            app.model.syncthing.connection_stats = Some(conn_stats);
        }

        if !app.model.syncthing.folders.is_empty() {
            app.model.navigation.folders_state_selection = Some(0);
            // Try to load root level, but don't fail initialization if it errors (e.g., Syncthing down)
            let _ = app.load_root_level(true).await; // Preview mode - focus stays on folders
        }

        Ok(app)
    }

    async fn load_folder_statuses(&mut self) {
        for folder in &self.model.syncthing.folders {
            // Try cache first - use it without validation on initial load
            if !self.model.syncthing.statuses_loaded {
                if let Ok(Some(cached_status)) = self.cache.get_folder_status(&folder.id) {
                    self.model.syncthing.folder_statuses
                        .insert(folder.id.clone(), cached_status);
                    continue;
                }
            }

            // Cache miss or this is a refresh - fetch from API
            if let Ok(status) = self.client.get_folder_status(&folder.id).await {
                let sequence = status.sequence;

                // Check if sequence changed from last known value
                if let Some(&last_seq) = self.model.performance.last_known_sequences.get(&folder.id) {
                    if last_seq != sequence {
                        // Sequence changed! Invalidate cached data for this folder
                        let _ = self.cache.invalidate_folder(&folder.id);

                        // Clear in-memory sync states for this folder if we're currently viewing it
                        // This ensures files that changed get refreshed
                        if !self.model.navigation.breadcrumb_trail.is_empty()
                            && self.model.navigation.breadcrumb_trail[0].folder_id == folder.id
                        {
                            for level in &mut self.model.navigation.breadcrumb_trail {
                                if level.folder_id == folder.id {
                                    level.file_sync_states.clear();
                                }
                            }
                        }
                    }
                }

                // Update last known sequence
                self.model.performance.last_known_sequences
                    .insert(folder.id.clone(), sequence);

                // Save fresh status and use it
                let _ = self.cache.save_folder_status(&folder.id, &status, sequence);
                self.model.syncthing.folder_statuses.insert(folder.id.clone(), status);
            }
        }
        self.model.syncthing.statuses_loaded = true;
        self.last_status_update = Instant::now();
    }

    fn refresh_folder_statuses_nonblocking(&mut self) {
        // Non-blocking version for background polling
        // Sends status requests via API service
        if self.model.syncthing.folders.is_empty() {
            // No folders - send SystemStatus as a connection probe
            let _ = self.api_tx.send(services::api::ApiRequest::GetSystemStatus);
        } else {
            for folder in &self.model.syncthing.folders {
                let _ = self.api_tx.send(services::api::ApiRequest::GetFolderStatus {
                    folder_id: folder.id.clone(),
                });
            }
        }
    }

    /// Handle API responses from background worker
    /// Delegated to handlers::api module
    fn handle_api_response(&mut self, response: services::api::ApiResponse) {
        handlers::handle_api_response(self, response);
    }

    /// Handle cache invalidation messages from event listener
    /// Delegated to handlers::events module
    fn handle_cache_invalidation(&mut self, invalidation: services::events::CacheInvalidation) {
        handlers::handle_cache_invalidation(self, invalidation);
    }

    /// Merge local-only files from receive-only folders into browse items
    /// Returns the names of merged local items so we can mark their sync state
    async fn merge_local_only_files(
        &self,
        folder_id: &str,
        items: &mut Vec<BrowseItem>,
        prefix: Option<&str>,
    ) -> Vec<String> {
        let mut local_item_names = Vec::new();

        // Check if folder has local changes
        let has_local_changes = logic::folder::has_local_changes(
            self.model.syncthing.folder_statuses.get(folder_id)
        );

        if !has_local_changes {
            return local_item_names;
        }

        // Fetch local-only items for this directory
        if let Ok(local_items) = self.client.get_local_changed_items(folder_id, prefix).await {
            // Add local-only items that aren't already in the browse results
            for local_item in local_items {
                if !items.iter().any(|i| i.name == local_item.name) {
                    local_item_names.push(local_item.name.clone());
                    items.push(local_item);
                }
            }
        }

        local_item_names
    }

    fn load_sync_states_from_cache(
        &self,
        folder_id: &str,
        items: &[BrowseItem],
        prefix: Option<&str>,
    ) -> HashMap<String, SyncState> {
        let mut sync_states = HashMap::new();

        for item in items {
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

    /// Recursively prefetch all subdirectories for search
    ///
    /// Queues browse requests for all subdirectories found in the cache.
    /// Uses discovered_dirs to prevent duplicate fetches.
    fn prefetch_subdirectories_for_search(&mut self, folder_id: &str, prefix: Option<&str>) {
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
                    if self.model.performance.discovered_dirs.contains(&prefetch_key) {
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
    fn apply_search_filter(&mut self) {
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
                Ok(items) => items,
                Err(e) => {
                    log_debug(&format!("Failed to get cached items for search: {:?}", e));
                    // Fallback to simple filtering of current level.items only
                    let filtered = logic::search::filter_items(
                        &level.items,
                        &query,
                        prefix.as_deref(),
                    );
                    level.filtered_items = if filtered.is_empty() {
                        None
                    } else {
                        Some(filtered)
                    };
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

            // Restore selection to same item name if possible, otherwise reset to first item
            level.selected_index = if level.display_items().is_empty() {
                None
            } else if let Some(name) = selected_name {
                // Try to find previously selected item in filtered list
                logic::navigation::find_item_index_by_name(level.display_items(), &name)
                    .or(Some(0)) // If not found, default to first item
            } else {
                Some(0) // No previous selection, default to first item
            };
        }
    }

    /// Invalidate out-of-sync cache and refresh summary modal if open
    ///
    /// This is the single operation for "folder's out-of-sync state changed".
    /// Invalidates cache AND triggers refresh of summary modal (if open).
    fn invalidate_and_refresh_out_of_sync_summary(&mut self, folder_id: &str) {
        // Invalidate cache
        let _ = self.cache.invalidate_out_of_sync_categories(folder_id);

        // If summary modal is open, refresh it
        if self.model.ui.out_of_sync_summary.is_some() {
            let _ = self.api_tx.send(services::api::ApiRequest::GetNeededFiles {
                folder_id: folder_id.to_string(),
                page: None,
                perpage: Some(1000),
            });
        }
    }

    /// Apply out-of-sync filter to current breadcrumb level
    fn apply_out_of_sync_filter(&mut self) {
        // Don't filter folder list (only breadcrumbs)
        if self.model.navigation.focus_level == 0 || self.model.navigation.breadcrumb_trail.is_empty() {
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
            let any_filtered = self.model.navigation.breadcrumb_trail.iter().any(|l| l.filtered_items.is_some());

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
                    let _ = self.api_tx.send(services::api::ApiRequest::GetLocalChanged {
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
                    out_of_sync_items.contains_key(&full_path) || local_changed_items.contains(&full_path)
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

    /// Refresh current breadcrumb without search filter (restore original items)
    async fn refresh_current_breadcrumb(&mut self) -> Result<()> {
        if self.model.navigation.focus_level == 0 {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get(level_idx) {
            let folder_id = level.folder_id.clone();
            let prefix = level.prefix.clone();

            // Re-fetch from API (will go through cache)
            let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                folder_id,
                prefix,
                priority: services::api::Priority::High,
            });
        }

        Ok(())
    }

    /// Refresh all breadcrumb levels (used when clearing search)
    async fn refresh_all_breadcrumbs(&mut self) -> Result<()> {
        // Refresh each level in the breadcrumb trail
        for level in &self.model.navigation.breadcrumb_trail {
            let folder_id = level.folder_id.clone();
            let prefix = level.prefix.clone();

            // Re-fetch from API (will go through cache)
            let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                folder_id,
                prefix,
                priority: services::api::Priority::High,
            });
        }

        Ok(())
    }



    /// Handle keyboard input
    /// Delegated to handlers::keyboard module
    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        handlers::handle_key(self, key).await
    }

    pub fn open_out_of_sync_summary(&mut self) {
        use crate::model::types::OutOfSyncSummaryState;
        use crate::services::api::ApiRequest;
        use std::collections::{HashMap, HashSet};

        // Initialize summary state
        let mut summary_state = OutOfSyncSummaryState {
            selected_index: 0,
            breakdowns: HashMap::new(),
            loading: HashSet::new(),
        };

        // For each folder, check status and queue requests if needed
        for folder in &self.model.syncthing.folders {
            let folder_id = folder.id.clone();

            // Check if folder has out-of-sync items (from cached status)
            if let Some(status) = self.model.syncthing.folder_statuses.get(&folder_id) {
                let has_needed = status.need_total_items > 0;
                let has_local_changes = status.receive_only_total_items > 0;

                if has_needed || has_local_changes {
                    // Queue GetNeededFiles request for remote changes
                    // TODO: Local changes (receive-only deletions) require /rest/db/localchanged API
                    //       Currently only shows remote out-of-sync items from /rest/db/need
                    summary_state.loading.insert(folder_id.clone());

                    let _ = self.api_tx.send(ApiRequest::GetNeededFiles {
                        folder_id: folder_id.clone(),
                        page: None,
                        perpage: Some(1000), // Get all items
                    });
                }

                if !has_needed && !has_local_changes {
                    // All synced - set empty breakdown
                    summary_state.breakdowns.insert(folder_id, Default::default());
                }
            }
        }

        self.model.ui.out_of_sync_summary = Some(summary_state);
    }

    pub fn close_out_of_sync_summary(&mut self) {
        self.model.ui.out_of_sync_summary = None;
    }

    /// Toggle out-of-sync filter in breadcrumb view
    pub fn toggle_out_of_sync_filter(&mut self) {
        // Only works in breadcrumb view
        if self.model.navigation.focus_level == 0 {
            return;
        }

        // If filter is already active, clear it (regardless of what level we're on)
        if self.model.ui.out_of_sync_filter.is_some() {
            self.clear_out_of_sync_filter(true, None);  // Preserve selection, no toast (user toggling off)
            return;
        }

        // Clear any stale filter from a different folder/level
        self.clear_out_of_sync_filter(false, None);  // Don't preserve (stale context), no toast

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
            self.model
                .ui
                .show_toast("All files synced!".to_string());
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
            let _ = self.api_tx.send(services::api::ApiRequest::GetLocalChanged {
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

    /// Clear search state and filtered items
    ///
    /// # Arguments
    /// * `show_toast` - Optional toast message to display
    fn clear_search(&mut self, show_toast: Option<&str>) {
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

    /// Clear out-of-sync filter state and filtered items
    ///
    /// # Arguments
    /// * `preserve_selection` - Whether to keep cursor on same item by name
    /// * `show_toast` - Optional toast message to display
    fn clear_out_of_sync_filter(&mut self, preserve_selection: bool, show_toast: Option<&str>) {
        self.model.ui.out_of_sync_filter = None;

        if preserve_selection {
            // Clear filtered items while preserving selection by name
            for level in &mut self.model.navigation.breadcrumb_trail {
                let selected_name = level.selected_index
                    .and_then(|idx| level.display_items().get(idx))
                    .map(|item| item.name.clone());

                level.filtered_items = None;

                if let Some(name) = selected_name {
                    level.selected_index = logic::navigation::find_item_index_by_name(&level.items, &name);
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

    /// Enter search mode (handles mutual exclusion with filter)
    fn enter_search_mode(&mut self) {
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

    /// Activate out-of-sync filter (handles mutual exclusion with search)
    fn activate_out_of_sync_filter(&mut self) {
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

/// Determine the config file path with fallback logic
fn get_config_path(cli_path: Option<String>) -> Result<std::path::PathBuf> {
    use std::path::PathBuf;

    // If CLI argument provided, use it
    if let Some(path) = cli_path {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        } else {
            anyhow::bail!("Config file not found at specified path: {}", path);
        }
    }

    // Try ~/.config/synctui/config.yaml
    if let Some(config_dir) = dirs::config_dir() {
        let synctui_dir = config_dir.join("synctui");
        let config_path = synctui_dir.join("config.yaml");

        if config_path.exists() {
            return Ok(config_path);
        }
    }

    // Fallback to ./config.yaml
    let local_config = PathBuf::from("config.yaml");
    if local_config.exists() {
        return Ok(local_config);
    }

    // No config found, provide helpful error
    let expected_path = if let Some(config_dir) = dirs::config_dir() {
        config_dir
            .join("synctui")
            .join("config.yaml")
            .display()
            .to_string()
    } else {
        "~/.config/synctui/config.yaml".to_string()
    };

    anyhow::bail!(
        "Config file not found. Expected locations:\n\
         1. {} (preferred)\n\
         2. ./config.yaml (fallback)\n\
         \n\
         Use --config <path> to specify a custom location.",
        expected_path
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

    // Set debug mode
    DEBUG_MODE.store(args.debug, Ordering::Relaxed);

    if args.debug {
        log_debug("Debug mode enabled");
    }

    // Determine config file path
    let config_path = get_config_path(args.config)?;
    let config_path_str = config_path.display().to_string();

    if args.debug {
        log_debug(&format!("Loading config from: {:?}", config_path));
    }

    // Load configuration
    let config_str = fs::read_to_string(&config_path)?;
    let mut config: Config = serde_yaml::from_str(&config_str)?;

    // Override config with CLI flags
    if args.vim {
        config.vim_mode = true;
    }

    // Initialize app
    let mut app = App::new(config, config_path_str).await?;

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
    let mut last_stats_update = Instant::now();

    loop {
        // Clear terminal to remove sixel graphics if needed (brief flash but necessary)
        if app.model.ui.sixel_cleanup_frames > 0 {
            terminal.clear()?;
            app.model.ui.sixel_cleanup_frames = 0;
        }

        // Always render (Elm Architecture approach)
        terminal.draw(|f| {
            ui::render(f, app);
        })?;

        // Auto-dismiss toast after 1.5 seconds
        if let Some((_, timestamp)) = app.model.ui.toast_message {
            if crate::logic::ui::should_dismiss_toast(timestamp.elapsed().as_millis()) {
                app.model.ui.toast_message = None;
            }
        }

        if app.model.ui.should_quit {
            // Flush any pending writes before quitting
            app.flush_pending_db_writes();
            break;
        }

        // Process API responses (non-blocking)
        while let Ok(response) = app.api_rx.try_recv() {
            app.handle_api_response(response);
        }

        // Flush if batch is ready
        if app.should_flush_db() {
            app.flush_pending_db_writes();
        }

        // Process cache invalidation messages from event listener (non-blocking)
        // Throttle to max 50 events per frame to prevent UI freezing during bulk syncs
        let mut events_processed = 0;
        const MAX_EVENTS_PER_FRAME: usize = 50;

        while let Ok(invalidation) = app.invalidation_rx.try_recv() {
            app.handle_cache_invalidation(invalidation);
            events_processed += 1;

            if events_processed >= MAX_EVENTS_PER_FRAME {
                // Stop processing events this frame, continue next frame
                // This keeps UI responsive during event floods (hundreds of ItemStarted/ItemFinished)
                break;
            }
        }

        // Process event ID updates from event listener (non-blocking)
        while let Ok(event_id) = app.event_id_rx.try_recv() {
            // Persist event ID to cache periodically
            let _ = app.cache.save_last_event_id(event_id);
        }

        // Process image updates from background loading tasks (non-blocking)
        while let Ok((file_path, image_state)) = app.image_update_rx.try_recv() {
            // Store image state in runtime map (ImagePreviewState is not Clone, so kept separate from Model)
            app.image_state_map.insert(file_path.clone(), image_state);

            // Update popup if it's still showing the same file
            if let Some(ref mut popup_state) = app.model.ui.file_info_popup {
                if popup_state.file_path == file_path {
                    log_debug(&format!("Updating image state for {}", file_path));

                    // Update file_content based on image state
                    if let Some(img_state) = app.image_state_map.get(&file_path) {
                        match img_state {
                            ImagePreviewState::Ready { .. } => {
                                popup_state.file_content =
                                    Ok("[Image preview - see right panel]".to_string());
                            }
                            ImagePreviewState::Failed { .. } => {
                                popup_state.file_content = Err("Image preview unavailable".to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // NOTE: Removed periodic status polling - we now rely on events for cache invalidation
        // Status updates now only happen:
        // 1. On app startup (initial load)
        // 2. After user-initiated rescan (to get updated sequence)

        // Background reconnection: Attempt to reconnect when disconnected or connecting
        // Uses exponential backoff: 5s -> 10s -> 20s -> 40s -> 60s (capped)
        if matches!(
            app.model.syncthing.connection_state,
            model::syncthing::ConnectionState::Disconnected { .. }
                | model::syncthing::ConnectionState::Connecting { .. }
        ) {
            if app.last_reconnect_attempt.elapsed() >= app.reconnect_delay {
                crate::log_debug(&format!(
                    "DEBUG [Background Reconnection]: Attempting to reconnect (delay: {:?})...",
                    app.reconnect_delay
                ));

                // Calculate next delay BEFORE updating state (so UI shows correct next retry time)
                let next_delay = std::cmp::min(
                    app.reconnect_delay * 2,
                    Duration::from_secs(60)
                );

                // Update state to show we're attempting
                if let model::syncthing::ConnectionState::Disconnected { message, .. } =
                    &app.model.syncthing.connection_state
                {
                    app.model.syncthing.connection_state = model::syncthing::ConnectionState::Connecting {
                        attempt: 1,
                        last_error: Some(message.clone()),
                        next_retry_secs: next_delay.as_secs(),
                    };
                } else if let model::syncthing::ConnectionState::Connecting { attempt, last_error, .. } =
                    &app.model.syncthing.connection_state
                {
                    let new_attempt = attempt + 1;
                    app.model.syncthing.connection_state = model::syncthing::ConnectionState::Connecting {
                        attempt: new_attempt,
                        last_error: last_error.clone(),
                        next_retry_secs: next_delay.as_secs(),
                    };
                }

                // Try to refresh folder statuses to test connection
                app.refresh_folder_statuses_nonblocking();
                app.last_reconnect_attempt = Instant::now();

                // Update reconnect delay for next attempt
                app.reconnect_delay = next_delay;
            }
        } else {
            // Connected - reset reconnect delay for next disconnection
            app.reconnect_delay = Duration::from_secs(5);
        }

        // Check if we need to fetch folders after reconnection
        if app.model.ui.needs_folder_refresh {
            app.model.ui.needs_folder_refresh = false;
            match app.client.get_folders().await {
                Ok(folders) => {
                    let _ = app.cache.save_folders(&folders);
                    app.model.syncthing.folders = folders;

                    // If we now have folders, load their statuses and select first one
                    if !app.model.syncthing.folders.is_empty() {
                        app.model.navigation.folders_state_selection = Some(0);
                        app.load_folder_statuses().await;
                        let _ = app.load_root_level(true).await;
                        app.model.ui.show_setup_help = false;
                    } else {
                        log_debug("WARNING: Fetched 0 folders after reconnection");
                    }
                }
                Err(e) => {
                    log_debug(&format!("Failed to fetch folders after reconnection: {}", e));
                }
            }
        }

        // Refresh device/system status periodically (less frequently than folder stats)
        // System status every 30 seconds, connection stats every 2-3 seconds
        if app.last_system_status_update.elapsed() >= std::time::Duration::from_secs(30) {
            let _ = app.api_tx.send(services::api::ApiRequest::GetSystemStatus);
            app.last_system_status_update = Instant::now();
        }

        if app.last_connection_stats_fetch.elapsed() >= std::time::Duration::from_millis(5000) {
            let _ = app.api_tx.send(services::api::ApiRequest::GetConnectionStats);
            app.last_connection_stats_fetch = Instant::now();
        }

        // Poll folders in transient states (scanning, syncing, cleaning)
        // These states don't generate file change events, so we need periodic polling
        // Check every 2 seconds to catch state transitions
        if app.last_status_update.elapsed() >= std::time::Duration::from_millis(2000) {
            for (folder_id, status) in &app.model.syncthing.folder_statuses {
                if matches!(
                    status.state.as_str(),
                    "scanning" | "syncing" | "cleaning" | "scan-waiting" | "sync-waiting"
                ) {
                    crate::log_debug(&format!(
                        "DEBUG [Transient State Poll]: Polling folder '{}' in state '{}'",
                        folder_id, status.state
                    ));
                    let _ = app.api_tx.send(services::api::ApiRequest::GetFolderStatus {
                        folder_id: folder_id.clone(),
                    });
                }
            }
            app.last_status_update = Instant::now();
        }

        // Update UI periodically for live stats (uptime, transfer rates)
        if last_stats_update.elapsed() >= std::time::Duration::from_secs(1) {
            last_stats_update = Instant::now();
        }

        // Only run prefetch operations when user has been idle for 300ms
        // This prevents blocking keyboard input and reduces CPU usage
        let idle_time = app.model.performance.last_user_action.elapsed();
        if idle_time >= std::time::Duration::from_millis(300) {
            // Cleanup stale pending deletes (60s timeout fallback)
            app.cleanup_stale_pending_deletes();

            // Check and remove completed pending deletes
            app.verify_and_cleanup_pending_deletes();

            // Flush pending writes before idle operations
            app.flush_pending_db_writes();

            // HIGHEST PRIORITY: If hovering over a directory, recursively discover all subdirectories
            // and fetch their states (non-blocking, uses cache only)
            app.prefetch_hovered_subdirectories(10, 15);

            // Fetch directory metadata states for visible directories in current level
            app.fetch_directory_states(10);

            // Fetch selected item specifically (high priority for user interaction)
            app.fetch_selected_item_sync_state();

            // LOWEST PRIORITY: Batch fetch file sync states for visible files
            app.batch_fetch_visible_sync_states(5);

            // If search is active and we haven't updated recently, do a final update
            // This ensures results appear after prefetch completes
            if !app.model.ui.search_query.is_empty()
                && app.model.ui.search_query.len() >= 2
                && app.model.performance.last_search_filter_update.elapsed().as_millis() >= 300
            {
                app.model.performance.last_search_filter_update = std::time::Instant::now();
                app.apply_search_filter();
            }

            // Update directory states based on their children (uses cache only, non-blocking)
            if app.model.navigation.focus_level > 0 && !app.model.navigation.breadcrumb_trail.is_empty() {
                app.update_directory_states(app.model.navigation.focus_level - 1);
            }
        }

        // Increased poll timeout from 100ms to 250ms to reduce CPU usage when idle
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Flush before processing user input to ensure consistency
                app.flush_pending_db_writes();
                app.handle_key(key).await?;
            }
        }
    }

    Ok(())
}
