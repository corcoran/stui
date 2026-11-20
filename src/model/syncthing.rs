//! Syncthing API Model
//!
//! This sub-model contains all state related to the Syncthing API:
//! folders, devices, statuses, system information, and connection stats.

use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime};

use crate::api::{ConnectionStats, Device, Folder, FolderStatus, SyncState, SystemStatus};
use crate::logic::errors::ErrorType;
use crate::logic::ignore::IgnoreMatcher;

/// Connection state for the Syncthing API
#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionState {
    /// Successfully connected to Syncthing API
    Connected,
    /// Attempting to connect (with retry attempt number, optional error, and next retry delay in seconds)
    Connecting {
        attempt: u32,
        last_error: Option<String>,
        next_retry_secs: u64,
    },
    /// Failed to connect (with error type and user-friendly message)
    Disconnected {
        error_type: ErrorType,
        message: String,
    },
}

/// Out-of-sync paths of one folder, from /rest/db/need and /rest/db/localchanged.
///
/// Paths are relative to the folder root with no trailing slash. In-flight downloads (the
/// `progress` list) are not stored here; they live in `SyncthingModel::syncing_files`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeedIndex {
    /// Queued/rest entries of /rest/db/need
    pub need: HashMap<String, SyncState>,
    /// Folder sequence the need list was requested for
    pub sequence: Option<u64>,
    /// Whether the need list holds every needed item of `sequence`
    pub complete: bool,
    /// Ignore patterns fetched with the need list (None: not fetched or failed)
    pub ignore: Option<IgnoreMatcher>,
    /// Receive-only local changes from /rest/db/localchanged
    pub local_changed: HashMap<String, SyncState>,
    /// Folder sequence the local changes were requested for
    pub local_changed_sequence: Option<u64>,
    /// Send time of the need request this index was built from
    pub requested_at: Option<Instant>,
    /// Worst state among the indexed descendants of each directory path
    pub descendants: HashMap<String, SyncState>,
}

/// The latest /rest/db/need request of a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedFetch {
    /// When the latest request was sent
    pub at: Instant,
    /// Folder sequence the latest request was sent for
    pub sequence: u64,
    /// Responses to requests sent before this are dropped (bumped when the folder leaves
    /// syncing, so a late response can't resurrect the syncing set)
    pub floor: Instant,
    /// The latest request failed; the next one waits for the refresh interval
    pub failed: bool,
}

impl NeedFetch {
    /// A fresh, outstanding request sent at `at`
    pub fn new(at: Instant, sequence: u64) -> Self {
        Self {
            at,
            sequence,
            floor: at,
            failed: false,
        }
    }
}

/// Syncthing API data (external state from Syncthing REST API)
#[derive(Clone, Debug)]
pub struct SyncthingModel {
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

    // ============================================
    // SYSTEM STATUS
    // ============================================
    /// Connection state to Syncthing API
    pub connection_state: ConnectionState,

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

    /// Connected device count
    pub connected_device_count: Option<usize>,

    // ============================================
    // FOLDER-SPECIFIC STATE
    // ============================================
    /// Last update timestamp for each folder (folder_id -> (timestamp, filename))
    pub last_folder_updates: HashMap<String, (SystemTime, String)>,

    /// Files Syncthing is actively downloading (folder_id -> paths relative to the folder root,
    /// no trailing slash). Overlaid on breadcrumb sync states at render time.
    pub syncing_files: HashMap<String, HashSet<String>>,

    /// Latest /rest/db/need request per folder; feeds both `syncing_files` and `need_index`
    pub need_fetches: HashMap<String, NeedFetch>,

    /// Out-of-sync paths per folder, used to derive states of items never fetched
    pub need_index: HashMap<String, NeedIndex>,

    /// Folder sequence of the latest /rest/db/localchanged request feeding `need_index`
    pub local_changed_requested: HashMap<String, u64>,

    /// When the latest /rest/db/localchanged request failed; retried after the refresh interval
    pub local_changed_failed_at: HashMap<String, Instant>,
}

impl SyncthingModel {
    /// Create initial empty Syncthing model
    pub fn new() -> Self {
        Self {
            folders: Vec::new(),
            devices: Vec::new(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            connection_state: ConnectionState::Connecting {
                attempt: 0,
                last_error: None,
                next_retry_secs: 5,
            },
            system_status: None,
            connection_stats: None,
            last_connection_stats: None,
            device_name: None,
            last_transfer_rates: None,
            connected_device_count: None,
            last_folder_updates: HashMap::new(),
            syncing_files: HashMap::new(),
            need_fetches: HashMap::new(),
            need_index: HashMap::new(),
            local_changed_requested: HashMap::new(),
            local_changed_failed_at: HashMap::new(),
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

    /// Get summary of local state (total files, dirs, bytes)
    pub fn get_local_state_summary(&self) -> (u64, u64, u64) {
        self.folder_statuses
            .values()
            .fold((0, 0, 0), |(files, dirs, bytes), status| {
                (
                    files + status.local_files,
                    dirs + status.local_directories,
                    bytes + status.local_bytes,
                )
            })
    }
}

impl Default for SyncthingModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syncthing_model_creation() {
        let model = SyncthingModel::new();
        assert_eq!(model.folders.len(), 0);
        assert_eq!(model.devices.len(), 0);
        assert!(!model.statuses_loaded);
    }

    #[test]
    fn test_syncthing_model_is_cloneable() {
        let model = SyncthingModel::new();
        let _cloned = model.clone();
    }

    #[test]
    fn test_connection_state_initial() {
        let model = SyncthingModel::new();
        assert!(matches!(
            model.connection_state,
            ConnectionState::Connecting { attempt: 0, .. }
        ));
    }

    #[test]
    fn test_connection_state_is_cloneable() {
        let state = ConnectionState::Connected;
        let _cloned = state.clone();
    }

    #[test]
    fn test_connection_state_equality() {
        let state1 = ConnectionState::Connected;
        let state2 = ConnectionState::Connected;
        assert_eq!(state1, state2);

        let state3 = ConnectionState::Connecting {
            attempt: 1,
            last_error: None,
            next_retry_secs: 5,
        };
        let state4 = ConnectionState::Connecting {
            attempt: 1,
            last_error: None,
            next_retry_secs: 5,
        };
        assert_eq!(state3, state4);
    }

    #[test]
    fn test_devices_storage() {
        let mut model = SyncthingModel::new();

        let devices = vec![
            crate::api::Device {
                id: "DEVICE1".to_string(),
                name: "Device 1".to_string(),
            },
            crate::api::Device {
                id: "DEVICE2".to_string(),
                name: "Device 2".to_string(),
            },
        ];

        model.devices = devices.clone();
        assert_eq!(model.devices.len(), 2);
        assert_eq!(model.devices[0].name, "Device 1");
    }

    #[test]
    fn test_connected_device_count() {
        let mut model = SyncthingModel::new();

        model.connected_device_count = Some(3);
        assert_eq!(model.connected_device_count, Some(3));
    }
}
