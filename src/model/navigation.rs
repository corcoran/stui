//! Navigation Model
//!
//! This sub-model contains all state related to navigation:
//! breadcrumb trail, focus level, and folder selection.

use super::types::BreadcrumbLevel;

/// Navigation state (breadcrumbs, focus, selection)
#[derive(Clone, Debug)]
pub struct NavigationModel {
    /// Breadcrumb trail for directory navigation
    pub breadcrumb_trail: Vec<BreadcrumbLevel>,

    /// Currently focused breadcrumb level (0 = folder list)
    pub focus_level: usize,

    /// Selected folder in the folder list
    pub folders_state_selection: Option<usize>,
}

impl NavigationModel {
    /// Create initial empty navigation model
    pub fn new() -> Self {
        Self {
            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            folders_state_selection: None,
        }
    }

    /// Get current breadcrumb level if any
    pub fn current_level(&self) -> Option<&BreadcrumbLevel> {
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            None
        } else {
            self.breadcrumb_trail.get(self.focus_level - 1)
        }
    }

    /// Check if currently in breadcrumb view
    pub fn in_breadcrumb_view(&self) -> bool {
        self.focus_level > 0 && !self.breadcrumb_trail.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_model_creation() {
        let model = NavigationModel::new();
        assert_eq!(model.focus_level, 0);
        assert!(model.folders_state_selection.is_none());
        assert!(model.breadcrumb_trail.is_empty());
    }

    #[test]
    fn test_current_level() {
        let model = NavigationModel::new();
        assert!(model.current_level().is_none());
    }

    #[test]
    fn test_navigation_model_is_cloneable() {
        let model = NavigationModel::new();
        let _cloned = model.clone();
    }
}
