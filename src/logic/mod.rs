//! Business Logic
//!
//! This module contains pure business logic functions that can be unit tested:
//! - folder: Folder validation and business logic
//! - ignore: Pattern matching for .stignore rules
//! - path: Path mapping and translation utilities
//! - sync_states: Sync state priority and transitions

pub mod folder;
pub mod ignore;
pub mod path;
pub mod sync_states;
