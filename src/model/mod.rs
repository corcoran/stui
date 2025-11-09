//! Pure Application Model - Elm Architecture
//!
//! This module defines the pure, cloneable state for the application.
//! The Model is organized into focused sub-models for maintainability:
//!
//! - **SyncthingModel**: Syncthing API data (folders, devices, statuses)
//! - **NavigationModel**: Breadcrumb trail, focus, selection
//! - **UiModel**: User preferences, dialogs, popups
//! - **PerformanceModel**: Loading tracking, metrics, operations
//!
//! Key principles:
//! - Clone + Debug + PartialEq: Can snapshot and compare state
//! - No services: All I/O lives in Runtime
//! - Pure accessors: Helper methods are side-effect free
//! - Serializable: Can save/restore app state

pub mod navigation;
pub mod performance;
pub mod syncthing;
pub mod types;
pub mod ui;

pub use navigation::NavigationModel;
pub use performance::PerformanceModel;
pub use syncthing::SyncthingModel;
pub use types::*;
pub use ui::UiModel;

/// Root application model composed of focused sub-models
#[derive(Clone, Debug)]
pub struct Model {
    /// Syncthing API data (folders, devices, statuses)
    pub syncthing: SyncthingModel,

    /// Navigation state (breadcrumbs, focus, selection)
    pub navigation: NavigationModel,

    /// UI preferences and popups
    pub ui: UiModel,

    /// Performance tracking and operational state
    pub performance: PerformanceModel,
}

impl Model {
    /// Create initial model with default settings
    pub fn new(vim_mode: bool) -> Self {
        Self {
            syncthing: SyncthingModel::new(),
            navigation: NavigationModel::new(),
            ui: UiModel::new(vim_mode),
            performance: PerformanceModel::new(),
        }
    }

    /// Get currently selected folder (if any)
    pub fn selected_folder(&self) -> Option<&crate::api::Folder> {
        self.navigation
            .folders_state_selection
            .and_then(|idx| self.syncthing.folders.get(idx))
    }

    /// Get current breadcrumb level (if any)
    pub fn current_level(&self) -> Option<&BreadcrumbLevel> {
        self.navigation.current_level()
    }

    /// Check if system is idle (no user input for 300ms)
    pub fn is_idle(&self) -> bool {
        self.performance.is_idle()
    }

    /// Check if any modal dialog is showing
    pub fn has_modal(&self) -> bool {
        self.ui.has_modal()
    }

    /// Get folder by ID from Syncthing model
    pub fn get_folder(&self, folder_id: &str) -> Option<&crate::api::Folder> {
        self.syncthing.get_folder(folder_id)
    }

    /// Get folder status by ID
    pub fn get_folder_status(&self, folder_id: &str) -> Option<&crate::api::FolderStatus> {
        self.syncthing.get_folder_status(folder_id)
    }

    /// Get summary of local state (total files, dirs, bytes)
    pub fn get_local_state_summary(&self) -> (u64, u64, u64) {
        self.syncthing.get_local_state_summary()
    }

    /// Record user action (for idle detection)
    pub fn record_user_action(&mut self) {
        self.performance.record_user_action();
    }

    /// Close all modal dialogs
    pub fn close_all_modals(&mut self) {
        self.ui.close_all_modals();
    }

    /// Show toast message
    pub fn show_toast(&mut self, message: String) {
        self.ui.show_toast(message);
    }

    /// Check if toast should be dismissed
    pub fn should_dismiss_toast(&self) -> bool {
        self.ui.should_dismiss_toast()
    }

    /// Dismiss toast message
    pub fn dismiss_toast(&mut self) {
        self.ui.dismiss_toast();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_creation() {
        let model = Model::new(false);
        assert_eq!(model.syncthing.folders.len(), 0);
        assert_eq!(model.navigation.focus_level, 0);
        assert!(!model.ui.vim_mode);
        assert!(model.performance.prefetch_enabled);
    }

    #[test]
    fn test_model_is_cloneable() {
        let model = Model::new(false);
        let _cloned = model.clone();
    }

    #[test]
    fn test_selected_folder() {
        let model = Model::new(false);
        assert!(model.selected_folder().is_none());
    }

    #[test]
    fn test_current_level() {
        let model = Model::new(false);
        assert!(model.current_level().is_none());
    }

    #[test]
    fn test_has_modal() {
        let mut model = Model::new(false);
        assert!(!model.has_modal());

        model.ui.confirm_action = Some(ConfirmAction::Revert {
            folder_id: "test".to_string(),
            changed_files: vec![],
        });
        assert!(model.has_modal());
    }

    #[test]
    fn test_toast() {
        let mut model = Model::new(false);
        assert!(model.ui.toast_message.is_none());

        model.show_toast("Test".to_string());
        assert!(model.ui.toast_message.is_some());

        model.dismiss_toast();
        assert!(model.ui.toast_message.is_none());
    }

    #[test]
    fn test_vim_command_state() {
        let model = Model::new(false);
        assert_eq!(model.ui.vim_command_state, VimCommandState::None);
    }
}
