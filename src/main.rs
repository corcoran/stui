use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
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
use std::{collections::{HashMap, HashSet}, fs, io, sync::atomic::{AtomicBool, Ordering}, time::Instant};
use unicode_width::UnicodeWidthStr;

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

    /// Path to config file (default: ~/.config/synctui/config.yaml)
    #[arg(short, long)]
    config: Option<String>,
}

// Global flag for debug mode
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

mod api;
mod api_service;
mod cache;
mod config;
mod event_listener;
mod icons;

use api::{BrowseItem, ConnectionStats, Folder, FolderStatus, SyncState, SyncthingClient, SystemStatus};
use cache::CacheDb;
use config::Config;
use icons::{FolderState, IconMode, IconRenderer, IconTheme};

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

fn sync_state_priority(state: SyncState) -> u8 {
    // Lower number = higher priority (displayed first)
    match state {
        SyncState::OutOfSync => 0,   // ⚠️ Most important
        SyncState::RemoteOnly => 1,  // ☁️
        SyncState::LocalOnly => 2,   // 💻
        SyncState::Ignored => 3,     // 🚫
        SyncState::Unknown => 4,     // ❓
        SyncState::Synced => 5,      // ✅ Least important
    }
}

fn format_timestamp(timestamp: &str) -> String {
    // Parse ISO timestamp and format as human-readable
    // Input format: "2025-10-26T20:58:21.580021398Z"
    // Output format: "2025-10-26 20:58"
    if timestamp.is_empty() {
        return String::new();
    }

    // Try to parse and format
    if let Some(datetime_part) = timestamp.split('T').next() {
        if let Some(time_part) = timestamp.split('T').nth(1) {
            let time = time_part.split(':').take(2).collect::<Vec<_>>().join(":");
            return format!("{} {}", datetime_part, time);
        }
        return datetime_part.to_string();
    }

    timestamp.to_string()
}

fn format_human_size(size: u64) -> String {
    // Format size as human-readable, always 4 characters for alignment
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if size == 0 {
        return "   0".to_string();
    } else if size < KB {
        // Show raw bytes as digits (no 'B' suffix)
        return format!("{:>4}", size);
    } else if size < MB {
        let kb = size as f64 / KB as f64;
        if kb < 10.0 {
            return format!("{:.1}K", kb);
        } else {
            return format!("{:>3}K", (size / KB));
        }
    } else if size < GB {
        let mb = size as f64 / MB as f64;
        if mb < 10.0 {
            return format!("{:.1}M", mb);
        } else {
            return format!("{:>3}M", (size / MB));
        }
    } else if size < TB {
        let gb = size as f64 / GB as f64;
        if gb < 10.0 {
            return format!("{:.1}G", gb);
        } else {
            return format!("{:>3}G", (size / GB));
        }
    } else {
        let tb = size as f64 / TB as f64;
        if tb < 10.0 {
            return format!("{:.1}T", tb);
        } else {
            return format!("{:>3}T", (size / TB));
        }
    }
}

fn format_uptime(seconds: u64) -> String {
    // Format uptime as "3d 15h" or "15h 44m" or "44m 30s"
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn format_transfer_rate(bytes_per_sec: f64) -> String {
    // Format transfer rate as human-readable (B/s, KB/s, MB/s, GB/s)
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes_per_sec < KB {
        format!("{:.0}B/s", bytes_per_sec)
    } else if bytes_per_sec < MB {
        format!("{:.1}K/s", bytes_per_sec / KB)
    } else if bytes_per_sec < GB {
        format!("{:.1}M/s", bytes_per_sec / MB)
    } else {
        format!("{:.2}G/s", bytes_per_sec / GB)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
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
enum SortMode {
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

    fn as_str(&self) -> &str {
        match self {
            SortMode::VisualIndicator => "Icon",
            SortMode::Alphabetical => "A-Z",
            SortMode::LastModified => "Timestamp",
            SortMode::FileSize => "Size",
        }
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
    display_mode: DisplayMode, // Toggle for displaying timestamps and/or size
    vim_mode: bool, // Enable vim keybindings
    last_key_was_g: bool, // Track 'g' key for 'gg' command
    last_user_action: Instant, // Track last user interaction for idle detection
    sort_mode: SortMode,     // Current sort mode (session-wide)
    sort_reverse: bool,      // Whether to reverse sort order (session-wide)
    // Track in-flight operations to prevent duplicate fetches
    loading_browse: std::collections::HashSet<String>, // Set of "folder_id:prefix" currently being loaded
    loading_sync_states: std::collections::HashSet<String>, // Set of "folder_id:path" currently being loaded
    discovered_dirs: std::collections::HashSet<String>, // Set of "folder_id:prefix" already discovered (to prevent re-querying cache)
    prefetch_enabled: bool, // Flag to enable/disable prefetching when system is busy
    last_known_sequences: HashMap<String, u64>, // Track last known sequence per folder to detect changes
    last_known_receive_only_counts: HashMap<String, u64>, // Track receiveOnlyTotalItems to detect local-only file changes
    // Track last update info per folder (for display)
    last_folder_updates: HashMap<String, (std::time::SystemTime, String)>, // folder_id -> (timestamp, last_changed_file)
    // Confirmation prompt state
    confirm_revert: Option<(String, Vec<String>)>, // If Some, shows confirmation prompt for reverting (folder_id, changed_files)
    confirm_delete: Option<(String, String, bool)>, // If Some, shows confirmation prompt for deleting (host_path, display_name, is_dir)
    // Pattern selection menu for removing ignores
    pattern_selection: Option<(String, String, Vec<String>, ListState)>, // If Some, shows pattern selection menu (folder_id, item_name, patterns, selection_state)
    // API service channels
    api_tx: tokio::sync::mpsc::UnboundedSender<api_service::ApiRequest>,
    api_rx: tokio::sync::mpsc::UnboundedReceiver<api_service::ApiResponse>,
    // Event listener channels
    invalidation_rx: tokio::sync::mpsc::UnboundedReceiver<event_listener::CacheInvalidation>,
    event_id_rx: tokio::sync::mpsc::UnboundedReceiver<u64>,
    // Performance metrics
    last_load_time_ms: Option<u64>,  // Time to load current directory (milliseconds)
    cache_hit: Option<bool>,          // Whether last load was a cache hit
    // Device/System status
    system_status: Option<SystemStatus>,  // Device name, uptime, etc.
    connection_stats: Option<ConnectionStats>,  // Global transfer stats
    last_connection_stats: Option<(ConnectionStats, Instant)>,  // Previous stats + timestamp for rate calc
    device_name: Option<String>,      // Cached device name
    last_system_status_update: Instant,  // Track when we last fetched system status
    last_connection_stats_fetch: Instant,  // Track when we last fetched connection stats
    last_transfer_rates: Option<(f64, f64)>,  // Cached transfer rates (download, upload) in bytes/sec
    // Icon rendering
    icon_renderer: IconRenderer,      // Centralized icon renderer
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

    fn get_local_state_summary(&self) -> (u64, u64, u64) {
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

        // Spawn API service worker
        let (api_tx, api_rx) = api_service::spawn_api_service(client.clone());

        // Get last event ID from cache
        let last_event_id = cache.get_last_event_id().unwrap_or(0);

        // Create channels for event listener
        let (invalidation_tx, invalidation_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_id_tx, event_id_rx) = tokio::sync::mpsc::unbounded_channel();

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
            display_mode: DisplayMode::TimestampAndSize, // Start with most info
            vim_mode: config.vim_mode,
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
            icon_renderer,
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

                        // Clear in-memory sync states for this folder if we're currently viewing it
                        // This ensures files that changed get refreshed
                        if !self.breadcrumb_trail.is_empty() && self.breadcrumb_trail[0].folder_id == folder.id {
                            for level in &mut self.breadcrumb_trail {
                                if level.folder_id == folder.id {
                                    level.file_sync_states.clear();
                                }
                            }
                        }
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
    fn handle_api_response(&mut self, response: api_service::ApiResponse) {
        use api_service::ApiResponse;

        match response {
            ApiResponse::BrowseResult { folder_id, prefix, items } => {
                // Mark browse as no longer loading
                let browse_key = format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
                self.loading_browse.remove(&browse_key);

                let Ok(mut items) = items else {
                    return; // Silently ignore errors
                };

                // Get folder sequence for cache
                let folder_sequence = self.folder_statuses
                    .get(&folder_id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                // Check if this folder has local changes and merge them synchronously
                let has_local_changes = self.folder_statuses
                    .get(&folder_id)
                    .map(|s| s.receive_only_total_items > 0)
                    .unwrap_or(false);

                let mut local_item_names = Vec::new();

                if has_local_changes {
                    log_debug(&format!("DEBUG [BrowseResult]: Folder has local changes, fetching..."));

                    // Block to wait for local items synchronously
                    let folder_id_clone = folder_id.clone();
                    let prefix_clone = prefix.clone();
                    let client = self.client.clone();

                    // Use block_in_place to run async code synchronously
                    let local_result = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            client.get_local_changed_items(&folder_id_clone, prefix_clone.as_deref()).await
                        })
                    });

                    if let Ok(local_items) = local_result {
                        log_debug(&format!("DEBUG [BrowseResult]: Fetched {} local items", local_items.len()));

                        // Merge local items
                        for local_item in local_items {
                            if !items.iter().any(|i| i.name == local_item.name) {
                                log_debug(&format!("DEBUG [BrowseResult]: Merging local item: {}", local_item.name));
                                local_item_names.push(local_item.name.clone());
                                items.push(local_item);
                            }
                        }
                    } else {
                        log_debug("DEBUG [BrowseResult]: Failed to fetch local items");
                    }
                }

                // Save merged items to cache
                let _ = self.cache.save_browse_items(&folder_id, prefix.as_deref(), &items, folder_sequence);

                // Update UI if this browse result matches current navigation
                if prefix.is_none() && self.focus_level == 1 && !self.breadcrumb_trail.is_empty() {
                    // Root level update
                    if self.breadcrumb_trail[0].folder_id == folder_id {
                        // Save currently selected item name BEFORE replacing items
                        let selected_name = self.breadcrumb_trail[0].state.selected()
                            .and_then(|idx| self.breadcrumb_trail[0].items.get(idx))
                            .map(|item| item.name.clone());

                        self.breadcrumb_trail[0].items = items.clone();

                        // Load sync states from cache
                        let mut sync_states = self.load_sync_states_from_cache(&folder_id, &items, None);

                        // Mark local-only items with LocalOnly sync state
                        for local_item_name in &local_item_names {
                            sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                        }

                        self.breadcrumb_trail[0].file_sync_states = sync_states;

                        // Sort and restore selection using the saved name
                        self.sort_level_with_selection(0, selected_name);
                    }
                } else if let Some(ref target_prefix) = prefix {
                    // Load sync states first (before mutable borrow)
                    let mut sync_states = self.load_sync_states_from_cache(&folder_id, &items, Some(target_prefix));

                    // Mark local-only items with LocalOnly sync state
                    for local_item_name in &local_item_names {
                        sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                    }

                    // Check if this matches a breadcrumb level
                    for (idx, level) in self.breadcrumb_trail.iter_mut().enumerate() {
                        if level.folder_id == folder_id && level.prefix.as_ref() == Some(target_prefix) {
                            // Save currently selected item name BEFORE replacing items
                            let selected_name = level.state.selected()
                                .and_then(|sel_idx| level.items.get(sel_idx))
                                .map(|item| item.name.clone());

                            level.items = items.clone();
                            level.file_sync_states = sync_states.clone();

                            // Sort and restore selection using the saved name
                            self.sort_level_with_selection(idx, selected_name);
                            break;
                        }
                    }
                }
            }

            ApiResponse::FileInfoResult { folder_id, file_path, details } => {
                // Mark as no longer loading
                let sync_key = format!("{}:{}", folder_id, file_path);
                self.loading_sync_states.remove(&sync_key);

                let Ok(file_details) = details else {
                    log_debug(&format!("DEBUG [FileInfoResult ERROR]: folder={} path={} error={:?}", folder_id, file_path, details.err()));
                    return; // Silently ignore errors
                };

                let file_sequence = file_details.local.as_ref()
                    .or(file_details.global.as_ref())
                    .map(|f| f.sequence)
                    .unwrap_or(0);

                let state = file_details.determine_sync_state();
                log_debug(&format!("DEBUG [FileInfoResult]: folder={} path={} state={:?} seq={}", folder_id, file_path, state, file_sequence));
                let _ = self.cache.save_sync_state(&folder_id, &file_path, state, file_sequence);

                // Update UI if this file is visible in current level
                let mut updated = false;
                for level in &mut self.breadcrumb_trail {
                    if level.folder_id == folder_id {
                        // Check if this file path belongs to this level
                        let level_prefix = level.prefix.as_deref().unwrap_or("");
                        log_debug(&format!("DEBUG [FileInfoResult UI update]: checking level with prefix={:?}", level_prefix));
                        if file_path.starts_with(level_prefix) {
                            let item_name = file_path.strip_prefix(level_prefix).unwrap_or(&file_path);
                            log_debug(&format!("DEBUG [FileInfoResult UI update]: MATCH! Updating item_name={} to state={:?}", item_name, state));
                            level.file_sync_states.insert(item_name.to_string(), state);
                            updated = true;
                        } else {
                            log_debug(&format!("DEBUG [FileInfoResult UI update]: NO MATCH - file_path={} doesn't start with level_prefix={}", file_path, level_prefix));
                        }
                    }
                }
                if !updated {
                    log_debug(&format!("DEBUG [FileInfoResult UI update]: WARNING - No matching level found for folder={} path={}", folder_id, file_path));
                }
            }

            ApiResponse::FolderStatusResult { folder_id, status } => {
                let Ok(status) = status else {
                    return; // Silently ignore errors
                };

                let sequence = status.sequence;
                let receive_only_count = status.receive_only_total_items;

                // Check if sequence changed
                if let Some(&last_seq) = self.last_known_sequences.get(&folder_id) {
                    if last_seq != sequence {
                        // Sequence changed! Invalidate cached data for this folder
                        let _ = self.cache.invalidate_folder(&folder_id);

                        // Clear in-memory sync states for this folder
                        if !self.breadcrumb_trail.is_empty() && self.breadcrumb_trail[0].folder_id == folder_id {
                            for level in &mut self.breadcrumb_trail {
                                if level.folder_id == folder_id {
                                    level.file_sync_states.clear();
                                }
                            }
                        }

                        // Clear discovered directories for this folder (so they get re-discovered with new sequence)
                        self.discovered_dirs.retain(|key| !key.starts_with(&format!("{}:", folder_id)));
                    }
                }

                // Check if receive-only item count changed (indicates local-only files added/removed)
                if let Some(&last_count) = self.last_known_receive_only_counts.get(&folder_id) {
                    if last_count != receive_only_count {
                        log_debug(&format!("DEBUG [FolderStatusResult]: receiveOnlyTotalItems changed from {} to {} for folder={}", last_count, receive_only_count, folder_id));

                        // Trigger refresh for currently viewed directory
                        if !self.breadcrumb_trail.is_empty() && self.breadcrumb_trail[0].folder_id == folder_id {
                            for level in &mut self.breadcrumb_trail {
                                if level.folder_id == folder_id {
                                    let browse_key = format!("{}:{}", folder_id, level.prefix.as_deref().unwrap_or(""));
                                    if !self.loading_browse.contains(&browse_key) {
                                        self.loading_browse.insert(browse_key);

                                        let _ = self.api_tx.send(api_service::ApiRequest::BrowseFolder {
                                            folder_id: folder_id.clone(),
                                            prefix: level.prefix.clone(),
                                            priority: api_service::Priority::High,
                                        });

                                        log_debug(&format!("DEBUG [FolderStatusResult]: Triggered browse refresh for prefix={:?}", level.prefix));
                                    }
                                }
                            }
                        }
                    }
                }

                // Update last known values
                self.last_known_sequences.insert(folder_id.clone(), sequence);
                self.last_known_receive_only_counts.insert(folder_id.clone(), receive_only_count);

                // Save and use fresh status
                let _ = self.cache.save_folder_status(&folder_id, &status, sequence);
                self.folder_statuses.insert(folder_id, status);
            }

            ApiResponse::RescanResult { folder_id, success, error } => {
                if success {
                    log_debug(&format!("DEBUG [RescanResult]: Successfully rescanned folder={}, requesting immediate status update", folder_id));

                    // Immediately request folder status to detect sequence changes
                    // This makes the rescan feel more responsive
                    let _ = self.api_tx.send(api_service::ApiRequest::GetFolderStatus {
                        folder_id,
                    });
                } else {
                    log_debug(&format!("DEBUG [RescanResult ERROR]: Failed to rescan folder={} error={:?}", folder_id, error));
                }
            }

            ApiResponse::SystemStatusResult { status } => {
                match status {
                    Ok(sys_status) => {
                        log_debug(&format!("DEBUG [SystemStatusResult]: Received system status, uptime={}", sys_status.uptime));
                        self.system_status = Some(sys_status);
                    }
                    Err(e) => {
                        log_debug(&format!("DEBUG [SystemStatusResult ERROR]: {}", e));
                    }
                }
            }

            ApiResponse::ConnectionStatsResult { stats } => {
                match stats {
                    Ok(conn_stats) => {
                        // Update current stats with new data
                        self.connection_stats = Some(conn_stats.clone());

                        // Calculate transfer rates if we have previous stats
                        if let Some((prev_stats, prev_instant)) = &self.last_connection_stats {
                            let elapsed = prev_instant.elapsed().as_secs_f64();
                            if elapsed > 0.0 {
                                let in_delta = (conn_stats.total.in_bytes_total as i64 - prev_stats.total.in_bytes_total as i64).max(0) as f64;
                                let out_delta = (conn_stats.total.out_bytes_total as i64 - prev_stats.total.out_bytes_total as i64).max(0) as f64;
                                let in_rate = in_delta / elapsed;
                                let out_rate = out_delta / elapsed;

                                // Store the calculated rates for UI display
                                self.last_transfer_rates = Some((in_rate, out_rate));
                            }

                            // Update baseline every ~10 seconds to prevent drift
                            if elapsed > 10.0 {
                                self.last_connection_stats = Some((conn_stats, Instant::now()));
                            }
                        } else {
                            // First fetch, store as baseline
                            self.last_connection_stats = Some((conn_stats, Instant::now()));
                            // Initialize rates to zero on first fetch
                            self.last_transfer_rates = Some((0.0, 0.0));
                        }
                    }
                    Err(e) => {
                        log_debug(&format!("DEBUG [ConnectionStatsResult ERROR]: {}", e));
                    }
                }
            }
        }
    }

    /// Handle cache invalidation messages from event listener
    fn handle_cache_invalidation(&mut self, invalidation: event_listener::CacheInvalidation) {
        match invalidation {
            event_listener::CacheInvalidation::File { folder_id, file_path } => {
                log_debug(&format!("DEBUG [Event]: Invalidating file: folder={} path={}", folder_id, file_path));
                let _ = self.cache.invalidate_single_file(&folder_id, &file_path);

                // Invalidate folder status cache to refresh receiveOnlyTotalItems count
                let _ = self.cache.invalidate_folder_status(&folder_id);

                // Request fresh folder status
                let _ = self.api_tx.send(api_service::ApiRequest::GetFolderStatus {
                    folder_id: folder_id.clone(),
                });

                // Update last change info for this folder
                self.last_folder_updates.insert(
                    folder_id.clone(),
                    (std::time::SystemTime::now(), file_path.clone())
                );

                // Extract parent directory path
                let parent_dir = if let Some(last_slash) = file_path.rfind('/') {
                    Some(&file_path[..last_slash + 1])
                } else {
                    None // File is in root directory
                };

                // Check if we're currently viewing this directory - if so, trigger refresh
                if !self.breadcrumb_trail.is_empty() && self.breadcrumb_trail[0].folder_id == folder_id {
                    for (_idx, level) in self.breadcrumb_trail.iter_mut().enumerate() {
                        if level.folder_id == folder_id {
                            // Clear in-memory sync state for this file
                            level.file_sync_states.remove(&file_path);

                            // Check if this level is showing the parent directory
                            let level_prefix = level.prefix.as_deref();

                            if level_prefix == parent_dir {
                                // This level is showing the directory containing the changed file
                                // Trigger a fresh browse request
                                let browse_key = format!("{}:{}", folder_id, parent_dir.unwrap_or(""));
                                if !self.loading_browse.contains(&browse_key) {
                                    self.loading_browse.insert(browse_key);

                                    let _ = self.api_tx.send(api_service::ApiRequest::BrowseFolder {
                                        folder_id: folder_id.clone(),
                                        prefix: parent_dir.map(|s| s.to_string()),
                                        priority: api_service::Priority::High,
                                    });

                                    log_debug(&format!("DEBUG [Event]: Triggered refresh for currently viewed directory: {:?}", parent_dir));
                                }
                            }
                        }
                    }
                }
            }
            event_listener::CacheInvalidation::Directory { folder_id, dir_path } => {
                log_debug(&format!("DEBUG [Event]: Invalidating directory: folder={} path={}", folder_id, dir_path));
                let _ = self.cache.invalidate_directory(&folder_id, &dir_path);

                // Invalidate folder status cache to refresh receiveOnlyTotalItems count
                let _ = self.cache.invalidate_folder_status(&folder_id);

                // Request fresh folder status
                let _ = self.api_tx.send(api_service::ApiRequest::GetFolderStatus {
                    folder_id: folder_id.clone(),
                });

                // Update last change info for this folder
                self.last_folder_updates.insert(
                    folder_id.clone(),
                    (std::time::SystemTime::now(), dir_path.clone())
                );

                // Clear in-memory state for all files in this directory and trigger refresh if viewing
                if !self.breadcrumb_trail.is_empty() && self.breadcrumb_trail[0].folder_id == folder_id {
                    for (_idx, level) in self.breadcrumb_trail.iter_mut().enumerate() {
                        if level.folder_id == folder_id {
                            // Remove all states that start with this directory path
                            let dir_prefix = if dir_path.is_empty() {
                                String::new()
                            } else if dir_path.ends_with('/') {
                                dir_path.clone()
                            } else {
                                format!("{}/", dir_path)
                            };

                            level.file_sync_states.retain(|path, _| {
                                if dir_prefix.is_empty() {
                                    false // Clear everything for root
                                } else {
                                    !path.starts_with(&dir_prefix)
                                }
                            });

                            // Check if this level is showing the changed directory
                            let level_prefix = level.prefix.as_deref();

                            let normalized_dir = if dir_path.is_empty() {
                                None
                            } else {
                                Some(if dir_path.ends_with('/') {
                                    dir_path.as_str()
                                } else {
                                    &format!("{}/", dir_path)[..]
                                })
                            };

                            if level_prefix == normalized_dir {
                                // This level is showing the changed directory - trigger refresh
                                let browse_key = format!("{}:{}", folder_id, level_prefix.unwrap_or(""));
                                if !self.loading_browse.contains(&browse_key) {
                                    self.loading_browse.insert(browse_key);

                                    let _ = self.api_tx.send(api_service::ApiRequest::BrowseFolder {
                                        folder_id: folder_id.clone(),
                                        prefix: level_prefix.map(|s| s.to_string()),
                                        priority: api_service::Priority::High,
                                    });

                                    log_debug(&format!("DEBUG [Event]: Triggered refresh for currently viewed directory: {:?}", level_prefix));
                                }
                            }
                        }
                    }
                }

                // Clear discovered directories cache for this path
                let dir_key_prefix = format!("{}:{}", folder_id, dir_path);
                self.discovered_dirs.retain(|key| !key.starts_with(&dir_key_prefix));
            }
        }
    }

    /// Merge local-only files from receive-only folders into browse items
    /// Returns the names of merged local items so we can mark their sync state
    async fn merge_local_only_files(&self, folder_id: &str, items: &mut Vec<BrowseItem>, prefix: Option<&str>) -> Vec<String> {
        let mut local_item_names = Vec::new();

        // Check if folder has local changes
        let has_local_changes = self.folder_statuses
            .get(folder_id)
            .map(|s| s.receive_only_total_items > 0)
            .unwrap_or(false);

        if !has_local_changes {
            return local_item_names;
        }

        log_debug(&format!("DEBUG [merge_local_only_files]: Fetching local items for folder={} prefix={:?}", folder_id, prefix));

        // Fetch local-only items for this directory
        if let Ok(local_items) = self.client.get_local_changed_items(folder_id, prefix).await {
            log_debug(&format!("DEBUG [merge_local_only_files]: Got {} local items", local_items.len()));

            // Add local-only items that aren't already in the browse results
            for local_item in local_items {
                if !items.iter().any(|i| i.name == local_item.name) {
                    log_debug(&format!("DEBUG [merge_local_only_files]: Adding local item: {}", local_item.name));
                    local_item_names.push(local_item.name.clone());
                    items.push(local_item);
                } else {
                    log_debug(&format!("DEBUG [merge_local_only_files]: Skipping duplicate item: {}", local_item.name));
                }
            }
        } else {
            log_debug(&format!("DEBUG [merge_local_only_files]: Failed to fetch local items"));
        }

        local_item_names
    }

    fn load_sync_states_from_cache(&self, folder_id: &str, items: &[BrowseItem], prefix: Option<&str>) -> HashMap<String, SyncState> {
        log_debug(&format!("DEBUG [load_sync_states_from_cache]: START folder={} prefix={:?} item_count={}", folder_id, prefix, items.len()));
        let mut sync_states = HashMap::new();

        for item in items {
            // Build the file path
            let file_path = if let Some(prefix) = prefix {
                format!("{}{}", prefix, item.name)
            } else {
                item.name.clone()
            };

            log_debug(&format!("DEBUG [load_sync_states_from_cache]: Querying cache for file_path={}", file_path));

            // Load from cache without validation (will be validated on next fetch if needed)
            match self.cache.get_sync_state_unvalidated(folder_id, &file_path) {
                Ok(Some(state)) => {
                    log_debug(&format!("DEBUG [load_sync_states_from_cache]: FOUND state={:?} for file_path={}", state, file_path));
                    sync_states.insert(item.name.clone(), state);
                }
                Ok(None) => {
                    log_debug(&format!("DEBUG [load_sync_states_from_cache]: NOT FOUND in cache for file_path={}", file_path));
                }
                Err(e) => {
                    log_debug(&format!("DEBUG [load_sync_states_from_cache]: ERROR querying cache for file_path={}: {}", file_path, e));
                }
            }
        }

        log_debug(&format!("DEBUG [load_sync_states_from_cache]: END returning {} states", sync_states.len()));
        sync_states
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

            log_debug(&format!("DEBUG [batch_fetch]: Requesting file_path={} for folder={}", file_path, folder_id));

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

        // Try to get from cache first
        let items = if let Ok(Some(cached_items)) = self.cache.get_browse_items(folder_id, Some(dir_path), folder_sequence) {
            cached_items
        } else {
            // Not cached - request it non-blocking and skip for now
            // It will be available on the next iteration
            if !self.loading_browse.contains(&browse_key) {
                self.loading_browse.insert(browse_key.clone());

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

    async fn load_root_level(&mut self) -> Result<()> {
        if let Some(selected) = self.folders_state.selected() {
            if let Some(folder) = self.folders.get(selected) {
                // Don't try to browse paused folders
                if folder.paused {
                    // Stay on folder list, don't enter the folder
                    return Ok(());
                }

                // Start timing
                let start = Instant::now();

                // Get folder sequence for cache validation
                let folder_sequence = self.folder_statuses
                    .get(&folder.id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                log_debug(&format!("DEBUG [load_root_level]: folder={} using sequence={}", folder.id, folder_sequence));

                // Create key for tracking in-flight operations
                let browse_key = format!("{}:", folder.id); // Empty prefix for root

                // Remove from loading_browse set if it's there (cleanup from previous attempts)
                self.loading_browse.remove(&browse_key);

                // Try cache first
                let (items, local_items) = if let Ok(Some(cached_items)) = self.cache.get_browse_items(&folder.id, None, folder_sequence) {
                    self.cache_hit = Some(true);
                    let mut items = cached_items;
                    // Merge local files even from cache
                    let local_items = self.merge_local_only_files(&folder.id, &mut items, None).await;
                    (items, local_items)
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());

                    // Cache miss - fetch from API (BLOCKING for root level)
                    self.cache_hit = Some(false);
                    let mut items = self.client.browse_folder(&folder.id, None).await?;

                    // Merge local-only files from receive-only folders
                    let local_items = self.merge_local_only_files(&folder.id, &mut items, None).await;

                    if let Err(e) = self.cache.save_browse_items(&folder.id, None, &items, folder_sequence) {
                        log_debug(&format!("ERROR saving root cache: {}", e));
                    }

                    // Done loading
                    self.loading_browse.remove(&browse_key);

                    (items, local_items)
                };

                // Record load time
                self.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

                let mut state = ListState::default();
                if !items.is_empty() {
                    state.select(Some(0));
                }

                // Compute translated base path once
                let translated_base_path = self.translate_path(folder, "");

                // Load cached sync states for items
                let mut file_sync_states = self.load_sync_states_from_cache(&folder.id, &items, None);

                // Mark local-only items with LocalOnly sync state and save to cache
                for local_item_name in &local_items {
                    file_sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                    // Save to cache so it persists
                    let _ = self.cache.save_sync_state(&folder.id, local_item_name, SyncState::LocalOnly, 0);
                }

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
                let folder_sequence = self.folder_statuses
                    .get(&folder_id)
                    .map(|s| s.sequence)
                    .unwrap_or(0);

                // Create key for tracking in-flight operations
                let browse_key = format!("{}:{}", folder_id, new_prefix);

                // Remove from loading_browse set if it's there (cleanup from previous attempts)
                self.loading_browse.remove(&browse_key);

                // Try cache first
                let (items, local_items) = if let Ok(Some(cached_items)) = self.cache.get_browse_items(&folder_id, Some(&new_prefix), folder_sequence) {
                    self.cache_hit = Some(true);
                    let mut items = cached_items;
                    // Merge local files even from cache
                    let local_items = self.merge_local_only_files(&folder_id, &mut items, Some(&new_prefix)).await;
                    (items, local_items)
                } else {
                    // Mark as loading
                    self.loading_browse.insert(browse_key.clone());
                    self.cache_hit = Some(false);

                    // Cache miss - fetch from API (BLOCKING)
                    let mut items = self.client.browse_folder(&folder_id, Some(&new_prefix)).await?;

                    // Merge local-only files from receive-only folders
                    let local_items = self.merge_local_only_files(&folder_id, &mut items, Some(&new_prefix)).await;

                    let _ = self.cache.save_browse_items(&folder_id, Some(&new_prefix), &items, folder_sequence);

                    // Done loading
                    self.loading_browse.remove(&browse_key);

                    (items, local_items)
                };

                // Record load time
                self.last_load_time_ms = Some(start.elapsed().as_millis() as u64);

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
                let mut file_sync_states = self.load_sync_states_from_cache(&folder_id, &items, Some(&new_prefix));
                log_debug(&format!("DEBUG [enter_directory]: Loaded {} cached states for new level with prefix={}", file_sync_states.len(), new_prefix));

                // Mark local-only items with LocalOnly sync state and save to cache
                for local_item_name in &local_items {
                    file_sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                    // Save to cache so it persists
                    let file_path = format!("{}{}", new_prefix, local_item_name);
                    let _ = self.cache.save_sync_state(&folder_id, &file_path, SyncState::LocalOnly, 0);
                }

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

    fn sort_level_with_selection(&mut self, level_idx: usize, preserve_selection_name: Option<String>) {
        if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
            let sort_mode = self.sort_mode;
            let reverse = self.sort_reverse;

            // Use provided name if given, otherwise get currently selected item name
            let selected_name = preserve_selection_name.or_else(|| {
                level.state.selected()
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
                    return if a_is_dir { Ordering::Less } else { Ordering::Greater };
                }

                let result = match sort_mode {
                    SortMode::VisualIndicator => {
                        // Sort by sync state priority
                        let a_state = level.file_sync_states.get(&a.name).copied().unwrap_or(SyncState::Unknown);
                        let b_state = level.file_sync_states.get(&b.name).copied().unwrap_or(SyncState::Unknown);

                        let a_priority = sync_state_priority(a_state);
                        let b_priority = sync_state_priority(b_state);

                        a_priority.cmp(&b_priority)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    },
                    SortMode::Alphabetical => {
                        a.name.to_lowercase().cmp(&b.name.to_lowercase())
                    },
                    SortMode::LastModified => {
                        // Reverse order for modified time (newest first)
                        // Use mod_time from BrowseItem directly
                        b.mod_time.cmp(&a.mod_time)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    },
                    SortMode::FileSize => {
                        // Reverse order for size (largest first)
                        // Use size from BrowseItem directly
                        b.size.cmp(&a.size)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    },
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
            return; // No sorting for folders list
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

    fn jump_to_first(&mut self) {
        if self.focus_level == 0 {
            if !self.folders.is_empty() {
                self.folders_state.select(Some(0));
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

    fn jump_to_last(&mut self) {
        if self.focus_level == 0 {
            if !self.folders.is_empty() {
                self.folders_state.select(Some(self.folders.len() - 1));
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

    fn page_down(&mut self, page_size: usize) {
        if self.focus_level == 0 {
            if self.folders.is_empty() {
                return;
            }
            let i = match self.folders_state.selected() {
                Some(i) => (i + page_size).min(self.folders.len() - 1),
                None => 0,
            };
            self.folders_state.select(Some(i));
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

    fn page_up(&mut self, page_size: usize) {
        if self.focus_level == 0 {
            if self.folders.is_empty() {
                return;
            }
            let i = match self.folders_state.selected() {
                Some(i) => i.saturating_sub(page_size),
                None => 0,
            };
            self.folders_state.select(Some(i));
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

    fn half_page_down(&mut self, visible_height: usize) {
        self.page_down(visible_height / 2);
    }

    fn half_page_up(&mut self, visible_height: usize) {
        self.page_up(visible_height / 2);
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

        log_debug(&format!("DEBUG [rescan_selected_folder]: Requesting rescan for folder={}", folder_id));

        // Trigger rescan via non-blocking API
        let _ = self.api_tx.send(api_service::ApiRequest::RescanFolder {
            folder_id,
        });

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
        let prefix = level.prefix.clone();

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
        let relative_path = if let Some(ref prefix) = prefix {
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

                // Immediately show as Unknown to give user feedback
                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                    level.file_sync_states.insert(item_name.clone(), SyncState::Unknown);
                }

                // Wait for rescan to complete so Syncthing processes the ignore change
                self.client.rescan_folder(&folder_id).await?;

                // Now fetch the fresh state
                let file_path_for_api = if let Some(ref prefix) = prefix {
                    format!("{}/{}", prefix.trim_matches('/'), item_name)
                } else {
                    item_name.clone()
                };

                // Fetch and update state
                if let Ok(file_details) = self.client.get_file_info(&folder_id, &file_path_for_api).await {
                    let new_state = file_details.determine_sync_state();
                    if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                        level.file_sync_states.insert(item_name, new_state);
                    }
                }
            } else {
                // Multiple patterns match - show selection menu
                let mut selection_state = ListState::default();
                selection_state.select(Some(0));
                self.pattern_selection = Some((folder_id, item_name, matching_patterns, selection_state));
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

            // Immediately mark as ignored in UI
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                level.file_sync_states.insert(item_name.clone(), SyncState::Ignored);
            }

            // Trigger rescan in background
            let client = self.client.clone();
            let folder_id_clone = folder_id.clone();
            tokio::spawn(async move {
                let _ = client.rescan_folder(&folder_id_clone).await;
            });
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

                // Immediately mark as ignored in UI
                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                    level.file_sync_states.insert(item_name.clone(), SyncState::Ignored);
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
            // Immediately mark as ignored in UI
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                level.file_sync_states.insert(item_name.clone(), SyncState::Ignored);
            }

            // Trigger rescan in background
            let client = self.client.clone();
            tokio::spawn(async move {
                let _ = client.rescan_folder(&folder_id).await;
            });
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
                            let deletion_info = if let Some(level) = self.breadcrumb_trail.get(level_idx) {
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
                                    let _ = self.cache.invalidate_single_file(&folder_id, &file_path);
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
                                let browse_key = format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
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

                        self.client.set_ignore_patterns(&folder_id, updated_patterns).await?;

                        // Immediately show as Unknown to give user feedback
                        if self.focus_level > 0 && self.focus_level <= self.breadcrumb_trail.len() {
                            let level_idx = self.focus_level - 1;
                            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                                level.file_sync_states.insert(item_name.clone(), SyncState::Unknown);
                            }
                        }

                        // Wait for rescan to complete so Syncthing processes the ignore change
                        self.client.rescan_folder(&folder_id).await?;

                        // Now fetch the fresh state
                        if self.focus_level > 0 && self.focus_level <= self.breadcrumb_trail.len() {
                            let level_idx = self.focus_level - 1;

                            let file_path_for_api = if let Some(level) = self.breadcrumb_trail.get(level_idx) {
                                if let Some(ref prefix) = level.prefix {
                                    format!("{}/{}", prefix.trim_matches('/'), &item_name)
                                } else {
                                    item_name.clone()
                                }
                            } else {
                                item_name.clone()
                            };

                            // Fetch and update state
                            if let Ok(file_details) = self.client.get_file_info(&folder_id, &file_path_for_api).await {
                                let new_state = file_details.determine_sync_state();
                                if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                                    level.file_sync_states.insert(item_name, new_state);
                                }
                            }
                        }
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
                let _ = self.rescan_selected_folder();
            }
            KeyCode::Char('R') => {
                // Restore selected file (if remote-only/deleted locally)
                let _ = self.restore_selected_file().await;
            }
            // Vim keybindings with Ctrl modifiers (check before 'd' and other letters)
            KeyCode::Char('d') if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.last_key_was_g = false;
                self.half_page_down(20); // Use reasonable default, will be more precise with frame height
            }
            KeyCode::Char('u') if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.last_key_was_g = false;
                self.half_page_up(20);
            }
            KeyCode::Char('f') if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.last_key_was_g = false;
                self.page_down(40);
            }
            KeyCode::Char('b') if self.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.last_key_was_g = false;
                self.page_up(40);
            }
            KeyCode::Char('d') => {
                // Delete file from disk (with confirmation)
                let _ = self.delete_file().await;
            }
            KeyCode::Char('i') => {
                // Toggle ignore state (add or remove from .stignore)
                let _ = self.toggle_ignore().await;
            }
            KeyCode::Char('I') => {
                // Ignore file AND delete from disk
                let _ = self.ignore_and_delete().await;
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
            // Vim keybindings
            KeyCode::Char('h') if self.vim_mode => {
                self.last_key_was_g = false;
                self.go_back();
            }
            KeyCode::Char('j') if self.vim_mode => {
                self.last_key_was_g = false;
                self.next_item();
            }
            KeyCode::Char('k') if self.vim_mode => {
                self.last_key_was_g = false;
                self.previous_item();
            }
            KeyCode::Char('l') if self.vim_mode => {
                self.last_key_was_g = false;
                if self.focus_level == 0 {
                    self.load_root_level().await?;
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Char('g') if self.vim_mode => {
                if self.last_key_was_g {
                    // gg - jump to first
                    self.jump_to_first();
                    self.last_key_was_g = false;
                } else {
                    // First 'g' press
                    self.last_key_was_g = true;
                }
            }
            KeyCode::Char('G') if self.vim_mode => {
                self.last_key_was_g = false;
                self.jump_to_last();
            }
            // Standard navigation keys (not advertised)
            KeyCode::PageDown => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.page_down(40);
            }
            KeyCode::PageUp => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.page_up(40);
            }
            KeyCode::Home => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.jump_to_first();
            }
            KeyCode::End => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.jump_to_last();
            }
            KeyCode::Left | KeyCode::Backspace => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.go_back();
            }
            KeyCode::Right | KeyCode::Enter => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                if self.focus_level == 0 {
                    self.load_root_level().await?;
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Up => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.previous_item();
            }
            KeyCode::Down => {
                if self.vim_mode {
                    self.last_key_was_g = false;
                }
                self.next_item();
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
    anyhow::bail!(
        "Config file not found. Expected locations:\n\
         1. ~/.config/synctui/config.yaml (preferred)\n\
         2. ./config.yaml (fallback)\n\
         \n\
         Use --config <path> to specify a custom location."
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

            // Determine if we have breadcrumb trails to show legend for
            let has_breadcrumbs = !app.breadcrumb_trail.is_empty();
            let folders_visible = start_pane == 0;

            // Create horizontal split for all panes
            let constraints: Vec<Constraint> = (0..visible_panes)
                .map(|_| Constraint::Ratio(1, visible_panes as u32))
                .collect();

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(content_area);

            // Split breadcrumb areas vertically if needed for legend
            let (breadcrumb_chunks, legend_area) = if has_breadcrumbs && folders_visible && chunks.len() > 1 {
                // Split all chunks except the first (folders) to make room for legend
                let breadcrumb_area = ratatui::layout::Rect {
                    x: chunks[1].x,
                    y: chunks[1].y,
                    width: content_area.width - chunks[0].width,
                    height: content_area.height,
                };

                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),      // Breadcrumb panes area
                        Constraint::Length(3),   // Legend area (3 lines)
                    ])
                    .split(breadcrumb_area);

                // Create new chunks for breadcrumb panels
                let breadcrumb_constraints: Vec<Constraint> = (0..(visible_panes - 1))
                    .map(|_| Constraint::Ratio(1, (visible_panes - 1) as u32))
                    .collect();

                let bc = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(breadcrumb_constraints)
                    .split(split[0]);

                (Some(bc), Some(split[1]))
            } else if has_breadcrumbs && !folders_visible {
                // No folders visible - split entire area
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),      // Panes area
                        Constraint::Length(3),   // Legend area (3 lines)
                    ])
                    .split(content_area);

                let breadcrumb_constraints: Vec<Constraint> = (0..visible_panes)
                    .map(|_| Constraint::Ratio(1, visible_panes as u32))
                    .collect();

                let bc = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(breadcrumb_constraints)
                    .split(split[0]);

                (Some(bc), Some(split[1]))
            } else {
                (None, None)
            };

            let mut chunk_idx = 0;

            // Render folders pane if visible
            if start_pane == 0 {
                // Split folders pane into folders list (top) + device status bar (bottom)
                let folders_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),      // Folders list content
                        Constraint::Length(3),   // Device status bar (3 lines: top border, text, bottom border)
                    ])
                    .split(chunks[0]);

                // Render Device Status Bar
                let device_status_line = if let Some(ref sys_status) = app.system_status {
                    let uptime_str = format_uptime(sys_status.uptime);

                    // Calculate local state summary
                    let (total_files, total_dirs, total_bytes) = app.get_local_state_summary();

                    let mut spans = vec![
                        Span::raw(app.device_name.as_deref().unwrap_or("Unknown")),
                        Span::raw(" | "),
                        Span::styled("Up:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {}", uptime_str)),
                    ];

                    // Add local state (use trimmed size to avoid padding)
                    let size_str = format_human_size(total_bytes).trim().to_string();
                    spans.push(Span::raw(" | "));
                    spans.push(Span::styled("Local:", Style::default().fg(Color::Yellow)));
                    spans.push(Span::raw(format!(" {} files, {} dirs, {}", total_files, total_dirs, size_str)));

                    // Add rates if available (display pre-calculated rates)
                    if let Some((in_rate, out_rate)) = app.last_transfer_rates {
                        spans.push(Span::raw(" | "));
                        spans.push(Span::styled("↓", Style::default().fg(Color::Yellow)));
                        spans.push(Span::raw(format_transfer_rate(in_rate)));
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled("↑", Style::default().fg(Color::Yellow)));
                        spans.push(Span::raw(format_transfer_rate(out_rate)));
                    }

                    Line::from(spans)
                } else {
                    Line::from(Span::raw("Device: Loading..."))
                };

                let device_status_widget = Paragraph::new(device_status_line)
                    .block(Block::default().borders(Borders::ALL).title("System"))
                    .style(Style::default().fg(Color::Gray));

                let folders_items: Vec<ListItem> = app
                    .folders
                    .iter()
                    .map(|folder| {
                        let display_name = folder.label.as_ref().unwrap_or(&folder.id);

                        // Determine folder state
                        let folder_state = if !app.statuses_loaded {
                            FolderState::Loading
                        } else if folder.paused {
                            FolderState::Paused
                        } else if let Some(status) = app.folder_statuses.get(&folder.id) {
                            if status.state == "" || status.state == "paused" {
                                FolderState::Paused
                            } else if status.state == "syncing" {
                                FolderState::Syncing
                            } else if status.need_total_items > 0 || status.receive_only_total_items > 0 {
                                FolderState::OutOfSync
                            } else if status.state == "idle" {
                                FolderState::Synced
                            } else if status.state.starts_with("sync") {
                                FolderState::Syncing
                            } else if status.state == "scanning" {
                                FolderState::Scanning
                            } else {
                                FolderState::Unknown
                            }
                        } else {
                            FolderState::Error
                        };

                        // Build the folder display with optional last update info
                        let mut icon_spans = app.icon_renderer.folder_with_status(folder_state);
                        icon_spans.push(Span::raw(display_name));
                        let folder_line = Line::from(icon_spans);

                        // Add last update info if available
                        if let Some((timestamp, last_file)) = app.last_folder_updates.get(&folder.id) {
                            // Calculate time since last update
                            let elapsed = timestamp.elapsed().unwrap_or(std::time::Duration::from_secs(0));
                            let time_str = if elapsed.as_secs() < 60 {
                                format!("{}s ago", elapsed.as_secs())
                            } else if elapsed.as_secs() < 3600 {
                                format!("{}m ago", elapsed.as_secs() / 60)
                            } else if elapsed.as_secs() < 86400 {
                                format!("{}h ago", elapsed.as_secs() / 3600)
                            } else {
                                format!("{}d ago", elapsed.as_secs() / 86400)
                            };

                            // Truncate filename if too long
                            let max_file_len = 40;
                            let file_display = if last_file.len() > max_file_len {
                                format!("...{}", &last_file[last_file.len() - max_file_len..])
                            } else {
                                last_file.clone()
                            };

                            // Multi-line item with update info
                            ListItem::new(vec![
                                folder_line,
                                Line::from(Span::styled(
                                    format!("  ↳ {} - {}", time_str, file_display),
                                    Style::default().fg(Color::Rgb(150, 150, 150)) // Medium gray visible on both dark gray and black backgrounds
                                ))
                            ])
                        } else {
                            // Single-line item without update info
                            ListItem::new(folder_line)
                        }
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

                f.render_stateful_widget(folders_list, folders_chunks[0], &mut app.folders_state);

                // Render Device Status Bar at bottom
                f.render_widget(device_status_widget, folders_chunks[1]);

                chunk_idx += 1;
            }

            // Render breadcrumb levels
            let mut breadcrumb_idx = 0;
            for (idx, level) in app.breadcrumb_trail.iter_mut().enumerate() {
                if idx + 1 < start_pane {
                    continue; // Skip panes that are off-screen to the left
                }

                // Get panel width for truncation
                let panel_width = if let Some(ref bc) = breadcrumb_chunks {
                    if breadcrumb_idx < bc.len() {
                        bc[breadcrumb_idx].width
                    } else {
                        60 // fallback
                    }
                } else if chunk_idx < chunks.len() {
                    chunks[chunk_idx].width
                } else {
                    60 // fallback
                };

                let items: Vec<ListItem> = level
                    .items
                    .iter()
                    .map(|item| {
                        // Use cached state directly (directories show their own metadata state, not aggregate)
                        let sync_state = level.file_sync_states.get(&item.name).copied().unwrap_or(SyncState::Unknown);

                        // Build icon as spans (for coloring)
                        let is_directory = item.item_type == "FILE_INFO_TYPE_DIRECTORY";
                        let icon_spans: Vec<Span> = if sync_state == SyncState::Ignored {
                            // Special handling for ignored items - check if exists on disk
                            let relative_path = if let Some(ref prefix) = level.prefix {
                                format!("{}/{}", prefix, item.name)
                            } else {
                                item.name.clone()
                            };
                            let host_path = format!("{}/{}", level.translated_base_path.trim_end_matches('/'), relative_path);
                            let exists = std::path::Path::new(&host_path).exists();
                            app.icon_renderer.ignored_item(exists)
                        } else {
                            app.icon_renderer.item_with_sync_state(is_directory, sync_state)
                        };

                        // Build display string with optional timestamp and/or size
                        let display_str = if app.display_mode != DisplayMode::Off && !item.mod_time.is_empty() {
                            let full_timestamp = format_timestamp(&item.mod_time);
                            // For width calculation, create a temporary string from icons
                            let icon_string: String = icon_spans.iter().map(|s| s.content.as_ref()).collect();
                            let icon_and_name = format!("{}{}", icon_string, item.name);
                            let is_directory = item.item_type == "FILE_INFO_TYPE_DIRECTORY";

                            // Calculate available space: panel_width - borders(2) - highlight(2) - padding(2)
                            let available_width = panel_width.saturating_sub(6) as usize;
                            let spacing = 2; // Minimum spacing between name and info

                            // Use unicode width for proper emoji handling
                            let name_width = icon_and_name.width();

                            // Determine what to show based on display mode (omit size for directories)
                            let info_string = match app.display_mode {
                                DisplayMode::TimestampOnly => full_timestamp.clone(),
                                DisplayMode::TimestampAndSize => {
                                    if is_directory {
                                        // Directories: show only timestamp
                                        full_timestamp.clone()
                                    } else {
                                        // Files: show size + timestamp
                                        let human_size = format_human_size(item.size);
                                        format!("{} {}", human_size, full_timestamp)
                                    }
                                },
                                DisplayMode::Off => String::new(),
                            };

                            let info_width = info_string.width();

                            // If everything fits, show it all
                            if name_width + spacing + info_width <= available_width {
                                let padding = available_width - name_width - info_width;
                                format!("{}{}{}", icon_and_name, " ".repeat(padding), info_string)
                            } else {
                                // Truncate info to make room for name
                                let space_left = available_width.saturating_sub(name_width + spacing);

                                if space_left >= 5 {
                                    // Show truncated info (prioritize time over date)
                                    let truncated_info = if app.display_mode == DisplayMode::TimestampAndSize && !is_directory {
                                        // Files with size: Try "1.2K HH:MM" (10 chars) or just "HH:MM" (5 chars)
                                        if space_left >= 10 && full_timestamp.len() >= 16 {
                                            // Show size + time only
                                            let human_size = format_human_size(item.size);
                                            format!("{} {}", human_size, &full_timestamp[11..16])
                                        } else if space_left >= 5 && full_timestamp.len() >= 16 {
                                            // Show just time
                                            full_timestamp[11..16].to_string()
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        // TimestampOnly OR directory: progressively truncate timestamp
                                        if space_left >= 16 {
                                            full_timestamp
                                        } else if space_left >= 10 && full_timestamp.len() >= 16 {
                                            // Show "MM-DD HH:MM" (10 chars)
                                            full_timestamp[5..16].to_string()
                                        } else if space_left >= 5 && full_timestamp.len() >= 16 {
                                            // Show just time "HH:MM" (5 chars)
                                            full_timestamp[11..16].to_string()
                                        } else {
                                            String::new()
                                        }
                                    };

                                    if !truncated_info.is_empty() {
                                        let info_width = truncated_info.width();
                                        let padding = available_width - name_width - info_width;
                                        format!("{}{}{}", icon_and_name, " ".repeat(padding), truncated_info)
                                    } else {
                                        icon_and_name
                                    }
                                } else {
                                    // Not enough room for info, just show name
                                    icon_and_name
                                }
                            }
                        } else {
                            let icon_string: String = icon_spans.iter().map(|s| s.content.as_ref()).collect();
                            format!("{}{}", icon_string, item.name)
                        };

                        // Create ListItem with styled info (timestamp and/or size)
                        if app.display_mode != DisplayMode::Off && !item.mod_time.is_empty() {
                            let full_timestamp = format_timestamp(&item.mod_time);
                            let icon_string: String = icon_spans.iter().map(|s| s.content.as_ref()).collect();
                            let icon_and_name = format!("{}{}", icon_string, item.name);
                            let is_directory = item.item_type == "FILE_INFO_TYPE_DIRECTORY";

                            // Calculate available space
                            let available_width = panel_width.saturating_sub(6) as usize;
                            let spacing = 2;
                            let name_width = icon_and_name.width();

                            // Determine what to show based on display mode (omit size for directories)
                            let info_string = match app.display_mode {
                                DisplayMode::TimestampOnly => full_timestamp.clone(),
                                DisplayMode::TimestampAndSize => {
                                    if is_directory {
                                        full_timestamp.clone()
                                    } else {
                                        let human_size = format_human_size(item.size);
                                        format!("{} {}", human_size, full_timestamp)
                                    }
                                },
                                DisplayMode::Off => String::new(),
                            };

                            let info_width = info_string.width();

                            if name_width + spacing + info_width <= available_width {
                                // Everything fits - use styled spans
                                let padding = available_width - name_width - info_width;
                                let mut line_spans = icon_spans.clone();
                                line_spans.push(Span::raw(&item.name));
                                line_spans.push(Span::raw(" ".repeat(padding)));
                                line_spans.push(Span::styled(info_string, Style::default().fg(Color::Rgb(120, 120, 120))));
                                ListItem::new(Line::from(line_spans))
                            } else {
                                // Truncated info - calculate which one
                                let space_left = available_width.saturating_sub(name_width + spacing);
                                let truncated_info = if app.display_mode == DisplayMode::TimestampAndSize && !is_directory {
                                    // Files with size: Try "1.2K HH:MM" (10 chars) or just "HH:MM" (5 chars)
                                    if space_left >= 10 && full_timestamp.len() >= 16 {
                                        let human_size = format_human_size(item.size);
                                        format!("{} {}", human_size, &full_timestamp[11..16])
                                    } else if space_left >= 5 && full_timestamp.len() >= 16 {
                                        full_timestamp[11..16].to_string()
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    // TimestampOnly OR directory: progressively truncate timestamp
                                    if space_left >= 16 {
                                        full_timestamp.clone()
                                    } else if space_left >= 10 && full_timestamp.len() >= 16 {
                                        full_timestamp[5..16].to_string()
                                    } else if space_left >= 5 && full_timestamp.len() >= 16 {
                                        full_timestamp[11..16].to_string()
                                    } else {
                                        String::new()
                                    }
                                };

                                if !truncated_info.is_empty() {
                                    let info_width = truncated_info.width();
                                    let padding = available_width - name_width - info_width;
                                    let mut line_spans = icon_spans.clone();
                                    line_spans.push(Span::raw(&item.name));
                                    line_spans.push(Span::raw(" ".repeat(padding)));
                                    line_spans.push(Span::styled(truncated_info, Style::default().fg(Color::Rgb(120, 120, 120))));
                                    ListItem::new(Line::from(line_spans))
                                } else {
                                    let mut line_spans = icon_spans.clone();
                                    line_spans.push(Span::raw(&item.name));
                                    ListItem::new(Line::from(line_spans))
                                }
                            }
                        } else {
                            let mut line_spans = icon_spans.clone();
                            line_spans.push(Span::raw(&item.name));
                            ListItem::new(Line::from(line_spans))
                        }
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

                // Use breadcrumb_chunks if available, otherwise use regular chunks
                if let Some(ref bc) = breadcrumb_chunks {
                    if breadcrumb_idx < bc.len() {
                        f.render_stateful_widget(list, bc[breadcrumb_idx], &mut level.state);
                        breadcrumb_idx += 1;
                    }
                } else if chunk_idx < chunks.len() {
                    f.render_stateful_widget(list, chunks[chunk_idx], &mut level.state);
                    chunk_idx += 1;
                }
            }

            // Render hotkey legend spanning across breadcrumb panels
            if let Some(legend_rect) = legend_area {
                // Build a single line with all hotkeys that will wrap automatically
                let mut hotkey_spans = vec![];

                // Navigation keys
                if app.vim_mode {
                    hotkey_spans.extend(vec![
                        Span::styled("hjkl", Style::default().fg(Color::Yellow)),
                        Span::raw(":Nav  "),
                        Span::styled("gg/G", Style::default().fg(Color::Yellow)),
                        Span::raw(":First/Last  "),
                        Span::styled("^d/^u", Style::default().fg(Color::Yellow)),
                        Span::raw(":½Page  "),
                        Span::styled("^f/^b", Style::default().fg(Color::Yellow)),
                        Span::raw(":FullPage  "),
                    ]);
                } else {
                    hotkey_spans.extend(vec![
                        Span::styled("↑/↓", Style::default().fg(Color::Yellow)),
                        Span::raw(":Nav  "),
                        Span::styled("Enter", Style::default().fg(Color::Yellow)),
                        Span::raw(":Open  "),
                        Span::styled("←", Style::default().fg(Color::Yellow)),
                        Span::raw(":Back  "),
                    ]);
                }

                // Common actions
                hotkey_spans.extend(vec![
                    Span::styled("s", Style::default().fg(Color::Yellow)),
                    Span::raw(":Sort  "),
                    Span::styled("S", Style::default().fg(Color::Yellow)),
                    Span::raw(":Reverse  "),
                    Span::styled("t", Style::default().fg(Color::Yellow)),
                    Span::raw(":Info  "),
                    Span::styled("i", Style::default().fg(Color::Yellow)),
                    Span::raw(":Ignore  "),
                    Span::styled("I", Style::default().fg(Color::Yellow)),
                    Span::raw(":Ign+Del  "),
                    Span::styled("d", Style::default().fg(Color::Yellow)),
                    Span::raw(":Delete  "),
                    Span::styled("r", Style::default().fg(Color::Yellow)),
                    Span::raw(":Rescan  "),
                    Span::styled("R", Style::default().fg(Color::Yellow)),
                    Span::raw(":Restore  "),
                    Span::styled("q", Style::default().fg(Color::Yellow)),
                    Span::raw(":Quit"),
                ]);

                let hotkey_line = Line::from(hotkey_spans);

                let legend = Paragraph::new(vec![hotkey_line])
                    .block(Block::default().borders(Borders::ALL).title("Hotkeys"))
                    .style(Style::default().fg(Color::Gray))
                    .wrap(ratatui::widgets::Wrap { trim: false });

                f.render_widget(legend, legend_rect);
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
                // Show current directory performance metrics
                let level_idx = app.focus_level - 1;
                if let Some(level) = app.breadcrumb_trail.get(level_idx) {
                    let folder_name = &level.folder_label;
                    let item_count = level.items.len();

                    // Build performance metrics string
                    let mut metrics = Vec::new();
                    metrics.push(format!("Folder: {}", folder_name));
                    metrics.push(format!("{} items", item_count));

                    // Show sort mode
                    let sort_display = format!("Sort: {}{}",
                        app.sort_mode.as_str(),
                        if app.sort_reverse { "↓" } else { "↑" }
                    );
                    metrics.push(sort_display);

                    if let Some(load_time) = app.last_load_time_ms {
                        metrics.push(format!("Load: {}ms", load_time));
                    }

                    if let Some(cache_hit) = app.cache_hit {
                        metrics.push(format!("Cache: {}", if cache_hit { "HIT" } else { "MISS" }));
                    }

                    // Show selected item info if available
                    if let Some(selected) = level.state.selected() {
                        if let Some(item) = level.items.get(selected) {
                            let item_type = match item.item_type.as_str() {
                                "FILE_INFO_TYPE_DIRECTORY" => "Dir",
                                "FILE_INFO_TYPE_FILE" => "File",
                                _ => "Item",
                            };
                            metrics.push(format!("Selected: {} ({})", item.name, item_type));
                        }
                    }

                    metrics.join(" | ")
                } else {
                    "".to_string()
                }
            };

            // Parse status_line and color the labels (before colons)
            let status_spans: Vec<Span> = if status_line.is_empty() {
                vec![Span::raw("")]
            } else {
                let mut spans = vec![];
                // Check for both separators: " │ " (focus_level 0) and " | " (focus_level > 0)
                let parts: Vec<&str> = if status_line.contains(" │ ") {
                    status_line.split(" │ ").collect()
                } else {
                    status_line.split(" | ").collect()
                };

                for (idx, part) in parts.iter().enumerate() {
                    if idx > 0 {
                        // Use the appropriate separator
                        if status_line.contains(" │ ") {
                            spans.push(Span::raw(" │ "));
                        } else {
                            spans.push(Span::raw(" | "));
                        }
                    }
                    // Split on first colon to separate label from value
                    if let Some(colon_pos) = part.find(':') {
                        let label = &part[..=colon_pos];
                        let value = &part[colon_pos + 1..];
                        spans.push(Span::styled(label, Style::default().fg(Color::Yellow)));
                        spans.push(Span::raw(value));
                    } else {
                        spans.push(Span::raw(*part));
                    }
                }
                spans
            };

            let status_bar = Paragraph::new(Line::from(status_spans))
                .block(Block::default().borders(Borders::ALL).title("Status"))
                .style(Style::default().fg(Color::Gray));

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
            if let Some((_folder_id, _item_name, patterns, state)) = &mut app.pattern_selection {
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

        // Process API responses (non-blocking)
        while let Ok(response) = app.api_rx.try_recv() {
            app.handle_api_response(response);
        }

        // Process cache invalidation messages from event listener (non-blocking)
        while let Ok(invalidation) = app.invalidation_rx.try_recv() {
            app.handle_cache_invalidation(invalidation);
        }

        // Process event ID updates from event listener (non-blocking)
        while let Ok(event_id) = app.event_id_rx.try_recv() {
            // Persist event ID to cache periodically
            let _ = app.cache.save_last_event_id(event_id);
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

        // Only run prefetch operations when user has been idle for 300ms
        // This prevents blocking keyboard input and reduces CPU usage
        let idle_time = app.last_user_action.elapsed();
        if idle_time >= std::time::Duration::from_millis(300) {
            // HIGHEST PRIORITY: If hovering over a directory, recursively discover all subdirectories
            // and fetch their states (non-blocking, uses cache only)
            app.prefetch_hovered_subdirectories(10, 15);

            // Fetch directory metadata states for visible directories in current level
            app.fetch_directory_states(10);

            // Fetch selected item specifically (high priority for user interaction)
            app.fetch_selected_item_sync_state();

            // LOWEST PRIORITY: Batch fetch file sync states for visible files
            app.batch_fetch_visible_sync_states(5);
        }

        // Increased poll timeout from 100ms to 250ms to reduce CPU usage when idle
        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await?;
            }
        }
    }

    Ok(())
}
