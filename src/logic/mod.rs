//! Business Logic
//!
//! This module contains pure business logic functions that can be unit tested:
//! - ignore: Pattern matching for .stignore rules
//! - sync_states: Sync state priority and transitions
//! - path: Path mapping and translation utilities

pub mod ignore;
pub mod path;
pub mod sync_states;
