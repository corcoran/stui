//! Sorting orchestration methods
//!
//! Methods for sorting breadcrumb levels:
//! - Multiple sort modes (sync state, alphabetical, timestamp, size)
//! - Reversible sorting
//! - Selection preservation across sorts

use crate::{App, logic};

impl App {
    /// Sort a specific breadcrumb level by its index
    pub(crate) fn sort_level(&mut self, level_idx: usize) {
        self.sort_level_with_selection(level_idx, None);
    }

    pub(crate) fn sort_level_with_selection(
        &mut self,
        level_idx: usize,
        preserve_selection_name: Option<String>,
    ) {
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            let sort_mode = self.model.ui.sort_mode;
            let reverse = self.model.ui.sort_reverse;

            // Use provided name if given, otherwise get currently selected item name
            let selected_name = preserve_selection_name.or_else(|| {
                level
                    .selected_index

                    .and_then(|idx| level.items.get(idx))
                    .map(|item| item.name.clone())
            });

            // Sort items
            level.items.sort_by(|a, b| {
                logic::sorting::compare_browse_items(
                    a,
                    b,
                    sort_mode,
                    reverse,
                    &level.file_sync_states,
                )
            });

            // Restore selection to the same item
            if let Some(name) = selected_name {
                let new_idx = logic::navigation::find_item_index_by_name(&level.items, &name);
                level.selected_index = new_idx.or(Some(0)); // Default to first item if not found
            } else if !level.items.is_empty() {
                // No previous selection, select first item
                level.selected_index = Some(0);
            }
        }
    }

    pub(crate) fn sort_current_level(&mut self) {
        if self.model.navigation.focus_level == 0 {
            // In preview mode (focus on folder list), sort the first breadcrumb if it exists
            if !self.model.navigation.breadcrumb_trail.is_empty() {
                self.sort_level(0);
            }
            return;
        }
        let level_idx = self.model.navigation.focus_level - 1;
        self.sort_level(level_idx);
    }

    pub(crate) fn sort_all_levels(&mut self) {
        // Apply sorting to all breadcrumb levels
        let num_levels = self.model.navigation.breadcrumb_trail.len();
        for idx in 0..num_levels {
            self.sort_level(idx);
        }
    }

    pub(crate) fn cycle_sort_mode(&mut self) {
        if let Some(new_mode) = logic::ui::cycle_sort_mode(
            self.model.ui.sort_mode,
            self.model.navigation.focus_level,
        ) {
            self.model.ui.sort_mode = new_mode;
            self.model.ui.sort_reverse = false; // Reset reverse when changing mode
            self.sort_all_levels(); // Apply to all levels
        }
    }

    pub(crate) fn toggle_sort_reverse(&mut self) {
        if let Some(new_reverse) = logic::ui::toggle_sort_reverse(
            self.model.ui.sort_reverse,
            self.model.navigation.focus_level,
        ) {
            self.model.ui.sort_reverse = new_reverse;
            self.sort_all_levels(); // Apply to all levels
        }
    }
}
