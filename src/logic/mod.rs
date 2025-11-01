//! Business Logic
//!
//! This module contains pure business logic functions that can be unit tested:
//! - file: File type detection and utilities
//! - folder: Folder validation and business logic
//! - formatting: Data formatting for human-readable display
//! - ignore: Pattern matching for .stignore rules
//! - layout: UI layout calculations and constraints
//! - navigation: Navigation selection calculations
//! - path: Path mapping and translation utilities
//! - performance: Batching and performance optimizations
//! - search: Search query matching and filtering
//! - sync_states: Sync state priority and transitions
//! - ui: UI state transitions and cycling

pub mod file;
pub mod folder;
pub mod formatting;
pub mod ignore;
pub mod layout;
pub mod navigation;
pub mod path;
pub mod performance;
pub mod search;
pub mod sync_states;
pub mod ui;
