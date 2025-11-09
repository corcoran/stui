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

// Old types moved to model module:
// - BreadcrumbLevel (now uses Option<usize> instead of ListState)
// - model::FileInfoPopupState (without image_state field)
// - PendingDeleteInfo

pub struct App {
    // ✅ Pure application state (Elm Architecture Model)
    pub model: model::Model,

    // 🔧 Services (Runtime) - NOT part of Model
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

    // ⚙️ Config (Runtime)
    path_map: HashMap<String, String>,
    open_command: Option<String>,
    clipboard_command: Option<String>,
    base_url: String,

    // ⏱️ Timing (Runtime)
    last_status_update: Instant,
    last_system_status_update: Instant,
    last_connection_stats_fetch: Instant,
    last_directory_update: Instant,
    last_db_flush: Instant,
    last_reconnect_attempt: Instant,
    reconnect_delay: Duration,

    // 📊 Performance Optimizations (Runtime)
    pending_sync_state_writes: Vec<(String, String, SyncState, u64)>,

    // 🖼️ Image State (Runtime - not cloneable)
    /// Maps file paths to their image preview states (ImagePreviewState is not Clone, so kept separate from Model)
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
                // Success! Cache them for next time
                log_debug(&format!("Successfully fetched {} folders from API", folders.len()));
                if let Err(e) = cache.save_folders(&folders) {
                    log_debug(&format!("Failed to cache folders: {}", e));
                } else {
                    log_debug("Successfully cached folders");
                }
                (folders, model::syncthing::ConnectionState::Connected)
            }
            Err(e) => {
                // Failed to fetch - try cache
                log_debug(&format!("Failed to fetch folders from API: {}", e));
                let cached_folders = cache.get_all_folders().unwrap_or_else(|cache_err| {
                    log_debug(&format!("Failed to load folders from cache: {}", cache_err));
                    vec![]
                });

                if cached_folders.is_empty() {
                    // No cache - need to show setup help
                    log_debug("No cached folders available - will show setup help dialog");
                    (
                        vec![],
                        model::syncthing::ConnectionState::Disconnected {
                            error_type: logic::errors::classify_error(&e),
                            message: logic::errors::format_error_message(&e),
                        },
                    )
                } else {
                    // Use cache, will auto-retry in background
                    log_debug(&format!("Using {} cached folders, will auto-retry connection", cached_folders.len()));
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

        log_debug(&format!(
            "DEBUG [merge_local_only_files]: Fetching local items for folder={} prefix={:?}",
            folder_id, prefix
        ));

        // Fetch local-only items for this directory
        if let Ok(local_items) = self.client.get_local_changed_items(folder_id, prefix).await {
            log_debug(&format!(
                "DEBUG [merge_local_only_files]: Got {} local items",
                local_items.len()
            ));

            // Add local-only items that aren't already in the browse results
            for local_item in local_items {
                if !items.iter().any(|i| i.name == local_item.name) {
                    log_debug(&format!(
                        "DEBUG [merge_local_only_files]: Adding local item: {}",
                        local_item.name
                    ));
                    local_item_names.push(local_item.name.clone());
                    items.push(local_item);
                } else {
                    log_debug(&format!(
                        "DEBUG [merge_local_only_files]: Skipping duplicate item: {}",
                        local_item.name
                    ));
                }
            }
        } else {
            log_debug(&format!(
                "DEBUG [merge_local_only_files]: Failed to fetch local items"
            ));
        }

        local_item_names
    }

    fn load_sync_states_from_cache(
        &self,
        folder_id: &str,
        items: &[BrowseItem],
        prefix: Option<&str>,
    ) -> HashMap<String, SyncState> {
        log_debug(&format!(
            "DEBUG [load_sync_states_from_cache]: START folder={} prefix={:?} item_count={}",
            folder_id,
            prefix,
            items.len()
        ));
        let mut sync_states = HashMap::new();

        for item in items {
            // Build the file path
            let file_path = if let Some(prefix) = prefix {
                format!("{}{}", prefix, item.name)
            } else {
                item.name.clone()
            };

            log_debug(&format!(
                "DEBUG [load_sync_states_from_cache]: Querying cache for file_path={}",
                file_path
            ));

            // Load from cache without validation (will be validated on next fetch if needed)
            match self.cache.get_sync_state_unvalidated(folder_id, &file_path) {
                Ok(Some(state)) => {
                    log_debug(&format!(
                        "DEBUG [load_sync_states_from_cache]: FOUND state={:?} for file_path={}",
                        state, file_path
                    ));
                    sync_states.insert(item.name.clone(), state);
                }
                Ok(None) => {
                    log_debug(&format!(
                        "DEBUG [load_sync_states_from_cache]: NOT FOUND in cache for file_path={}",
                        file_path
                    ));
                }
                Err(e) => {
                    log_debug(&format!("DEBUG [load_sync_states_from_cache]: ERROR querying cache for file_path={}: {}", file_path, e));
                }
            }
        }

        log_debug(&format!(
            "DEBUG [load_sync_states_from_cache]: END returning {} states",
            sync_states.len()
        ));
        sync_states
    }

    /// Update directory states based on their children's states
    /// Directories should reflect the "worst" state of their children (Syncing > RemoteOnly > OutOfSync > Synced)
    /// Throttled to run at most once every 2 seconds to prevent excessive cache queries


    // Recursively discover and fetch states for subdirectories when hovering over a directory
    // This ensures we have complete subdirectory information for deep trees

    // Helper to recursively discover subdirectories (browse only, no state fetching)
    // This is synchronous and only uses cached data - no blocking API calls

    // Fetch directory-level sync states for subdirectories (their own metadata, not children)
    // This is cheap and gives immediate feedback for navigation (ignored/deleted/out-of-sync dirs)


    /// Check which ignored files exist on disk (done once on directory load, not per-frame)

    /// Update ignored_exists status for a single file in a breadcrumb level

    /// Sort a specific breadcrumb level by its index

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

                    crate::log_debug(&format!(
                        "DEBUG [prefetch]: Queuing browse for '{}'",
                        subdir_prefix
                    ));

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

            // If query is empty, reload without filter
            if query.is_empty() {
                // Queue API request to reload the breadcrumb level
                let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                    folder_id,
                    prefix,
                    priority: services::api::Priority::High,
                });
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
            crate::log_debug(&format!(
                "DEBUG [apply_search_filter]: Calling get_all_browse_items for folder={}, sequence={}",
                folder_id,
                folder_sequence
            ));
            let all_items = match self.cache.get_all_browse_items(&folder_id, folder_sequence) {
                Ok(items) => {
                    crate::log_debug(&format!(
                        "DEBUG [apply_search_filter]: Got {} total cached items for folder {}",
                        items.len(),
                        folder_id
                    ));
                    items
                }
                Err(e) => {
                    crate::log_debug(&format!(
                        "DEBUG [apply_search_filter]: Failed to get all items: {:?}",
                        e
                    ));
                    // Fallback to current directory only if cache query fails
                    // Fallback to current directory only if cache query fails
                    if let Ok(Some(cached_items)) =
                        self.cache
                            .get_browse_items(&folder_id, prefix.as_deref(), folder_sequence)
                    {
                        let filtered = logic::search::filter_items(
                            &cached_items,
                            &query,
                            prefix.as_deref(),
                        );
                        level.items = filtered;
                        level.selected_index = if level.items.is_empty() {
                            None
                        } else {
                            Some(0)
                        };
                    }
                    return;
                }
            };

            // Get current directory items from cache (unfiltered)
            let current_items = match self
                .cache
                .get_browse_items(&folder_id, prefix.as_deref(), folder_sequence)
            {
                Ok(Some(items)) => items,
                _ => {
                    // If we can't get current items, nothing to filter
                    return;
                }
            };

            // Build current path for comparison
            let current_path = prefix.as_deref().unwrap_or("");

            // Filter current level items: show items that match OR have matching descendants
            crate::log_debug(&format!(
                "DEBUG [apply_search_filter]: Filtering {} items at path '{}' with query '{}'",
                current_items.len(),
                current_path,
                query
            ));

            let filtered_items: Vec<api::BrowseItem> = current_items
                .into_iter()
                .filter(|item| {
                    let item_path = if current_path.is_empty() {
                        item.name.clone()
                    } else {
                        // current_path already ends with /, so just append
                        format!("{}{}", current_path, item.name)
                    };

                    crate::log_debug(&format!(
                        "DEBUG [apply_search_filter]: Processing item '{}' (type: {}), all_items.len()={}",
                        item_path,
                        item.item_type,
                        all_items.len()
                    ));

                    // Check if item itself matches
                    if logic::search::search_matches(&query, &item_path) {
                        crate::log_debug(&format!(
                            "DEBUG [apply_search_filter]: MATCH - Item '{}' matches query",
                            item_path
                        ));
                        return true;
                    }

                    // For directories, check if any descendant (at any depth) matches
                    if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                        let descendant_prefix = format!("{}/", item_path);
                        crate::log_debug(&format!(
                            "DEBUG [apply_search_filter]: Checking directory '{}' for descendants matching '{}'",
                            item_path,
                            query
                        ));

                        // Check if ANY file/folder inside this directory tree matches the query
                        let has_matching_descendant = all_items.iter().any(|(full_path, _)| {
                            // If this item is a descendant of our directory
                            if full_path.starts_with(&descendant_prefix) {
                                let matches = logic::search::search_matches(&query, full_path);
                                if matches {
                                    crate::log_debug(&format!(
                                        "DEBUG [apply_search_filter]: Found matching descendant '{}' in directory '{}'",
                                        full_path,
                                        item_path
                                    ));
                                }
                                matches
                            } else {
                                false
                            }
                        });

                        if has_matching_descendant {
                            crate::log_debug(&format!(
                                "DEBUG [apply_search_filter]: MATCH - Directory '{}' has matching descendants",
                                item_path
                            ));
                        } else {
                            crate::log_debug(&format!(
                                "DEBUG [apply_search_filter]: NO MATCH - Directory '{}' has no matching descendants",
                                item_path
                            ));
                        }

                        has_matching_descendant
                    } else {
                        crate::log_debug(&format!(
                            "DEBUG [apply_search_filter]: NO MATCH - File '{}' does not match query",
                            item_path
                        ));
                        false
                    }
                })
                .collect();

            crate::log_debug(&format!(
                "DEBUG [apply_search_filter]: Filtered to {} items",
                filtered_items.len()
            ));

            // Update items with filtered list
            level.items = filtered_items;

            // Reset selection to first item (if any matches)
            level.selected_index = if level.items.is_empty() {
                None
            } else {
                Some(0)
            };
        }
    }

    /// Apply out-of-sync filter to current breadcrumb level
    fn apply_out_of_sync_filter(&mut self) {
        // Don't filter folder list (only breadcrumbs)
        if self.model.navigation.focus_level == 0 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            let folder_id = level.folder_id.clone();
            let prefix = level.prefix.clone();

            // Get folder sequence for cache validation
            let folder_sequence = self
                .model
                .syncthing
                .folder_statuses
                .get(&folder_id)
                .map(|status| status.sequence)
                .unwrap_or(0);

            // Get out-of-sync items from cache
            let out_of_sync_items = match self.cache.get_out_of_sync_items(&folder_id) {
                Ok(items) => items,
                Err(_) => {
                    // If cache query fails, show nothing
                    level.items = vec![];
                    level.selected_index = None;
                    return;
                }
            };

            // If no out-of-sync items, show nothing
            if out_of_sync_items.is_empty() {
                level.items = vec![];
                level.selected_index = None;
                return;
            }

            // Get current directory items from cache (unfiltered)
            let current_items = match self
                .cache
                .get_browse_items(&folder_id, prefix.as_deref(), folder_sequence)
            {
                Ok(Some(items)) => items,
                _ => {
                    // If we can't get current items, nothing to filter
                    return;
                }
            };

            // Build current path for comparison
            let current_path = prefix.as_deref().unwrap_or("");

            // Filter current level items: show items that are out-of-sync OR have out-of-sync descendants
            let filtered_items: Vec<api::BrowseItem> = current_items
                .into_iter()
                .filter(|item| {
                    let item_path = if current_path.is_empty() {
                        item.name.clone()
                    } else {
                        format!("{}{}", current_path, item.name)
                    };

                    // Check if item itself is out-of-sync
                    if out_of_sync_items.contains_key(&item_path) {
                        return true;
                    }

                    // For directories, check if any descendant is out-of-sync
                    if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                        let descendant_prefix = format!("{}/", item_path);

                        // Check if ANY file/folder inside this directory tree is out-of-sync
                        let has_out_of_sync_descendant = out_of_sync_items
                            .keys()
                            .any(|path| path.starts_with(&descendant_prefix));

                        has_out_of_sync_descendant
                    } else {
                        false
                    }
                })
                .collect();

            // Update items with filtered list
            level.items = filtered_items;

            // Reset selection to first item (if any matches)
            level.selected_index = if level.items.is_empty() {
                None
            } else {
                Some(0)
            };
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

        // If filter is already active, clear it
        if self.model.ui.out_of_sync_filter.is_some() {
            self.model.ui.out_of_sync_filter = None;
            // Reload current breadcrumb to show all items
            let level_idx = self.model.navigation.focus_level - 1;
            if let Some(level) = self.model.navigation.breadcrumb_trail.get(level_idx) {
                let folder_id = level.folder_id.clone();
                let prefix = level.prefix.clone();
                let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                    folder_id,
                    prefix,
                    priority: services::api::Priority::High,
                });
            }
            return;
        }

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

        if !has_cached_data {
            // Queue GetNeededFiles request
            let _ = self.api_tx.send(services::api::ApiRequest::GetNeededFiles {
                folder_id: folder_id.clone(),
                page: None,
                perpage: Some(1000), // Get all items
            });

            // Show loading toast
            self.model
                .ui
                .show_toast("Loading out-of-sync files...".to_string());
        }

        // Activate filter
        self.model.ui.out_of_sync_filter = Some(model::types::OutOfSyncFilterState {
            origin_level: self.model.navigation.focus_level,
            last_refresh: std::time::SystemTime::now(),
        });

        // Apply filter to current level
        self.apply_out_of_sync_filter();
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
            crate::log_debug(&format!(
                "DEBUG [Main Loop]: needs_folder_refresh flag set, fetching folders (show_setup_help={})",
                app.model.ui.show_setup_help
            ));
            app.model.ui.needs_folder_refresh = false;
            match app.client.get_folders().await {
                Ok(folders) => {
                    crate::log_debug(&format!("DEBUG [Main Loop]: Successfully fetched {} folders", folders.len()));
                    let _ = app.cache.save_folders(&folders);
                    app.model.syncthing.folders = folders;

                    // If we now have folders, load their statuses and select first one
                    if !app.model.syncthing.folders.is_empty() {
                        crate::log_debug("DEBUG [Main Loop]: Loading folder statuses and selecting first folder");
                        app.model.navigation.folders_state_selection = Some(0);
                        app.load_folder_statuses().await;
                        let _ = app.load_root_level(true).await;
                        // Dismiss setup help dialog since we now have folders
                        app.model.ui.show_setup_help = false;
                        crate::log_debug("DEBUG [Main Loop]: Folders loaded and setup help dismissed");
                    } else {
                        crate::log_debug("DEBUG [Main Loop]: WARNING - fetched 0 folders!");
                    }
                }
                Err(e) => {
                    crate::log_debug(&format!("DEBUG [Main Loop]: Failed to fetch folders: {}", e));
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
