//! Application State Management
//!
//! This module defines the pure state structures for the application,
//! separated from I/O services and business logic.
//!
//! The state is organized into focused modules:
//! - AppState: Core application data (folders, devices, statuses)
//! - NavigationState: Breadcrumb navigation and UI preferences
//! - UiState: Modal dialogs and popups
//! - OperationalState: Performance tracking and operational flags
//!
//! Note: Currently unused - will be integrated in Phase 1.3+ as we refactor App struct

#![allow(dead_code)]

use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use crate::api::{BrowseItem, ConnectionStats, Device, Folder, FolderStatus, SyncState, SystemStatus};
use crate::model::{FileInfoPopupState, PendingDeleteInfo};
use crate::{DisplayMode, SortMode};

/// Core application state - folders, devices, and sync status
#[derive(Debug)]
pub struct AppState {
    /// All Syncthing folders from config
    pub folders: Vec<Folder>,

    /// All Syncthing devices from config
    pub devices: Vec<Device>,

    /// Current status for each folder (synced bytes, items, etc.)
    pub folder_statuses: HashMap<String, FolderStatus>,

    /// Whether initial folder statuses have been loaded
    pub statuses_loaded: bool,

    /// Track last file change per folder for display
    /// folder_id -> (timestamp, last_changed_file_path)
    pub last_folder_updates: HashMap<String, (SystemTime, String)>,

    /// System status (device info, uptime)
    pub system_status: Option<SystemStatus>,

    /// Global connection statistics
    pub connection_stats: Option<ConnectionStats>,

    /// Previous connection stats for rate calculation
    /// (stats, timestamp)
    pub last_connection_stats: Option<(ConnectionStats, Instant)>,

    /// Cached device name (from system status)
    pub device_name: Option<String>,

    /// Cached transfer rates (download, upload) in bytes/sec
    pub last_transfer_rates: Option<(f64, f64)>,
}

impl AppState {
    /// Create new empty application state
    pub fn new() -> Self {
        Self {
            folders: Vec::new(),
            devices: Vec::new(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            last_folder_updates: HashMap::new(),
            system_status: None,
            connection_stats: None,
            last_connection_stats: None,
            device_name: None,
            last_transfer_rates: None,
        }
    }

    /// Get folder by ID
    pub fn get_folder(&self, folder_id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == folder_id)
    }

    /// Get folder status by ID
    pub fn get_folder_status(&self, folder_id: &str) -> Option<&FolderStatus> {
        self.folder_statuses.get(folder_id)
    }

    /// Update folder status
    pub fn update_folder_status(&mut self, folder_id: String, status: FolderStatus) {
        self.folder_statuses.insert(folder_id, status);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Navigation state - breadcrumb trail and UI preferences
#[derive(Debug)]
pub struct NavigationState {
    /// Breadcrumb navigation trail (each level = directory depth)
    pub breadcrumb_trail: Vec<BreadcrumbLevel>,

    /// Current focus level (0 = folder list, 1+ = breadcrumb levels)
    pub focus_level: usize,

    /// Selection state for folder list panel
    pub folders_state: ListState,

    /// Display mode for file info (off, timestamp only, timestamp + size)
    pub display_mode: DisplayMode,

    /// Sort mode (icon, alphabetical, timestamp, size)
    pub sort_mode: SortMode,

    /// Whether to reverse sort order
    pub sort_reverse: bool,

    /// Track if last key was 'g' (for 'gg' vim command)
    pub last_key_was_g: bool,
}

impl NavigationState {
    /// Create new empty navigation state
    pub fn new() -> Self {
        Self {
            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            folders_state: ListState::default(),
            display_mode: DisplayMode::Off,
            sort_mode: SortMode::VisualIndicator,
            sort_reverse: false,
            last_key_was_g: false,
        }
    }

    /// Get currently selected folder index (if focused on folder list)
    pub fn selected_folder_index(&self) -> Option<usize> {
        if self.focus_level == 0 {
            self.folders_state.selected()
        } else {
            None
        }
    }

    /// Get current breadcrumb level (if focused on breadcrumbs)
    pub fn current_breadcrumb_level(&self) -> Option<&BreadcrumbLevel> {
        if self.focus_level > 0 {
            let level_idx = self.focus_level - 1;
            self.breadcrumb_trail.get(level_idx)
        } else {
            None
        }
    }

    /// Get current breadcrumb level (mutable)
    pub fn current_breadcrumb_level_mut(&mut self) -> Option<&mut BreadcrumbLevel> {
        if self.focus_level > 0 {
            let level_idx = self.focus_level - 1;
            self.breadcrumb_trail.get_mut(level_idx)
        } else {
            None
        }
    }
}

impl Default for NavigationState {
    fn default() -> Self {
        Self::new()
    }
}

/// Breadcrumb level representing one directory depth
#[derive(Debug, Clone)]
pub struct BreadcrumbLevel {
    /// Folder ID this breadcrumb belongs to
    pub folder_id: String,

    /// Human-readable folder label
    pub folder_label: String,

    /// Container path for this folder
    pub folder_path: String,

    /// Directory prefix relative to folder root (None = root)
    pub prefix: Option<String>,

    /// Items (files/folders) at this level
    pub items: Vec<BrowseItem>,

    /// Selection state for this level's list
    pub state: ListState,

    /// Translated base path (host filesystem path for this level)
    pub translated_base_path: String,

    /// Cached sync states by filename
    pub file_sync_states: HashMap<String, SyncState>,

    /// Track if ignored files exist on disk (for icon display)
    pub ignored_exists: HashMap<String, bool>,
}

impl BreadcrumbLevel {
    /// Get currently selected item index
    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    /// Get currently selected item
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        self.state.selected().and_then(|idx| self.items.get(idx))
    }

    /// Get sync state for a file/directory
    pub fn get_sync_state(&self, name: &str) -> Option<SyncState> {
        self.file_sync_states.get(name).copied()
    }
}

/// UI state - modal dialogs and popups
#[derive(Debug)]
pub struct UiState {
    /// Revert confirmation dialog (folder_id, changed_files)
    pub confirm_revert: Option<(String, Vec<String>)>,

    /// Delete confirmation dialog (host_path, display_name, is_dir)
    pub confirm_delete: Option<(String, String, bool)>,

    /// Pattern selection menu (folder_id, item_name, patterns, selection_state)
    pub pattern_selection: Option<(String, String, Vec<String>, ListState)>,

    /// File information popup
    pub show_file_info: Option<FileInfoPopupState>,

    /// Toast notification (message, timestamp)
    pub toast_message: Option<(String, Instant)>,

    /// Sixel cleanup counter (render white screen for N frames after closing image)
    pub sixel_cleanup_frames: u8,
}

impl UiState {
    /// Create new empty UI state
    pub fn new() -> Self {
        Self {
            confirm_revert: None,
            confirm_delete: None,
            pattern_selection: None,
            show_file_info: None,
            toast_message: None,
            sixel_cleanup_frames: 0,
        }
    }

    /// Check if any modal dialog is open
    pub fn has_modal(&self) -> bool {
        self.confirm_revert.is_some()
            || self.confirm_delete.is_some()
            || self.pattern_selection.is_some()
            || self.show_file_info.is_some()
    }

    /// Close all modals
    pub fn close_all_modals(&mut self) {
        self.confirm_revert = None;
        self.confirm_delete = None;
        self.pattern_selection = None;
        self.show_file_info = None;
    }

    /// Show toast notification
    pub fn show_toast(&mut self, message: String) {
        self.toast_message = Some((message, Instant::now()));
    }

    /// Check if toast should be dismissed (after 1.5 seconds)
    pub fn should_dismiss_toast(&self) -> bool {
        self.toast_message
            .as_ref()
            .map(|(_, timestamp)| timestamp.elapsed().as_millis() >= 1500)
            .unwrap_or(false)
    }

    /// Dismiss toast notification
    pub fn dismiss_toast(&mut self) {
        self.toast_message = None;
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Operational state - performance tracking and cache management
#[derive(Debug)]
pub struct OperationalState {
    /// In-flight browse requests (to prevent duplicates)
    /// Format: "folder_id:prefix"
    pub loading_browse: HashSet<String>,

    /// In-flight sync state requests (to prevent duplicates)
    /// Format: "folder_id:file_path"
    pub loading_sync_states: HashSet<String>,

    /// Already-discovered directories (to prevent re-querying cache)
    /// Format: "folder_id:prefix"
    pub discovered_dirs: HashSet<String>,

    /// Whether prefetching is enabled (disabled when system is busy)
    pub prefetch_enabled: bool,

    /// Last known sequence number per folder (for detecting changes)
    pub last_known_sequences: HashMap<String, u64>,

    /// Last known receive-only item count per folder (for detecting local changes)
    pub last_known_receive_only_counts: HashMap<String, u64>,

    /// Pending ignore+delete operations (blocks un-ignore during deletion)
    pub pending_ignore_deletes: HashMap<String, PendingDeleteInfo>,

    /// Pending database writes for batch processing
    /// Format: (folder_id, file_path, state, sequence)
    pub pending_sync_state_writes: Vec<(String, String, SyncState, u64)>,

    /// Time to load current directory (milliseconds)
    pub last_load_time_ms: Option<u64>,

    /// Whether last load was a cache hit
    pub cache_hit: Option<bool>,

    /// Flag indicating UI needs redrawing
    pub ui_dirty: bool,

    /// Flag indicating should quit application
    pub should_quit: bool,
}

impl OperationalState {
    /// Create new operational state
    pub fn new() -> Self {
        Self {
            loading_browse: HashSet::new(),
            loading_sync_states: HashSet::new(),
            discovered_dirs: HashSet::new(),
            prefetch_enabled: true,
            last_known_sequences: HashMap::new(),
            last_known_receive_only_counts: HashMap::new(),
            pending_ignore_deletes: HashMap::new(),
            pending_sync_state_writes: Vec::new(),
            last_load_time_ms: None,
            cache_hit: None,
            ui_dirty: true, // Start dirty to trigger initial render
            should_quit: false,
        }
    }

    /// Check if a browse request is already in flight
    pub fn is_loading_browse(&self, folder_id: &str, prefix: Option<&str>) -> bool {
        let key = format!("{}:{}", folder_id, prefix.unwrap_or(""));
        self.loading_browse.contains(&key)
    }

    /// Mark browse request as in-flight
    pub fn start_loading_browse(&mut self, folder_id: &str, prefix: Option<&str>) {
        let key = format!("{}:{}", folder_id, prefix.unwrap_or(""));
        self.loading_browse.insert(key);
    }

    /// Mark browse request as completed
    pub fn finish_loading_browse(&mut self, folder_id: &str, prefix: Option<&str>) {
        let key = format!("{}:{}", folder_id, prefix.unwrap_or(""));
        self.loading_browse.remove(&key);
    }

    /// Check if a path or parent is pending deletion
    pub fn is_path_pending_deletion(&self, folder_id: &str, path: &PathBuf) -> Option<PathBuf> {
        if let Some(pending_info) = self.pending_ignore_deletes.get(folder_id) {
            // Exact match
            if pending_info.paths.contains(path) {
                return Some(path.clone());
            }

            // Check if any parent directory is pending
            for pending_path in &pending_info.paths {
                if path.starts_with(pending_path) {
                    return Some(pending_path.clone());
                }
            }
        }
        None
    }

    /// Add path to pending deletions
    pub fn add_pending_delete(&mut self, folder_id: String, path: PathBuf) {
        let pending_info = self.pending_ignore_deletes
            .entry(folder_id)
            .or_insert_with(|| PendingDeleteInfo {
                paths: HashSet::new(),
                initiated_at: Instant::now(),
                rescan_triggered: false,
            });
        pending_info.paths.insert(path);
    }

    /// Remove path from pending deletions
    pub fn remove_pending_delete(&mut self, folder_id: &str, path: &PathBuf) -> bool {
        if let Some(pending_info) = self.pending_ignore_deletes.get_mut(folder_id) {
            let removed = pending_info.paths.remove(path);

            // Clean up empty folder entry
            if pending_info.paths.is_empty() {
                self.pending_ignore_deletes.remove(folder_id);
            }

            removed
        } else {
            false
        }
    }
}

impl Default for OperationalState {
    fn default() -> Self {
        Self::new()
    }
}

/// Timing state - track when various operations last occurred
#[derive(Debug)]
pub struct TimingState {
    /// Last time folder status was updated
    pub last_status_update: Instant,

    /// Last user interaction (for idle detection)
    pub last_user_action: Instant,

    /// Last system status fetch
    pub last_system_status_update: Instant,

    /// Last connection stats fetch
    pub last_connection_stats_fetch: Instant,

    /// Last directory state update
    pub last_directory_update: Instant,

    /// Last database write flush
    pub last_db_flush: Instant,
}

impl TimingState {
    /// Create new timing state with all timers set to now
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            last_status_update: now,
            last_user_action: now,
            last_system_status_update: now,
            last_connection_stats_fetch: now,
            last_directory_update: now,
            last_db_flush: now,
        }
    }

    /// Check if user has been idle for given duration
    pub fn is_idle(&self, duration: std::time::Duration) -> bool {
        self.last_user_action.elapsed() >= duration
    }

    /// Record user action (resets idle timer)
    pub fn record_user_action(&mut self) {
        self.last_user_action = Instant::now();
    }
}

impl Default for TimingState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_creation() {
        let state = AppState::new();
        assert!(state.folders.is_empty());
        assert!(!state.statuses_loaded);
        assert_eq!(state.folders.len(), 0);
    }

    #[test]
    fn test_navigation_state() {
        let mut nav = NavigationState::new();
        assert_eq!(nav.focus_level, 0);
        assert!(nav.breadcrumb_trail.is_empty());
        assert!(!nav.last_key_was_g);

        // Test selected folder when focused on folder list
        assert_eq!(nav.selected_folder_index(), None);
        nav.folders_state.select(Some(2));
        assert_eq!(nav.selected_folder_index(), Some(2));

        // When focused on breadcrumbs, selected_folder_index returns None
        nav.focus_level = 1;
        assert_eq!(nav.selected_folder_index(), None);
    }

    #[test]
    fn test_ui_state_modals() {
        let mut ui = UiState::new();
        assert!(!ui.has_modal());

        ui.confirm_delete = Some(("/tmp/test".to_string(), "test".to_string(), false));
        assert!(ui.has_modal());

        ui.close_all_modals();
        assert!(!ui.has_modal());
    }

    #[test]
    fn test_ui_state_toast() {
        let mut ui = UiState::new();
        ui.show_toast("Test message".to_string());
        assert!(ui.toast_message.is_some());

        // Toast should not be dismissed immediately
        assert!(!ui.should_dismiss_toast());

        ui.dismiss_toast();
        assert!(ui.toast_message.is_none());
    }

    #[test]
    fn test_operational_state_loading_tracking() {
        let mut ops = OperationalState::new();
        assert!(!ops.is_loading_browse("test-folder", None));

        ops.start_loading_browse("test-folder", None);
        assert!(ops.is_loading_browse("test-folder", None));

        ops.finish_loading_browse("test-folder", None);
        assert!(!ops.is_loading_browse("test-folder", None));
    }

    #[test]
    fn test_operational_state_pending_deletes() {
        let mut ops = OperationalState::new();
        let path = PathBuf::from("/tmp/test");

        ops.add_pending_delete("folder1".to_string(), path.clone());
        assert!(ops.is_path_pending_deletion("folder1", &path).is_some());

        let removed = ops.remove_pending_delete("folder1", &path);
        assert!(removed);
        assert!(ops.is_path_pending_deletion("folder1", &path).is_none());
    }

    #[test]
    fn test_timing_state_idle_detection() {
        let mut timing = TimingState::new();

        // Should not be idle immediately
        assert!(!timing.is_idle(std::time::Duration::from_millis(100)));

        timing.record_user_action();
        assert!(!timing.is_idle(std::time::Duration::from_millis(1)));
    }
}
