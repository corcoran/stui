//! Syncthing TUI Library
//!
//! Exposes modules for testing

use std::sync::atomic::AtomicBool;

pub mod api;
pub mod cache;
pub mod logic;
pub mod model;
pub mod services;
pub mod utils;

// DEBUG_MODE for cache logging (defaults to false in tests)
pub(crate) static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

// Re-export common types from main.rs that are needed by other modules
// These will be made available at crate:: level

/// File info display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Off,              // No timestamp or size
    TimestampOnly,    // Show timestamp only
    TimestampAndSize, // Show both size and timestamp
}

/// Sort mode for file listings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    VisualIndicator, // Sort by sync state icon (directories first, then by state priority)
    Alphabetical,    // Sort alphabetically
    LastModified,    // Sort by last modified time (if available)
    FileSize,        // Sort by file size
}

impl SortMode {
    pub fn as_str(&self) -> &str {
        match self {
            SortMode::VisualIndicator => "Sync State",
            SortMode::Alphabetical => "A-Z",
            SortMode::LastModified => "Timestamp",
            SortMode::FileSize => "Size",
        }
    }
}
