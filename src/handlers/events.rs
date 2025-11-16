//! Event Handler
//!
//! Handles cache invalidation events from the Syncthing event stream.
//! These events tell us when files/directories change so we can refresh the UI.

use crate::App;
use crate::services::api::{ApiRequest, Priority};
use crate::services::events::CacheInvalidation;

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
            timestamp,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: Invalidating file: folder={} path={}",
                folder_id, file_path
            ));

            // Invalidate the specific file
            let _ = app.cache.invalidate_single_file(&folder_id, &file_path);

            // Invalidate all folder-level caches (common pattern)
            app.invalidate_folder_caches(&folder_id);

            // Update last change info for this folder (use event timestamp, not current time)
            app.model
                .syncthing
                .last_folder_updates
                .insert(folder_id.clone(), (timestamp, file_path.clone()));

            // Extract parent directory path
            let parent_dir = file_path
                .rfind('/')
                .map(|last_slash| &file_path[..last_slash + 1]);

            // Check if we're currently viewing this directory - if so, trigger refresh
            if !app.model.navigation.breadcrumb_trail.is_empty()
                && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
            {
                for level in app.model.navigation.breadcrumb_trail.iter_mut() {
                    if level.folder_id == folder_id {
                        // Don't clear state immediately - causes flicker to Unknown
                        // The Browse response will naturally update the state with fresh data

                        // Check if this level is showing the parent directory
                        let level_prefix = level.prefix.as_deref();

                        if level_prefix == parent_dir {
                            // This level is showing the directory containing the changed file
                            // Trigger a fresh browse request
                            let browse_key = format!("{}:{}", folder_id, parent_dir.unwrap_or(""));
                            if !app.model.performance.loading_browse.contains(&browse_key) {
                                app.model.performance.loading_browse.insert(browse_key);

                                let _ = app.api_tx.send(ApiRequest::BrowseFolder {
                                    folder_id: folder_id.clone(),
                                    prefix: parent_dir.map(|s| s.to_string()),
                                    priority: Priority::High,
                                });

                                crate::log_debug(&format!(
                                    "DEBUG [Event]: Triggered refresh for currently viewed directory: {:?}",
                                    parent_dir
                                ));
                            }
                        }
                    }
                }
            }
        }
        CacheInvalidation::Directory {
            folder_id,
            dir_path,
            timestamp: _,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: Invalidating directory: folder={} path={}",
                folder_id, dir_path
            ));

            // Invalidate the directory
            let _ = app.cache.invalidate_directory(&folder_id, &dir_path);

            // Invalidate all folder-level caches (common pattern)
            app.invalidate_folder_caches(&folder_id);

            // Don't update last_folder_updates here - Directory events (RemoteIndexUpdated)
            // don't have specific file paths. We get accurate file paths from:
            // 1. /rest/stats/folder on startup (all files, local and remote)
            // 2. File events with exact paths: LocalIndexUpdated, ItemFinished, LocalChangeDetected, RemoteChangeDetected

            // Clear in-memory state for all files in this directory and trigger refresh if viewing
            if !app.model.navigation.breadcrumb_trail.is_empty()
                && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
            {
                for level in app.model.navigation.breadcrumb_trail.iter_mut() {
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

                                let _ = app.api_tx.send(ApiRequest::BrowseFolder {
                                    folder_id: folder_id.clone(),
                                    prefix: level_prefix.map(|s| s.to_string()),
                                    priority: Priority::High,
                                });

                                crate::log_debug(&format!(
                                    "DEBUG [Event]: Triggered refresh for directory: {:?} (dir_path={:?})",
                                    level_prefix, dir_path
                                ));
                            }
                        }
                    }
                }
            }

            // Clear discovered directories cache for this path
            let dir_key_prefix = format!("{}:{}", folder_id, dir_path);
            app.model
                .performance
                .discovered_dirs
                .retain(|key| !key.starts_with(&dir_key_prefix));
        }
        CacheInvalidation::ItemStarted {
            folder_id,
            file_path,
            timestamp: _,
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
            timestamp: _,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: ItemFinished: folder={} path={}",
                folder_id, file_path
            ));

            // Invalidate out-of-sync cache and refresh summary modal if open
            // File just finished syncing, so need_category may have changed
            app.invalidate_and_refresh_out_of_sync_summary(&folder_id);

            // Don't clear state or fetch FileInfo - causes flicker and API flood
            // LocalIndexUpdated event will trigger Browse refresh with fresh data
        }
        CacheInvalidation::Activity {
            folder_id,
            event_message,
            timestamp,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: Activity: folder='{}' message='{}' timestamp={:?}",
                folder_id, event_message, timestamp
            ));

            // Only update if this event is newer than existing activity
            let should_update = app
                .model
                .ui
                .folder_activity
                .get(&folder_id)
                .map(|(_, existing_time)| timestamp > *existing_time)
                .unwrap_or(true); // Always insert if no existing entry

            if should_update {
                app.model
                    .ui
                    .folder_activity
                    .insert(folder_id.clone(), (event_message, timestamp));
            } else {
                crate::log_debug(&format!(
                    "DEBUG [Event]: Skipping older activity event for folder '{}'",
                    folder_id
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// Create a minimal test App instance
    /// Only initializes the fields needed for Activity event testing
    fn create_test_app() -> App {
        use crate::SyncthingClient;
        use crate::cache::CacheDb;
        use crate::config::Config;
        use crate::model::Model;
        use crate::ui::icons::{IconMode, IconRenderer, IconTheme};
        use std::collections::HashMap;
        use std::time::Duration;

        let config = Config {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:8384".to_string(),
            path_map: HashMap::new(),
            vim_mode: false,
            icon_mode: "emoji".to_string(),
            open_command: None,
            clipboard_command: None,
            image_preview_enabled: false,
            image_protocol: "auto".to_string(),
        };

        let client = SyncthingClient::new(config.api_key.clone(), config.base_url.clone());
        let cache = CacheDb::new_in_memory().expect("Failed to create test cache");
        let (api_tx, _api_rx_temp) = tokio::sync::mpsc::unbounded_channel();
        let (_api_response_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_invalidation_tx, invalidation_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_event_id_tx, event_id_rx) = tokio::sync::mpsc::unbounded_channel();
        let (image_update_tx, image_update_rx) = tokio::sync::mpsc::unbounded_channel();

        App {
            model: Model::new(config.vim_mode),
            client,
            cache,
            api_tx,
            api_rx,
            invalidation_rx,
            event_id_rx,
            icon_renderer: IconRenderer::new(IconMode::Emoji, IconTheme::default()),
            image_picker: None,
            image_update_tx,
            image_update_rx,
            path_map: config.path_map,
            open_command: config.open_command,
            clipboard_command: config.clipboard_command,
            base_url: config.base_url,
            last_status_update: std::time::Instant::now(),
            last_system_status_update: std::time::Instant::now(),
            last_connection_stats_fetch: std::time::Instant::now(),
            last_directory_update: std::time::Instant::now(),
            last_db_flush: std::time::Instant::now(),
            last_reconnect_attempt: std::time::Instant::now(),
            reconnect_delay: Duration::from_secs(1),
            pending_sync_state_writes: Vec::new(),
            image_state_map: HashMap::new(),
        }
    }

    #[test]
    fn test_activity_only_updates_with_newer_timestamp() {
        // Test that older events don't overwrite newer activity
        let mut app = create_test_app();

        let newer_time = SystemTime::now();
        let older_time = newer_time - Duration::from_secs(300); // 5 min ago

        // First, add newer activity
        let newer_invalidation = CacheInvalidation::Activity {
            folder_id: "test-folder".to_string(),
            event_message: "SYNCED file 'new.txt'".to_string(),
            timestamp: newer_time,
        };
        handle_cache_invalidation(&mut app, newer_invalidation);

        // Verify newer activity is stored
        let stored = app.model.ui.folder_activity.get("test-folder");
        assert_eq!(stored.unwrap().0, "SYNCED file 'new.txt'");

        // Now receive older event (replay scenario)
        let older_invalidation = CacheInvalidation::Activity {
            folder_id: "test-folder".to_string(),
            event_message: "SYNCED file 'old.txt'".to_string(),
            timestamp: older_time,
        };
        handle_cache_invalidation(&mut app, older_invalidation);

        // Verify newer activity is STILL there (older event ignored)
        let stored = app.model.ui.folder_activity.get("test-folder");
        assert_eq!(stored.unwrap().0, "SYNCED file 'new.txt'");
        assert_eq!(stored.unwrap().1, newer_time);
    }

    #[test]
    fn test_activity_updates_with_equal_timestamp() {
        // Equal timestamps should NOT update (keep first)
        let mut app = create_test_app();
        let same_time = SystemTime::now();

        let first = CacheInvalidation::Activity {
            folder_id: "test-folder".to_string(),
            event_message: "SYNCED file 'first.txt'".to_string(),
            timestamp: same_time,
        };
        handle_cache_invalidation(&mut app, first);

        let second = CacheInvalidation::Activity {
            folder_id: "test-folder".to_string(),
            event_message: "SYNCED file 'second.txt'".to_string(),
            timestamp: same_time,
        };
        handle_cache_invalidation(&mut app, second);

        let stored = app.model.ui.folder_activity.get("test-folder");
        assert_eq!(stored.unwrap().0, "SYNCED file 'first.txt'");
    }

    #[test]
    fn test_activity_allows_first_event_for_folder() {
        // First event for a folder should always be stored
        let mut app = create_test_app();

        let first_event = CacheInvalidation::Activity {
            folder_id: "new-folder".to_string(),
            event_message: "SYNCED file 'initial.txt'".to_string(),
            timestamp: SystemTime::now(),
        };
        handle_cache_invalidation(&mut app, first_event);

        assert!(app.model.ui.folder_activity.contains_key("new-folder"));
    }

    #[test]
    fn test_activity_independent_per_folder() {
        // Each folder tracks timestamps independently
        let mut app = create_test_app();
        let time1 = SystemTime::now();
        let time2 = time1 + Duration::from_secs(10);

        let folder1 = CacheInvalidation::Activity {
            folder_id: "folder1".to_string(),
            event_message: "SYNCED file 'a.txt'".to_string(),
            timestamp: time1,
        };
        handle_cache_invalidation(&mut app, folder1);

        let folder2 = CacheInvalidation::Activity {
            folder_id: "folder2".to_string(),
            event_message: "SYNCED file 'b.txt'".to_string(),
            timestamp: time2,
        };
        handle_cache_invalidation(&mut app, folder2);

        assert_eq!(app.model.ui.folder_activity.len(), 2);
        assert_eq!(
            app.model.ui.folder_activity.get("folder1").unwrap().1,
            time1
        );
        assert_eq!(
            app.model.ui.folder_activity.get("folder2").unwrap().1,
            time2
        );
    }
}
