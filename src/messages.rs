//! Message types for the Elm Architecture pattern
//!
//! This module defines all events/messages that can occur in the application.
//! Messages flow into the update loop, get processed, and produce new state.
//!
//! Message sources:
//! - User input (keyboard events)
//! - API responses (from Syncthing REST API)
//! - Event stream (cache invalidation from Syncthing events)
//! - Background tasks (image loading)
//! - Timers (periodic updates)
//!
//! **Current Status:** Msg enum is defined but NOT integrated into main loop.
//! We're taking an incremental approach instead of full Elm Architecture.
//! See ELM_REWRITE_PREP.md for migration strategy.

use crossterm::event::KeyEvent;

use crate::api::{BrowseItem, ConnectionStats, FileDetails, FolderStatus, SystemStatus};
use crate::services::api::ApiResponse;
use crate::services::events::CacheInvalidation;
use crate::ImagePreviewState;

/// Unified message type for all application events
///
/// In Elm Architecture, all state changes flow through messages.
/// This enum captures every event that can occur in the application.
///
/// **Note:** Currently unused - designed for future Elm Architecture integration.
/// The app still uses separate channels (api_rx, invalidation_rx, etc.) for now.
#[allow(dead_code)]
#[derive(Debug)]
pub enum Msg {
    // ============================================
    // USER INPUT
    // ============================================
    /// User pressed a key (triggers navigation, actions, etc.)
    KeyPress(KeyEvent),

    // ============================================
    // SYNCTHING API (updates SyncthingModel)
    // ============================================
    /// Generic API response wrapper (for backward compatibility during migration)
    /// TODO: Phase out in favor of specific variants below
    ApiResponse(ApiResponse),

    /// Browse results loaded from API
    BrowseResult {
        folder_id: String,
        prefix: Option<String>,
        items: Result<Vec<BrowseItem>, String>,
    },

    /// File details loaded from API
    FileInfoResult {
        folder_id: String,
        file_path: String,
        details: Result<FileDetails, String>,
    },

    /// Folder status loaded from API
    FolderStatusResult {
        folder_id: String,
        status: Result<FolderStatus, String>,
    },

    /// Folder rescan completed
    RescanResult {
        folder_id: String,
        success: bool,
        error: Option<String>,
    },

    /// System status loaded from API
    SystemStatusResult {
        status: Result<SystemStatus, String>,
    },

    /// Connection statistics loaded from API
    ConnectionStatsResult {
        stats: Result<ConnectionStats, String>,
    },

    // ============================================
    // CACHE INVALIDATION (triggers background updates)
    // ============================================
    /// Cache invalidation event from Syncthing event stream
    /// Triggers background data refresh when Syncthing state changes
    CacheInvalidation(CacheInvalidation),

    // ============================================
    // PERFORMANCE TRACKING (updates PerformanceModel)
    // ============================================
    /// Event ID update for persistence across restarts
    EventIdUpdate(u64),

    // ============================================
    // UI UPDATES (updates UiModel)
    // ============================================
    /// Background image loading completed (for file info popup)
    ImageUpdate {
        file_path: String,
        state: ImagePreviewState,
    },

    // ============================================
    // PERIODIC UPDATES (all models)
    // ============================================
    /// Periodic tick for time-based updates
    /// (system status refresh, connection stats, UI refresh for live data)
    Tick(TickType),
}

/// Types of periodic updates
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickType {
    /// Update system status (device info, uptime) - every 30s
    SystemStatus,

    /// Update connection statistics (transfer rates) - every 5s
    ConnectionStats,

    /// Update UI for live stats (uptime counter, transfer rates) - every 1s
    UiRefresh,

    /// Check for stale pending deletes and cleanup - every frame when idle
    CleanupCheck,
}

#[allow(dead_code)]
impl Msg {
    /// Create a key press message
    pub fn key_press(key: KeyEvent) -> Self {
        Msg::KeyPress(key)
    }

    /// Create an API response message
    pub fn api_response(response: ApiResponse) -> Self {
        Msg::ApiResponse(response)
    }

    /// Create a cache invalidation message
    pub fn cache_invalidation(invalidation: CacheInvalidation) -> Self {
        Msg::CacheInvalidation(invalidation)
    }

    /// Create an event ID update message
    pub fn event_id_update(event_id: u64) -> Self {
        Msg::EventIdUpdate(event_id)
    }

    /// Create an image update message
    pub fn image_update(file_path: String, state: ImagePreviewState) -> Self {
        Msg::ImageUpdate { file_path, state }
    }

    /// Create a tick message
    pub fn tick(tick_type: TickType) -> Self {
        Msg::Tick(tick_type)
    }

    /// Create a browse result message
    pub fn browse_result(
        folder_id: String,
        prefix: Option<String>,
        items: Result<Vec<BrowseItem>, String>,
    ) -> Self {
        Msg::BrowseResult {
            folder_id,
            prefix,
            items,
        }
    }

    /// Create a file info result message
    pub fn file_info_result(
        folder_id: String,
        file_path: String,
        details: Result<FileDetails, String>,
    ) -> Self {
        Msg::FileInfoResult {
            folder_id,
            file_path,
            details,
        }
    }

    /// Create a folder status result message
    pub fn folder_status_result(folder_id: String, status: Result<FolderStatus, String>) -> Self {
        Msg::FolderStatusResult { folder_id, status }
    }

    /// Create a rescan result message
    pub fn rescan_result(folder_id: String, success: bool, error: Option<String>) -> Self {
        Msg::RescanResult {
            folder_id,
            success,
            error,
        }
    }

    /// Create a system status result message
    pub fn system_status_result(status: Result<SystemStatus, String>) -> Self {
        Msg::SystemStatusResult { status }
    }

    /// Create a connection stats result message
    pub fn connection_stats_result(stats: Result<ConnectionStats, String>) -> Self {
        Msg::ConnectionStatsResult { stats }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        // Test helper functions work correctly
        let msg = Msg::tick(TickType::UiRefresh);
        assert!(matches!(msg, Msg::Tick(TickType::UiRefresh)));

        let event_msg = Msg::event_id_update(42);
        assert!(matches!(event_msg, Msg::EventIdUpdate(42)));
    }

    #[test]
    fn test_tick_types() {
        // Ensure tick types are distinct
        assert_ne!(TickType::SystemStatus, TickType::ConnectionStats);
        assert_ne!(TickType::UiRefresh, TickType::CleanupCheck);
    }
}
