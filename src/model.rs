//! Pure Application Model - Elm Architecture
//!
//! This module defines the pure, cloneable state for the application.
//! The Model contains ONLY data - no services, no channels, no I/O.
//!
//! Key principles:
//! - Clone + Debug: Can snapshot state for debugging/undo
//! - No services: All I/O lives in Runtime
//! - Pure accessors: Helper methods are side-effect free
//! - Serializable: Can save/restore app state

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crate::api::{
    BrowseItem, ConnectionStats, Device, FileDetails, Folder, FolderStatus, SyncState,
    SystemStatus,
};
use crate::{DisplayMode, SortMode};

// ============================================
// SUPPORTING TYPES
// ============================================

/// Vim command state for tracking double-key commands like 'gg'
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VimCommandState {
    None,
    WaitingForSecondG, // First 'g' pressed, waiting for second 'g'
}

/// Pattern selection menu state (for removing ignore patterns)
#[derive(Clone, Debug)]
pub struct PatternSelectionState {
    pub folder_id: String,
    pub item_name: String,
    pub patterns: Vec<String>,
    pub selected_index: Option<usize>,
}

/// A single level in the breadcrumb trail
#[derive(Clone, Debug)]
pub struct BreadcrumbLevel {
    pub folder_id: String,
    pub folder_label: String,
    pub folder_path: String, // Container path for this folder
    pub prefix: Option<String>,
    pub items: Vec<BrowseItem>,
    pub selected_index: Option<usize>, // ✅ Changed from ListState to Option<usize>
    pub file_sync_states: HashMap<String, SyncState>,
    pub ignored_exists: HashMap<String, bool>,
    pub translated_base_path: String,
}

impl BreadcrumbLevel {
    /// Get currently selected item
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        self.selected_index.and_then(|idx| self.items.get(idx))
    }

    /// Get sync state for a file/directory
    pub fn get_sync_state(&self, name: &str) -> Option<SyncState> {
        self.file_sync_states.get(name).copied()
    }

    /// Get relative path for an item
    pub fn relative_path(&self, item_name: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}/{}", prefix, item_name),
            None => item_name.to_string(),
        }
    }
}

/// Information about a pending ignore+delete operation
#[derive(Debug, Clone)]
pub struct PendingDeleteInfo {
    pub paths: HashSet<PathBuf>,
    pub initiated_at: Instant,
    pub rescan_triggered: bool,
}

/// File information popup state
/// Note: image_state removed - stays in Runtime (not cloneable)
#[derive(Clone, Debug)]
pub struct FileInfoPopupState {
    pub folder_id: String,
    pub file_path: String,
    pub browse_item: BrowseItem,
    pub file_details: Option<FileDetails>,
    pub file_content: Result<String, String>, // Ok(content) or Err(error message)
    pub exists_on_disk: bool,
    pub is_binary: bool,
    pub is_image: bool,
    pub scroll_offset: u16,
    // image_state moved to Runtime - ImagePreviewState is not Clone
}

// ============================================
// MAIN MODEL
// ============================================

/// Pure application state - no services, no I/O
/// This is cloneable, serializable, and easy to test
#[derive(Clone, Debug)]
pub struct Model {
    // ============================================
    // CORE DATA
    // ============================================
    /// List of all Syncthing folders
    pub folders: Vec<Folder>,

    /// List of all known devices
    pub devices: Vec<Device>,

    /// Sync status for each folder (keyed by folder_id)
    pub folder_statuses: HashMap<String, FolderStatus>,

    /// Whether folder statuses have been loaded
    pub statuses_loaded: bool,

    /// System status (device name, uptime, etc.)
    pub system_status: Option<SystemStatus>,

    /// Connection statistics (download/upload rates)
    pub connection_stats: Option<ConnectionStats>,

    /// Previous connection stats for rate calculation
    pub last_connection_stats: Option<(ConnectionStats, Instant)>,

    /// Cached device name
    pub device_name: Option<String>,

    /// Cached transfer rates (download, upload) in bytes/sec
    pub last_transfer_rates: Option<(f64, f64)>,

    // ============================================
    // NAVIGATION STATE
    // ============================================
    /// Breadcrumb trail for directory navigation
    pub breadcrumb_trail: Vec<BreadcrumbLevel>,

    /// Currently focused breadcrumb level (0 = folder list)
    pub focus_level: usize,

    /// Selected folder in the folder list
    pub folders_state_selection: Option<usize>,

    // ============================================
    // UI PREFERENCES
    // ============================================
    /// Current sort mode
    pub sort_mode: SortMode,

    /// Whether sort is reversed
    pub sort_reverse: bool,

    /// Display mode for file info
    pub display_mode: DisplayMode,

    /// Whether vim keybindings are enabled
    pub vim_mode: bool,

    /// Vim command state (for 'gg' double-key)
    pub vim_command_state: VimCommandState,

    // ============================================
    // UI DIALOGS & POPUPS
    // ============================================
    /// Confirmation dialog for revert operation
    pub confirm_revert: Option<(String, Vec<String>)>, // (folder_id, changed_files)

    /// Confirmation dialog for delete operation
    pub confirm_delete: Option<(String, String, bool)>, // (host_path, display_name, is_dir)

    /// Confirmation dialog for ignore+delete operation
    pub confirm_ignore_delete: Option<(String, String, bool)>, // (host_path, display_name, is_dir)

    /// Pattern selection menu for un-ignore
    pub pattern_selection: Option<PatternSelectionState>,

    /// File info popup (metadata + preview)
    pub file_info_popup: Option<FileInfoPopupState>,

    /// Toast message (text, timestamp)
    pub toast_message: Option<(String, Instant)>,

    // ============================================
    // OPERATIONAL STATE
    // ============================================
    /// Folders currently being loaded
    pub folders_loading: HashSet<String>,

    /// Pending ignore+delete operations (blocks un-ignore)
    pub pending_ignore_deletes: HashMap<String, PendingDeleteInfo>,

    /// Last update timestamp for each folder (folder_id -> (timestamp, filename))
    pub last_folder_updates: HashMap<String, (SystemTime, String)>,

    /// Last time user interacted with UI
    pub last_user_action: Instant,

    /// Whether app should quit
    pub should_quit: bool,

    // ============================================
    // PERFORMANCE TRACKING
    // ============================================
    /// In-flight browse requests (to prevent duplicates)
    pub loading_browse: HashSet<String>, // "folder_id:prefix"

    /// In-flight sync state requests (to prevent duplicates)
    pub loading_sync_states: HashSet<String>, // "folder_id:path"

    /// Already-discovered directories (to prevent re-querying cache)
    pub discovered_dirs: HashSet<String>, // "folder_id:prefix"

    /// Whether prefetching is enabled
    pub prefetch_enabled: bool,

    /// Last known sequence per folder (for detecting changes)
    pub last_known_sequences: HashMap<String, u64>,

    /// Last known receive-only counts per folder
    pub last_known_receive_only_counts: HashMap<String, u64>,

    /// Time to load current directory (milliseconds)
    pub last_load_time_ms: Option<u64>,

    /// Whether last load was a cache hit
    pub cache_hit: Option<bool>,

    /// Flag indicating UI needs redrawing
    pub ui_dirty: bool,

    // ============================================
    // IMAGE PREVIEW
    // ============================================
    /// Sixel cleanup counter (render white screen for N frames)
    pub sixel_cleanup_frames: u8,

    /// Font size for image preview (width, height)
    pub image_font_size: Option<(u16, u16)>,

    // NOTE: IconRenderer and image_picker are NOT cloneable
    // They will stay in Runtime, not Model
}

impl Model {
    /// Create initial empty model
    pub fn new(vim_mode: bool) -> Self {
        Self {
            folders: Vec::new(),
            devices: Vec::new(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            system_status: None,
            connection_stats: None,
            last_connection_stats: None,
            device_name: None,
            last_transfer_rates: None,

            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            folders_state_selection: None,

            sort_mode: SortMode::Alphabetical,
            sort_reverse: false,
            display_mode: DisplayMode::TimestampAndSize,
            vim_mode,
            vim_command_state: VimCommandState::None,

            confirm_revert: None,
            confirm_delete: None,
            confirm_ignore_delete: None,
            pattern_selection: None,
            file_info_popup: None,
            toast_message: None,

            folders_loading: HashSet::new(),
            pending_ignore_deletes: HashMap::new(),
            last_folder_updates: HashMap::new(),
            last_user_action: Instant::now(),
            should_quit: false,

            loading_browse: HashSet::new(),
            loading_sync_states: HashSet::new(),
            discovered_dirs: HashSet::new(),
            prefetch_enabled: true,
            last_known_sequences: HashMap::new(),
            last_known_receive_only_counts: HashMap::new(),
            last_load_time_ms: None,
            cache_hit: None,
            ui_dirty: true, // Start dirty to trigger initial render

            sixel_cleanup_frames: 0,
            image_font_size: None,
        }
    }

    // ============================================
    // ACCESSORS
    // ============================================

    /// Get currently selected folder (if any)
    pub fn selected_folder(&self) -> Option<&Folder> {
        self.folders_state_selection
            .and_then(|idx| self.folders.get(idx))
    }

    /// Get currently selected folder (mutable)
    pub fn selected_folder_mut(&mut self) -> Option<&mut Folder> {
        self.folders_state_selection
            .and_then(|idx| self.folders.get_mut(idx))
    }

    /// Get current breadcrumb level (if navigating)
    pub fn current_level(&self) -> Option<&BreadcrumbLevel> {
        if self.focus_level == 0 {
            None
        } else {
            self.breadcrumb_trail.get(self.focus_level - 1)
        }
    }

    /// Get current breadcrumb level (mutable)
    pub fn current_level_mut(&mut self) -> Option<&mut BreadcrumbLevel> {
        if self.focus_level == 0 {
            None
        } else {
            self.breadcrumb_trail.get_mut(self.focus_level - 1)
        }
    }

    /// Check if we're idle (no user action in 300ms)
    pub fn is_idle(&self) -> bool {
        self.last_user_action.elapsed() > Duration::from_millis(300)
    }

    /// Check if any modal dialog is open
    pub fn has_modal(&self) -> bool {
        self.confirm_revert.is_some()
            || self.confirm_delete.is_some()
            || self.confirm_ignore_delete.is_some()
            || self.pattern_selection.is_some()
            || self.file_info_popup.is_some()
    }

    /// Get folder by ID
    pub fn get_folder(&self, folder_id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == folder_id)
    }

    /// Get folder status by ID
    pub fn get_folder_status(&self, folder_id: &str) -> Option<&FolderStatus> {
        self.folder_statuses.get(folder_id)
    }

    /// Get local state summary (files, directories, bytes)
    pub fn get_local_state_summary(&self) -> (u64, u64, u64) {
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

    // ============================================
    // STATE MUTATIONS (Pure - return new value or mutate self)
    // ============================================

    /// Record user action (for idle detection)
    pub fn record_user_action(&mut self) {
        self.last_user_action = Instant::now();
    }

    /// Close all modal dialogs
    pub fn close_all_modals(&mut self) {
        self.confirm_revert = None;
        self.confirm_delete = None;
        self.confirm_ignore_delete = None;
        self.pattern_selection = None;
        self.file_info_popup = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_is_cloneable() {
        let model = Model::new(false);
        let _cloned = model.clone(); // Should compile
    }

    #[test]
    fn test_model_creation() {
        let model = Model::new(true);
        assert!(model.folders.is_empty());
        assert!(!model.statuses_loaded);
        assert_eq!(model.focus_level, 0);
        assert!(model.vim_mode);
        assert!(!model.should_quit);
    }

    #[test]
    fn test_selected_folder() {
        let mut model = Model::new(false);
        assert!(model.selected_folder().is_none());

        model.folders = vec![Folder {
            id: "test".to_string(),
            label: Some("Test".to_string()),
            path: "/test".to_string(),
            paused: false,
        }];
        model.folders_state_selection = Some(0);

        assert!(model.selected_folder().is_some());
        assert_eq!(model.selected_folder().unwrap().id, "test");
    }

    #[test]
    fn test_current_level() {
        let mut model = Model::new(false);
        assert!(model.current_level().is_none());

        model.breadcrumb_trail = vec![BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![],
            selected_index: None,
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
        }];
        model.focus_level = 1;

        assert!(model.current_level().is_some());
    }

    #[test]
    fn test_has_modal() {
        let mut model = Model::new(false);
        assert!(!model.has_modal());

        model.confirm_delete = Some(("/tmp/test".to_string(), "test".to_string(), false));
        assert!(model.has_modal());

        model.close_all_modals();
        assert!(!model.has_modal());
    }

    #[test]
    fn test_toast() {
        let mut model = Model::new(false);
        model.show_toast("Test message".to_string());
        assert!(model.toast_message.is_some());

        model.dismiss_toast();
        assert!(model.toast_message.is_none());
    }

    #[test]
    fn test_vim_command_state() {
        let mut model = Model::new(true);
        assert_eq!(model.vim_command_state, VimCommandState::None);

        model.vim_command_state = VimCommandState::WaitingForSecondG;
        assert_eq!(
            model.vim_command_state,
            VimCommandState::WaitingForSecondG
        );
    }
}
