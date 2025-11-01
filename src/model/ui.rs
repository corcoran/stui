//! UI Model
//!
//! This sub-model contains all state related to the user interface:
//! preferences, dialogs, popups, and visual state.

use std::time::Instant;

use super::types::{FileInfoPopupState, FolderTypeSelectionState, PatternSelectionState, VimCommandState};
use crate::{DisplayMode, SortMode};

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
    /// Confirmation dialog for revert operation
    pub confirm_revert: Option<(String, Vec<String>)>, // (folder_id, changed_files)

    /// Confirmation dialog for delete operation
    pub confirm_delete: Option<(String, String, bool)>, // (host_path, display_name, is_dir)

    /// Confirmation dialog for ignore+delete operation
    pub confirm_ignore_delete: Option<(String, String, bool)>, // (host_path, display_name, is_dir)

    /// Confirmation dialog for pause/resume folder
    pub confirm_pause_resume: Option<(String, String, bool)>, // (folder_id, folder_label, is_paused)

    /// Pattern selection menu for un-ignore
    pub pattern_selection: Option<PatternSelectionState>,

    /// Folder type selection menu
    pub folder_type_selection: Option<FolderTypeSelectionState>,

    /// File info popup (metadata + preview)
    pub file_info_popup: Option<FileInfoPopupState>,

    /// Toast message (text, timestamp)
    pub toast_message: Option<(String, Instant)>,

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
            confirm_revert: None,
            confirm_delete: None,
            confirm_ignore_delete: None,
            confirm_pause_resume: None,
            pattern_selection: None,
            folder_type_selection: None,
            file_info_popup: None,
            toast_message: None,
            search_mode: false,
            search_query: String::new(),
            search_origin_level: None,
            sixel_cleanup_frames: 0,
            image_font_size: None,
            should_quit: false,
        }
    }

    /// Check if any modal dialog is currently showing
    pub fn has_modal(&self) -> bool {
        self.confirm_revert.is_some()
            || self.confirm_delete.is_some()
            || self.confirm_ignore_delete.is_some()
            || self.confirm_pause_resume.is_some()
            || self.pattern_selection.is_some()
            || self.folder_type_selection.is_some()
            || self.file_info_popup.is_some()
            || self.search_mode
    }

    /// Close all modal dialogs
    pub fn close_all_modals(&mut self) {
        self.confirm_revert = None;
        self.confirm_delete = None;
        self.confirm_ignore_delete = None;
        self.confirm_pause_resume = None;
        self.pattern_selection = None;
        self.folder_type_selection = None;
        self.file_info_popup = None;
        self.search_mode = false;
        self.search_query.clear();
        self.search_origin_level = None;
    }

    /// Show toast message
    pub fn show_toast(&mut self, message: String) {
        self.toast_message = Some((message, Instant::now()));
    }

    /// Check if toast should be dismissed (older than 1 second)
    pub fn should_dismiss_toast(&self) -> bool {
        if let Some((_, timestamp)) = &self.toast_message {
            crate::logic::ui::should_dismiss_toast(timestamp.elapsed().as_millis())
        } else {
            false
        }
    }

    /// Dismiss toast message
    pub fn dismiss_toast(&mut self) {
        self.toast_message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        model.confirm_revert = Some(("test".to_string(), vec![]));
        assert!(model.has_modal());
    }

    #[test]
    fn test_close_all_modals() {
        let mut model = UiModel::new(false);
        model.confirm_revert = Some(("test".to_string(), vec![]));
        model.confirm_delete = Some(("path".to_string(), "name".to_string(), false));

        model.close_all_modals();
        assert!(!model.has_modal());
    }

    #[test]
    fn test_toast() {
        let mut model = UiModel::new(false);
        assert!(model.toast_message.is_none());

        model.show_toast("Test".to_string());
        assert!(model.toast_message.is_some());

        model.dismiss_toast();
        assert!(model.toast_message.is_none());
    }

    #[test]
    fn test_ui_model_is_cloneable() {
        let model = UiModel::new(false);
        let _cloned = model.clone();
    }

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
}
