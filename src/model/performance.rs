//! Performance Model
//!
//! This sub-model contains all state related to performance tracking
//! and operational state: loading indicators, metrics, and pending operations.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::types::PendingDeleteInfo;

/// Performance tracking and operational state
#[derive(Clone, Debug)]
pub struct PerformanceModel {
    // ============================================
    // IN-FLIGHT TRACKING
    // ============================================
    /// Folders currently being loaded
    pub folders_loading: HashSet<String>,

    /// In-flight browse requests (to prevent duplicates)
    pub loading_browse: HashSet<String>, // "folder_id:prefix"

    /// In-flight sync state requests (to prevent duplicates)
    pub loading_sync_states: HashSet<String>, // "folder_id:path"

    /// Already-discovered directories (to prevent re-querying cache)
    pub discovered_dirs: HashSet<String>, // "folder_id:prefix"

    /// Whether prefetching is enabled
    pub prefetch_enabled: bool,

    // ============================================
    // SEQUENCE TRACKING
    // ============================================
    /// Last known sequence per folder (for detecting changes)
    pub last_known_sequences: HashMap<String, u64>,

    /// Last known receive-only counts per folder
    pub last_known_receive_only_counts: HashMap<String, u64>,

    // ============================================
    // METRICS
    // ============================================
    /// Time to load current directory (milliseconds)
    pub last_load_time_ms: Option<u64>,

    /// Whether last load was a cache hit
    pub cache_hit: Option<bool>,

    // ============================================
    // OPERATIONS
    // ============================================
    /// Pending ignore+delete operations (blocks un-ignore)
    pub pending_ignore_deletes: HashMap<String, PendingDeleteInfo>,

    /// Last time user interacted with UI
    pub last_user_action: Instant,

    /// Last time search filter was updated (for throttling)
    pub last_search_filter_update: Instant,
}

impl PerformanceModel {
    /// Create initial performance model
    pub fn new() -> Self {
        Self {
            folders_loading: HashSet::new(),
            loading_browse: HashSet::new(),
            loading_sync_states: HashSet::new(),
            discovered_dirs: HashSet::new(),
            prefetch_enabled: true,
            last_known_sequences: HashMap::new(),
            last_known_receive_only_counts: HashMap::new(),
            last_load_time_ms: None,
            cache_hit: None,
            pending_ignore_deletes: HashMap::new(),
            last_user_action: Instant::now(),
            last_search_filter_update: Instant::now(),
        }
    }

    /// Check if system is idle (no user input for 300ms)
    pub fn is_idle(&self) -> bool {
        self.last_user_action.elapsed().as_millis() > 300
    }

    /// Record user action (for idle detection)
    pub fn record_user_action(&mut self) {
        self.last_user_action = Instant::now();
    }
}

impl Default for PerformanceModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_model_creation() {
        let model = PerformanceModel::new();
        assert!(model.prefetch_enabled);
        assert!(model.loading_browse.is_empty());
        assert!(model.pending_ignore_deletes.is_empty());
    }

    #[test]
    fn test_idle_detection() {
        let model = PerformanceModel::new();
        // Should not be idle immediately after creation
        assert!(!model.is_idle());
    }

    #[test]
    fn test_record_user_action() {
        let mut model = PerformanceModel::new();
        let old_time = model.last_user_action;

        std::thread::sleep(std::time::Duration::from_millis(10));
        model.record_user_action();

        assert!(model.last_user_action > old_time);
    }

    #[test]
    fn test_performance_model_is_cloneable() {
        let model = PerformanceModel::new();
        let _cloned = model.clone();
    }

    #[test]
    fn test_discovered_dirs_tracking() {
        let mut model = PerformanceModel::new();

        // Should start empty
        assert!(model.discovered_dirs.is_empty());

        // Can add items
        model.discovered_dirs.insert("folder1:subdir/".to_string());
        assert_eq!(model.discovered_dirs.len(), 1);
        assert!(model.discovered_dirs.contains("folder1:subdir/"));

        // Can check if already discovered
        assert!(model.discovered_dirs.contains("folder1:subdir/"));
        assert!(!model.discovered_dirs.contains("folder1:other/"));

        // Can add multiple
        model.discovered_dirs.insert("folder1:other/".to_string());
        assert_eq!(model.discovered_dirs.len(), 2);

        // Can clear
        model.discovered_dirs.clear();
        assert!(model.discovered_dirs.is_empty());
    }

    #[test]
    fn test_discovered_dirs_prevents_duplicates() {
        let mut model = PerformanceModel::new();

        // Insert same key twice
        model.discovered_dirs.insert("folder:path/".to_string());
        model.discovered_dirs.insert("folder:path/".to_string());

        // Should only have one entry (HashSet behavior)
        assert_eq!(model.discovered_dirs.len(), 1);
    }

    #[test]
    fn test_discovered_dirs_cleared_on_new_search() {
        let mut model = PerformanceModel::new();

        // Simulate previous search session
        model.discovered_dirs.insert("folder:dir1/".to_string());
        model.discovered_dirs.insert("folder:dir2/".to_string());
        assert_eq!(model.discovered_dirs.len(), 2);

        // Simulate starting new search (should clear)
        model.discovered_dirs.clear();
        assert!(model.discovered_dirs.is_empty());

        // Can add new entries for new search
        model.discovered_dirs.insert("folder:dir3/".to_string());
        assert_eq!(model.discovered_dirs.len(), 1);
    }
}
