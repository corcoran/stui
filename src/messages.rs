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
//! Note: Currently unused - will be integrated in Phase 1.3 (Extract Handlers)

#![allow(dead_code)]

use crossterm::event::KeyEvent;

use crate::api_service::ApiResponse;
use crate::event_listener::CacheInvalidation;
use crate::ImagePreviewState;

/// Unified message type for all application events
#[derive(Debug)]
pub enum AppMessage {
    /// User pressed a key
    KeyPress(KeyEvent),

    /// API response received from background service
    ApiResponse(ApiResponse),

    /// Cache invalidation event from Syncthing event stream
    CacheInvalidation(CacheInvalidation),

    /// Event ID update for persistence
    EventIdUpdate(u64),

    /// Background image loading completed
    ImageUpdate {
        file_path: String,
        state: ImagePreviewState,
    },

    /// Periodic tick for time-based updates
    /// (system status refresh, connection stats, UI refresh for live data)
    Tick(TickType),
}

/// Types of periodic updates
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

impl AppMessage {
    /// Create a key press message
    pub fn key_press(key: KeyEvent) -> Self {
        AppMessage::KeyPress(key)
    }

    /// Create an API response message
    pub fn api_response(response: ApiResponse) -> Self {
        AppMessage::ApiResponse(response)
    }

    /// Create a cache invalidation message
    pub fn cache_invalidation(invalidation: CacheInvalidation) -> Self {
        AppMessage::CacheInvalidation(invalidation)
    }

    /// Create an event ID update message
    pub fn event_id_update(event_id: u64) -> Self {
        AppMessage::EventIdUpdate(event_id)
    }

    /// Create an image update message
    pub fn image_update(file_path: String, state: ImagePreviewState) -> Self {
        AppMessage::ImageUpdate { file_path, state }
    }

    /// Create a tick message
    pub fn tick(tick_type: TickType) -> Self {
        AppMessage::Tick(tick_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        // Test helper functions work correctly
        let msg = AppMessage::tick(TickType::UiRefresh);
        assert!(matches!(msg, AppMessage::Tick(TickType::UiRefresh)));

        let event_msg = AppMessage::event_id_update(42);
        assert!(matches!(event_msg, AppMessage::EventIdUpdate(42)));
    }

    #[test]
    fn test_tick_types() {
        // Ensure tick types are distinct
        assert_ne!(TickType::SystemStatus, TickType::ConnectionStats);
        assert_ne!(TickType::UiRefresh, TickType::CleanupCheck);
    }
}
