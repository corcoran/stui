//! Event Handlers
//!
//! This module contains handlers for different types of events:
//! - events: Cache invalidation from Syncthing event stream
//! - api: API responses from background service
//! - keyboard: User keyboard input
//!
//! Handlers are currently methods that take &mut App and process events.
//! Future: Refactor to pure functions that take state and return commands (Elm pattern)

pub mod api;
pub mod events;

// Re-export for convenience
pub use api::handle_api_response;
pub use events::handle_cache_invalidation;
