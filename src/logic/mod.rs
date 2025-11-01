//! Business Logic
//!
//! This module contains pure business logic functions that can be unit tested:
//! - file: File type detection and utilities
//! - folder: Folder validation and business logic
//! - ignore: Pattern matching for .stignore rules
//! - navigation: Navigation selection calculations
//! - path: Path mapping and translation utilities
//! - sync_states: Sync state priority and transitions
//! - ui: UI state transitions and cycling

pub mod file;
pub mod folder;
pub mod ignore;
pub mod navigation;
pub mod path;
pub mod sync_states;
pub mod ui;
