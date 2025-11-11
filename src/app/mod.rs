//! App Orchestration Methods
//!
//! This module contains App implementation methods grouped by domain.
//! Each submodule contains methods that orchestrate between:
//! - Model state (pure, in src/model/)
//! - Services (API client, cache)
//! - Handlers (in src/handlers/)
//! - Logic (pure business logic in src/logic/)
//! - UI rendering (in src/ui/)
//!
//! Methods are kept as `impl App` but organized by functional domain
//! for better discoverability and maintainability.

pub(crate) mod file_ops;
pub(crate) mod filters;
pub(crate) mod ignore;
pub(crate) mod navigation;
pub(crate) mod preview;
pub(crate) mod sorting;
pub(crate) mod sync_states;
