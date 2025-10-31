//! External Services
//!
//! This module contains services that interact with external systems:
//! - api: API request queue service
//! - events: Event stream listener service

pub mod api;
pub mod events;

// Re-export commonly used types for convenience
pub use api::{ApiRequest, ApiResponse, Priority};
pub use events::CacheInvalidation;
