use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
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

    /// Enable targeted bug debugging logs to /tmp/synctui-bug.log
    #[arg(long)]
    bug: bool,

    /// Enable vim keybindings (hjkl, ^D/U, ^F/B, gg/G)
    #[arg(long)]
    vim: bool,

    /// Path to config file (default: platform-specific, see docs)
    #[arg(short, long)]
    config: Option<String>,
}

// Global flag for debug mode
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);
// Global flag for targeted bug debugging
static BUG_MODE: AtomicBool = AtomicBool::new(false);

mod api;
mod api_service;
mod cache;
mod config;
mod event_listener;
mod handlers;
mod messages;
mod state;
mod ui;
mod utils;

use api::{
    BrowseItem, ConnectionStats, Device, FileDetails, Folder, FolderStatus, SyncState,
    SyncthingClient, SystemStatus,
};
use cache::CacheDb;
use config::Config;
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

fn log_bug(msg: &str) {
    // Only log if bug mode is enabled
    if !BUG_MODE.load(Ordering::Relaxed) {
        return;
    }

    use std::fs::OpenOptions;
    use std::io::Write;
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/synctui-bug.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

fn sync_state_priority(state: SyncState) -> u8 {
    // Lower number = higher priority (displayed first)
    match state {
        SyncState::OutOfSync => 0,  // ⚠️ Most important
        SyncState::Syncing => 1,    // 🔄 Active operation
        SyncState::RemoteOnly => 2, // ☁️
        SyncState::LocalOnly => 3,  // 💻
        SyncState::Ignored => 4,    // 🚫
        SyncState::Unknown => 5,    // ❓
        SyncState::Synced => 6,     // ✅ Least important
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Off,              // No timestamp or size
    TimestampOnly,    // Show timestamp only
    TimestampAndSize, // Show both size and timestamp
}

impl DisplayMode {
    fn next(&self) -> Self {
        match self {
            DisplayMode::Off => DisplayMode::TimestampOnly,
            DisplayMode::TimestampOnly => DisplayMode::TimestampAndSize,
            DisplayMode::TimestampAndSize => DisplayMode::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    VisualIndicator, // Sort by sync state icon (directories first, then by state priority)
    Alphabetical,    // Sort alphabetically
    LastModified,    // Sort by last modified time (if available)
    FileSize,        // Sort by file size
}

impl SortMode {
    fn next(&self) -> Self {
        match self {
            SortMode::VisualIndicator => SortMode::Alphabetical,
            SortMode::Alphabetical => SortMode::LastModified,
            SortMode::LastModified => SortMode::FileSize,
            SortMode::FileSize => SortMode::VisualIndicator,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            SortMode::VisualIndicator => "Icon",
            SortMode::Alphabetical => "A-Z",
            SortMode::LastModified => "Timestamp",
            SortMode::FileSize => "Size",
        }
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

#[derive(Debug)]
pub struct FileInfoPopupState {
    pub folder_id: String,
    pub file_path: String,
    pub browse_item: BrowseItem,
    pub file_details: Option<FileDetails>,
    pub file_content: Result<String, String>, // Ok(content) or Err(error message)
    pub exists_on_disk: bool,
    pub is_binary: bool,
    pub is_image: bool,
    pub scroll_offset: u16, // Vertical scroll offset for preview
    pub image_state: Option<ImagePreviewState>,
}

#[derive(Clone)]
pub struct BreadcrumbLevel {
    pub folder_id: String,
    pub folder_label: String,
    pub folder_path: String, // Cache the folder's container path
    pub prefix: Option<String>,
    pub items: Vec<BrowseItem>,
    pub state: ListState,
    pub translated_base_path: String, // Cached translated base path for this level
    pub file_sync_states: HashMap<String, SyncState>, // Cache sync states by filename
    pub ignored_exists: HashMap<String, bool>, // Track if ignored files exist on disk (checked once on load)
}

/// Information about a pending ignore+delete operation
#[derive(Debug, Clone)]
pub struct PendingDeleteInfo {
    pub paths: HashSet<PathBuf>,  // Paths being deleted
    pub initiated_at: Instant,    // When operation started (for timeout fallback)
    pub rescan_triggered: bool,   // Whether rescan has been triggered
}

pub struct App {
    client: SyncthingClient,
    cache: CacheDb,
    pub folders: Vec<Folder>,
    pub devices: Vec<Device>,
    pub folders_state: ListState,
    pub folder_statuses: HashMap<String, FolderStatus>,
    pub statuses_loaded: bool,
    last_status_update: Instant,
    path_map: HashMap<String, String>,
    pub breadcrumb_trail: Vec<BreadcrumbLevel>,
    pub focus_level: usize, // 0 = folders, 1+ = breadcrumb levels
    pub should_quit: bool,
    pub display_mode: DisplayMode, // Toggle for displaying timestamps and/or size
    pub vim_mode: bool,            // Enable vim keybindings
    open_command: Option<String>,  // Optional command to open files/directories
    clipboard_command: Option<String>, // Optional command to copy to clipboard (receives text via stdin)
    last_key_was_g: bool,              // Track 'g' key for 'gg' command
    last_user_action: Instant,         // Track last user interaction for idle detection
    pub sort_mode: SortMode,           // Current sort mode (session-wide)
    pub sort_reverse: bool,            // Whether to reverse sort order (session-wide)
    // Track in-flight operations to prevent duplicate fetches
    loading_browse: std::collections::HashSet<String>, // Set of "folder_id:prefix" currently being loaded
    loading_sync_states: std::collections::HashSet<String>, // Set of "folder_id:path" currently being loaded
    discovered_dirs: std::collections::HashSet<String>, // Set of "folder_id:prefix" already discovered (to prevent re-querying cache)
    prefetch_enabled: bool, // Flag to enable/disable prefetching when system is busy
    last_known_sequences: HashMap<String, u64>, // Track last known sequence per folder to detect changes
    last_known_receive_only_counts: HashMap<String, u64>, // Track receiveOnlyTotalItems to detect local-only file changes
    // Track last update info per folder (for display)
    pub last_folder_updates: HashMap<String, (std::time::SystemTime, String)>, // folder_id -> (timestamp, last_changed_file)
    // Confirmation prompt state
    pub confirm_revert: Option<(String, Vec<String>)>, // If Some, shows confirmation prompt for reverting (folder_id, changed_files)
    pub confirm_delete: Option<(String, String, bool)>, // If Some, shows confirmation prompt for deleting (host_path, display_name, is_dir)
    // Pattern selection menu for removing ignores
    pub pattern_selection: Option<(String, String, Vec<String>, ListState)>, // If Some, shows pattern selection menu (folder_id, item_name, patterns, selection_state)
    // File information popup
    pub show_file_info: Option<FileInfoPopupState>, // If Some, shows file information popup
    // Toast notification
    pub toast_message: Option<(String, Instant)>, // If Some, shows toast notification (message, timestamp)
    // Pending ignore+delete operations (safety mechanism to prevent un-ignore during deletion)
    pub pending_ignore_deletes: HashMap<String, PendingDeleteInfo>, // folder_id -> pending delete info
    // API service channels
    api_tx: tokio::sync::mpsc::UnboundedSender<api_service::ApiRequest>,
    api_rx: tokio::sync::mpsc::UnboundedReceiver<api_service::ApiResponse>,
    // Event listener channels
    invalidation_rx: tokio::sync::mpsc::UnboundedReceiver<event_listener::CacheInvalidation>,
    event_id_rx: tokio::sync::mpsc::UnboundedReceiver<u64>,
    // Performance metrics
    pub last_load_time_ms: Option<u64>, // Time to load current directory (milliseconds)
    pub cache_hit: Option<bool>,        // Whether last load was a cache hit
    // Device/System status
    pub system_status: Option<SystemStatus>, // Device name, uptime, etc.
    connection_stats: Option<ConnectionStats>, // Global transfer stats
    last_connection_stats: Option<(ConnectionStats, Instant)>, // Previous stats + timestamp for rate calc
    pub device_name: Option<String>,                           // Cached device name
    last_system_status_update: Instant, // Track when we last fetched system status
    last_connection_stats_fetch: Instant, // Track when we last fetched connection stats
    pub last_transfer_rates: Option<(f64, f64)>, // Cached transfer rates (download, upload) in bytes/sec
    last_directory_update: Instant, // Track when we last ran update_directory_states() for throttling
    // Icon rendering
    pub icon_renderer: IconRenderer, // Centralized icon renderer
    // Image preview protocol
    pub image_picker: Option<ratatui_image::picker::Picker>, // Protocol picker for image rendering
    pub image_font_size: Option<(u16, u16)>, // Font size (width, height) for image cell calculations
    // Image update channel for non-blocking image loading
    image_update_tx: tokio::sync::mpsc::UnboundedSender<(String, ImagePreviewState)>, // Send (file_path, state) when image loads
    image_update_rx: tokio::sync::mpsc::UnboundedReceiver<(String, ImagePreviewState)>, // Receive image updates
    // Sixel cleanup counter - render white screen for N frames after closing image preview
    pub sixel_cleanup_frames: u8, // If > 0, render white rectangle and decrement
    // Performance optimizations: Batched database writes
    pending_sync_state_writes: Vec<(String, String, SyncState, u64)>, // (folder_id, file_path, state, sequence)
    last_db_flush: Instant, // Track when we last flushed pending writes
    // Performance optimizations: Dirty flag for UI rendering
    ui_dirty: bool, // Flag indicating UI needs redrawing
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

    /// Check if a path or any of its parent directories are pending deletion
    /// Returns Some(pending_path) if blocked, None if allowed
    fn is_path_or_parent_pending(&self, folder_id: &str, path: &PathBuf) -> Option<PathBuf> {
        if let Some(pending_info) = self.pending_ignore_deletes.get(folder_id) {
            // First check for exact match
            if pending_info.paths.contains(path) {
                return Some(path.clone());
            }

            // Check if any parent directory is pending
            // For example, if "/foo/bar" is pending, block "/foo/bar/baz"
            for pending_path in &pending_info.paths {
                if path.starts_with(pending_path) {
                    return Some(pending_path.clone());
                }
            }
        }
        None
    }

    /// Add a path to pending deletions for a folder
    fn add_pending_delete(&mut self, folder_id: String, path: PathBuf) {
        let pending_info = self.pending_ignore_deletes
            .entry(folder_id)
            .or_insert_with(|| PendingDeleteInfo {
                paths: HashSet::new(),
                initiated_at: Instant::now(),
                rescan_triggered: false,
            });

        pending_info.paths.insert(path);
        log_debug(&format!("Added pending delete: {:?}", pending_info.paths));
    }

    /// Remove a path from pending deletions after verification
    fn remove_pending_delete(&mut self, folder_id: &str, path: &PathBuf) {
        if let Some(pending_info) = self.pending_ignore_deletes.get_mut(folder_id) {
            pending_info.paths.remove(path);
            log_debug(&format!("Removed pending delete: {:?}, remaining: {:?}", path, pending_info.paths));

            // Clean up empty folder entry
            if pending_info.paths.is_empty() {
                self.pending_ignore_deletes.remove(folder_id);
                log_debug(&format!("Cleared pending deletes for folder: {}", folder_id));
            }
        }
    }

    /// Clean up stale pending deletes (older than 60 seconds)
    fn cleanup_stale_pending_deletes(&mut self) {
        let stale_timeout = Duration::from_secs(60);
        let now = Instant::now();

        self.pending_ignore_deletes.retain(|folder_id, info| {
            if now.duration_since(info.initiated_at) > stale_timeout {
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
        let buffer_time = Duration::from_secs(5);
        let now = Instant::now();

        for (_folder_id, info) in self.pending_ignore_deletes.iter_mut() {
            // Only check if rescan has been triggered and buffer time has passed
            if !info.rescan_triggered || now.duration_since(info.initiated_at) < buffer_time {
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
        self.pending_ignore_deletes.retain(|folder_id, info| {
            if info.paths.is_empty() {
                log_debug(&format!("All pending deletes completed for folder: {}", folder_id));
                false
            } else {
                true
            }
        });
    }

    pub fn get_local_state_summary(&self) -> (u64, u64, u64) {
        // Calculate aggregate local state across all folders: (files, directories, bytes)
        let mut total_files = 0u64;
        let mut total_dirs = 0u64;
        let mut total_bytes = 0u64;

        for status in self.folder_statuses.values() {
            total_files += status.local_files;
            total_dirs += status.local_directories;
            total_bytes += status.local_bytes;
        }

        (total_files, total_dirs, total_bytes)
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
        const MAX_BATCH_SIZE: usize = 50;
        const MAX_BATCH_AGE_MS: u64 = 100;

        if self.pending_sync_state_writes.is_empty() {
            return false;
        }

        self.pending_sync_state_writes.len() >= MAX_BATCH_SIZE
            || self.last_db_flush.elapsed() > Duration::from_millis(MAX_BATCH_AGE_MS)
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
        let devices = client.get_devices().await.unwrap_or_default();

        // Spawn API service worker
        let (api_tx, api_rx) = api_service::spawn_api_service(client.clone());

        // Get last event ID from cache
        let last_event_id = cache.get_last_event_id().unwrap_or(0);

        // Create channels for event listener
        let (invalidation_tx, invalidation_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_id_tx, event_id_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create channel for image updates
        let (image_update_tx, image_update_rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn event listener
        event_listener::spawn_event_listener(
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

        let mut app = App {
            client,
            cache,
            folders,
            devices,
            folders_state: ListState::default(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            last_status_update: Instant::now(),
            path_map: config.path_map,
            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            should_quit: false,
            display_mode: DisplayMode::TimestampAndSize, // Start with most info
            vim_mode: config.vim_mode,
            open_command: config.open_command,
            clipboard_command: config.clipboard_command,
            last_key_was_g: false,
            last_user_action: Instant::now(),
            sort_mode: SortMode::Alphabetical,
            sort_reverse: false,
            loading_browse: HashSet::new(),
            loading_sync_states: HashSet::new(),
            discovered_dirs: HashSet::new(),
            prefetch_enabled: true,
            last_known_sequences: HashMap::new(),
            last_known_receive_only_counts: HashMap::new(),
            last_folder_updates: HashMap::new(),
            confirm_revert: None,
            confirm_delete: None,
            pattern_selection: None,
            show_file_info: None,
            toast_message: None,
            pending_ignore_deletes: HashMap::new(),
            api_tx,
            api_rx,
            invalidation_rx,
            event_id_rx,
            last_load_time_ms: None,
            cache_hit: None,
            system_status: None,
            connection_stats: None,
            last_connection_stats: None,
            device_name: None,
            last_system_status_update: Instant::now(),
            last_connection_stats_fetch: Instant::now(),
            last_transfer_rates: None,
            last_directory_update: Instant::now(),
            icon_renderer,
            image_picker,
            image_font_size,
            image_update_tx,
            image_update_rx,
            sixel_cleanup_frames: 0,
            // Performance optimizations
            pending_sync_state_writes: Vec::new(),
            last_db_flush: Instant::now(),
            ui_dirty: true, // Start dirty to draw initial frame
        };

        // Load folder statuses first (needed for cache validation)
        app.load_folder_statuses().await;

        // Initialize system status and connection stats
        if let Ok(device_name) = app.client.get_device_name().await {
            app.device_name = Some(device_name);
        }

        if let Ok(sys_status) = app.client.get_system_status().await {
            app.system_status = Some(sys_status);
        }

        if let Ok(conn_stats) = app.client.get_connection_stats().await {
            app.last_connection_stats = Some((conn_stats.clone(), Instant::now()));
            app.connection_stats = Some(conn_stats);
        }

        if !app.folders.is_empty() {
            app.folders_state.select(Some(0));
            app.load_root_level(true).await?; // Preview mode - focus stays on folders
        }

        Ok(app)
    }

    async fn load_folder_statuses(&mut self) {
        for folder in &self.folders {
            // Try cache first - use it without validation on initial load
            if !self.statuses_loaded {
                if let Ok(Some(cached_status)) = self.cache.get_folder_status(&folder.id) {
                    self.folder_statuses
                        .insert(folder.id.clone(), cached_status);
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

                        // Clear in-memory sync states for this folder if we're currently viewing it
                        // This ensures files that changed get refreshed
                        if !self.breadcrumb_trail.is_empty()
                            && self.breadcrumb_trail[0].folder_id == folder.id
                        {
                            for level in &mut self.breadcrumb_trail {
                                if level.folder_id == folder.id {
                                    level.file_sync_states.clear();
                                }
                            }
                        }
                    }
                }

                // Update last known sequence
                self.last_known_sequences
                    .insert(folder.id.clone(), sequence);

                // Save fresh status and use it
                let _ = self.cache.save_folder_status(&folder.id, &status, sequence);
                self.folder_statuses.insert(folder.id.clone(), status);
            }
        }
        self.statuses_loaded = true;
        self.last_status_update = Instant::now();
    }

    fn refresh_folder_statuses_nonblocking(&mut self) {
        // Non-blocking version for background polling
        // Sends status requests via API service
        for folder in &self.folders {
            let _ = self.api_tx.send(api_service::ApiRequest::GetFolderStatus {
                folder_id: folder.id.clone(),
            });
        }
    }

    /// Handle API responses from background worker
    /// Delegated to handlers::api module
    fn handle_api_response(&mut self, response: api_service::ApiResponse) {
        handlers::handle_api_response(self, response);
    }

    /// Handle cache invalidation messages from event listener
    /// Delegated to handlers::events module
    fn handle_cache_invalidation(&mut self, invalidation: event_listener::CacheInvalidation) {
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
        let has_local_changes = self
            .folder_statuses
            .get(folder_id)
            .map(|s| s.receive_only_total_items > 0)
            .unwrap_or(false);

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
    fn update_directory_states(&mut self, level_idx: usize) {
        // Throttle: only run once every 2 seconds to prevent continuous cache queries
        if self.last_directory_update.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_directory_update = Instant::now();

        if level_idx >= self.breadcrumb_trail.len() {
            return;
        }

        let level = &self.breadcrumb_trail[level_idx];
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

            // Start with the direct state, or default to Synced if not set
            let mut aggregate_state = direct_state.unwrap_or(SyncState::Synced);

            // If the directory itself is RemoteOnly or Ignored, that takes precedence
            // (it means the directory itself doesn't exist locally or is ignored)
            if matches!(aggregate_state, SyncState::RemoteOnly | SyncState::Ignored) {
                dir_states.insert(dir_name.clone(), aggregate_state);
                continue;
            }

            // Check if we have cached children states
            let dir_prefix = if let Some(ref prefix) = level.prefix {
                format!("{}{}/", prefix, dir_name)
            } else {
                format!("{}/", dir_name)
            };

            // Get folder sequence for cache validation
            let folder_sequence = self.folder_statuses.get(&level.folder_id)
                .map(|status| status.sequence)
                .unwrap_or_else(|| {
                    log_debug(&format!("DEBUG [update_directory_states]: folder_id '{}' not found in folder_statuses, using sequence=0", level.folder_id));
                    0
                });

            // Try to get cached browse items for this directory
            if let Ok(Some(children)) =
                self.cache
                    .get_browse_items(&level.folder_id, Some(&dir_prefix), folder_sequence)
            {
                // Check children states
                let mut has_syncing = false;
                let mut has_remote_only = false;
                let mut has_out_of_sync = false;
                let mut has_local_only = false;

                for child in &children {
                    let child_path = format!("{}{}", dir_prefix, child.name);
                    if let Ok(Some(child_state)) = self
                        .cache
                        .get_sync_state_unvalidated(&level.folder_id, &child_path)
                    {
                        match child_state {
                            SyncState::Syncing => has_syncing = true,
                            SyncState::RemoteOnly => has_remote_only = true,
                            SyncState::OutOfSync => has_out_of_sync = true,
                            SyncState::LocalOnly => has_local_only = true,
                            _ => {}
                        }
                    }
                }

                // Determine aggregate state (priority order: Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced)
                aggregate_state = if has_syncing {
                    SyncState::Syncing
                } else if has_remote_only {
                    SyncState::RemoteOnly
                } else if has_out_of_sync {
                    SyncState::OutOfSync
                } else if has_local_only {
                    SyncState::LocalOnly
                } else {
                    // All children synced, use the directory's direct state
                    direct_state.unwrap_or(SyncState::Synced)
                };
            }

            dir_states.insert(dir_name.clone(), aggregate_state);
        }

        // Apply computed states
        if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
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

    fn batch_fetch_visible_sync_states(&mut self, max_concurrent: usize) {
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
                if self.breadcrumb_trail[level_idx]
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

                !self.loading_sync_states.contains(&sync_key)
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
            if self.loading_sync_states.contains(&sync_key) {
                continue;
            }

            // Mark as loading
            self.loading_sync_states.insert(sync_key.clone());

            log_debug(&format!(
                "DEBUG [batch_fetch]: Requesting file_path={} for folder={}",
                file_path, folder_id
            ));

            // Send non-blocking request via channel
            let _ = self.api_tx.send(api_service::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: file_path.clone(),
                priority: api_service::Priority::Medium,
            });
        }
    }

    // Recursively discover and fetch states for subdirectories when hovering over a directory
    // This ensures we have complete subdirectory information for deep trees
    fn prefetch_hovered_subdirectories(&mut self, max_depth: usize, max_dirs_per_frame: usize) {
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

        let selected_item = if let Some(item) = self.breadcrumb_trail[level_idx]
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

        let folder_id = self.breadcrumb_trail[level_idx].folder_id.clone();
        let prefix = self.breadcrumb_trail[level_idx].prefix.clone();
        let folder_sequence = self
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
            if self.loading_sync_states.contains(&sync_key) {
                continue;
            }

            // Check if already cached
            if let Ok(Some(_)) = self.cache.get_sync_state_unvalidated(&folder_id, dir_path) {
                continue;
            }

            // Mark as loading
            self.loading_sync_states.insert(sync_key.clone());

            // Send non-blocking request for directory's sync state
            let _ = self.api_tx.send(api_service::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: dir_path.to_string(),
                priority: api_service::Priority::Low, // Low priority, it's speculative prefetch
            });
        }
    }

    // Helper to recursively discover subdirectories (browse only, no state fetching)
    // This is synchronous and only uses cached data - no blocking API calls
    fn discover_subdirectories_sync(
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

        // Check if already discovered (prevent re-querying cache every frame)
        if self.discovered_dirs.contains(&browse_key) {
            return;
        }

        // Check if already loading
        if self.loading_browse.contains(&browse_key) {
            return;
        }

        // Try to get from cache first (accept any cached value, even if sequence is old)
        // For prefetch, we prioritize speed over perfect accuracy
        // Use sequence=0 to accept any cached value regardless of sequence number
        let items = if let Ok(Some(cached_items)) = self
            .cache
            .get_browse_items(folder_id, Some(dir_path), 0)
        {
            cached_items
        } else {
            // Not cached - request it non-blocking and mark as discovered to prevent re-requesting
            if !self.loading_browse.contains(&browse_key) {
                self.loading_browse.insert(browse_key.clone());

                // Mark as discovered immediately to prevent repeated requests
                self.discovered_dirs.insert(browse_key.clone());

                // Send non-blocking browse request
                let _ = self.api_tx.send(api_service::ApiRequest::BrowseFolder {
                    folder_id: folder_id.to_string(),
                    prefix: Some(dir_path.to_string()),
                    priority: api_service::Priority::Low,
                });
            }
            return; // Skip this iteration, will process when cached
        };

        // Mark this directory as discovered to prevent re-querying
        self.discovered_dirs.insert(browse_key);

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
                    folder_sequence,
                    current_depth + 1,
                    max_depth,
                    result,
                );
            }
        }
    }

    // Fetch directory-level sync states for subdirectories (their own metadata, not children)
    // This is cheap and gives immediate feedback for navigation (ignored/deleted/out-of-sync dirs)
    fn fetch_directory_states(&mut self, max_concurrent: usize) {
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
                self.breadcrumb_trail[level_idx]
                    .file_sync_states
                    .get(&item.name)
                    .is_none()
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

            // Send non-blocking request via API service
            // Response will be handled by handle_api_response
            let _ = self.api_tx.send(api_service::ApiRequest::GetFileInfo {
                folder_id: folder_id.clone(),
                file_path: dir_path.clone(),
                priority: api_service::Priority::Medium,
            });
        }
    }

    fn fetch_selected_item_sync_state(&mut self) {
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

                    // Send non-blocking request via API service
                    // Response will be handled by handle_api_response
                    let _ = self.api_tx.send(api_service::ApiRequest::GetFileInfo {
                        folder_id: level.folder_id.clone(),
                        file_path: file_path.clone(),
                        priority: api_service::Priority::High, // High priority for selected item
                    });
                }
            }
        }
    }

    /// Check which ignored files exist on disk (done once on directory load, not per-frame)
    fn check_ignored_existence(
        &self,
        items: &[BrowseItem],
        file_sync_states: &HashMap<String, SyncState>,
        translated_base_path: &str,
        prefix: Option<&str>,
        parent_exists: Option<bool>,
    ) -> HashMap<String, bool> {
        let mut ignored_exists = HashMap::new();

        for item in items {
            if let Some(SyncState::Ignored) = file_sync_states.get(&item.name) {
                // Optimization: If parent directory doesn't exist, children can't either
                if parent_exists == Some(false) {
                    ignored_exists.insert(item.name.clone(), false);
                    log_debug(&format!(
                        "DEBUG [check_ignored_existence]: item={} skipped (parent doesn't exist)",
                        item.name
                    ));
                    continue;
                }

                // Check filesystem for this item
                let host_path = format!(
                    "{}/{}",
                    translated_base_path.trim_end_matches('/'),
                    item.name
                );
                let exists = std::path::Path::new(&host_path).exists();
                log_debug(&format!("DEBUG [check_ignored_existence]: item={} prefix={:?} translated_base_path={} host_path={} exists={}",
                    item.name, prefix, translated_base_path, host_path, exists));
                ignored_exists.insert(item.name.clone(), exists);
            }
        }

        ignored_exists
    }

    /// Update ignored_exists status for a single file in a breadcrumb level
    fn update_ignored_exists_for_file(
        &mut self,
        level_idx: usize,
        file_name: &str,
        new_state: SyncState,
    ) {
        if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
            if new_state == SyncState::Ignored {
                // File is now ignored - check if it exists
                // translated_base_path already includes the full path to this directory level
                let host_path = format!(
                    "{}/{}",
                    level.translated_base_path.trim_end_matches('/'),
                    file_name
                );
                let exists = std::path::Path::new(&host_path).exists();
                log_debug(&format!("DEBUG [update_ignored_exists_for_file]: file_name={} prefix={:?} translated_base_path={} host_path={} exists={}",
                    file_name, level.prefix, level.translated_base_path, host_path, exists));
                level.ignored_exists.insert(file_name.to_string(), exists);
            } else {
                // File is no longer ignored - remove from ignored_exists
                level.ignored_exists.remove(file_name);
            }
        }
    }

    async fn load_root_level(&mut self, preview_only: bool) -> Result<()> {
        if let Some(selected) = self.folders_state.selected() {
            if let Some(folder) = self.folders.get(selected).cloned() {
                // Don't try to browse paused folders
                if folder.paused {
                    // Stay on folder list, don't enter the folder
                    return Ok(());
                }

                // Start timing
                let start = Instant::now();

                // Get folder sequence for cache validation
                let folder_sequence = self
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
                self.loading_browse.remove(&browse_key);

                // Try cache first
                let (items, local_items) = if let Ok(Some(cached_items)) = self
                    .cache
                    .get_browse_items(&folder.id, None, folder_sequence)
                {
                    self.cache_hit = Some(true);
                    let mut items = cached_items;
                    // Merge local files even from cache
                    let local_items = self
                        .merge_local_only_files(&folder.id, &mut items, None)
                        .await;
                    (items, local_items)
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());

                    // Cache miss - fetch from API
                    self.cache_hit = Some(false);
                    let mut items = self.client.browse_folder(&folder.id, None).await?;

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
                    self.loading_browse.remove(&browse_key);

                    (items, local_items)
                };

                // Record load time
                self.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

                // Create state without selecting anything - sort_current_level will handle selection
                let state = ListState::default();

                // Compute translated base path once
                let translated_base_path = self.translate_path(&folder, "");

                // Load cached sync states for items
                let mut file_sync_states =
                    self.load_sync_states_from_cache(&folder.id, &items, None);

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
                let ignored_exists = self.check_ignored_existence(
                    &items,
                    &file_sync_states,
                    &translated_base_path,
                    None,
                    None,
                );

                self.breadcrumb_trail = vec![BreadcrumbLevel {
                    folder_id: folder.id.clone(),
                    folder_label: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
                    folder_path: folder.path.clone(),
                    prefix: None,
                    items,
                    state,
                    translated_base_path,
                    file_sync_states,
                    ignored_exists,
                }];

                // Only change focus if not in preview mode
                if !preview_only {
                    self.focus_level = 1;
                }

                // Apply initial sorting
                self.sort_current_level();
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

        // Start timing
        let start = Instant::now();

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
                let folder_sequence = self
                    .folder_statuses
                    .get(&folder_id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                // Create key for tracking in-flight operations
                let browse_key = format!("{}:{}", folder_id, new_prefix);

                // Remove from loading_browse set if it's there (cleanup from previous attempts)
                self.loading_browse.remove(&browse_key);

                // Try cache first
                let (items, local_items) = if let Ok(Some(cached_items)) = self
                    .cache
                    .get_browse_items(&folder_id, Some(&new_prefix), folder_sequence)
                {
                    self.cache_hit = Some(true);
                    let mut items = cached_items;
                    // Merge local files even from cache
                    let local_items = self
                        .merge_local_only_files(&folder_id, &mut items, Some(&new_prefix))
                        .await;
                    (items, local_items)
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());
                    self.cache_hit = Some(false);

                    // Cache miss - fetch from API (BLOCKING)
                    let mut items = self
                        .client
                        .browse_folder(&folder_id, Some(&new_prefix))
                        .await?;

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
                    self.loading_browse.remove(&browse_key);

                    (items, local_items)
                };

                // Record load time
                self.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

                // Create state without selecting anything - sort_current_level will handle selection
                let state = ListState::default();

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
                self.breadcrumb_trail.truncate(level_idx + 1);

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
                    let _ =
                        self.cache
                            .save_sync_state(&folder_id, &file_path, SyncState::LocalOnly, 0);
                }

                // Check if we're inside an ignored directory (check all ancestors) - if so, mark all children as ignored
                // This handles the case where you ignore a directory and immediately drill into it
                // Ancestor checking removed - FileInfo API will provide correct states

                // Check which ignored files exist on disk (one-time check, not per-frame)
                // Determine if parent directory exists (optimization for ignored directories)
                let parent_exists = Some(std::path::Path::new(&translated_base_path).exists());
                let ignored_exists = self.check_ignored_existence(
                    &items,
                    &file_sync_states,
                    &translated_base_path,
                    Some(&new_prefix),
                    parent_exists,
                );

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
                    ignored_exists,
                });

                self.focus_level += 1;

                // Apply initial sorting
                self.sort_current_level();
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

    /// Sort a specific breadcrumb level by its index
    fn sort_level(&mut self, level_idx: usize) {
        self.sort_level_with_selection(level_idx, None);
    }

    fn sort_level_with_selection(
        &mut self,
        level_idx: usize,
        preserve_selection_name: Option<String>,
    ) {
        if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
            let sort_mode = self.sort_mode;
            let reverse = self.sort_reverse;

            // Use provided name if given, otherwise get currently selected item name
            let selected_name = preserve_selection_name.or_else(|| {
                level
                    .state
                    .selected()
                    .and_then(|idx| level.items.get(idx))
                    .map(|item| item.name.clone())
            });

            // Sort items
            level.items.sort_by(|a, b| {
                use std::cmp::Ordering;

                // Always prioritize directories first
                let a_is_dir = a.item_type == "FILE_INFO_TYPE_DIRECTORY";
                let b_is_dir = b.item_type == "FILE_INFO_TYPE_DIRECTORY";

                if a_is_dir != b_is_dir {
                    return if a_is_dir {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }

                let result = match sort_mode {
                    SortMode::VisualIndicator => {
                        // Sort by sync state priority
                        let a_state = level
                            .file_sync_states
                            .get(&a.name)
                            .copied()
                            .unwrap_or(SyncState::Unknown);
                        let b_state = level
                            .file_sync_states
                            .get(&b.name)
                            .copied()
                            .unwrap_or(SyncState::Unknown);

                        let a_priority = sync_state_priority(a_state);
                        let b_priority = sync_state_priority(b_state);

                        a_priority
                            .cmp(&b_priority)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    }
                    SortMode::Alphabetical => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    SortMode::LastModified => {
                        // Reverse order for modified time (newest first)
                        // Use mod_time from BrowseItem directly
                        b.mod_time
                            .cmp(&a.mod_time)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    }
                    SortMode::FileSize => {
                        // Reverse order for size (largest first)
                        // Use size from BrowseItem directly
                        b.size
                            .cmp(&a.size)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    }
                };

                if reverse {
                    result.reverse()
                } else {
                    result
                }
            });

            // Restore selection to the same item
            if let Some(name) = selected_name {
                let new_idx = level.items.iter().position(|item| item.name == name);
                level.state.select(new_idx.or(Some(0))); // Default to first item if not found
            } else if !level.items.is_empty() {
                // No previous selection, select first item
                level.state.select(Some(0));
            }
        }
    }

    fn sort_current_level(&mut self) {
        if self.focus_level == 0 {
            // In preview mode (focus on folder list), sort the first breadcrumb if it exists
            if !self.breadcrumb_trail.is_empty() {
                self.sort_level(0);
            }
            return;
        }
        let level_idx = self.focus_level - 1;
        self.sort_level(level_idx);
    }

    fn sort_all_levels(&mut self) {
        // Apply sorting to all breadcrumb levels
        let num_levels = self.breadcrumb_trail.len();
        for idx in 0..num_levels {
            self.sort_level(idx);
        }
    }

    fn cycle_sort_mode(&mut self) {
        if self.focus_level == 0 {
            return; // No sorting for folders list
        }

        self.sort_mode = self.sort_mode.next();
        self.sort_reverse = false; // Reset reverse when changing mode
        self.sort_all_levels(); // Apply to all levels
    }

    fn toggle_sort_reverse(&mut self) {
        if self.focus_level == 0 {
            return; // No sorting for folders list
        }

        self.sort_reverse = !self.sort_reverse;
        self.sort_all_levels(); // Apply to all levels
    }

    async fn next_item(&mut self) {
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
            // Auto-load the selected folder's root directory as preview (don't change focus)
            let _ = self.load_root_level(true).await;
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

    async fn previous_item(&mut self) {
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
            // Auto-load the selected folder's root directory as preview (don't change focus)
            let _ = self.load_root_level(true).await;
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

    async fn jump_to_first(&mut self) {
        if self.focus_level == 0 {
            if !self.folders.is_empty() {
                self.folders_state.select(Some(0));
                // Auto-load the selected folder's root directory as preview (don't change focus)
                let _ = self.load_root_level(true).await;
            }
        } else {
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if !level.items.is_empty() {
                    level.state.select(Some(0));
                }
            }
        }
    }

    async fn jump_to_last(&mut self) {
        if self.focus_level == 0 {
            if !self.folders.is_empty() {
                self.folders_state.select(Some(self.folders.len() - 1));
                // Auto-load the selected folder's root directory as preview (don't change focus)
                let _ = self.load_root_level(true).await;
            }
        } else {
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if !level.items.is_empty() {
                    level.state.select(Some(level.items.len() - 1));
                }
            }
        }
    }

    async fn page_down(&mut self, page_size: usize) {
        if self.focus_level == 0 {
            if self.folders.is_empty() {
                return;
            }
            let i = match self.folders_state.selected() {
                Some(i) => (i + page_size).min(self.folders.len() - 1),
                None => 0,
            };
            self.folders_state.select(Some(i));
            // Auto-load the selected folder's root directory as preview (don't change focus)
            let _ = self.load_root_level(true).await;
        } else {
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => (i + page_size).min(level.items.len() - 1),
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    async fn page_up(&mut self, page_size: usize) {
        if self.focus_level == 0 {
            if self.folders.is_empty() {
                return;
            }
            let i = match self.folders_state.selected() {
                Some(i) => i.saturating_sub(page_size),
                None => 0,
            };
            self.folders_state.select(Some(i));
            // Auto-load the selected folder's root directory as preview (don't change focus)
            let _ = self.load_root_level(true).await;
        } else {
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => i.saturating_sub(page_size),
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    async fn half_page_down(&mut self, visible_height: usize) {
        self.page_down(visible_height / 2).await;
    }

    async fn half_page_up(&mut self, visible_height: usize) {
        self.page_up(visible_height / 2).await;
    }

    fn rescan_selected_folder(&mut self) -> Result<()> {
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

        log_debug(&format!(
            "DEBUG [rescan_selected_folder]: Requesting rescan for folder={}",
            folder_id
        ));

        // Trigger rescan via non-blocking API
        let _ = self
            .api_tx
            .send(api_service::ApiRequest::RescanFolder { folder_id });

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
                let changed_files = self
                    .client
                    .get_local_changed_files(&folder_id)
                    .await
                    .unwrap_or_else(|_| Vec::new());

                // Show confirmation prompt with file list
                self.confirm_revert = Some((folder_id, changed_files));
                return Ok(());
            }
        }

        // Not receive-only or no local changes - just rescan
        self.client.rescan_folder(&folder_id).await?;

        // Refresh statuses in background (non-blocking)
        self.refresh_folder_statuses_nonblocking();

        Ok(())
    }

    async fn delete_file(&mut self) -> Result<()> {
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

        // Build the full host path
        // Note: translated_base_path already includes full directory path (with prefix),
        // so we only append the item name (not relative_path which duplicates the prefix)
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

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

    fn open_selected_item(&mut self) -> Result<()> {
        // Check if open_command is configured
        let Some(ref open_cmd) = self.open_command else {
            self.toast_message = Some((
                "Error: open_command not configured".to_string(),
                Instant::now(),
            ));
            return Ok(());
        };

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

        // Build the full host path
        // Note: translated_base_path already includes the full path to this directory level
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

        // Check if file/directory exists on disk before trying to open
        if !std::path::Path::new(&host_path).exists() {
            log_debug(&format!(
                "open_selected_item: Path does not exist: {}",
                host_path
            ));
            return Ok(()); // Nothing to open
        }

        // Execute command in background (spawn, don't wait for completion)
        // This allows GUI apps and editors to open without blocking the TUI
        let result = std::process::Command::new(open_cmd)
            .arg(&host_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(_child) => {
                // Log in debug mode
                if crate::DEBUG_MODE.load(std::sync::atomic::Ordering::Relaxed) {
                    log_debug(&format!("open_command: spawned {} {}", open_cmd, host_path));
                }
                // Show toast notification with full path
                let toast_msg = format!("Opened: {}", host_path);
                self.toast_message = Some((toast_msg, Instant::now()));
            }
            Err(e) => {
                log_debug(&format!(
                    "Failed to execute open_command '{}': {}",
                    open_cmd, e
                ));
                // Show error toast
                let toast_msg = format!("Error: Failed to open with '{}'", open_cmd);
                self.toast_message = Some((toast_msg, Instant::now()));
            }
        }

        Ok(())
    }

    fn copy_to_clipboard(&mut self) -> Result<()> {
        let text_to_copy = if self.focus_level == 0 {
            // In folder list - copy folder ID
            if let Some(selected) = self.folders_state.selected() {
                if let Some(folder) = self.folders.get(selected) {
                    Some(folder.id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            // In breadcrumbs - copy file/directory path (mapped host path)
            if self.breadcrumb_trail.is_empty() {
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

            // Build the full host path
            // Note: translated_base_path already includes the full path to this directory level
            let host_path = format!(
                "{}/{}",
                level.translated_base_path.trim_end_matches('/'),
                item.name
            );

            Some(host_path)
        };

        // Copy to clipboard if we have text
        if let Some(text) = text_to_copy {
            // Always log clipboard operations (not just in debug mode) since they can fail silently
            use std::io::Write;
            let log_file = std::path::Path::new("/tmp/synctui-debug.log");

            if let Some(ref clipboard_cmd) = self.clipboard_command {
                // Use user-configured clipboard command (text sent via stdin)
                // Spawn in background and write to stdin without waiting
                let result = std::process::Command::new(clipboard_cmd)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .and_then(|mut child| {
                        if let Some(mut stdin) = child.stdin.take() {
                            stdin.write_all(text.as_bytes())?;
                            // Close stdin to signal EOF
                            drop(stdin);
                        }
                        Ok(())
                    });

                match result {
                    Ok(_) => {
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(log_file)
                            .and_then(|mut f| {
                                writeln!(f, "Copied to clipboard via {}: {}", clipboard_cmd, text)
                            });
                        // Show toast notification with full path
                        let toast_msg = format!("Copied to clipboard: {}", text);
                        self.toast_message = Some((toast_msg, Instant::now()));
                    }
                    Err(e) => {
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(log_file)
                            .and_then(|mut f| {
                                writeln!(
                                    f,
                                    "ERROR: Failed to execute clipboard command '{}': {}",
                                    clipboard_cmd, e
                                )
                            });
                        // Show error toast
                        let toast_msg = format!("Error: Failed to copy with '{}'", clipboard_cmd);
                        self.toast_message = Some((toast_msg, Instant::now()));
                    }
                }
            } else {
                // No clipboard command configured - log message
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file)
                    .and_then(|mut f| {
                        writeln!(f, "No clipboard_command configured - set clipboard_command in config.yaml")
                    });
                // Show error toast
                self.toast_message = Some((
                    "Error: clipboard_command not configured".to_string(),
                    Instant::now(),
                ));
            }
        }

        Ok(())
    }

    async fn fetch_file_info_and_content(
        &mut self,
        folder_id: String,
        file_path: String,
        browse_item: BrowseItem,
    ) {
        // Find the folder
        let folder = match self.folders.iter().find(|f| f.id == folder_id) {
            Some(f) => f.clone(),
            None => {
                self.show_file_info = Some(FileInfoPopupState {
                    folder_id,
                    file_path,
                    browse_item,
                    file_details: None,
                    file_content: Err("Folder not found".to_string()),
                    exists_on_disk: false,
                    is_binary: false,
                    is_image: false,
                    scroll_offset: 0,
                    image_state: None,
                });
                return;
            }
        };

        // Check if file is an image
        let is_image = Self::is_image_file(&file_path);

        // Initialize popup state with loading message first
        self.show_file_info = Some(FileInfoPopupState {
            folder_id: folder_id.clone(),
            file_path: file_path.clone(),
            browse_item: browse_item.clone(),
            file_details: None,
            file_content: Err("Loading...".to_string()),
            exists_on_disk: false,
            is_binary: false,
            is_image,
            scroll_offset: 0,
            image_state: if is_image {
                Some(ImagePreviewState::Loading)
            } else {
                None
            },
        });

        // 1. Fetch file details from API
        let file_details = self.client.get_file_info(&folder_id, &file_path).await.ok();

        // 2. If image, spawn background loading; otherwise read as text
        let (file_content, exists_on_disk, is_binary, image_state) = if is_image {
            // Translate path for image loading
            let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), file_path);
            let mut host_path = container_path.clone();
            for (container_prefix, host_prefix) in &self.path_map {
                if let Some(suffix) = container_path.strip_prefix(container_prefix) {
                    host_path = format!("{}{}", host_prefix, suffix);
                    break;
                }
            }

            let host_path_buf = std::path::PathBuf::from(&host_path);
            let exists = tokio::fs::metadata(&host_path_buf).await.is_ok();

            if exists && self.image_picker.is_some() {
                // Spawn background task to load image
                let picker = self.image_picker.as_ref().unwrap().clone();
                let image_tx = self.image_update_tx.clone();
                let image_file_path = file_path.clone();

                tokio::spawn(async move {
                    log_debug(&format!("Background: Loading image {}", image_file_path));
                    match Self::load_image_preview(host_path_buf, picker).await {
                        Ok((protocol, metadata)) => {
                            log_debug(&format!(
                                "Background: Image loaded successfully {}",
                                image_file_path
                            ));
                            let _ = image_tx.send((
                                image_file_path,
                                ImagePreviewState::Ready { protocol, metadata },
                            ));
                        }
                        Err(metadata) => {
                            log_debug(&format!(
                                "Background: Image load failed {}",
                                image_file_path
                            ));
                            let _ = image_tx
                                .send((image_file_path, ImagePreviewState::Failed { metadata }));
                        }
                    }
                });

                // Return loading state immediately
                (
                    Ok("Loading image preview...".to_string()),
                    true,
                    true,
                    Some(ImagePreviewState::Loading),
                )
            } else if !exists {
                (
                    Err("File not found on disk".to_string()),
                    false,
                    false,
                    Some(ImagePreviewState::Failed {
                        metadata: ImageMetadata {
                            dimensions: None,
                            format: Some("File not found".to_string()),
                            file_size: 0,
                        },
                    }),
                )
            } else {
                (
                    Err("Image preview disabled".to_string()),
                    true,
                    true,
                    Some(ImagePreviewState::Failed {
                        metadata: ImageMetadata {
                            dimensions: None,
                            format: Some("Image preview disabled in config".to_string()),
                            file_size: 0,
                        },
                    }),
                )
            }
        } else {
            // Read as text
            let (content, exists, binary) =
                Self::read_file_content_static(&self.path_map, &folder, &file_path).await;
            (content, exists, binary, None)
        };

        // 3. Update popup state with results
        self.show_file_info = Some(FileInfoPopupState {
            folder_id,
            file_path,
            browse_item,
            file_details,
            file_content,
            exists_on_disk,
            is_binary,
            is_image,
            scroll_offset: 0,
            image_state,
        });
    }

    fn is_image_file(path: &str) -> bool {
        let path_lower = path.to_lowercase();
        path_lower.ends_with(".png")
            || path_lower.ends_with(".jpg")
            || path_lower.ends_with(".jpeg")
            || path_lower.ends_with(".gif")
            || path_lower.ends_with(".bmp")
            || path_lower.ends_with(".webp")
            || path_lower.ends_with(".tiff")
            || path_lower.ends_with(".tif")
    }

    async fn load_image_preview(
        host_path: std::path::PathBuf,
        picker: ratatui_image::picker::Picker,
    ) -> Result<(ratatui_image::protocol::StatefulProtocol, ImageMetadata), ImageMetadata> {
        let max_size_bytes = 20 * 1024 * 1024; // 20MB limit

        // Check file size
        let metadata = match tokio::fs::metadata(&host_path).await {
            Ok(m) => m,
            Err(_e) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: None,
                    file_size: 0,
                });
            }
        };

        let file_size = metadata.len();
        if file_size > max_size_bytes {
            return Err(ImageMetadata {
                dimensions: None,
                format: Some("Too large".to_string()),
                file_size,
            });
        }

        // Load image
        let img_result = tokio::task::spawn_blocking(move || image::open(&host_path)).await;

        let img = match img_result {
            Ok(Ok(img)) => img,
            Ok(Err(e)) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: Some(format!("Load error: {}", e)),
                    file_size,
                });
            }
            Err(e) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: Some(format!("Task error: {}", e)),
                    file_size,
                });
            }
        };

        // Extract metadata (original dimensions)
        let dimensions = (img.width(), img.height());
        let format = match img.color() {
            image::ColorType::L8 => "Grayscale 8-bit",
            image::ColorType::La8 => "Grayscale+Alpha 8-bit",
            image::ColorType::Rgb8 => "RGB 8-bit",
            image::ColorType::Rgba8 => "RGBA 8-bit",
            image::ColorType::L16 => "Grayscale 16-bit",
            image::ColorType::La16 => "Grayscale+Alpha 16-bit",
            image::ColorType::Rgb16 => "RGB 16-bit",
            image::ColorType::Rgba16 => "RGBA 16-bit",
            image::ColorType::Rgb32F => "RGB 32-bit float",
            image::ColorType::Rgba32F => "RGBA 32-bit float",
            _ => "Unknown",
        };

        let load_start = std::time::Instant::now();
        log_debug(&format!(
            "Loading image: {}x{} pixels",
            img.width(),
            img.height()
        ));

        // Pre-downscale large images with adaptive quality/performance balance
        let font_size = picker.font_size();

        // Estimate maximum reasonable size: ~200 cells × ~60 cells (typical large terminal)
        // Use 1.25x headroom for quality (balanced for performance)
        let max_reasonable_width = 200 * font_size.0 as u32 * 5 / 4;
        let max_reasonable_height = 60 * font_size.1 as u32 * 5 / 4;

        let processed_img =
            if img.width() > max_reasonable_width || img.height() > max_reasonable_height {
                let scale_factor = (img.width() as f32 / max_reasonable_width as f32)
                    .max(img.height() as f32 / max_reasonable_height as f32);

                log_debug(&format!(
                    "Pre-downscaling {}x{} by {:.2}x to fit {}x{} for better quality",
                    img.width(),
                    img.height(),
                    scale_factor,
                    max_reasonable_width,
                    max_reasonable_height
                ));

                // Adaptive filter selection based on downscale amount
                let filter = if scale_factor > 4.0 {
                    // Extreme downscale (>4x): Use Triangle for speed
                    image::imageops::FilterType::Triangle
                } else if scale_factor > 2.0 {
                    // Large downscale (2-4x): Use CatmullRom for balance
                    image::imageops::FilterType::CatmullRom
                } else {
                    // Moderate downscale (<2x): Use Lanczos3 for quality
                    image::imageops::FilterType::Lanczos3
                };

                log_debug(&format!(
                    "Using {:?} filter for {:.2}x downscale",
                    filter, scale_factor
                ));
                let resize_start = std::time::Instant::now();
                let resized = img.resize(max_reasonable_width, max_reasonable_height, filter);
                log_debug(&format!(
                    "Resize took {:.2}s",
                    resize_start.elapsed().as_secs_f32()
                ));
                resized
            } else {
                img
            };

        log_debug(&format!("Creating protocol..."));
        let protocol_start = std::time::Instant::now();
        let protocol = picker.new_resize_protocol(processed_img);
        log_debug(&format!(
            "Protocol creation took {:.2}s",
            protocol_start.elapsed().as_secs_f32()
        ));
        log_debug(&format!(
            "Total image load took {:.2}s",
            load_start.elapsed().as_secs_f32()
        ));

        // Return both protocol and metadata
        let metadata = ImageMetadata {
            dimensions: Some(dimensions),
            format: Some(format.to_string()),
            file_size,
        };

        Ok((protocol, metadata))
    }

    async fn read_file_content_static(
        path_map: &HashMap<String, String>,
        folder: &Folder,
        relative_path: &str,
    ) -> (Result<String, String>, bool, bool) {
        const MAX_SIZE: u64 = 20 * 1024 * 1024; // 20MB
        const BINARY_CHECK_SIZE: usize = 8192; // First 8KB

        // Translate container path to host path
        let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), relative_path);
        let mut host_path = container_path.clone();

        // Try to map container path to host path using path_map
        for (container_prefix, host_prefix) in path_map {
            if container_path.starts_with(container_prefix) {
                let remainder = container_path.strip_prefix(container_prefix).unwrap_or("");
                host_path = format!("{}{}", host_prefix.trim_end_matches('/'), remainder);
                break;
            }
        }

        // Check if file exists
        let metadata = match tokio::fs::metadata(&host_path).await {
            Ok(m) => m,
            Err(_) => return (Err("File not found on disk".to_string()), false, false),
        };

        let exists = true;

        // Check if it's a directory
        if metadata.is_dir() {
            return (Ok("[Directory]".to_string()), exists, false);
        }

        // Check file size
        if metadata.len() > MAX_SIZE {
            return (
                Err(format!(
                    "File too large ({}) - max 20MB",
                    utils::format_bytes(metadata.len())
                )),
                exists,
                false,
            );
        }

        // Read file content
        match tokio::fs::read(&host_path).await {
            Ok(bytes) => {
                // Check if binary (null bytes in first 8KB)
                let check_size = std::cmp::min(bytes.len(), BINARY_CHECK_SIZE);
                let is_binary = bytes[..check_size].contains(&0);

                if is_binary {
                    // Attempt text extraction (similar to 'strings' command)
                    let extracted = Self::extract_text_from_binary(&bytes);
                    (Ok(extracted), exists, true)
                } else {
                    // Try to decode as UTF-8
                    match String::from_utf8(bytes.clone()) {
                        Ok(content) => (Ok(content), exists, false),
                        Err(_) => {
                            // Try lossy conversion
                            let content = String::from_utf8_lossy(&bytes).to_string();
                            (Ok(content), exists, true)
                        }
                    }
                }
            }
            Err(e) => (Err(format!("Failed to read file: {}", e)), exists, false),
        }
    }

    fn extract_text_from_binary(bytes: &[u8]) -> String {
        // Extract printable ASCII strings (similar to 'strings' command)
        let mut result = String::new();
        let mut current_string = String::new();
        const MIN_STRING_LENGTH: usize = 4;

        for &byte in bytes {
            if (32..=126).contains(&byte) || byte == b'\n' || byte == b'\t' {
                current_string.push(byte as char);
            } else {
                if current_string.len() >= MIN_STRING_LENGTH {
                    result.push_str(&current_string);
                    result.push('\n');
                }
                current_string.clear();
            }
        }

        if current_string.len() >= MIN_STRING_LENGTH {
            result.push_str(&current_string);
        }

        if result.is_empty() {
            result = "[Binary file - no readable text found]".to_string();
        } else {
            result = format!("[Binary file - extracted text]\n\n{}", result);
        }

        result
    }

    async fn toggle_ignore(&mut self) -> Result<()> {
        log_bug("toggle_ignore: START");

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
        let prefix = level.prefix.clone();
        log_bug(&format!(
            "toggle_ignore: folder={} states={}",
            folder_id,
            level.file_sync_states.len()
        ));

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
        let sync_state = level
            .file_sync_states
            .get(&item.name)
            .copied()
            .unwrap_or(SyncState::Unknown);
        log_bug(&format!(
            "toggle_ignore: item={} state={:?}",
            item_name, sync_state
        ));

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
            let level = &self.breadcrumb_trail[level_idx];
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
                self.toast_message = Some((message, Instant::now()));
                log_debug(&format!("Blocked un-ignore: path {:?} is pending deletion", pending_path));
                return Ok(());
            }

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

                self.client
                    .set_ignore_patterns(&folder_id, updated_patterns)
                    .await?;
                log_bug("toggle_ignore: updated .stignore");

                // Don't add optimistic update for unignore - the final state is unpredictable
                // (could be Synced, RemoteOnly, OutOfSync, LocalOnly, or Syncing)
                // Let the FileInfo API response provide the correct state

                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                    // Clear Ignored state (will be updated by FileInfo)
                    level.file_sync_states.remove(&item_name);

                    // Update ignored_exists (file is no longer ignored)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Unknown);

                    log_bug(&format!(
                        "toggle_ignore: cleared {} state (un-ignoring), no optimistic update",
                        item_name
                    ));
                }

                // Trigger rescan in background - ItemStarted/ItemFinished events will update state
                // Also fetch file info after delay as fallback (for files that don't need syncing)
                let client = self.client.clone();
                let folder_id_clone = folder_id.clone();
                let file_path_for_api = if let Some(ref prefix) = prefix {
                    format!("{}/{}", prefix.trim_matches('/'), item_name)
                } else {
                    item_name.clone()
                };
                let api_tx = self.api_tx.clone();

                tokio::spawn(async move {
                    // Wait a moment for Syncthing to process the .stignore change
                    log_bug(
                        "toggle_ignore: waiting 200ms for Syncthing to process .stignore change",
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                    // Now trigger rescan
                    log_bug(&format!(
                        "toggle_ignore: calling rescan for folder={}",
                        folder_id_clone
                    ));
                    match client.rescan_folder(&folder_id_clone).await {
                        Ok(_) => log_bug("toggle_ignore: rescan completed successfully"),
                        Err(e) => log_bug(&format!("toggle_ignore: rescan FAILED: {:?}", e)),
                    }

                    // Wait longer for ItemStarted event (for files that need syncing)
                    // Syncthing needs time to discover file, calculate hashes, start transfer
                    log_bug("toggle_ignore: waiting 3 seconds for ItemStarted event");
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                    log_bug(&format!(
                        "toggle_ignore: requesting file info for {}",
                        file_path_for_api
                    ));
                    // Fetch file info as fallback (for files already synced, no ItemStarted will fire)
                    let _ = api_tx.send(api_service::ApiRequest::GetFileInfo {
                        folder_id: folder_id_clone,
                        file_path: file_path_for_api,
                        priority: api_service::Priority::Medium,
                    });
                });
            } else {
                // Multiple patterns match - show selection menu
                let mut selection_state = ListState::default();
                selection_state.select(Some(0));
                self.pattern_selection =
                    Some((folder_id, item_name, matching_patterns, selection_state));
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

            self.client
                .set_ignore_patterns(&folder_id, updated_patterns)
                .await?;

            // Immediately mark as ignored in UI
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
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

                log_bug(&format!(
                    "toggle_ignore: set {} to Ignored",
                    item_name
                ));

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

        // Log HashMap size when returning
        if !self.breadcrumb_trail.is_empty() {
            log_bug(&format!(
                "toggle_ignore: END - HashMap now has {} states",
                self.breadcrumb_trail[0].file_sync_states.len()
            ));
        } else {
            log_bug("toggle_ignore: END");
        }
        Ok(())
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

                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
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
                    log_debug(&format!("Warning: File still exists after deletion: {}", host_path));
                    // Keep in pending set for safety
                } else {
                    log_debug(&format!("Successfully deleted file: {}", host_path));
                }

                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                    level
                        .file_sync_states
                        .insert(item_name.clone(), SyncState::Ignored);

                    // Update ignored_exists (file is now ignored and deleted)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Ignored);
                }

                // Mark that rescan has been triggered for this folder
                if let Some(pending_info) = self.pending_ignore_deletes.get_mut(&folder_id) {
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

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Update last user action timestamp for idle detection
        self.last_user_action = Instant::now();

        // Handle confirmation prompts first
        if let Some((folder_id, _)) = &self.confirm_revert {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - revert the folder
                    let folder_id = folder_id.clone();
                    self.confirm_revert = None;
                    let _ = self.client.revert_folder(&folder_id).await;

                    // Refresh statuses in background (non-blocking)
                    self.refresh_folder_statuses_nonblocking();

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
                        // Get current folder info for cache invalidation
                        if self.focus_level > 0 && !self.breadcrumb_trail.is_empty() {
                            let level_idx = self.focus_level - 1;

                            // Extract all needed data first (immutable borrow)
                            let deletion_info =
                                if let Some(level) = self.breadcrumb_trail.get(level_idx) {
                                    let selected_idx = level.state.selected();
                                    selected_idx.and_then(|idx| {
                                        level.items.get(idx).map(|item| {
                                            (
                                                level.folder_id.clone(),
                                                item.name.clone(),
                                                level.prefix.clone(),
                                                idx,
                                            )
                                        })
                                    })
                                } else {
                                    None
                                };

                            // Now do the mutations
                            if let Some((folder_id, item_name, prefix, idx)) = deletion_info {
                                // Build file path for cache invalidation
                                let file_path = if let Some(ref prefix) = prefix {
                                    format!("{}{}", prefix, item_name)
                                } else {
                                    item_name.clone()
                                };

                                // Invalidate cache for this file/directory
                                if is_dir {
                                    let _ = self.cache.invalidate_directory(&folder_id, &file_path);
                                } else {
                                    let _ =
                                        self.cache.invalidate_single_file(&folder_id, &file_path);
                                }

                                // Immediately remove from current view (mutable borrow)
                                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                                    // Remove from items
                                    if idx < level.items.len() {
                                        level.items.remove(idx);
                                    }
                                    // Remove from sync states
                                    level.file_sync_states.remove(&item_name);

                                    // Adjust selection
                                    let new_selection = if level.items.is_empty() {
                                        None
                                    } else if idx >= level.items.len() {
                                        Some(level.items.len() - 1)
                                    } else {
                                        Some(idx)
                                    };
                                    level.state.select(new_selection);
                                }

                                // Invalidate browse cache for this directory
                                let browse_key =
                                    format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
                                self.loading_browse.remove(&browse_key);
                            }
                        }

                        // Trigger rescan after successful deletion
                        let _ = self.rescan_selected_folder();
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
        if let Some((folder_id, item_name, patterns, state)) = &mut self.pattern_selection {
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
                        let item_name = item_name.clone();
                        self.pattern_selection = None;

                        // Get all patterns and remove the selected one
                        let all_patterns = self.client.get_ignore_patterns(&folder_id).await?;
                        let updated_patterns: Vec<String> = all_patterns
                            .into_iter()
                            .filter(|p| p != &pattern_to_remove)
                            .collect();

                        self.client
                            .set_ignore_patterns(&folder_id, updated_patterns)
                            .await?;
                        log_bug("pattern_selection: updated .stignore");

                        // Immediately show as Unknown to give user feedback
                        if self.focus_level > 0 && self.focus_level <= self.breadcrumb_trail.len() {
                            let level_idx = self.focus_level - 1;
                            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                                level
                                    .file_sync_states
                                    .insert(item_name.clone(), SyncState::Unknown);

                                // Update ignored_exists (file is no longer ignored) - do it inline to avoid borrow issues
                                level.ignored_exists.remove(&item_name);

                                // Don't add optimistic update for unignore - final state is unpredictable
                                log_bug(&format!(
                                    "pattern_selection: cleared {} state (un-ignoring), no optimistic update",
                                    item_name
                                ));
                            }
                        }

                        // Wait for Syncthing to process .stignore change before rescanning
                        log_bug("pattern_selection: waiting 200ms for Syncthing to process .stignore change");
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                        // Trigger rescan - ItemStarted/ItemFinished events will update state
                        // Also fetch file info after delay as fallback (for files that don't need syncing)
                        log_bug(&format!(
                            "pattern_selection: calling rescan for folder={}",
                            folder_id
                        ));
                        self.client.rescan_folder(&folder_id).await?;
                        log_bug("pattern_selection: rescan completed");

                        let folder_id_clone = folder_id.clone();
                        let api_tx = self.api_tx.clone();
                        let item_name_clone = item_name.clone();

                        let file_path_for_api = if self.focus_level > 0
                            && self.focus_level <= self.breadcrumb_trail.len()
                        {
                            let level_idx = self.focus_level - 1;
                            if let Some(level) = self.breadcrumb_trail.get(level_idx) {
                                if let Some(ref prefix) = level.prefix {
                                    format!("{}/{}", prefix.trim_matches('/'), &item_name_clone)
                                } else {
                                    item_name_clone.clone()
                                }
                            } else {
                                item_name_clone.clone()
                            }
                        } else {
                            item_name_clone
                        };

                        tokio::spawn(async move {
                            // Wait longer for ItemStarted event to potentially fire
                            // Syncthing needs time to discover file, calculate hashes, start transfer
                            log_bug("pattern_selection: waiting 3 seconds for ItemStarted event");
                            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                            log_bug(&format!(
                                "pattern_selection: requesting file info for {}",
                                file_path_for_api
                            ));
                            // Fetch file info as fallback
                            let _ = api_tx.send(api_service::ApiRequest::GetFileInfo {
                                folder_id: folder_id_clone,
                                file_path: file_path_for_api,
                                priority: api_service::Priority::Medium,
                            });
                        });
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

        // Handle file info popup
        if let Some(popup_state) = &mut self.show_file_info {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => {
                    // Close popup and trigger sixel cleanup if it was an image (terminal.clear once)
                    if popup_state.is_image {
                        self.sixel_cleanup_frames = 1;
                    }
                    self.show_file_info = None;
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    // Scroll down
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(1);
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    // Scroll up
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(1);
                    return Ok(());
                }
                KeyCode::PageDown => {
                    // Scroll down by page (10 lines)
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(10);
                    return Ok(());
                }
                KeyCode::PageUp => {
                    // Scroll up by page (10 lines)
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(10);
                    return Ok(());
                }
                // Vim keybindings for scrolling
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl-d: Half page down
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(10);
                    return Ok(());
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl-u: Half page up
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(10);
                    return Ok(());
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl-f: Full page down
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(20);
                    return Ok(());
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl-b: Full page up
                    popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(20);
                    return Ok(());
                }
                KeyCode::Char('g') => {
                    // First 'g' in 'gg' sequence - need to track this
                    if self.last_key_was_g {
                        // This is the second 'g' - go to top
                        popup_state.scroll_offset = 0;
                        self.last_key_was_g = false;
                    } else {
                        // First 'g' - wait for second one
                        self.last_key_was_g = true;
                    }
                    return Ok(());
                }
                KeyCode::Char('G') => {
                    // Go to bottom (set to a very large number, will be clamped by rendering)
                    popup_state.scroll_offset = u16::MAX;
                    self.last_key_was_g = false;
                    return Ok(());
                }
                _ => {
                    // Reset 'gg' sequence on any other key
                    self.last_key_was_g = false;
                    // Ignore other keys while popup is showing
                    return Ok(());
                }
            }
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => {
                // Rescan the selected/current folder
                let _ = self.rescan_selected_folder();
            }
            KeyCode::Char('R') => {
                // Restore selected file (if remote-only/deleted locally)
                let _ = self.restore_selected_file().await;
            }
            // Vim keybindings with Ctrl modifiers (check before 'd' and other letters)
            KeyCode::Char('d')
                if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.last_key_was_g = false;
                self.half_page_down(20).await; // Use reasonable default, will be more precise with frame height
            }
            KeyCode::Char('u')
                if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.last_key_was_g = false;
                self.half_page_up(20).await;
            }
            KeyCode::Char('f')
                if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.last_key_was_g = false;
                self.page_down(40).await;
            }
            KeyCode::Char('b')
                if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.last_key_was_g = false;
                self.page_up(40).await;
            }
            KeyCode::Char('d') => {
                // Flush pending writes before destructive operation
                self.flush_pending_db_writes();
                // Delete file from disk (with confirmation)
                let _ = self.delete_file().await;
            }
            KeyCode::Char('i') => {
                // Toggle ignore state (add or remove from .stignore)
                let _ = self.toggle_ignore().await;
            }
            KeyCode::Char('I') => {
                // Flush pending writes before destructive operation
                self.flush_pending_db_writes();
                // Ignore file AND delete from disk
                let _ = self.ignore_and_delete().await;
            }
            KeyCode::Char('o') => {
                // Open file/directory with configured command
                let _ = self.open_selected_item();
            }
            KeyCode::Char('c') => {
                // Copy folder ID (folders) or file/directory path (breadcrumbs)
                let _ = self.copy_to_clipboard();
            }
            KeyCode::Char('s') => {
                // Cycle through sort modes
                self.cycle_sort_mode();
            }
            KeyCode::Char('S') => {
                // Toggle reverse sort order
                self.toggle_sort_reverse();
            }
            KeyCode::Char('t') => {
                // Cycle through display modes: Off -> TimestampOnly -> TimestampAndSize -> Off
                self.display_mode = self.display_mode.next();
            }
            KeyCode::Char('?') if self.focus_level > 0 => {
                // Toggle file information popup
                if let Some(popup_state) = &self.show_file_info {
                    // Close popup and trigger sixel cleanup if it was an image (terminal.clear once)
                    if popup_state.is_image {
                        self.sixel_cleanup_frames = 1;
                    }
                    self.show_file_info = None;
                } else {
                    // Open popup for selected item
                    if let Some(level) = self.breadcrumb_trail.get(self.focus_level - 1) {
                        if let Some(selected_idx) = level.state.selected() {
                            if let Some(item) = level.items.get(selected_idx) {
                                // Construct full path
                                let file_path = if let Some(prefix) = &level.prefix {
                                    format!("{}{}", prefix, item.name)
                                } else {
                                    item.name.clone()
                                };

                                // Fetch file info and content (await since it's async)
                                self.fetch_file_info_and_content(
                                    level.folder_id.clone(),
                                    file_path,
                                    item.clone(),
                                )
                                .await;
                            }
                        }
                    }
                }
            }
            // Vim keybindings
            KeyCode::Char('h') if self.vim_mode => {
                self.last_key_was_g = false;
                self.go_back();
            }
            KeyCode::Char('j') if self.vim_mode => {
                self.last_key_was_g = false;
                self.next_item().await;
            }
            KeyCode::Char('k') if self.vim_mode => {
                self.last_key_was_g = false;
                self.previous_item().await;
            }
            KeyCode::Char('l') if self.vim_mode => {
                self.last_key_was_g = false;
                if self.focus_level == 0 {
                    self.load_root_level(false).await?; // Not preview - actually enter folder
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Char('g') if self.vim_mode => {
                if self.last_key_was_g {
                    // gg - jump to first
                    self.jump_to_first().await;
                    self.last_key_was_g = false;
                } else {
                    // First 'g' press
                    self.last_key_was_g = true;
                }
            }
            KeyCode::Char('G') if self.vim_mode => {
                self.last_key_was_g = false;
                self.jump_to_last().await;
            }
            // Standard navigation keys (not advertised)
            KeyCode::PageDown => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.page_down(40).await;
            }
            KeyCode::PageUp => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.page_up(40).await;
            }
            KeyCode::Home => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.jump_to_first().await;
            }
            KeyCode::End => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.jump_to_last().await;
            }
            KeyCode::Left | KeyCode::Backspace => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                // Flush before navigation to save state
                self.flush_pending_db_writes();
                self.go_back();
            }
            KeyCode::Right | KeyCode::Enter => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                // Flush before navigation to save state
                self.flush_pending_db_writes();
                if self.focus_level == 0 {
                    self.load_root_level(false).await?; // Not preview - actually enter folder
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Up => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.previous_item().await;
            }
            KeyCode::Down => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.next_item().await;
            }
            _ => {
                // Reset last_key_was_g on any other key
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
            }
        }
        Ok(())
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
    BUG_MODE.store(args.bug, Ordering::Relaxed);

    if args.debug {
        log_debug("Debug mode enabled");
    }

    if args.bug {
        log_bug("Bug mode enabled - logging to /tmp/synctui-bug.log");
    }

    // Determine config file path
    let config_path = get_config_path(args.config)?;

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
    let mut last_stats_update = Instant::now();

    loop {
        // Clear terminal to remove sixel graphics if needed (brief flash but necessary)
        if app.sixel_cleanup_frames > 0 {
            terminal.clear()?;
            app.sixel_cleanup_frames = 0;
        }

        // Conditional rendering based on dirty flag
        if app.ui_dirty {
            terminal.draw(|f| {
                ui::render(f, app);
            })?;
            app.ui_dirty = false;
        }

        // Auto-dismiss toast after 1.5 seconds
        if let Some((_, timestamp)) = app.toast_message {
            if timestamp.elapsed().as_millis() >= 1500 {
                app.toast_message = None;
                app.ui_dirty = true; // Toast changed, need redraw
            }
        }

        if app.should_quit {
            // Flush any pending writes before quitting
            app.flush_pending_db_writes();
            break;
        }

        // Process API responses (non-blocking)
        while let Ok(response) = app.api_rx.try_recv() {
            app.handle_api_response(response);
            app.ui_dirty = true; // API response may have updated state
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
            app.ui_dirty = true; // Cache invalidation may affect displayed data
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
            // Update popup if it's still showing the same file
            if let Some(ref mut popup_state) = app.show_file_info {
                if popup_state.file_path == file_path {
                    log_debug(&format!("Updating image state for {}", file_path));
                    popup_state.image_state = Some(image_state);

                    // Also update file_content to reflect loaded state
                    match &popup_state.image_state {
                        Some(ImagePreviewState::Ready { .. }) => {
                            popup_state.file_content =
                                Ok("[Image preview - see right panel]".to_string());
                        }
                        Some(ImagePreviewState::Failed { .. }) => {
                            popup_state.file_content = Err("Image preview unavailable".to_string());
                        }
                        _ => {}
                    }
                }
            }
        }

        // NOTE: Removed periodic status polling - we now rely on events for cache invalidation
        // Status updates now only happen:
        // 1. On app startup (initial load)
        // 2. After user-initiated rescan (to get updated sequence)

        // Refresh device/system status periodically (less frequently than folder stats)
        // System status every 30 seconds, connection stats every 2-3 seconds
        if app.last_system_status_update.elapsed() >= std::time::Duration::from_secs(30) {
            let _ = app.api_tx.send(api_service::ApiRequest::GetSystemStatus);
            app.last_system_status_update = Instant::now();
        }

        if app.last_connection_stats_fetch.elapsed() >= std::time::Duration::from_millis(5000) {
            let _ = app.api_tx.send(api_service::ApiRequest::GetConnectionStats);
            app.last_connection_stats_fetch = Instant::now();
        }

        // Update UI periodically for live stats (uptime, transfer rates)
        if last_stats_update.elapsed() >= std::time::Duration::from_secs(1) {
            app.ui_dirty = true;
            last_stats_update = Instant::now();
        }

        // Only run prefetch operations when user has been idle for 300ms
        // This prevents blocking keyboard input and reduces CPU usage
        let idle_time = app.last_user_action.elapsed();
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
            if app.focus_level > 0 && !app.breadcrumb_trail.is_empty() {
                app.update_directory_states(app.focus_level - 1);
            }
        }

        // Increased poll timeout from 100ms to 250ms to reduce CPU usage when idle
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Flush before processing user input to ensure consistency
                app.flush_pending_db_writes();
                app.handle_key(key).await?;
                app.ui_dirty = true; // User input likely changed state
            }
        }
    }

    Ok(())
}
