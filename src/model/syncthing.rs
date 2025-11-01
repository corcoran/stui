//! Syncthing API Model
//!
//! This sub-model contains all state related to the Syncthing API:
//! folders, devices, statuses, system information, and connection stats.

use std::collections::HashMap;
use std::time::{Instant, SystemTime};

use crate::api::{ConnectionStats, Device, Folder, FolderStatus, SystemStatus};
use crate::logic::errors::ErrorType;

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

    // ============================================
    // FOLDER-SPECIFIC STATE
    // ============================================
    /// Last update timestamp for each folder (folder_id -> (timestamp, filename))
    pub last_folder_updates: HashMap<String, (SystemTime, String)>,
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
            last_folder_updates: HashMap::new(),
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
}
