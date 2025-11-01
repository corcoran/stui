//! UI Model
//!
//! This sub-model contains all state related to the user interface:
//! preferences, dialogs, popups, and visual state.

use std::time::Instant;

use super::types::{FileInfoPopupState, PatternSelectionState, VimCommandState};
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

    /// Pattern selection menu for un-ignore
    pub pattern_selection: Option<PatternSelectionState>,

    /// File info popup (metadata + preview)
    pub file_info_popup: Option<FileInfoPopupState>,

    /// Toast message (text, timestamp)
    pub toast_message: Option<(String, Instant)>,

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
            pattern_selection: None,
            file_info_popup: None,
            toast_message: None,
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
            || self.pattern_selection.is_some()
            || self.file_info_popup.is_some()
    }

    /// Close all modal dialogs
    pub fn close_all_modals(&mut self) {
        self.confirm_revert = None;
        self.confirm_delete = None;
        self.confirm_ignore_delete = None;
        self.pattern_selection = None;
        self.file_info_popup = None;
    }

    /// Show toast message
    pub fn show_toast(&mut self, message: String) {
        self.toast_message = Some((message, Instant::now()));
    }

    /// Check if toast should be dismissed (older than 3 seconds)
    pub fn should_dismiss_toast(&self) -> bool {
        if let Some((_, timestamp)) = &self.toast_message {
            timestamp.elapsed().as_secs() >= 3
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
}
