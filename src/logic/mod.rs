//! Business Logic
//!
//! This module contains pure business logic functions that can be unit tested:
//! - display: How an item is shown (icon and status text)
//! - errors: Error classification and formatting
//! - file: File type detection and utilities
//! - file_navigation: File navigation logic for jumping to files
//! - folder: Folder validation and business logic
//! - formatting: Data formatting for human-readable display
//! - ignore: Pattern matching for .stignore rules
//! - layout: UI layout calculations and constraints
//! - navigation: Navigation selection calculations
//! - need_paging: Paging of the need list behind the out-of-sync filter
//! - path: Path mapping and translation utilities
//! - performance: Batching and performance optimizations
//! - platform: Terminal detection for image previews
//! - search: Search query matching and filtering
//! - sorting: Comparison functions for sorting browse items
//! - sync_states: Sync state priority and transitions
//! - ui: UI state transitions and cycling

pub mod clipboard;
pub mod command;
pub mod display;
pub mod errors;
pub mod file;
pub mod file_navigation;
pub mod folder;
pub mod folder_card;
pub mod folder_groups;
pub mod folder_history;
pub mod formatting;
pub mod ignore;
pub mod json;
pub mod layout;
pub mod markdown;
pub mod navigation;
pub mod need_paging;
pub mod open;
pub mod path;
pub mod pending;
pub mod performance;
pub mod platform;
pub mod search;
pub mod sorting;
pub mod sync_states;
pub mod ui;
