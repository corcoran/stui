//! Shared types for the Model
//!
//! These types are used across multiple sub-models and represent
//! fundamental domain concepts.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use crate::api::{BrowseItem, FileDetails, SyncState};

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
    pub selected_index: Option<usize>,
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
#[derive(Debug, Clone, PartialEq)]
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
