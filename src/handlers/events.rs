//! Event Handler
//!
//! Handles cache invalidation events from the Syncthing event stream.
//! These events tell us when files/directories change so we can refresh the UI.

use crate::services::api::{ApiRequest, Priority};
use crate::services::events::CacheInvalidation;
use crate::App;

/// Handle cache invalidation messages from event listener
///
/// This processes events from Syncthing's event stream (/rest/events endpoint)
/// and updates the cache and UI accordingly.
///
/// Event types handled:
/// - File: Single file changed
/// - Directory: Directory changed (affects all children)
/// - ItemStarted: File started syncing (currently skipped for performance)
/// - ItemFinished: File finished syncing (state updated by LocalIndexUpdated)
pub fn handle_cache_invalidation(app: &mut App, invalidation: CacheInvalidation) {
    match invalidation {
        CacheInvalidation::File {
            folder_id,
            file_path,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: Invalidating file: folder={} path={}",
                folder_id, file_path
            ));
            let _ = app.cache.invalidate_single_file(&folder_id, &file_path);
            let _ = app.cache.invalidate_folder_status(&folder_id);

            // Invalidate out-of-sync categories for this folder (will trigger re-fetch of /rest/db/need)
            let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);

            // If out-of-sync summary modal is open, queue fresh data fetch
            if app.model.ui.out_of_sync_summary.is_some() {
                let _ = app.api_tx.send(ApiRequest::GetNeededFiles {
                    folder_id: folder_id.clone(),
                    page: None,
                    perpage: Some(1000),
                });
            }

            // Invalidate local changed cache for this folder
            let _ = app.cache.invalidate_local_changed(&folder_id);

            // Request fresh folder status
            let _ = app.api_tx.send(ApiRequest::GetFolderStatus {
                folder_id: folder_id.clone(),
            });

            // Update last change info for this folder
            app.model.syncthing.last_folder_updates.insert(
                folder_id.clone(),
                (std::time::SystemTime::now(), file_path.clone()),
            );

            // Extract parent directory path
            let parent_dir = if let Some(last_slash) = file_path.rfind('/') {
                Some(&file_path[..last_slash + 1])
            } else {
                None // File is in root directory
            };

            // Check if we're currently viewing this directory - if so, trigger refresh
            if !app.model.navigation.breadcrumb_trail.is_empty()
                && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
            {
                for (_idx, level) in app.model.navigation.breadcrumb_trail.iter_mut().enumerate() {
                    if level.folder_id == folder_id {
                        // Don't clear state immediately - causes flicker to Unknown
                        // The Browse response will naturally update the state with fresh data

                        // Check if this level is showing the parent directory
                        let level_prefix = level.prefix.as_deref();

                        if level_prefix == parent_dir {
                            // This level is showing the directory containing the changed file
                            // Trigger a fresh browse request
                            let browse_key =
                                format!("{}:{}", folder_id, parent_dir.unwrap_or(""));
                            if !app.model.performance.loading_browse.contains(&browse_key) {
                                app.model.performance.loading_browse.insert(browse_key);

                                let _ =
                                    app.api_tx.send(ApiRequest::BrowseFolder {
                                        folder_id: folder_id.clone(),
                                        prefix: parent_dir.map(|s| s.to_string()),
                                        priority: Priority::High,
                                    });

                                crate::log_debug(&format!("DEBUG [Event]: Triggered refresh for currently viewed directory: {:?}", parent_dir));
                            }
                        }
                    }
                }
            }
        }
        CacheInvalidation::Directory {
            folder_id,
            dir_path,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: Invalidating directory: folder={} path={}",
                folder_id, dir_path
            ));
            let _ = app.cache.invalidate_directory(&folder_id, &dir_path);

            // Invalidate folder status cache to refresh receiveOnlyTotalItems count
            let _ = app.cache.invalidate_folder_status(&folder_id);

            // Invalidate out-of-sync categories for this folder (will trigger re-fetch of /rest/db/need)
            let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);

            // If out-of-sync summary modal is open, queue fresh data fetch
            if app.model.ui.out_of_sync_summary.is_some() {
                let _ = app.api_tx.send(ApiRequest::GetNeededFiles {
                    folder_id: folder_id.clone(),
                    page: None,
                    perpage: Some(1000),
                });
            }

            // Invalidate local changed cache for this folder
            let _ = app.cache.invalidate_local_changed(&folder_id);

            // Request fresh folder status
            let _ = app.api_tx.send(ApiRequest::GetFolderStatus {
                folder_id: folder_id.clone(),
            });

            // Update last change info for this folder
            // RemoteIndexUpdated sends empty dir_path, so show a generic message
            let display_path = if dir_path.is_empty() {
                "(remote changes)".to_string()
            } else {
                dir_path.clone()
            };
            app.model.syncthing.last_folder_updates.insert(
                folder_id.clone(),
                (std::time::SystemTime::now(), display_path),
            );

            // Clear in-memory state for all files in this directory and trigger refresh if viewing
            if !app.model.navigation.breadcrumb_trail.is_empty()
                && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
            {
                for (_idx, level) in app.model.navigation.breadcrumb_trail.iter_mut().enumerate() {
                    if level.folder_id == folder_id {
                        // Remove all states that start with this directory path
                        let dir_prefix = if dir_path.is_empty() {
                            String::new()
                        } else if dir_path.ends_with('/') {
                            dir_path.clone()
                        } else {
                            format!("{}/", dir_path)
                        };

                        level.file_sync_states.retain(|path, _| {
                            if dir_prefix.is_empty() {
                                false // Clear everything for root
                            } else {
                                !path.starts_with(&dir_prefix)
                            }
                        });

                        // Check if this level is showing the changed directory
                        let level_prefix = level.prefix.as_deref();

                        let normalized_dir = if dir_path.is_empty() {
                            None
                        } else {
                            Some(if dir_path.ends_with('/') {
                                dir_path.as_str()
                            } else {
                                &format!("{}/", dir_path)[..]
                            })
                        };

                        // If dir_path is empty (entire folder changed), refresh ALL levels
                        // Otherwise only refresh the specific directory that changed
                        let should_refresh = if dir_path.is_empty() {
                            true // Refresh all levels when entire folder changed
                        } else {
                            level_prefix == normalized_dir
                        };

                        if should_refresh {
                            // This level needs refresh - trigger browse request
                            let browse_key =
                                format!("{}:{}", folder_id, level_prefix.unwrap_or(""));
                            if !app.model.performance.loading_browse.contains(&browse_key) {
                                app.model.performance.loading_browse.insert(browse_key);

                                let _ =
                                    app.api_tx.send(ApiRequest::BrowseFolder {
                                        folder_id: folder_id.clone(),
                                        prefix: level_prefix.map(|s| s.to_string()),
                                        priority: Priority::High,
                                    });

                                crate::log_debug(&format!("DEBUG [Event]: Triggered refresh for directory: {:?} (dir_path={:?})", level_prefix, dir_path));
                            }
                        }
                    }
                }
            }

            // Clear discovered directories cache for this path
            let dir_key_prefix = format!("{}:{}", folder_id, dir_path);
            app.model.performance.discovered_dirs
                .retain(|key| !key.starts_with(&dir_key_prefix));
        }
        CacheInvalidation::ItemStarted {
            folder_id,
            file_path,
        } => {
            // Skip ItemStarted processing entirely during bulk syncs
            // The Syncing state adds visual feedback but isn't essential
            // Files will show correct final state after ItemFinished/LocalIndexUpdated
            // This prevents O(n×m) iteration overhead during bulk operations

            crate::log_debug(&format!(
                "DEBUG [Event]: ItemStarted: folder={} path={} (skipped UI update)",
                folder_id, file_path
            ));
        }
        CacheInvalidation::ItemFinished {
            folder_id,
            file_path,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: ItemFinished: folder={} path={}",
                folder_id, file_path
            ));

            // Invalidate out-of-sync categories for this folder
            // File just finished syncing, so need_category may have changed
            let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);

            // If out-of-sync summary modal is open, queue fresh data fetch
            if app.model.ui.out_of_sync_summary.is_some() {
                let _ = app.api_tx.send(ApiRequest::GetNeededFiles {
                    folder_id: folder_id.clone(),
                    page: None,
                    perpage: Some(1000),
                });
            }

            // Don't clear state or fetch FileInfo - causes flicker and API flood
            // LocalIndexUpdated event will trigger Browse refresh with fresh data
        }
    }
}
