//! UI Model
//!
//! This sub-model contains all state related to the user interface:
//! preferences, dialogs, popups, and visual state.

use std::time::Instant;

use super::types::{
    ConfirmAction, FileInfoPopupState, FolderGroupMenuState, FolderHistoryModal,
    FolderTypeSelectionState, GroupNameInputState, OutOfSyncFilterState, PatternSelectionState,
    SystemLogState, VimCommandState,
};
use crate::{DisplayMode, SortMode};

/// What a toast reports, which sets its icon and color
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    /// The action was refused or couldn't start, and nothing changed
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub shown_at: Instant,
}

/// Something a handler asks the main loop to do with the terminal
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRequest {
    /// Copy text to the terminal's clipboard with OSC 52
    Copy(String),
    /// Hand the terminal to a program and wait for it to exit
    RunInTerminal { program: String, args: Vec<String> },
}

/// UI preferences and popups
#[derive(Clone, Debug)]
pub struct UiModel {
    // ============================================
    // PREFERENCES
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
    // DIALOGS & POPUPS
    // ============================================
    /// Unified confirmation dialog
    pub confirm_action: Option<ConfirmAction>,

    /// Pattern selection menu for un-ignore
    pub pattern_selection: Option<PatternSelectionState>,

    /// Folder type selection menu
    pub folder_type_selection: Option<FolderTypeSelectionState>,

    /// Folder group menu (`e` in the folder view)
    pub folder_group_menu: Option<FolderGroupMenuState>,

    /// New group name input opened from the folder group menu
    pub group_name_input: Option<GroupNameInputState>,

    /// File info popup (metadata + preview)
    pub file_info_popup: Option<FileInfoPopupState>,

    /// Setup help dialog state (shown when no cache and connection fails)
    pub show_setup_help: bool,

    /// Config file path (for setup help dialog)
    pub config_path: String,

    /// Flag to trigger folder refresh in main loop (after reconnection)
    pub needs_folder_refresh: bool,
    /// Syncthing saved its config; the folder list must be refetched
    pub folder_config_stale: bool,

    pub toast: Option<Toast>,

    /// Work only the main loop can do, because it owns the terminal; drained before each draw
    pub terminal_requests: Vec<TerminalRequest>,

    // ============================================
    // SEARCH
    // ============================================
    /// Whether search input is active (receiving keystrokes)
    pub search_mode: bool,

    /// Current search query
    pub search_query: String,

    /// Focus level where search was initiated (None if no search active)
    pub search_origin_level: Option<usize>,

    // ============================================
    // OUT-OF-SYNC FILTER
    // ============================================
    /// Out-of-sync filter state (hierarchical breadcrumb filter)
    pub out_of_sync_filter: Option<OutOfSyncFilterState>,

    /// Folder update history modal state
    pub folder_history_modal: Option<FolderHistoryModal>,

    /// Log viewer; `None` when closed
    pub system_log: Option<SystemLogState>,

    /// Generation of the last log viewer opened
    pub system_log_generation: u64,

    /// Per-folder activity tracking (folder_id -> (event_message, timestamp))
    pub folder_activity: std::collections::HashMap<String, (String, std::time::SystemTime)>,

    /// Device details modal state
    pub device_details_modal: Option<super::types::DeviceDetailsModal>,

    // ============================================
    // VISUAL STATE
    // ============================================
    /// Sixel cleanup counter (render white screen for N frames)
    pub sixel_cleanup_frames: u8,

    /// Font size for image preview (width, height)
    pub image_font_size: Option<(u16, u16)>,

    /// Whether app should quit
    pub should_quit: bool,
}

impl UiModel {
    /// Create initial UI model with default preferences
    pub fn new(vim_mode: bool) -> Self {
        Self {
            sort_mode: SortMode::Alphabetical,
            sort_reverse: false,
            display_mode: DisplayMode::TimestampAndSize,
            vim_mode,
            vim_command_state: VimCommandState::None,
            confirm_action: None,
            pattern_selection: None,
            folder_type_selection: None,
            folder_group_menu: None,
            group_name_input: None,
            file_info_popup: None,
            show_setup_help: false,
            config_path: String::new(),
            needs_folder_refresh: false,
            folder_config_stale: false,
            toast: None,
            terminal_requests: Vec::new(),
            search_mode: false,
            search_query: String::new(),
            search_origin_level: None,
            out_of_sync_filter: None,
            folder_history_modal: None,
            system_log: None,
            system_log_generation: 0,
            folder_activity: std::collections::HashMap::new(),
            device_details_modal: None,
            sixel_cleanup_frames: 0,
            image_font_size: None,
            should_quit: false,
        }
    }

    /// Check if any modal dialog is currently showing
    pub fn has_modal(&self) -> bool {
        self.confirm_action.is_some()
            || self.pattern_selection.is_some()
            || self.folder_type_selection.is_some()
            || self.folder_group_menu.is_some()
            || self.group_name_input.is_some()
            || self.file_info_popup.is_some()
            || self.show_setup_help
            || self.search_mode
    }

    /// Close all modal dialogs
    pub fn close_all_modals(&mut self) {
        self.confirm_action = None;
        self.pattern_selection = None;
        self.folder_type_selection = None;
        self.folder_group_menu = None;
        self.group_name_input = None;
        self.file_info_popup = None;
        self.show_setup_help = false;
        self.search_mode = false;
        self.search_query.clear();
        self.search_origin_level = None;
    }

    fn show(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toast = Some(Toast {
            message: message.into(),
            kind,
            shown_at: Instant::now(),
        });
    }

    pub fn show_success(&mut self, message: impl Into<String>) {
        self.show(ToastKind::Success, message);
    }

    pub fn show_info(&mut self, message: impl Into<String>) {
        self.show(ToastKind::Info, message);
    }

    pub fn show_warning(&mut self, message: impl Into<String>) {
        self.show(ToastKind::Warning, message);
    }

    pub fn show_error(&mut self, message: impl Into<String>) {
        self.show(ToastKind::Error, message);
    }

    /// Check if toast should be dismissed (older than 1 second)
    pub fn should_dismiss_toast(&self) -> bool {
        self.toast.as_ref().is_some_and(|t| {
            crate::logic::ui::should_dismiss_toast(t.shown_at.elapsed().as_millis())
        })
    }

    /// Dismiss toast message
    pub fn dismiss_toast(&mut self) {
        self.toast = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // UI MODEL CREATION
    // ========================================

    #[test]
    fn test_ui_model_creation() {
        let model = UiModel::new(false);
        assert_eq!(model.sort_mode, SortMode::Alphabetical);
        assert!(!model.sort_reverse);
        assert!(!model.vim_mode);
        assert!(!model.should_quit);
    }

    #[test]
    fn test_has_modal() {
        let mut model = UiModel::new(false);
        assert!(!model.has_modal());

        model.confirm_action = Some(ConfirmAction::Revert {
            folder_id: "test".to_string(),
            changed_files: vec![],
        });
        assert!(model.has_modal());
    }

    #[test]
    fn test_close_all_modals() {
        let mut model = UiModel::new(false);
        model.confirm_action = Some(ConfirmAction::Delete {
            path: "path".to_string(),
            name: "name".to_string(),
            is_dir: false,
        });

        model.close_all_modals();
        assert!(!model.has_modal());
    }

    #[test]
    fn test_toast() {
        let mut model = UiModel::new(false);
        assert!(model.toast.is_none());

        model.show_success("Test");
        assert!(model.toast.is_some());

        model.dismiss_toast();
        assert!(model.toast.is_none());
    }

    #[test]
    fn test_toast_kind_set_by_show_method() {
        type Show = fn(&mut UiModel, &str);
        let mut model = UiModel::new(false);
        let cases: [(Show, ToastKind); 4] = [
            (|m, s| m.show_success(s), ToastKind::Success),
            (|m, s| m.show_info(s), ToastKind::Info),
            (|m, s| m.show_warning(s), ToastKind::Warning),
            (|m, s| m.show_error(s), ToastKind::Error),
        ];
        for (show, kind) in cases {
            show(&mut model, "msg");
            let toast = model.toast.as_ref().unwrap();
            assert_eq!(toast.kind, kind);
            assert_eq!(toast.message, "msg");
        }
    }

    #[test]
    fn test_ui_model_is_cloneable() {
        let model = UiModel::new(false);
        let _cloned = model.clone();
    }

    // ========================================
    // SEARCH MODE
    // ========================================

    #[test]
    fn test_has_modal_includes_search() {
        let mut model = UiModel::new(false);
        assert!(!model.has_modal());

        model.search_mode = true;
        assert!(model.has_modal());
    }

    #[test]
    fn test_close_all_modals_clears_search() {
        let mut model = UiModel::new(false);
        model.search_mode = true;
        model.search_query = "test".to_string();

        model.close_all_modals();
        assert!(!model.search_mode);
        assert!(model.search_query.is_empty());
    }

    #[test]
    fn test_search_mode_lifecycle() {
        let mut model = UiModel::new(false);

        // Start in non-search mode
        assert!(!model.search_mode);
        assert!(model.search_query.is_empty());

        // Enter search mode
        model.search_mode = true;
        assert!(model.search_mode);

        // Type query
        model.search_query = "test".to_string();
        assert_eq!(model.search_query, "test");

        // Accept search (Enter key - exit search mode but keep query)
        model.search_mode = false;
        assert!(!model.search_mode);
        assert_eq!(model.search_query, "test"); // Query persists

        // Clear search (Esc key - clear both)
        model.search_query.clear();
        assert!(model.search_query.is_empty());
    }

    // ========================================
    // SEARCH QUERY OPERATIONS
    // ========================================

    #[test]
    fn test_search_query_can_be_built_incrementally() {
        let mut model = UiModel::new(false);
        model.search_mode = true;

        // Simulate typing character by character
        model.search_query.push('t');
        assert_eq!(model.search_query, "t");

        model.search_query.push('e');
        assert_eq!(model.search_query, "te");

        model.search_query.push('s');
        model.search_query.push('t');
        assert_eq!(model.search_query, "test");
    }

    #[test]
    fn test_search_query_can_be_cleared_with_backspace() {
        let mut model = UiModel::new(false);
        model.search_mode = true;
        model.search_query = "test".to_string();

        // Simulate backspace
        model.search_query.pop();
        assert_eq!(model.search_query, "tes");

        model.search_query.pop();
        model.search_query.pop();
        model.search_query.pop();
        assert!(model.search_query.is_empty());
    }

    #[test]
    fn test_search_minimum_length_enforced() {
        let mut model = UiModel::new(false);
        model.search_mode = true;

        // Single character - too short for search
        model.search_query = "a".to_string();
        assert_eq!(model.search_query.len(), 1);

        // Two characters - minimum for search
        model.search_query = "ab".to_string();
        assert_eq!(model.search_query.len(), 2);
        assert!(model.search_query.len() >= 2);
    }

    // ========================================
    // SEARCH ORIGIN LEVEL
    // ========================================

    #[test]
    fn test_search_origin_level_set_on_start() {
        let mut model = UiModel::new(false);
        assert!(model.search_origin_level.is_none());

        // Start search at level 2
        model.search_mode = true;
        model.search_origin_level = Some(2);

        assert_eq!(model.search_origin_level, Some(2));
    }

    #[test]
    fn test_search_origin_level_cleared_on_explicit_clear() {
        let mut model = UiModel::new(false);
        model.search_mode = true;
        model.search_query = "test".to_string();
        model.search_origin_level = Some(3);

        // Simulate Esc key clearing search
        model.search_mode = false;
        model.search_query.clear();
        model.search_origin_level = None;

        assert!(model.search_origin_level.is_none());
        assert!(model.search_query.is_empty());
        assert!(!model.search_mode);
    }

    #[test]
    fn test_search_origin_level_persists_during_navigation() {
        let mut model = UiModel::new(false);

        // Start search at level 1 (Foo/)
        model.search_mode = true;
        model.search_query = "jeff".to_string();
        model.search_origin_level = Some(1);

        // Navigate deeper (Foo/Bar/) - origin level should persist
        // (focus_level would be 2 in actual app, but we're testing model state)
        assert_eq!(model.search_origin_level, Some(1));
        assert_eq!(model.search_query, "jeff");

        // Navigate even deeper (Foo/Bar/Baz/) - origin level still persists
        assert_eq!(model.search_origin_level, Some(1));
    }

    #[test]
    fn test_close_all_modals_clears_search_origin() {
        let mut model = UiModel::new(false);
        model.search_mode = true;
        model.search_query = "test".to_string();
        model.search_origin_level = Some(2);

        model.close_all_modals();

        assert!(!model.search_mode);
        assert!(model.search_query.is_empty());
        assert!(model.search_origin_level.is_none());
    }

    #[test]
    fn test_search_origin_level_different_levels() {
        let mut model = UiModel::new(false);

        // Can track different origin levels
        model.search_origin_level = Some(1);
        assert_eq!(model.search_origin_level, Some(1));

        model.search_origin_level = Some(5);
        assert_eq!(model.search_origin_level, Some(5));

        model.search_origin_level = None;
        assert!(model.search_origin_level.is_none());
    }

    #[test]
    fn test_folder_activity() {
        let mut model = UiModel::new(false);

        model.folder_activity.insert(
            "test-folder".to_string(),
            ("test.txt synced".to_string(), std::time::SystemTime::now()),
        );

        assert_eq!(model.folder_activity.len(), 1);
        assert!(model.folder_activity.contains_key("test-folder"));
    }
}
