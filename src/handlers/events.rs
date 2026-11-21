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
/// - ItemStarted: File started syncing (added to the folder's syncing set)
/// - ItemFinished: File finished syncing (removed from the syncing set; final state arrives
///   via LocalIndexUpdated)
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
                                    requested_at: std::time::Instant::now(),
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

            // Existing states stay on screen until the re-browse and its FileInfo requests replace them
            if !app.model.navigation.breadcrumb_trail.is_empty()
                && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
            {
                for level in app.model.navigation.breadcrumb_trail.iter_mut() {
                    if level.folder_id == folder_id {
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
                                    requested_at: std::time::Instant::now(),
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
            crate::log_debug(&format!(
                "DEBUG [Event]: ItemStarted: folder={} path={}",
                folder_id, file_path
            ));

            app.model.syncthing.record_path_event(
                &folder_id,
                &file_path,
                crate::model::syncthing::PathEventKind::Started,
                false,
            );
            app.model
                .syncthing
                .syncing_files
                .entry(folder_id)
                .or_default()
                .insert(file_path.trim_end_matches('/').to_string());
        }
        CacheInvalidation::ItemFinished {
            folder_id,
            file_path,
            failed,
            deleted,
            timestamp: _,
        } => {
            crate::log_debug(&format!(
                "DEBUG [Event]: ItemFinished: folder={} path={}",
                folder_id, file_path
            ));

            app.model.syncthing.record_path_event(
                &folder_id,
                &file_path,
                crate::model::syncthing::PathEventKind::Finished,
                !failed && !deleted,
            );
            app.observe_path(
                &folder_id,
                file_path.trim_end_matches('/'),
                crate::logic::pending::Observation::ItemFinished { failed, deleted },
            );
            if let Some(set) = app.model.syncthing.syncing_files.get_mut(&folder_id) {
                set.remove(file_path.trim_end_matches('/'));
            }

            // A downloaded item leaves the need list; a failed one stays
            if !failed {
                let _ = app
                    .cache
                    .clear_need_path(&folder_id, file_path.trim_end_matches('/'));
            }
            app.note_need_list_changed(&folder_id);

            // A finished download leaves the local copy matching the global one, so show it
            // synced now; the Browse refresh after LocalIndexUpdated confirms it. No FileInfo
            // fetch here: it would flood the API during large syncs.
            if !failed && !deleted {
                let path = file_path.trim_end_matches('/');
                // No longer a local change, even if a local-changes list fetched mid-restore
                // still lists it as deleted
                if let Some(index) = app.model.syncthing.need_index.get_mut(&folder_id) {
                    crate::logic::sync_states::forget_local_change(index, path);
                }
                for level in app
                    .model
                    .navigation
                    .breadcrumb_trail
                    .iter_mut()
                    .filter(|l| l.folder_id == folder_id)
                {
                    if let Some(name) =
                        crate::logic::sync_states::name_in_level(level.prefix.as_deref(), path)
                        && level.items.iter().any(|i| i.name == name)
                    {
                        level
                            .file_sync_states
                            .insert(name.to_string(), crate::api::SyncState::Synced);
                    }
                }
                app.pending_sync_state_writes.push((
                    folder_id.clone(),
                    path.to_string(),
                    crate::api::SyncState::Synced,
                    0,
                ));
            }
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
    use crate::test_support::*;
    use std::time::{Duration, SystemTime};

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

    // ========================================
    // SYNCING FILES
    // ========================================

    use crate::api::{BrowseItem, SyncState};
    use crate::handlers::api::handle_api_response;
    use crate::services::api::ApiResponse;
    use std::collections::HashMap;

    fn syncing_files_requests(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<String> {
        let mut folders = Vec::new();
        while let Ok(req) = rx.try_recv() {
            if let crate::services::api::ApiRequest::GetFolderNeed { folder_id, .. } = req {
                folders.push(folder_id);
            }
        }
        folders
    }

    #[test]
    fn test_item_started_adds_to_syncing_set() {
        let mut app = create_test_app();

        handle_cache_invalidation(&mut app, item_started("media", "movies/a.mkv"));

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["movies/a.mkv"]))
        );
    }

    #[test]
    fn test_item_finished_removes_from_syncing_set() {
        let mut app = create_test_app();
        handle_cache_invalidation(&mut app, item_started("media", "a.mkv"));
        handle_cache_invalidation(&mut app, item_started("media", "b.mkv"));

        handle_cache_invalidation(&mut app, item_finished("media", "a.mkv"));

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["b.mkv"]))
        );
    }

    #[test]
    fn test_item_events_show_syncing_icon_for_file_and_parent_dir() {
        let mut app = create_test_app();
        view_folder_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), dir_item("sub"), dir_item("subway")],
        );

        handle_cache_invalidation(&mut app, item_started("media", "a.mkv"));
        handle_cache_invalidation(&mut app, item_started("media", "sub/b.mkv"));

        let states = displayed_states(&app);
        assert_eq!(states.get("a.mkv"), Some(&SyncState::Syncing));
        assert_eq!(states.get("sub"), Some(&SyncState::Syncing));
        assert_eq!(states.get("subway"), Some(&SyncState::RemoteOnly));

        handle_cache_invalidation(&mut app, item_finished("media", "a.mkv"));
        assert_eq!(
            displayed_states(&app).get("a.mkv"),
            Some(&SyncState::Synced),
            "a finished download shows synced without waiting for a refresh"
        );
    }

    #[test]
    fn test_late_launch_syncing_files_response_marks_files_syncing() {
        // stui started after transfers began: no ItemStarted events ever arrive
        let mut app = create_test_app();
        view_folder_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), file_item("b.mkv"), file_item("c.mkv")],
        );

        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &["a.mkv", "b.mkv"], &[]));

        let states = displayed_states(&app);
        assert_eq!(states.get("a.mkv"), Some(&SyncState::Syncing));
        assert_eq!(states.get("b.mkv"), Some(&SyncState::Syncing));
        assert_eq!(states.get("c.mkv"), Some(&SyncState::RemoteOnly));
        // Base states are untouched
        assert_eq!(
            app.model.navigation.breadcrumb_trail[0]
                .file_sync_states
                .get("a.mkv"),
            Some(&SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_syncing_files_response_does_not_enable_out_of_sync_filter() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);

        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &["a.mkv"], &[]));

        assert!(app.model.ui.out_of_sync_filter.is_none());
        assert!(
            app.model.navigation.breadcrumb_trail[0]
                .filtered_items
                .is_none()
        );
        assert!(app.model.ui.toast.is_none());
    }

    #[test]
    fn test_syncing_files_response_replaces_folder_set() {
        let mut app = create_test_app();
        handle_cache_invalidation(&mut app, item_started("media", "old.mkv"));
        handle_cache_invalidation(&mut app, item_started("other", "x.mkv"));

        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &["new.mkv"], &[]));

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["new.mkv"]))
        );
        assert_eq!(
            app.model.syncthing.syncing_files.get("other"),
            Some(&paths(&["x.mkv"]))
        );
    }

    #[test]
    fn test_syncing_files_error_keeps_existing_set() {
        let mut app = create_test_app();
        handle_cache_invalidation(&mut app, item_started("media", "a.mkv"));

        let at = mark_fetched(&mut app, "media");
        handle_api_response(
            &mut app,
            ApiResponse::FolderNeed {
                folder_id: "media".to_string(),
                sequence: 0,
                requested_at: at,
                need: Err(anyhow::anyhow!("connection refused")),
                ignore_patterns: Err(anyhow::anyhow!("connection refused")),
            },
        );

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["a.mkv"]))
        );
    }

    #[test]
    fn test_folder_status_idle_clears_syncing_set() {
        let mut app = create_test_app();
        handle_api_response(&mut app, status_result("media", "syncing"));
        handle_cache_invalidation(&mut app, item_started("media", "a.mkv"));
        handle_cache_invalidation(&mut app, item_started("other", "x.mkv"));

        handle_api_response(&mut app, status_result("media", "idle"));

        assert!(!app.model.syncthing.syncing_files.contains_key("media"));
        assert_eq!(
            app.model.syncthing.syncing_files.get("other"),
            Some(&paths(&["x.mkv"]))
        );
    }

    #[test]
    fn test_folder_status_into_syncing_requests_syncing_files() {
        let (mut app, mut rx) = create_test_app_with_api_rx();

        // Not open anywhere: idle polls fetch nothing
        handle_api_response(&mut app, status_result("media", "idle"));
        assert!(syncing_files_requests(&mut rx).is_empty());

        handle_api_response(&mut app, status_result("media", "syncing"));
        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_folder_status_first_poll_syncing_requests_syncing_files() {
        // Late launch: the very first status already says syncing
        let (mut app, mut rx) = create_test_app_with_api_rx();

        handle_api_response(&mut app, status_result("media", "syncing"));

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_folder_status_repeated_syncing_poll_is_throttled() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        handle_api_response(&mut app, status_result("media", "syncing"));
        let _ = syncing_files_requests(&mut rx);

        handle_api_response(&mut app, status_result("media", "syncing"));
        handle_api_response(&mut app, status_result("media", "syncing"));

        assert!(syncing_files_requests(&mut rx).is_empty());
    }

    #[test]
    fn test_folder_status_syncing_refetches_after_interval() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        handle_api_response(&mut app, status_result("media", "syncing"));
        let _ = syncing_files_requests(&mut rx);

        let stale = std::time::Instant::now()
            - crate::logic::sync_states::SYNCING_FILES_REFRESH_INTERVAL
            - Duration::from_secs(1);
        app.model
            .syncthing
            .need_fetches
            .get_mut("media")
            .expect("fetch recorded")
            .at = stale;
        handle_api_response(&mut app, status_result("media", "syncing"));

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_late_syncing_files_response_after_idle_is_dropped() {
        let mut app = create_test_app();
        handle_api_response(&mut app, status_result("media", "syncing"));
        let syncing_fetch = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(&mut app, status_result("media", "idle"));

        // Response to the fetch queued by the "syncing" poll arrives after the clear
        handle_api_response(
            &mut app,
            need_result("media", syncing_fetch, &["a.mkv"], &["q.mkv"]),
        );

        assert!(!app.model.syncthing.syncing_files.contains_key("media"));
        assert!(
            app.model
                .syncthing
                .need_index
                .get("media")
                .is_none_or(|index| index.sequence.is_none()),
            "stale need list must not reach the index"
        );
    }

    #[test]
    fn test_item_started_trailing_slash_is_trimmed() {
        let mut app = create_test_app();

        handle_cache_invalidation(&mut app, item_started("media", "sub/dir/"));

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["sub/dir"]))
        );
    }

    #[tokio::test]
    async fn test_syncing_files_response_after_entering_folder_is_applied() {
        let mut app = create_test_app();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();

        app.enter_folder("media").await.unwrap();
        let at = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(&mut app, need_result("media", at, &["a.mkv"], &[]));

        assert_eq!(
            app.model.syncthing.syncing_files.get("media"),
            Some(&paths(&["a.mkv"]))
        );
    }

    fn media_folder() -> crate::api::Folder {
        serde_json::from_value(serde_json::json!({
            "id": "media",
            "label": "Media",
            "path": "/data/media",
            "paused": false,
            "type": "receiveonly",
        }))
        .expect("valid Folder")
    }

    #[tokio::test]
    async fn test_entering_folder_requests_syncing_files() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![
            serde_json::from_value(serde_json::json!({
                "id": "media",
                "label": "Media",
                "path": "/data/media",
                "paused": false,
                "type": "receiveonly",
            }))
            .expect("valid Folder"),
        ];
        app.model.navigation.folders_state_selection = Some(0);
        // Cache hit so no network call is made
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();

        app.enter_folder("media").await.unwrap();

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    // ========================================
    // STATE LOADING (no Unknown flashes)
    // ========================================

    fn file_info_result(folder: &str, path: &str, state: SyncState) -> ApiResponse {
        ApiResponse::FileInfoResult {
            folder_id: folder.to_string(),
            file_path: path.to_string(),
            details: Box::new(Ok(file_details(state))),
            requested_at: std::time::Instant::now(),
        }
    }

    fn file_info_result_at(
        folder: &str,
        path: &str,
        state: SyncState,
        requested_at: std::time::Instant,
    ) -> ApiResponse {
        ApiResponse::FileInfoResult {
            folder_id: folder.to_string(),
            file_path: path.to_string(),
            details: Box::new(Ok(file_details(state))),
            requested_at,
        }
    }

    #[test]
    fn test_file_info_requested_before_item_finished_is_dropped() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        let t0 = std::time::Instant::now();
        app.model
            .performance
            .loading_sync_states
            .insert("f:a".to_string());
        std::thread::sleep(std::time::Duration::from_millis(5));
        handle_cache_invalidation(&mut app, item_finished("f", "a"));

        handle_api_response(
            &mut app,
            file_info_result_at("f", "a", SyncState::RemoteOnly, t0),
        );

        assert_eq!(displayed_states(&app).get("a"), Some(&SyncState::Synced));
        assert!(!app.model.performance.loading_sync_states.contains("f:a"));
        assert!(
            app.pending_sync_state_writes
                .iter()
                .all(|(_, _, state, _)| *state == SyncState::Synced)
        );
    }

    #[test]
    fn test_file_info_requested_after_item_finished_is_applied() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .insert("a".to_string(), SyncState::Synced);
        handle_cache_invalidation(&mut app, item_finished("f", "a"));
        std::thread::sleep(std::time::Duration::from_millis(5));
        let t1 = std::time::Instant::now();

        handle_api_response(
            &mut app,
            file_info_result_at("f", "a", SyncState::RemoteOnly, t1),
        );

        assert_eq!(
            app.model.navigation.breadcrumb_trail[0]
                .file_sync_states
                .get("a"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            app.pending_sync_state_writes.last().map(|w| w.2),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_need_reply_requested_before_item_started_keeps_started_path() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a"), file_item("b")]);
        let at = mark_fetched(&mut app, "f");
        std::thread::sleep(std::time::Duration::from_millis(5));
        handle_cache_invalidation(&mut app, item_started("f", "b"));

        handle_api_response(&mut app, need_result("f", at, &["a"], &[]));

        let states = displayed_states(&app);
        assert_eq!(states.get("a"), Some(&SyncState::Syncing));
        assert_eq!(states.get("b"), Some(&SyncState::Syncing));
    }

    #[test]
    fn test_need_reply_requested_before_item_finished_drops_finished_path() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a"), file_item("b")]);
        let at = mark_fetched(&mut app, "f");
        std::thread::sleep(std::time::Duration::from_millis(5));
        handle_cache_invalidation(&mut app, item_finished("f", "a"));

        handle_api_response(&mut app, need_result("f", at, &["a", "b"], &[]));

        let syncing = app.model.syncthing.syncing_files.get("f").unwrap();
        assert!(!syncing.contains("a"));
        assert!(syncing.contains("b"));
    }

    fn file_info_requests(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<String> {
        let mut paths = Vec::new();
        while let Ok(req) = rx.try_recv() {
            if let crate::services::api::ApiRequest::GetFileInfo { file_path, .. } = req {
                paths.push(file_path);
            }
        }
        paths
    }

    fn file_info_requests_with_priority(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<(String, crate::services::api::Priority)> {
        let mut requests = Vec::new();
        while let Ok(req) = rx.try_recv() {
            let priority = req.priority();
            if let crate::services::api::ApiRequest::GetFileInfo { file_path, .. } = req {
                requests.push((file_path, priority));
            }
        }
        requests
    }

    fn sub_level(
        folder: &str,
        prefix: &str,
        items: Vec<BrowseItem>,
    ) -> crate::model::BreadcrumbLevel {
        crate::model::BreadcrumbLevel {
            folder_id: folder.to_string(),
            folder_label: folder.to_string(),
            folder_path: format!("/data/{}", folder),
            prefix: Some(prefix.to_string()),
            items,
            filtered_items: None,
            selected_index: Some(0),
            file_sync_states: HashMap::new(),
            direct_states: Default::default(),
            ignored_exists: HashMap::new(),
            translated_base_path: format!("/data/{}/{}", folder, prefix.trim_end_matches('/')),
        }
    }

    #[test]
    fn test_invalidate_and_refresh_keeps_states_through_empty_cache_browse() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), file_item("b.mkv")],
        );

        app.invalidate_and_refresh_folder("media");
        let browse_sent = std::iter::from_fn(|| rx.try_recv().ok())
            .any(|r| matches!(r, crate::services::api::ApiRequest::BrowseFolder { .. }));
        assert!(browse_sent, "refresh should re-browse the level");

        handle_api_response(
            &mut app,
            ApiResponse::BrowseResult {
                folder_id: "media".to_string(),
                prefix: None,
                items: Ok(vec![file_item("a.mkv"), file_item("b.mkv")]),
                requested_at: std::time::Instant::now(),
            },
        );

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("a.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            level.file_sync_states.get("b.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        // Kept states are only placeholders: fresh FileInfo must be requested for them
        assert_eq!(
            file_info_requests(&mut rx),
            vec!["a.mkv".to_string(), "b.mkv".to_string()]
        );
    }

    #[test]
    fn test_browse_result_drops_states_of_removed_items() {
        let mut app = create_test_app();
        view_folder_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), file_item("b.mkv")],
        );

        handle_api_response(
            &mut app,
            ApiResponse::BrowseResult {
                folder_id: "media".to_string(),
                prefix: None,
                items: Ok(vec![file_item("a.mkv")]),
                requested_at: std::time::Instant::now(),
            },
        );

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("a.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        assert!(!level.file_sync_states.contains_key("b.mkv"));
    }

    #[test]
    fn test_root_directory_invalidation_keeps_states() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv"), dir_item("sub")]);

        handle_cache_invalidation(
            &mut app,
            CacheInvalidation::Directory {
                folder_id: "media".to_string(),
                dir_path: String::new(),
                timestamp: SystemTime::now(),
            },
        );

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("a.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            level.file_sync_states.get("sub"),
            Some(&SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_file_info_sets_state_on_preview_level() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .clear();
        app.model.navigation.focus_level = 0;

        handle_api_response(
            &mut app,
            file_info_result("media", "a.mkv", SyncState::Synced),
        );

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("a.mkv"),
            Some(&SyncState::Synced)
        );
        assert_eq!(app.pending_sync_state_writes.len(), 1);
    }

    #[test]
    fn test_file_info_for_folder_not_in_trail_still_caches() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);

        handle_api_response(
            &mut app,
            file_info_result("docs", "a.mkv", SyncState::Synced),
        );

        assert_eq!(app.pending_sync_state_writes.len(), 1);
        assert_eq!(app.pending_sync_state_writes[0].0, "docs");
        assert_eq!(app.pending_sync_state_writes[0].1, "a.mkv");
        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("a.mkv"),
            Some(&SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_file_info_stored_only_at_direct_parent_level() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![dir_item("stui")]);
        app.model.navigation.breadcrumb_trail.push(sub_level(
            "media",
            "stui/",
            vec![file_item("README.md")],
        ));
        app.model.navigation.focus_level = 2;

        handle_api_response(
            &mut app,
            file_info_result("media", "stui/README.md", SyncState::Synced),
        );

        let root = &app.model.navigation.breadcrumb_trail[0];
        assert!(!root.file_sync_states.contains_key("stui/README.md"));
        let sub = &app.model.navigation.breadcrumb_trail[1];
        assert_eq!(
            sub.file_sync_states.get("README.md"),
            Some(&SyncState::Synced)
        );
    }

    #[tokio::test]
    async fn test_entering_directory_queues_file_info_for_unloaded_items() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.cache
            .save_browse_items(
                "media",
                Some("sub/"),
                &[file_item("x.mkv"), file_item("y.mkv")],
                0,
            )
            .unwrap();
        app.cache
            .save_sync_state("media", "sub/y.mkv", SyncState::Synced, 1)
            .unwrap();
        view_folder_root(&mut app, "media", vec![dir_item("sub")]);
        while rx.try_recv().is_ok() {}

        app.enter_directory().await.unwrap();

        assert_eq!(app.model.navigation.focus_level, 2);
        assert_eq!(file_info_requests(&mut rx), vec!["sub/x.mkv".to_string()]);
        assert!(
            app.model
                .performance
                .loading_sync_states
                .contains("media:sub/x.mkv")
        );
    }

    #[tokio::test]
    async fn test_preview_root_level_queues_low_priority_capped_file_info() {
        use crate::logic::sync_states::PREVIEW_FILE_INFO_CAP;
        use crate::services::api::Priority;
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        let items: Vec<BrowseItem> = (0..PREVIEW_FILE_INFO_CAP + 10)
            .map(|i| file_item(&format!("f{:02}.mkv", i)))
            .collect();
        app.cache
            .save_browse_items("media", None, &items, 0)
            .unwrap();

        app.load_root_level(true).await.unwrap();

        assert_eq!(app.model.navigation.focus_level, 0);
        let requests = file_info_requests_with_priority(&mut rx);
        assert_eq!(requests.len(), PREVIEW_FILE_INFO_CAP);
        assert!(requests.iter().all(|(_, p)| *p == Priority::Low));
    }

    #[tokio::test]
    async fn test_load_root_level_entry_queues_medium_file_info() {
        use crate::services::api::Priority;
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        let items: Vec<BrowseItem> = (0..60)
            .map(|i| file_item(&format!("f{:02}.mkv", i)))
            .collect();
        app.cache
            .save_browse_items("media", None, &items, 0)
            .unwrap();

        app.load_root_level(false).await.unwrap();

        let requests = file_info_requests_with_priority(&mut rx);
        assert_eq!(
            requests.len(),
            crate::logic::sync_states::NAVIGATION_FILE_INFO_CAP
        );
        assert!(requests.iter().all(|(_, p)| *p == Priority::Medium));
    }

    // ========================================
    // DERIVED STATES (need index)
    // ========================================

    /// Breadcrumb view of `folder` root with no states loaded yet
    fn view_unloaded_root(app: &mut App, folder: &str, items: Vec<BrowseItem>) {
        view_folder_root(app, folder, items);
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .clear();
    }

    fn set_status(app: &mut App, folder: &str, state: &str, receive_only_total: u64) {
        let mut status = folder_status(state);
        status.receive_only_total_items = receive_only_total;
        app.model
            .syncthing
            .folder_statuses
            .insert(folder.to_string(), status);
    }

    fn documents_local_changes() -> Vec<crate::api::LocalChangedFile> {
        [
            ("todo.txt", false),
            ("manuals/router-setup.txt", true),
            ("recipes/pancakes.md", false),
        ]
        .into_iter()
        .map(|(name, deleted)| crate::api::LocalChangedFile {
            name: name.to_string(),
            deleted,
        })
        .collect()
    }

    fn local_changed_result(folder: &str) -> ApiResponse {
        ApiResponse::LocalChangedIndex {
            folder_id: folder.to_string(),
            sequence: 0,
            files: Ok(documents_local_changes()),
            requested_at: std::time::Instant::now(),
        }
    }

    fn local_changed_index_requests(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<(String, u64)> {
        let mut requests = Vec::new();
        while let Ok(req) = rx.try_recv() {
            if let crate::services::api::ApiRequest::GetLocalChangedIndex {
                folder_id,
                sequence,
                ..
            } = req
            {
                requests.push((folder_id, sequence));
            }
        }
        requests
    }

    /// Documents root (receive-only, idle) with nothing visited and the local changes fetched
    fn documents_with_local_changes() -> App {
        let mut app = create_test_app();
        view_unloaded_root(
            &mut app,
            "docs",
            vec![
                dir_item("manuals"),
                dir_item("recipes"),
                file_item("todo.txt"),
            ],
        );
        // manuals/ itself has local == global, so its own FileInfo says Synced
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .insert("manuals".to_string(), SyncState::Synced);
        set_status(&mut app, "docs", "idle", 3);
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 0);
        handle_api_response(&mut app, local_changed_result("docs"));
        app
    }

    #[test]
    fn test_need_response_marks_queued_file_remote_only_on_open_level() {
        let mut app = create_test_app();
        view_unloaded_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), file_item("q.mkv")],
        );
        set_status(&mut app, "media", "syncing", 0);

        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &["a.mkv"], &["q.mkv"]));

        assert_eq!(
            app.model.syncthing.need_index["media"].need.get("q.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("q.mkv"),
            Some(&SyncState::RemoteOnly)
        );
        assert!(!level.file_sync_states.contains_key("a.mkv"));
        assert_eq!(
            displayed_states(&app).get("a.mkv"),
            Some(&SyncState::Syncing)
        );
    }

    #[test]
    fn test_local_changed_response_marks_unvisited_dirs_in_documents_root() {
        let app = documents_with_local_changes();

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("manuals"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            level.file_sync_states.get("recipes"),
            Some(&SyncState::LocalOnly)
        );
        assert_eq!(
            level.file_sync_states.get("todo.txt"),
            Some(&SyncState::LocalOnly)
        );
    }

    #[test]
    fn test_local_changed_response_updates_every_open_level_of_folder() {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "docs", vec![dir_item("manuals")]);
        app.model.navigation.breadcrumb_trail.push(sub_level(
            "docs",
            "manuals/",
            vec![file_item("router-setup.txt")],
        ));
        app.model.navigation.focus_level = 0;
        set_status(&mut app, "docs", "idle", 3);
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 0);

        handle_api_response(&mut app, local_changed_result("docs"));

        let trail = &app.model.navigation.breadcrumb_trail;
        assert_eq!(
            trail[0].file_sync_states.get("manuals"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            trail[1].file_sync_states.get("router-setup.txt"),
            Some(&SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_file_info_synced_for_dir_keeps_derived_remote_only_aggregate() {
        let mut app = documents_with_local_changes();

        handle_api_response(
            &mut app,
            file_info_result("docs", "manuals", SyncState::Synced),
        );

        assert_eq!(
            app.model.navigation.breadcrumb_trail[0]
                .file_sync_states
                .get("manuals"),
            Some(&SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_superseded_local_changed_response_is_dropped() {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "docs", vec![dir_item("manuals")]);
        set_status(&mut app, "docs", "idle", 3);
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 9);

        handle_api_response(&mut app, local_changed_result("docs"));

        assert!(!app.model.syncthing.need_index.contains_key("docs"));
    }

    #[test]
    fn test_idle_complete_need_index_marks_unlisted_items_synced() {
        let mut app = create_test_app();
        view_unloaded_root(
            &mut app,
            "projects",
            vec![dir_item("notes"), file_item("readme.md")],
        );
        set_status(&mut app, "projects", "idle", 0);

        let at = mark_fetched(&mut app, "projects");
        handle_api_response(&mut app, need_result("projects", at, &[], &[]));

        let level = &app.model.navigation.breadcrumb_trail[0];
        assert_eq!(
            level.file_sync_states.get("notes"),
            Some(&SyncState::Synced)
        );
        assert_eq!(
            level.file_sync_states.get("readme.md"),
            Some(&SyncState::Synced)
        );
    }

    #[test]
    fn test_receive_only_status_requests_local_changes_once_per_sequence() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_unloaded_root(&mut app, "docs", vec![dir_item("manuals")]);
        let mut status = folder_status("idle");
        status.receive_only_total_items = 3;
        status.sequence = 4;
        let result = |status: &crate::api::FolderStatus| ApiResponse::FolderStatusResult {
            folder_id: "docs".to_string(),
            status: Ok(status.clone()),
            requested_at: std::time::Instant::now(),
        };

        handle_api_response(&mut app, result(&status));
        assert_eq!(
            local_changed_index_requests(&mut rx),
            vec![("docs".to_string(), 4)]
        );

        handle_api_response(&mut app, result(&status));
        assert!(local_changed_index_requests(&mut rx).is_empty());

        status.receive_only_total_items = 0;
        status.sequence = 5;
        handle_api_response(&mut app, result(&status));
        assert!(local_changed_index_requests(&mut rx).is_empty());
        assert_eq!(
            app.model.syncthing.need_index["docs"].local_changed_sequence,
            Some(5)
        );
    }

    fn set_status_seq(app: &mut App, folder: &str, state: &str, sequence: u64, ro_total: u64) {
        set_status(app, folder, state, ro_total);
        app.model
            .syncthing
            .folder_statuses
            .get_mut(folder)
            .unwrap()
            .sequence = sequence;
    }

    fn root_state(app: &App, name: &str) -> Option<SyncState> {
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .get(name)
            .copied()
    }

    #[test]
    fn test_derived_dir_clears_after_sync_finishes() {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "media", vec![dir_item("d")]);
        set_status(&mut app, "media", "syncing", 0);
        let t1 = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", t1, &[], &["d/x.mkv"]));
        assert_eq!(root_state(&app, "d"), Some(SyncState::RemoteOnly));

        // The re-browse's FileInfo for d is resolved against the still-listing index
        handle_api_response(&mut app, file_info_result("media", "d", SyncState::Synced));
        assert_eq!(root_state(&app, "d"), Some(SyncState::RemoteOnly));

        // Idle with the same sequence: only the empty need page arrives
        set_status(&mut app, "media", "idle", 0);
        let t2 = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", t2, &[], &[]));
        assert_eq!(root_state(&app, "d"), Some(SyncState::Synced));
    }

    #[test]
    fn test_derived_dir_clears_after_revert() {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "docs", vec![dir_item("d")]);
        set_status_seq(&mut app, "docs", "idle", 1, 1);
        let t1 = mark_fetched(&mut app, "docs");
        handle_api_response(&mut app, need_result_seq("docs", t1, 1, &[], &[]));
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 1);
        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "docs".to_string(),
                sequence: 1,
                files: Ok(vec![crate::api::LocalChangedFile {
                    name: "d/x".to_string(),
                    deleted: true,
                }]),
                requested_at: std::time::Instant::now(),
            },
        );
        assert_eq!(root_state(&app, "d"), Some(SyncState::RemoteOnly));

        // Revert: a new sequence with no local changes, then its need page
        let mut status = folder_status("idle");
        status.sequence = 2;
        handle_api_response(
            &mut app,
            ApiResponse::FolderStatusResult {
                folder_id: "docs".to_string(),
                status: Ok(status),
                requested_at: std::time::Instant::now(),
            },
        );
        let t2 = app.model.syncthing.need_fetches["docs"].at;
        handle_api_response(&mut app, need_result_seq("docs", t2, 2, &[], &[]));
        assert_eq!(root_state(&app, "d"), Some(SyncState::Synced));
    }

    #[test]
    fn test_receive_only_count_drop_refreshes_open_levels() {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "docs", vec![dir_item("d")]);
        set_status_seq(&mut app, "docs", "idle", 1, 1);
        let t1 = mark_fetched(&mut app, "docs");
        app.model
            .syncthing
            .need_fetches
            .get_mut("docs")
            .unwrap()
            .sequence = 1;
        handle_api_response(&mut app, need_result_seq("docs", t1, 1, &[], &[]));
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 1);
        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "docs".to_string(),
                sequence: 1,
                files: Ok(vec![crate::api::LocalChangedFile {
                    name: "d/x".to_string(),
                    deleted: false,
                }]),
                requested_at: std::time::Instant::now(),
            },
        );
        assert_eq!(root_state(&app, "d"), Some(SyncState::LocalOnly));

        let mut status = folder_status("idle");
        status.sequence = 1;
        handle_api_response(
            &mut app,
            ApiResponse::FolderStatusResult {
                folder_id: "docs".to_string(),
                status: Ok(status),
                requested_at: std::time::Instant::now(),
            },
        );

        assert_eq!(root_state(&app, "d"), Some(SyncState::Synced));
    }

    #[test]
    fn test_failed_need_fetch_backs_off_before_retrying() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        handle_api_response(&mut app, status_result("media", "syncing"));
        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
        let at = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(
            &mut app,
            ApiResponse::FolderNeed {
                folder_id: "media".to_string(),
                sequence: 0,
                requested_at: at,
                need: Err(anyhow::anyhow!("timeout")),
                ignore_patterns: Err(anyhow::anyhow!("timeout")),
            },
        );

        handle_api_response(&mut app, status_result("media", "syncing"));
        assert!(syncing_files_requests(&mut rx).is_empty());

        app.model
            .syncthing
            .need_fetches
            .get_mut("media")
            .unwrap()
            .at = std::time::Instant::now()
            - crate::logic::sync_states::SYNCING_FILES_REFRESH_INTERVAL
            - Duration::from_secs(1);
        handle_api_response(&mut app, status_result("media", "syncing"));
        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_superseded_need_response_newer_than_index_is_applied() {
        let mut app = create_test_app();
        view_unloaded_root(
            &mut app,
            "media",
            vec![file_item("q.mkv"), file_item("r.mkv")],
        );
        set_status(&mut app, "media", "syncing", 0);
        let t1 = mark_fetched(&mut app, "media");
        // A 10s refresh supersedes t1 before its response arrives
        app.request_folder_need("media", 0, false);

        handle_api_response(&mut app, need_result("media", t1, &[], &["q.mkv"]));
        assert_eq!(root_state(&app, "q.mkv"), Some(SyncState::RemoteOnly));

        // An even older response than the index is dropped
        let t0 = t1 - Duration::from_secs(1);
        app.model
            .syncthing
            .need_fetches
            .get_mut("media")
            .unwrap()
            .floor = t0;
        handle_api_response(&mut app, need_result("media", t0, &[], &["r.mkv"]));
        assert!(
            !app.model.syncthing.need_index["media"]
                .need
                .contains_key("r.mkv")
        );
    }

    #[tokio::test]
    async fn test_entering_folder_with_current_index_skips_need_fetch() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();
        set_status(&mut app, "media", "idle", 0);
        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &[], &[]));
        while rx.try_recv().is_ok() {}

        app.enter_folder("media").await.unwrap();

        assert!(syncing_files_requests(&mut rx).is_empty());
    }

    // ========================================
    // INDEX FETCH SCOPE AND BACK-OFF
    // ========================================

    fn ro_status_result(folder: &str, sequence: u64, ro_total: u64) -> ApiResponse {
        let mut status = folder_status("idle");
        status.sequence = sequence;
        status.receive_only_total_items = ro_total;
        ApiResponse::FolderStatusResult {
            folder_id: folder.to_string(),
            status: Ok(status),
            requested_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn test_idle_folder_without_open_level_gets_no_index_fetches() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_unloaded_root(&mut app, "media", vec![file_item("a.mkv")]);

        handle_api_response(&mut app, ro_status_result("docs", 1, 3));
        handle_api_response(&mut app, ro_status_result("docs", 2, 3));

        let requests: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            !requests.iter().any(|r| matches!(
                r,
                crate::services::api::ApiRequest::GetFolderNeed { .. }
                    | crate::services::api::ApiRequest::GetLocalChangedIndex { .. }
            )),
            "unopened idle folder must not be indexed: {requests:?}"
        );
    }

    #[test]
    fn test_previewed_idle_folder_gets_need_fetch_on_status_poll() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_unloaded_root(&mut app, "media", vec![file_item("a.mkv")]);
        app.model.navigation.focus_level = 0;

        handle_api_response(&mut app, status_result("media", "idle"));

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_syncing_folder_without_open_level_gets_need_fetch() {
        let (mut app, mut rx) = create_test_app_with_api_rx();

        handle_api_response(&mut app, status_result("media", "syncing"));

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[tokio::test]
    async fn test_preview_load_requests_need_index() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();

        app.load_root_level(true).await.unwrap();
        app.model.performance.last_user_action = std::time::Instant::now() - Duration::from_secs(1);
        app.ensure_preview_need_index_if_idle();

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[test]
    fn test_failed_local_changed_fetch_backs_off_before_retrying() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_unloaded_root(&mut app, "docs", vec![dir_item("manuals")]);
        handle_api_response(&mut app, ro_status_result("docs", 4, 3));
        assert_eq!(
            local_changed_index_requests(&mut rx),
            vec![("docs".to_string(), 4)]
        );
        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "docs".to_string(),
                sequence: 4,
                files: Err(anyhow::anyhow!("timeout")),
                requested_at: std::time::Instant::now(),
            },
        );

        handle_api_response(&mut app, ro_status_result("docs", 4, 3));
        assert!(local_changed_index_requests(&mut rx).is_empty());

        app.model.syncthing.local_changed_failed_at.insert(
            "docs".to_string(),
            std::time::Instant::now()
                - crate::logic::sync_states::SYNCING_FILES_REFRESH_INTERVAL
                - Duration::from_secs(1),
        );
        handle_api_response(&mut app, ro_status_result("docs", 4, 3));
        assert_eq!(
            local_changed_index_requests(&mut rx),
            vec![("docs".to_string(), 4)]
        );
    }

    // ========================================
    // DIRECTORY CHILD AGGREGATE (cache queries)
    // ========================================

    /// Media root with an unvisited-by-index dir "sub" whose one child is cached as `child`
    fn root_with_cached_child(child: SyncState) -> App {
        let mut app = create_test_app();
        view_unloaded_root(&mut app, "media", vec![dir_item("sub")]);
        app.cache
            .save_browse_items("media", Some("sub/"), &[file_item("a.mkv")], 0)
            .unwrap();
        app.cache
            .save_sync_state("media", "sub/a.mkv", child, 1)
            .unwrap();
        app
    }

    #[test]
    fn test_dir_child_aggregate_reuses_cache_queries_within_interval() {
        let mut app = root_with_cached_child(SyncState::RemoteOnly);
        app.update_directory_states(0);
        assert_eq!(root_state(&app, "sub"), Some(SyncState::RemoteOnly));

        app.cache
            .save_sync_state("media", "sub/a.mkv", SyncState::Synced, 2)
            .unwrap();
        // A cache write that bypasses the app's write paths isn't seen until the memo expires
        app.update_directory_states(0);
        assert_eq!(
            root_state(&app, "sub"),
            Some(SyncState::RemoteOnly),
            "children are re-queried at most once per interval"
        );

        for (at, _) in app.dir_child_states.values_mut() {
            *at -= crate::logic::sync_states::DIR_CHILD_STATES_TTL + Duration::from_millis(1);
        }
        app.update_directory_states(0);
        assert_eq!(root_state(&app, "sub"), Some(SyncState::Synced));
    }

    #[test]
    fn test_derived_dir_state_applies_immediately_after_aggregate_run() {
        let mut app = root_with_cached_child(SyncState::Synced);
        app.update_directory_states(0);
        assert_eq!(root_state(&app, "sub"), Some(SyncState::Synced));

        set_status(&mut app, "media", "syncing", 0);
        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &[], &["sub/b.mkv"]));

        assert_eq!(root_state(&app, "sub"), Some(SyncState::RemoteOnly));
    }

    // ========================================
    // REVIEW REGRESSIONS (closed folders, memo, preview)
    // ========================================

    #[test]
    fn test_folder_leaving_syncing_while_closed_gets_fetch_when_opened() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        handle_api_response(&mut app, status_result("media", "syncing"));
        let pending = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(&mut app, status_result("media", "idle"));
        handle_api_response(&mut app, need_result("media", pending, &[], &["a.mkv"]));
        let _ = syncing_files_requests(&mut rx);

        view_unloaded_root(&mut app, "media", vec![file_item("a.mkv")]);
        app.ensure_need_index("media");

        assert_eq!(syncing_files_requests(&mut rx), vec!["media".to_string()]);
    }

    #[tokio::test]
    async fn test_open_after_skipped_refetch_shows_no_stale_remote_only() {
        let mut app = create_test_app();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.navigation.folders_state_selection = Some(0);
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();
        // Mid-sync index fetched while the folder is closed
        handle_api_response(&mut app, status_result("media", "syncing"));
        let mid_sync = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(&mut app, need_result("media", mid_sync, &[], &["a.mkv"]));
        // Sync finishes while closed: the refetch is skipped
        handle_api_response(&mut app, status_result("media", "idle"));

        app.enter_folder("media").await.unwrap();
        assert_ne!(root_state(&app, "a.mkv"), Some(SyncState::RemoteOnly));

        let fresh = app.model.syncthing.need_fetches["media"].at;
        handle_api_response(&mut app, need_result("media", fresh, &[], &[]));
        assert_eq!(root_state(&app, "a.mkv"), Some(SyncState::Synced));
    }

    #[test]
    fn test_ignore_inside_subdir_updates_parent_level_dir_on_next_tick() {
        let mut app = root_with_cached_child(SyncState::RemoteOnly);
        app.model.navigation.breadcrumb_trail.push(sub_level(
            "media",
            "sub/",
            vec![file_item("a.mkv")],
        ));
        app.model.navigation.focus_level = 2;
        app.update_directory_states(0);
        assert_eq!(root_state(&app, "sub"), Some(SyncState::RemoteOnly));

        handle_api_response(
            &mut app,
            file_info_result("media", "sub/a.mkv", SyncState::Ignored),
        );
        app.flush_pending_db_writes();
        app.last_directory_update = std::time::Instant::now() - Duration::from_secs(3);
        app.tick_directory_states();

        assert_eq!(root_state(&app, "sub"), Some(SyncState::Synced));
    }

    #[tokio::test]
    async fn test_rapid_preview_changes_defer_need_fetch_until_rest() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        let mut docs = media_folder();
        docs.id = "docs".to_string();
        app.model.syncthing.folders = vec![media_folder(), docs];
        for folder in ["media", "docs"] {
            app.cache
                .save_browse_items(folder, None, &[file_item("a.mkv")], 0)
                .unwrap();
        }

        app.model.navigation.folders_state_selection = Some(0);
        app.load_root_level(true).await.unwrap();
        app.model.navigation.folders_state_selection = Some(1);
        app.load_root_level(true).await.unwrap();
        app.model.performance.last_user_action = std::time::Instant::now();
        app.ensure_preview_need_index_if_idle();
        assert!(syncing_files_requests(&mut rx).is_empty());

        app.model.performance.last_user_action = std::time::Instant::now() - Duration::from_secs(1);
        app.ensure_preview_need_index_if_idle();
        assert_eq!(syncing_files_requests(&mut rx), vec!["docs".to_string()]);
    }

    #[test]
    fn test_late_local_changed_failure_for_old_sequence_does_not_back_off() {
        let mut app = create_test_app();
        app.model
            .syncthing
            .local_changed_requested
            .insert("docs".to_string(), 5);

        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "docs".to_string(),
                sequence: 4,
                files: Err(anyhow::anyhow!("timeout")),
                requested_at: std::time::Instant::now(),
            },
        );

        assert!(
            !app.model
                .syncthing
                .local_changed_failed_at
                .contains_key("docs")
        );
    }

    // ========================================
    // UNIGNORE
    // ========================================

    /// Minimal Syncthing stand-in: GET returns `ignore_patterns`, anything else returns 200.
    async fn spawn_ignore_stub(ignore_patterns: &[&str]) -> String {
        spawn_recording_stub(ignore_patterns).await.0
    }

    type RequestLog = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

    /// Like `spawn_ignore_stub`, also recording each request line (`POST /rest/db/scan?... HTTP/1.1`)
    async fn spawn_recording_stub(ignore_patterns: &[&str]) -> (String, RequestLog) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = serde_json::json!({ "ignore": ignore_patterns }).to_string();
        let log: RequestLog = Default::default();
        let log_for_task = log.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let body = body.clone();
                let log = log_for_task.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_string();
                    log.lock()
                        .unwrap()
                        .push(request.lines().next().unwrap_or("").to_string());
                    let payload = if request.starts_with("GET ") {
                        body.as_str()
                    } else {
                        ""
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{}", addr), log)
    }

    async fn app_viewing_item(
        ignore_patterns: &[&str],
        state: SyncState,
        prefix: Option<&str>,
        base_path: &str,
    ) -> (
        App,
        tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
        RequestLog,
    ) {
        let (mut app, rx) = create_test_app_with_api_rx();
        let (base_url, log) = spawn_recording_stub(ignore_patterns).await;
        app.client = crate::SyncthingClient::new(base_url, "k".to_string());
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);
        let level = &mut app.model.navigation.breadcrumb_trail[0];
        level.prefix = prefix.map(str::to_string);
        level.translated_base_path = base_path.to_string();
        level.file_sync_states.insert("a.mkv".to_string(), state);
        (app, rx, log)
    }

    /// Temp directory removed on drop, so a failing assertion doesn't leak it
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn with_file(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "stui-scan-test-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), b"x").unwrap();
            TempDir(dir)
        }

        fn path_str(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// (folder_id, paths) of every queued RescanFolder request
    fn queued_rescans(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<(String, Vec<String>)> {
        let mut rescans = Vec::new();
        while let Ok(req) = rx.try_recv() {
            if let crate::services::api::ApiRequest::RescanFolder { folder_id, paths } = req {
                rescans.push((folder_id, paths));
            }
        }
        rescans
    }

    async fn app_viewing_ignored_item(
        ignore_patterns: &[&str],
    ) -> (
        App,
        tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) {
        let (mut app, rx) = create_test_app_with_api_rx();
        let base_url = spawn_ignore_stub(ignore_patterns).await;
        app.client = crate::SyncthingClient::new(base_url, "k".to_string());
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);
        app.model.navigation.breadcrumb_trail[0]
            .file_sync_states
            .insert("a.mkv".to_string(), SyncState::Ignored);
        (app, rx)
    }

    async fn press(app: &mut App, code: crossterm::event::KeyCode) {
        let key = crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
        crate::handlers::keyboard::handle_key(app, key)
            .await
            .unwrap();
    }

    /// Answer the queued rescan of `media` and expect a FileInfo request for `path` after it
    fn expect_file_info_after_rescan(
        app: &mut App,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
        path: &str,
    ) {
        let mut file_info_before_scan = false;
        let mut rescanned = false;
        while let Ok(req) = rx.try_recv() {
            match req {
                crate::services::api::ApiRequest::RescanFolder { folder_id, .. }
                    if folder_id == "media" =>
                {
                    rescanned = true;
                }
                crate::services::api::ApiRequest::GetFileInfo { .. } => {
                    file_info_before_scan = true;
                }
                _ => {}
            }
        }
        assert!(rescanned, "the rescan goes through the API queue");
        assert!(
            !file_info_before_scan,
            "no FileInfo before the rescan returns"
        );

        handle_api_response(
            app,
            ApiResponse::RescanResult {
                folder_id: "media".to_string(),
                success: true,
                error: None,
            },
        );

        let requested: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|req| match req {
                crate::services::api::ApiRequest::GetFileInfo { file_path, .. } => Some(file_path),
                _ => None,
            })
            .collect();
        assert_eq!(requested, vec![path.to_string()]);
    }

    fn assert_unignore_feedback(app: &App) {
        assert_eq!(
            app.model.navigation.breadcrumb_trail[0]
                .file_sync_states
                .get("a.mkv"),
            Some(&SyncState::Ignored),
            "icon stays Ignored until FileInfo replaces it"
        );
        let toast = app.model.ui.toast.as_ref().expect("toast shown");
        assert_eq!(toast.message, "Unignored a.mkv, rescanning...");
        assert_eq!(toast.kind, crate::model::ToastKind::Success);
    }

    #[tokio::test]
    async fn test_unignore_key_keeps_state_shows_toast_and_requests_file_info_after_rescan() {
        let (mut app, mut rx) = app_viewing_ignored_item(&["/a.mkv"]).await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert!(app.model.ui.pattern_selection.is_none());
        assert_unignore_feedback(&app);
        expect_file_info_after_rescan(&mut app, &mut rx, "a.mkv");
    }

    #[tokio::test]
    async fn test_unignore_via_pattern_menu_keeps_state_shows_toast_and_requests_file_info_after_rescan()
     {
        let (mut app, mut rx) = app_viewing_ignored_item(&["/a.mkv", "/*.mkv"]).await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        assert!(app.model.ui.pattern_selection.is_some());
        assert!(
            app.model.ui.toast.is_none(),
            "no toast until a pattern is chosen"
        );

        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert_unignore_feedback(&app);
        expect_file_info_after_rescan(&mut app, &mut rx, "a.mkv");
    }

    #[tokio::test]
    async fn test_enter_folder_queues_file_info() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.cache
            .save_browse_items("media", None, &[file_item("a.mkv")], 0)
            .unwrap();

        app.enter_folder("media").await.unwrap();

        assert_eq!(
            file_info_requests_with_priority(&mut rx),
            vec![("a.mkv".to_string(), crate::services::api::Priority::Medium)]
        );
    }

    // ========================================
    // PENDING IGNORE CHANGES
    // ========================================

    fn displayed_media(app: &App, name: &str) -> Option<crate::logic::display::ItemDisplay> {
        let level = &app.model.navigation.breadcrumb_trail[0];
        let item = level
            .items
            .iter()
            .find(|i| i.name == name)
            .expect("item listed");
        crate::logic::display::level_item_display(
            &app.model.syncthing,
            level,
            item,
            &app.model.pending,
        )
    }

    fn pending_kind(app: &App, path: &str) -> Option<crate::model::pending::PendingKind> {
        app.model
            .pending
            .get("media", path)
            .map(|change| change.kind.clone())
    }

    #[tokio::test]
    async fn test_ignore_then_stale_file_info_keeps_ignored() {
        use crate::logic::display::ItemDisplay;
        let (mut app, _rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;
        let sent = std::time::Instant::now() - Duration::from_millis(1);

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        handle_api_response(
            &mut app,
            file_info_result_at("media", "a.mkv", SyncState::Synced, sent),
        );

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: false
            })
        );
        assert_eq!(
            app.model.navigation.breadcrumb_trail[0].file_sync_states["a.mkv"],
            SyncState::Ignored
        );
        assert_eq!(
            displayed_media(&app, "a.mkv"),
            Some(ItemDisplay::Ignored { exists: false })
        );
    }

    #[tokio::test]
    async fn test_ignore_records_whether_the_file_exists_on_disk() {
        let dir = TempDir::with_file("a.mkv");
        let (mut app, _rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, dir.path_str()).await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Ignored {
                exists: true,
                deleting: false
            })
        );
        assert_eq!(
            app.cache
                .get_sync_state_unvalidated("media", "a.mkv")
                .unwrap(),
            None,
            "the pending change guards the base state; the cache isn't written"
        );
    }

    #[tokio::test]
    async fn test_ignore_and_delete_then_stale_listing_keeps_ignored() {
        use crate::logic::display::ItemDisplay;
        let dir = TempDir::with_file("a.mkv");
        let (mut app, _rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, dir.path_str()).await;
        app.cache
            .save_sync_state("media", "a.mkv", SyncState::Synced, 0)
            .unwrap();
        let sent = std::time::Instant::now() - Duration::from_millis(1);

        press(&mut app, crossterm::event::KeyCode::Char('I')).await;
        handle_api_response(
            &mut app,
            ApiResponse::BrowseResult {
                folder_id: "media".to_string(),
                prefix: None,
                items: Ok(vec![file_item("a.mkv")]),
                requested_at: sent,
            },
        );

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true
            })
        );
        assert_eq!(
            displayed_media(&app, "a.mkv"),
            Some(ItemDisplay::Ignored { exists: false })
        );
    }

    #[tokio::test]
    async fn test_plain_ignore_of_missing_file_does_not_block_unignore() {
        let (mut app, _rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert!(app.model.pending.get("media", "a.mkv").is_some());
        assert!(!app.model.pending.blocks_unignore("media", "a.mkv"));
        assert_eq!(app.model.pending.ignore_delete_count(), 0);
    }

    #[tokio::test]
    async fn test_ignore_and_delete_of_missing_file_records_ignored() {
        let (mut app, _rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('I')).await;

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true
            })
        );
    }

    #[tokio::test]
    async fn test_unignore_pre_scan_reply_after_post_scan_reply_keeps_unignored_state() {
        let (mut app, _rx, _log) =
            app_viewing_item(&["/a.mkv"], SyncState::Ignored, None, "/nonexistent/media").await;
        let pre_scan = std::time::Instant::now() - Duration::from_millis(1);

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Unignored)
        );
        handle_api_response(
            &mut app,
            file_info_result("media", "a.mkv", SyncState::Synced),
        );
        assert_eq!(pending_kind(&app, "a.mkv"), None);
        handle_api_response(
            &mut app,
            file_info_result_at("media", "a.mkv", SyncState::Ignored, pre_scan),
        );

        assert_eq!(
            app.model.navigation.breadcrumb_trail[0].file_sync_states["a.mkv"],
            SyncState::Synced
        );
    }

    #[tokio::test]
    async fn test_unignore_menu_records_unignored() {
        let (mut app, _rx, _log) = app_viewing_item(
            &["/a.mkv", "/*.mkv"],
            SyncState::Ignored,
            None,
            "/nonexistent/media",
        )
        .await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Unignored)
        );
    }

    #[tokio::test]
    async fn test_unignore_menu_blocked_while_ignore_delete_pending() {
        let (mut app, _rx, log) = app_viewing_item(
            &["/a.mkv", "/*.mkv"],
            SyncState::Ignored,
            None,
            "/nonexistent/media",
        )
        .await;
        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        assert!(app.model.ui.pattern_selection.is_some());
        let before = ignore_bodies(&log).len();
        app.record_pending(
            "media",
            "a.mkv",
            crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true,
            },
        );

        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert_eq!(ignore_bodies(&log).len(), before, "no patterns are written");
        let toast = app.model.ui.toast.clone().expect("toast shown");
        assert_eq!(
            toast.message,
            "Cannot un-ignore: deletion in progress for a.mkv"
        );
        assert_eq!(toast.kind, crate::model::ToastKind::Warning);
        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true
            })
        );
    }

    #[tokio::test]
    async fn test_ignore_then_unignore_replaces_pending_change() {
        let (mut app, _rx, _log) =
            app_viewing_item(&["/a.mkv"], SyncState::Ignored, None, "/nonexistent/media").await;
        app.record_pending(
            "media",
            "a.mkv",
            crate::model::pending::PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            pending_kind(&app, "a.mkv"),
            Some(crate::model::pending::PendingKind::Unignored)
        );
    }

    #[tokio::test]
    async fn test_unignore_not_applied_by_syncthing_expires() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&["/a.mkv"], SyncState::Ignored, None, "/nonexistent/media").await;
        app.model.navigation.breadcrumb_trail[0]
            .ignored_exists
            .insert("a.mkv".to_string(), true);

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
        handle_api_response(
            &mut app,
            ApiResponse::RescanResult {
                folder_id: "media".to_string(),
                success: true,
                error: None,
            },
        );
        handle_api_response(
            &mut app,
            file_info_result("media", "a.mkv", SyncState::Ignored),
        );
        assert_eq!(displayed_media(&app, "a.mkv"), None);

        app.expire_pending(std::time::Instant::now() + crate::model::pending::PATH_DEADLINE);

        assert_eq!(pending_kind(&app, "a.mkv"), None);
        assert_eq!(
            displayed_media(&app, "a.mkv"),
            Some(crate::logic::display::ItemDisplay::Ignored { exists: true })
        );
    }

    // ========================================
    // SCOPED RESCANS
    // ========================================

    #[tokio::test]
    async fn test_ignore_key_scans_only_the_item() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_ignore_key_in_subdirectory_scans_the_folder_relative_path() {
        let (mut app, mut rx, _log) = app_viewing_item(
            &[],
            SyncState::Synced,
            Some("my notes/"),
            "/nonexistent/media/my notes",
        )
        .await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["my notes/a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_unignore_key_scans_only_the_item() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&["/a.mkv"], SyncState::Ignored, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_unignore_with_glob_pattern_scans_everything() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&["/*.mkv"], SyncState::Ignored, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), Vec::<String>::new())]
        );
    }

    #[tokio::test]
    async fn test_pattern_menu_literal_pattern_scans_only_the_item() {
        let (mut app, mut rx, _log) = app_viewing_item(
            &["/a.mkv", "/*.mkv"],
            SyncState::Ignored,
            None,
            "/nonexistent/media",
        )
        .await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_pattern_menu_glob_pattern_scans_everything() {
        let (mut app, mut rx, _log) = app_viewing_item(
            &["/a.mkv", "/*.mkv"],
            SyncState::Ignored,
            None,
            "/nonexistent/media",
        )
        .await;

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;
        press(&mut app, crossterm::event::KeyCode::Down).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), Vec::<String>::new())]
        );
    }

    #[tokio::test]
    async fn test_ignore_and_delete_scans_only_the_deleted_path() {
        let dir = TempDir::with_file("a.mkv");
        let (mut app, mut rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, dir.path_str()).await;

        press(&mut app, crossterm::event::KeyCode::Char('I')).await;

        assert!(!dir.0.join("a.mkv").exists());
        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_delete_requests_scan_of_the_deleted_path() {
        let dir = TempDir::with_file("a.mkv");
        let (mut app, mut rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, dir.path_str()).await;

        press(&mut app, crossterm::event::KeyCode::Char('d')).await;
        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        assert!(!dir.0.join("a.mkv").exists());
        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), vec!["a.mkv".to_string()])]
        );
    }

    #[tokio::test]
    async fn test_rescan_key_requests_full_scan() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), Vec::<String>::new())]
        );
    }

    #[tokio::test]
    async fn test_force_refresh_requests_full_scan() {
        let (mut app, mut rx, _log) =
            app_viewing_item(&[], SyncState::Synced, None, "/nonexistent/media").await;

        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        press(&mut app, crossterm::event::KeyCode::Char('f')).await;

        assert_eq!(
            queued_rescans(&mut rx),
            vec![("media".to_string(), Vec::<String>::new())]
        );
    }

    // ========================================
    // NEED LIST PAGING
    // ========================================

    fn status_with_need(sequence: u64, need_total: u64) -> crate::api::FolderStatus {
        let mut status = folder_status("idle");
        status.sequence = sequence;
        status.need_total_items = need_total;
        status
    }

    /// Page `page` of the need list, every entry in `rest`
    fn need_files_page(
        folder: &str,
        generation: u64,
        sequence: u64,
        page: u32,
        perpage: u32,
        names: &[&str],
    ) -> ApiResponse {
        ApiResponse::NeededFiles {
            folder_id: folder.to_string(),
            generation,
            sequence,
            page,
            perpage,
            response: Ok(crate::api::NeedResponse {
                progress: vec![],
                queued: vec![],
                rest: names
                    .iter()
                    .map(|n| crate::api::FileInfo {
                        name: n.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                page,
                perpage,
            }),
        }
    }

    #[derive(Debug, PartialEq)]
    struct NeedPageRequest {
        folder: String,
        page: u32,
        perpage: u32,
        generation: u64,
        sequence: u64,
        priority: crate::services::api::Priority,
    }

    fn need_page_requests(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> Vec<NeedPageRequest> {
        let mut requests = Vec::new();
        while let Ok(req) = rx.try_recv() {
            let priority = req.priority();
            if let crate::services::api::ApiRequest::GetNeededFiles {
                folder_id,
                page,
                perpage,
                generation,
                sequence,
            } = req
            {
                requests.push(NeedPageRequest {
                    folder: folder_id,
                    page,
                    perpage,
                    generation,
                    sequence,
                    priority,
                });
            }
        }
        requests
    }

    /// App viewing the root of "media" (files a..e), status sequence 10 needing `total` items,
    /// with the filter toggled on. Returns the generation of the page 1 request.
    fn app_paging_media(
        total: u64,
    ) -> (
        App,
        tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
        u64,
    ) {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(
            &mut app,
            "media",
            ["a", "b", "c", "d", "e"]
                .iter()
                .map(|n| file_item(n))
                .collect(),
        );
        app.model
            .syncthing
            .folder_statuses
            .insert("media".to_string(), status_with_need(10, total));
        app.model
            .performance
            .last_known_sequences
            .insert("media".to_string(), 10);

        app.toggle_out_of_sync_filter();
        let requests = need_page_requests(&mut rx);
        assert_eq!(requests.len(), 1, "toggle should request page 1");
        assert_eq!(requests[0].page, 1);
        assert_eq!(requests[0].sequence, 10);
        assert_eq!(requests[0].priority, crate::services::api::Priority::High);
        let generation = requests[0].generation;
        (app, rx, generation)
    }

    fn filtered_names(app: &App) -> Vec<String> {
        let mut names: Vec<String> = app.model.navigation.breadcrumb_trail[0]
            .filtered_items
            .as_ref()
            .map(|items| items.iter().map(|i| i.name.clone()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    fn paging_progress(app: &App) -> Option<crate::logic::need_paging::NeedPagingProgress> {
        app.model
            .syncthing
            .need_paging
            .get("media")
            .and_then(crate::logic::need_paging::need_paging_progress)
    }

    #[test]
    fn test_need_paging_complete_first_page_requests_nothing_more() {
        let (mut app, mut rx, generation) = app_paging_media(2);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );

        assert!(app.model.ui.out_of_sync_filter.is_some());
        assert_eq!(filtered_names(&app), vec!["a", "b"]);
        assert!(need_page_requests(&mut rx).is_empty());
        assert!(app.model.syncthing.need_paging["media"].finished);
    }

    #[test]
    fn test_need_paging_requests_next_page_at_low_priority() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );

        assert_eq!(
            need_page_requests(&mut rx),
            vec![NeedPageRequest {
                folder: "media".to_string(),
                page: 2,
                perpage: 2,
                generation,
                sequence: 10,
                priority: crate::services::api::Priority::Low,
            }]
        );
    }

    #[test]
    fn test_need_paging_merges_later_pages_into_filter() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b"]);

        let_reapply_interval_pass(&mut app, "media");
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d"]);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);
        let pages: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(pages, vec![2, 3]);
    }

    #[test]
    fn test_need_paging_progress_mid_paging_and_cleared_when_complete() {
        let (mut app, _rx, generation) = app_paging_media(5);
        assert_eq!(paging_progress(&app), None);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        assert_eq!(
            paging_progress(&app),
            Some(crate::logic::need_paging::NeedPagingProgress {
                loaded: 2,
                total: 5,
                partial: false
            })
        );

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert_eq!(paging_progress(&app).map(|p| p.loaded), Some(4));

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );
        assert_eq!(paging_progress(&app), None);
    }

    #[test]
    fn test_need_paging_drops_other_generation() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            need_files_page("media", generation + 1, 10, 1, 2, &["a", "b"]),
        );

        assert!(app.model.ui.out_of_sync_filter.is_none());
        assert!(app.cache.get_out_of_sync_items("media").unwrap().is_empty());
        assert!(need_page_requests(&mut rx).is_empty());
    }

    #[test]
    fn test_need_paging_drops_old_sequence() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 9, 1, 2, &["a", "b"]),
        );

        assert!(app.model.ui.out_of_sync_filter.is_none());
        assert!(app.cache.get_out_of_sync_items("media").unwrap().is_empty());
        assert!(need_page_requests(&mut rx).is_empty());
    }

    #[test]
    fn test_need_paging_drops_other_folder() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            need_files_page("docs", generation, 10, 1, 2, &["a", "b"]),
        );

        assert!(app.cache.get_out_of_sync_items("docs").unwrap().is_empty());
        assert!(need_page_requests(&mut rx).is_empty());
    }

    #[test]
    fn test_need_paging_filter_off_stops_page_requests() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        app.toggle_out_of_sync_filter();
        assert!(app.model.ui.out_of_sync_filter.is_none());
        assert!(app.model.syncthing.need_paging.is_empty());

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert!(need_page_requests(&mut rx).is_empty());
        assert!(
            app.model.ui.out_of_sync_filter.is_none(),
            "a late page must not turn the filter back on"
        );
    }

    #[test]
    fn test_need_paging_leaving_folder_stops_page_requests() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        app.clear_out_of_sync_filter(false, None);
        app.model.navigation.focus_level = 0;

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert!(need_page_requests(&mut rx).is_empty());
        assert!(app.model.syncthing.need_paging.is_empty());
    }

    /// Pretend the reapply throttle interval has passed for the folder's walk
    fn let_reapply_interval_pass(app: &mut App, folder: &str) {
        if let Some(p) = app.model.syncthing.need_paging.get_mut(folder) {
            p.last_apply = p
                .last_apply
                .map(|at| at - std::time::Duration::from_secs(1));
        }
    }

    /// Pretend the restart interval has passed since the folder's walk started
    fn let_restart_interval_pass(app: &mut App, folder: &str) {
        if let Some(p) = app.model.syncthing.need_paging.get_mut(folder) {
            p.started_at -= std::time::Duration::from_secs(60);
        }
    }

    /// A folder status poll for "media"
    fn status_tick(app: &mut App, sequence: u64, total: u64) {
        handle_api_response(
            app,
            ApiResponse::FolderStatusResult {
                folder_id: "media".to_string(),
                status: Ok(status_with_need(sequence, total)),
                requested_at: std::time::Instant::now(),
            },
        );
    }

    /// Answer pages of a 5-item walk: [a b] [c d] [e]
    fn finish_media_walk(app: &mut App, generation: u64, sequence: u64) {
        for (page, names) in [(1, &["a", "b"][..]), (2, &["c", "d"]), (3, &["e"])] {
            handle_api_response(
                app,
                need_files_page("media", generation, sequence, page, 2, names),
            );
        }
    }

    #[test]
    fn test_need_paging_sequence_change_restarts_after_walk_finishes() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        // Mid-walk sequence change: the walk carries on
        status_tick(&mut app, 11, 5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );
        let pages: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(pages, vec![3], "no restart while the walk runs");
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);

        let_restart_interval_pass(&mut app, "media");
        status_tick(&mut app, 11, 5);
        let restarted = need_page_requests(&mut rx);
        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].page, 1);
        assert_eq!(restarted[0].sequence, 11);
        assert_ne!(restarted[0].generation, generation);
        // The old walk's rows stay visible until the new one replaces them
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);

        // A late page of the old walk is dropped
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["zzz"]),
        );
        assert!(need_page_requests(&mut rx).is_empty());
        assert!(
            !app.cache
                .get_out_of_sync_items("media")
                .unwrap()
                .contains_key("zzz")
        );
    }

    #[test]
    fn test_item_finished_burst_during_walk_restarts_once_after_completion() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        for _ in 0..10 {
            handle_cache_invalidation(&mut app, item_finished("media", "a"));
            app.invalidate_folder_caches("media");
        }
        let during: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert!(during.is_empty(), "no restart mid-walk, got {:?}", during);
        assert!(app.model.syncthing.need_paging["media"].dirty);

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );
        need_page_requests(&mut rx);

        let_restart_interval_pass(&mut app, "media");
        status_tick(&mut app, 10, 5);
        status_tick(&mut app, 10, 5);
        let restarts: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(restarts, vec![1]);
    }

    #[test]
    fn test_item_finished_clears_only_that_path() {
        let (mut app, _rx, generation) = app_paging_media(5);
        finish_media_walk(&mut app, generation, 10);

        handle_cache_invalidation(&mut app, item_finished("media", "a"));
        handle_cache_invalidation(&mut app, item_failed("media", "b"));

        let items = app.cache.get_all_out_of_sync_items("media").unwrap();
        assert!(!items.contains_key("a"));
        assert!(items.contains_key("b"), "a failed item is still needed");
        assert!(items.contains_key("c"));
    }

    #[test]
    fn test_item_finished_while_first_page_in_flight_restarts_after() {
        let (mut app, mut rx, generation) = app_paging_media(2);

        handle_cache_invalidation(&mut app, item_finished("media", "a"));
        assert!(need_page_requests(&mut rx).is_empty());

        // Page 1 was built before "a" finished
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        assert!(app.model.syncthing.need_paging["media"].finished);

        let_restart_interval_pass(&mut app, "media");
        status_tick(&mut app, 10, 2);
        let restarts = need_page_requests(&mut rx);
        assert_eq!(restarts.len(), 1);
        assert_eq!(restarts[0].page, 1);
    }

    #[test]
    fn test_walk_ended_by_stale_total_does_not_sweep() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        finish_media_walk(&mut app, generation, 10);
        need_page_requests(&mut rx);

        // The status total drops below the real list; the next walk's full first page reaches it
        app.invalidate_folder_caches("media");
        let_restart_interval_pass(&mut app, "media");
        status_tick(&mut app, 10, 2);
        let restart = need_page_requests(&mut rx);
        assert_eq!(restart.len(), 1);
        handle_api_response(
            &mut app,
            need_files_page("media", restart[0].generation, 10, 1, 2, &["a", "b"]),
        );

        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_later_page_error_shows_pages_already_cached() {
        let (mut app, _rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b"], "page 2 is throttled");

        handle_api_response(
            &mut app,
            ApiResponse::NeededFiles {
                folder_id: "media".to_string(),
                generation,
                sequence: 10,
                page: 3,
                perpage: 2,
                response: Err(anyhow::anyhow!("boom")),
            },
        );

        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_search_cancels_pending_toggle() {
        let (mut app, _rx, generation) = app_paging_media(2);

        app.enter_search_mode();
        assert!(
            !app.model
                .syncthing
                .need_paging
                .get("media")
                .is_some_and(|p| p.pending_toggle)
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );

        assert!(app.model.ui.out_of_sync_filter.is_none());
    }

    #[test]
    fn test_second_toggle_cancels_pending_toggle() {
        let (mut app, mut rx, generation) = app_paging_media(2);

        app.toggle_out_of_sync_filter();
        assert!(need_page_requests(&mut rx).is_empty());
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );

        assert!(app.model.ui.out_of_sync_filter.is_none());
    }

    #[test]
    fn test_change_after_completion_triggers_one_restart() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        finish_media_walk(&mut app, generation, 10);
        need_page_requests(&mut rx);

        app.invalidate_folder_caches("media");
        let_restart_interval_pass(&mut app, "media");
        status_tick(&mut app, 10, 5);
        status_tick(&mut app, 10, 5);

        let restarts: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(restarts, vec![1]);
        // The old walk's rows stay visible until the new one replaces them
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_idle_folder_change_within_interval_restarts_via_tick() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        finish_media_walk(&mut app, generation, 10);

        // Changed right after the walk started: too soon to restart
        handle_cache_invalidation(&mut app, item_finished("media", "a"));
        app.tick_need_walks();
        assert!(need_page_requests(&mut rx).iter().all(|r| r.page != 1));

        // No status poll arrives for an idle folder; the main loop tick retries
        let_restart_interval_pass(&mut app, "media");
        app.tick_need_walks();
        app.tick_need_walks();
        let restarts: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(restarts, vec![1]);
    }

    #[test]
    fn test_done_walk_rows_outlive_ttl() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        finish_media_walk(&mut app, generation, 10);
        need_page_requests(&mut rx);

        app.cache.age_need_rows("media", 31);
        app.apply_out_of_sync_filter();

        assert!(need_page_requests(&mut rx).is_empty());
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_finished_short_walk_is_not_refetched_at_current_sequence() {
        // Status says 5 but the server only lists 3 (e.g. ignoreDelete)
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 1000, &["a", "b", "c"]),
        );
        need_page_requests(&mut rx);
        assert!(app.model.syncthing.need_paging["media"].finished);

        app.request_need_list("media");
        app.apply_out_of_sync_filter();

        assert!(need_page_requests(&mut rx).is_empty());
        assert_eq!(filtered_names(&app), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_need_paging_reapply_throttled_mid_walk() {
        let (mut app, _rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b"], "page 2 is throttled");

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );
        assert_eq!(filtered_names(&app), vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_filter_keeps_directories_with_needed_descendants() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(
            &mut app,
            "media",
            vec![dir_item("movies"), dir_item("music"), file_item("x")],
        );
        app.model
            .syncthing
            .folder_statuses
            .insert("media".to_string(), status_with_need(10, 1));
        app.toggle_out_of_sync_filter();
        let generation = need_page_requests(&mut rx)[0].generation;

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 1000, &["movies/2024/a.mkv"]),
        );

        assert_eq!(filtered_names(&app), vec!["movies"]);
    }

    #[test]
    fn test_need_paging_later_page_error_marks_partial() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        handle_api_response(
            &mut app,
            ApiResponse::NeededFiles {
                folder_id: "media".to_string(),
                generation,
                sequence: 10,
                page: 2,
                perpage: 2,
                response: Err(anyhow::anyhow!("boom")),
            },
        );

        assert!(need_page_requests(&mut rx).is_empty());
        let progress = paging_progress(&app).expect("partial list shows progress");
        assert!(progress.partial);
        assert_eq!(progress.loaded, 2);
    }

    #[test]
    fn test_need_paging_first_page_error_ends_walk() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        handle_api_response(
            &mut app,
            ApiResponse::NeededFiles {
                folder_id: "media".to_string(),
                generation,
                sequence: 10,
                page: 1,
                perpage: 2,
                response: Err(anyhow::anyhow!("boom")),
            },
        );

        assert!(need_page_requests(&mut rx).is_empty());
        assert!(
            !app.model.syncthing.need_paging.contains_key("media"),
            "a failed walk leaves nothing behind, so the next trigger retries"
        );
    }

    #[test]
    fn test_need_paging_capped_via_handler() {
        let (mut app, mut rx, generation) = app_paging_media(500);
        let max = crate::logic::need_paging::NEED_LIST_MAX_PAGES;
        for page in 1..=max {
            let name = format!("f{}", page);
            handle_api_response(
                &mut app,
                need_files_page("media", generation, 10, page, 1, &[name.as_str()]),
            );
        }

        let pages: Vec<u32> = need_page_requests(&mut rx).iter().map(|r| r.page).collect();
        assert_eq!(pages.last(), Some(&max), "no page past the cap");
        let progress = paging_progress(&app).expect("capped list shows progress");
        assert!(progress.partial);
        assert_eq!(progress.loaded, u64::from(max));
    }

    #[test]
    fn test_need_paging_unwanted_page_stops_without_prune() {
        let (mut app, mut rx, generation) = app_paging_media(5);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        need_page_requests(&mut rx);

        // Back at the folder list without going through clear_out_of_sync_filter
        app.model.navigation.focus_level = 0;
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );

        assert!(need_page_requests(&mut rx).is_empty());
        assert!(!app.model.syncthing.need_paging.contains_key("media"));
    }

    #[test]
    fn test_toggle_pending_first_page_survives_prune() {
        let (mut app, _rx, generation) = app_paging_media(2);

        app.prune_need_paging();
        assert!(app.model.syncthing.need_paging.contains_key("media"));

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        assert!(app.model.ui.out_of_sync_filter.is_some());
        assert_eq!(filtered_names(&app), vec!["a", "b"]);
    }

    fn selected_name(app: &App) -> Option<String> {
        let level = &app.model.navigation.breadcrumb_trail[app.model.navigation.focus_level - 1];
        level.selected_item().map(|i| i.name.clone())
    }

    /// Paging media with "e" selected, page 1 [a b] answered
    fn paging_with_e_selected() -> (
        App,
        tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
        u64,
    ) {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(
            &mut app,
            "media",
            ["a", "b", "c", "d", "e"]
                .iter()
                .map(|n| file_item(n))
                .collect(),
        );
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(4);
        app.model
            .syncthing
            .folder_statuses
            .insert("media".to_string(), status_with_need(10, 5));
        app.toggle_out_of_sync_filter();
        let generation = need_page_requests(&mut rx)[0].generation;
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 1, 2, &["a", "b"]),
        );
        assert_eq!(selected_name(&app).as_deref(), Some("a"));
        (app, rx, generation)
    }

    #[test]
    fn test_filter_restores_selection_when_item_appears() {
        let (mut app, _rx, generation) = paging_with_e_selected();

        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );

        assert_eq!(selected_name(&app).as_deref(), Some("e"));
        assert!(
            app.model
                .ui
                .out_of_sync_filter
                .as_ref()
                .unwrap()
                .pending_selection
                .is_none()
        );
    }

    #[test]
    fn test_filter_pending_selection_yields_to_user_move() {
        let (mut app, _rx, generation) = paging_with_e_selected();

        app.model.navigation.breadcrumb_trail[0].selected_index = Some(1);
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 2, 2, &["c", "d"]),
        );
        handle_api_response(
            &mut app,
            need_files_page("media", generation, 10, 3, 2, &["e"]),
        );

        assert_eq!(selected_name(&app).as_deref(), Some("b"));
    }

    #[test]
    fn test_filter_pending_selection_cleared_on_drill() {
        let (mut app, _rx, _generation) = paging_with_e_selected();
        assert!(
            app.model
                .ui
                .out_of_sync_filter
                .as_ref()
                .unwrap()
                .pending_selection
                .is_some()
        );

        app.model
            .navigation
            .breadcrumb_trail
            .push(sub_level("media", "a/", vec![file_item("x")]));
        app.model.navigation.focus_level = 2;
        app.apply_out_of_sync_filter();

        assert!(
            app.model
                .ui
                .out_of_sync_filter
                .as_ref()
                .unwrap()
                .pending_selection
                .is_none()
        );
    }

    #[test]
    fn test_request_need_list_coalesces_pending_first_page() {
        let (mut app, mut rx, generation) = app_paging_media(5);

        app.request_need_list("media");

        assert!(need_page_requests(&mut rx).is_empty());
        assert_eq!(
            app.model
                .syncthing
                .need_paging
                .get("media")
                .map(|p| p.generation),
            Some(generation)
        );
    }

    #[test]
    fn test_toggle_with_partial_cache_shows_it_and_refetches() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(
            &mut app,
            "media",
            ["a", "b", "c"].iter().map(|n| file_item(n)).collect(),
        );
        app.model
            .syncthing
            .folder_statuses
            .insert("media".to_string(), status_with_need(10, 3));
        if let ApiResponse::NeededFiles {
            response: Ok(page), ..
        } = need_files_page("media", 0, 10, 1, 2, &["a", "b"])
        {
            app.cache.cache_needed_files("media", &page, 0).unwrap();
        }

        app.toggle_out_of_sync_filter();

        assert!(app.model.ui.out_of_sync_filter.is_some());
        assert_eq!(filtered_names(&app), vec!["a", "b"]);
        let requests = need_page_requests(&mut rx);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].page, 1);
    }

    // ========================================
    // FILE PREVIEW RAW VIEW
    // ========================================

    fn open_preview(app: &mut App, path: &str) {
        view_folder_root(app, "media", vec![file_item(path)]);
        app.model.ui.file_info_popup = Some(crate::model::FileInfoPopupState {
            folder_id: "media".to_string(),
            file_path: path.to_string(),
            browse_item: file_item(path),
            file_details: None,
            file_content: Ok("# Title\n\nbody".to_string()),
            exists_on_disk: true,
            is_binary: false,
            is_image: false,
            scroll_offset: 5,
            raw_view: false,
        });
    }

    fn popup(app: &App) -> &crate::model::FileInfoPopupState {
        app.model.ui.file_info_popup.as_ref().unwrap()
    }

    #[tokio::test]
    async fn test_r_toggles_raw_view_on_markdown_preview() {
        let mut app = create_test_app();
        open_preview(&mut app, "README.md");

        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        assert!(popup(&app).raw_view);
        assert_eq!(popup(&app).scroll_offset, 0);

        app.model.ui.file_info_popup.as_mut().unwrap().scroll_offset = 3;
        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        assert!(!popup(&app).raw_view);
        assert_eq!(popup(&app).scroll_offset, 0);
    }

    #[tokio::test]
    async fn test_r_ignored_on_preview_without_rendered_view() {
        let mut app = create_test_app();
        open_preview(&mut app, "notes.txt");

        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        assert!(!popup(&app).raw_view);
        assert_eq!(popup(&app).scroll_offset, 5);
    }

    #[tokio::test]
    async fn test_r_in_preview_does_not_open_rescan() {
        let mut app = create_test_app();
        open_preview(&mut app, "README.md");

        press(&mut app, crossterm::event::KeyCode::Char('r')).await;
        assert!(app.model.ui.confirm_action.is_none());
        assert!(app.model.ui.file_info_popup.is_some());
    }

    // ========================================
    // FILE PREVIEW STEPPING
    // ========================================

    use crossterm::event::{KeyCode, KeyModifiers};

    async fn press_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        let key = crossterm::event::KeyEvent::new(code, modifiers);
        crate::handlers::keyboard::handle_key(app, key)
            .await
            .unwrap();
    }

    /// Breadcrumb view of `items` with `items[idx]` selected and previewed, scrolled
    /// and in raw view
    fn preview_at(app: &mut App, items: Vec<BrowseItem>, idx: usize) {
        let item = items[idx].clone();
        view_folder_root(app, "media", items);
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(idx);
        app.model.ui.file_info_popup = Some(crate::model::FileInfoPopupState {
            folder_id: "media".to_string(),
            file_path: item.name.clone(),
            browse_item: item,
            file_details: None,
            file_content: Ok("text".to_string()),
            exists_on_disk: true,
            is_binary: false,
            is_image: false,
            scroll_offset: 7,
            raw_view: true,
        });
    }

    fn selected(app: &App) -> Option<usize> {
        app.model.navigation.breadcrumb_trail[0].selected_index
    }

    fn create_vim_app() -> App {
        let mut app = create_test_app();
        app.model.ui.vim_mode = true;
        app
    }

    #[tokio::test]
    async fn test_ctrl_u_scrolls_half_page_up_in_breadcrumbs() {
        let mut app = create_vim_app();
        let items: Vec<BrowseItem> = (0..60).map(|i| file_item(&format!("f{i:02}"))).collect();
        view_folder_root(&mut app, "media", items);
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(50);

        press_with(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL).await;

        assert!(selected(&app).is_some_and(|i| i < 50));
        assert!(app.model.ui.toast.is_none());
    }

    fn three_files() -> Vec<BrowseItem> {
        vec![file_item("a.txt"), file_item("b.md"), file_item("c.txt")]
    }

    #[tokio::test]
    async fn test_vim_shift_j_previews_next_file() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 0);

        press_with(&mut app, KeyCode::Char('J'), KeyModifiers::SHIFT).await;

        assert_eq!(popup(&app).file_path, "b.md");
        assert_eq!(popup(&app).browse_item.name, "b.md");
        assert_eq!(popup(&app).scroll_offset, 0);
        assert!(!popup(&app).raw_view);
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_vim_shift_k_previews_previous_file() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 2);

        press_with(&mut app, KeyCode::Char('K'), KeyModifiers::SHIFT).await;

        assert_eq!(popup(&app).file_path, "b.md");
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_vim_shifted_letters_without_shift_flag_still_step() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 0);

        press(&mut app, KeyCode::Char('J')).await;
        assert_eq!(popup(&app).file_path, "b.md");
        press(&mut app, KeyCode::Char('K')).await;
        assert_eq!(popup(&app).file_path, "a.txt");
    }

    #[tokio::test]
    async fn test_shift_arrows_step_between_files() {
        let mut app = create_test_app();
        preview_at(&mut app, three_files(), 1);

        press_with(&mut app, KeyCode::Down, KeyModifiers::SHIFT).await;
        assert_eq!(popup(&app).file_path, "c.txt");
        assert_eq!(selected(&app), Some(2));

        press_with(&mut app, KeyCode::Up, KeyModifiers::SHIFT).await;
        press_with(&mut app, KeyCode::Up, KeyModifiers::SHIFT).await;
        assert_eq!(popup(&app).file_path, "a.txt");
        assert_eq!(selected(&app), Some(0));
    }

    #[tokio::test]
    async fn test_shift_jk_ignored_without_vim_mode() {
        let mut app = create_test_app();
        preview_at(&mut app, three_files(), 1);

        press_with(&mut app, KeyCode::Char('J'), KeyModifiers::SHIFT).await;
        press_with(&mut app, KeyCode::Char('K'), KeyModifiers::SHIFT).await;

        assert_eq!(popup(&app).file_path, "b.md");
        assert_eq!(popup(&app).scroll_offset, 7);
        assert!(popup(&app).raw_view);
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_shift_arrows_step_in_vim_mode() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 1);

        press_with(&mut app, KeyCode::Down, KeyModifiers::SHIFT).await;
        assert_eq!(popup(&app).file_path, "c.txt");
        press_with(&mut app, KeyCode::Up, KeyModifiers::SHIFT).await;
        assert_eq!(popup(&app).file_path, "b.md");
    }

    #[tokio::test]
    async fn test_plain_arrows_and_jk_still_scroll() {
        let mut app = create_test_app();
        preview_at(&mut app, three_files(), 1);

        press(&mut app, KeyCode::Down).await;
        press(&mut app, KeyCode::Char('j')).await;
        assert_eq!(popup(&app).scroll_offset, 9);
        press(&mut app, KeyCode::Up).await;
        press(&mut app, KeyCode::Char('k')).await;
        assert_eq!(popup(&app).scroll_offset, 7);
        assert_eq!(popup(&app).file_path, "b.md");
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_stepping_skips_directories() {
        let mut app = create_vim_app();
        let items = vec![file_item("a.txt"), dir_item("photos"), file_item("z.txt")];
        preview_at(&mut app, items, 0);

        press(&mut app, KeyCode::Char('J')).await;

        assert_eq!(popup(&app).file_path, "z.txt");
        assert_eq!(selected(&app), Some(2));
    }

    #[tokio::test]
    async fn test_stepping_stops_at_last_and_first_file() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 2);

        press(&mut app, KeyCode::Char('J')).await;
        assert_eq!(popup(&app).file_path, "c.txt");
        assert_eq!(popup(&app).scroll_offset, 7);
        assert!(popup(&app).raw_view);
        assert_eq!(selected(&app), Some(2));

        preview_at(&mut app, vec![dir_item("photos"), file_item("a.txt")], 1);
        press(&mut app, KeyCode::Char('K')).await;
        assert_eq!(popup(&app).file_path, "a.txt");
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_stepping_keeps_directory_prefix() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 0);
        app.model.navigation.breadcrumb_trail[0].prefix = Some("docs/".to_string());

        press(&mut app, KeyCode::Char('J')).await;

        assert_eq!(popup(&app).file_path, "docs/b.md");
        assert_eq!(popup(&app).folder_id, "media");
    }

    #[tokio::test]
    async fn test_stepping_follows_filtered_order() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 0);
        app.model.navigation.breadcrumb_trail[0].filtered_items =
            Some(vec![file_item("a.txt"), file_item("c.txt")]);

        press(&mut app, KeyCode::Char('J')).await;

        assert_eq!(popup(&app).file_path, "c.txt");
        assert_eq!(selected(&app), Some(1));
    }

    #[tokio::test]
    async fn test_stepping_resets_gg_sequence() {
        let mut app = create_vim_app();
        preview_at(&mut app, three_files(), 0);

        press(&mut app, KeyCode::Char('g')).await;
        press(&mut app, KeyCode::Char('J')).await;

        assert_eq!(
            app.model.ui.vim_command_state,
            crate::model::VimCommandState::None
        );
    }

    // ========================================
    // TEST APP
    // ========================================

    /// Stub server that records the first request it receives, headers included
    async fn spawn_capturing_stub() -> (String, std::sync::Arc<std::sync::Mutex<Option<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
        let captured_for_task = captured.clone();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                *captured_for_task.lock().unwrap() =
                    Some(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        (format!("http://{}", addr), captured)
    }

    #[tokio::test]
    async fn test_test_app_client_uses_configured_url_and_key() {
        let (stub_url, captured) = spawn_capturing_stub().await;
        let (app, _rx) = create_test_app_at(&stub_url);

        let _ = app.client.get_file_info("media", "a.txt").await;

        let request = captured
            .lock()
            .unwrap()
            .clone()
            .expect("the client should send its request to the configured base URL")
            .to_lowercase();
        assert!(request.starts_with("get /rest/db/file?"), "{}", request);
        assert!(request.contains("x-api-key: test-key"), "{}", request);
    }

    #[test]
    fn test_default_test_app_never_targets_local_syncthing() {
        let app = create_test_app();
        assert!(!app.base_url.contains(":8384"), "{}", app.base_url);
    }

    // ========================================
    // UN-IGNORE BLOCKED BY PENDING DELETE
    // ========================================

    #[tokio::test]
    async fn test_toggle_ignore_blocked_by_pending_delete_returns_path_without_toast() {
        let (mut app, _rx) = app_viewing_ignored_item(&["/a.mkv"]).await;
        app.record_pending(
            "media",
            "a.mkv",
            crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true,
            },
        );

        let result = app.toggle_ignore().await.unwrap();

        assert_eq!(
            result,
            crate::app::ignore::IgnoreToggle::BlockedByPendingDelete(std::path::PathBuf::from(
                "a.mkv"
            ))
        );
        assert!(app.model.ui.toast.is_none());
    }

    #[tokio::test]
    async fn test_i_on_item_pending_delete_shows_blocked_toast() {
        let (mut app, _rx) = app_viewing_ignored_item(&["/a.mkv"]).await;
        app.record_pending(
            "media",
            "a.mkv",
            crate::model::pending::PendingKind::Ignored {
                exists: false,
                deleting: true,
            },
        );

        press(&mut app, crossterm::event::KeyCode::Char('i')).await;

        let toast = app.model.ui.toast.clone().expect("toast shown");
        assert_eq!(
            toast.message,
            "Cannot un-ignore: deletion in progress for a.mkv"
        );
        assert_eq!(toast.kind, crate::model::ToastKind::Warning);
    }

    // ========================================
    // TOAST KINDS
    // ========================================

    fn toast_of(app: &App) -> (String, crate::model::ToastKind) {
        let t = app.model.ui.toast.as_ref().expect("toast shown");
        (t.message.clone(), t.kind)
    }

    #[tokio::test]
    async fn test_sort_change_is_info_toast() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.txt")]);

        press(&mut app, crossterm::event::KeyCode::Char('s')).await;

        let (message, kind) = toast_of(&app);
        assert!(message.starts_with("Sort: "), "{}", message);
        assert_eq!(kind, crate::model::ToastKind::Info);
    }

    #[tokio::test]
    async fn test_copy_config_path_without_clipboard_command_is_warning_toast() {
        let mut app = create_test_app();
        app.model.ui.show_setup_help = true;
        app.model.ui.config_path = "/home/u/.config/stui/config.yaml".to_string();

        press(&mut app, crossterm::event::KeyCode::Char('c')).await;

        assert_eq!(
            toast_of(&app),
            (
                "No clipboard command configured".to_string(),
                crate::model::ToastKind::Warning
            )
        );
    }

    #[tokio::test]
    async fn test_pause_failure_is_error_toast() {
        let mut app = create_test_app();
        app.model.ui.confirm_action = Some(crate::model::ConfirmAction::PauseResume {
            folder_id: "media".to_string(),
            label: "Media".to_string(),
            is_paused: false,
        });

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        let (message, kind) = toast_of(&app);
        assert!(
            message.starts_with("Failed to pause/resume folder"),
            "{}",
            message
        );
        assert_eq!(kind, crate::model::ToastKind::Error);
    }

    // ========================================
    // GROUPED FOLDER LIST SELECTION
    // ========================================

    #[tokio::test]
    async fn test_grouped_selection_does_not_drift() {
        let mut app = create_test_app();
        let folder = |id: &str, group: &str| -> crate::api::Folder {
            serde_json::from_value(serde_json::json!({
                "id": id, "label": id, "path": "/data", "paused": false,
                "type": "sendreceive", "group": group
            }))
            .unwrap()
        };
        app.model.set_folders(vec![
            folder("docs", ""),
            folder("photos", "Personal"),
            folder("projects", "Work"),
            folder("vault", "Personal"),
            folder("media", ""),
        ]);
        app.model.navigation.folders_state_selection = Some(4);
        app.model.navigation.focus_level = 0;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();

        for _ in 0..2 {
            terminal.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        }

        assert_eq!(app.model.navigation.folders_state_selection, Some(4));
        assert_eq!(app.model.syncthing.folders[4].id, "media");
    }

    // ========================================
    // FOLDER GROUP EDITOR
    // ========================================

    fn grouped_folder(id: &str, label: &str, group: Option<&str>) -> crate::api::Folder {
        let mut json = serde_json::json!({
            "id": id, "label": label, "path": "/data", "paused": false, "type": "sendreceive"
        });
        if let Some(group) = group {
            json["group"] = serde_json::json!(group);
        }
        serde_json::from_value(json).unwrap()
    }

    /// Folder view on the 2.1 demo folders with `select` selected
    fn app_with_groups(mut app: App, select: &str) -> App {
        app.model.set_folders(vec![
            grouped_folder("docs", "Documents", Some("")),
            grouped_folder("photos", "Photos", Some("Personal")),
            grouped_folder("projects", "Projects", Some("Work")),
        ]);
        let idx = app
            .model
            .syncthing
            .folders
            .iter()
            .position(|f| f.id == select)
            .unwrap();
        app.model.navigation.folders_state_selection = Some(idx);
        app.model.navigation.focus_level = 0;
        app
    }

    fn group_of(app: &App, id: &str) -> Option<String> {
        app.model
            .syncthing
            .folders
            .iter()
            .find(|f| f.id == id)
            .unwrap()
            .group
            .clone()
    }

    async fn captured_after_wait(
        captured: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    ) -> Option<String> {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        captured.lock().unwrap().clone()
    }

    async fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, crossterm::event::KeyCode::Char(c)).await;
        }
    }

    #[tokio::test]
    async fn test_e_on_older_syncthing_warns_with_version_and_sends_nothing() {
        let (url, captured) = spawn_capturing_stub().await;
        let (mut app, _rx) = create_test_app_at(&url);
        app.model
            .set_folders(vec![grouped_folder("docs", "Documents", None)]);
        app.model.navigation.folders_state_selection = Some(0);
        app.model.syncthing.version = Some("v2.0.3".to_string());

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;

        let (message, kind) = toast_of(&app);
        assert_eq!(
            message,
            "Folder groups need Syncthing 2.1 or newer (this device runs v2.0.3)"
        );
        assert_eq!(kind, crate::model::ToastKind::Warning);
        assert!(app.model.ui.folder_group_menu.is_none());
        assert_eq!(captured_after_wait(&captured).await, None);
    }

    #[tokio::test]
    async fn test_e_opens_menu_with_current_selected() {
        let mut app = app_with_groups(create_test_app(), "projects");

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;

        let menu = app.model.ui.folder_group_menu.as_ref().expect("menu open");
        use crate::model::types::GroupMenuOption as O;
        assert_eq!(
            menu.options,
            [
                O::Group("Personal".into()),
                O::Group("Work".into()),
                O::NoGroup,
                O::NewGroup
            ]
        );
        assert_eq!(menu.selected_index, 1);
        assert_eq!(menu.folder_label, "Projects");
    }

    #[tokio::test]
    async fn test_menu_apply_sends_patch_and_regroups() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "photos");

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;
        press(&mut app, crossterm::event::KeyCode::Char('j')).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let request = captured.lock().unwrap().clone().expect("PATCH sent");
        assert!(
            request.starts_with("PATCH /rest/config/folders/photos"),
            "{}",
            request
        );
        assert!(request.contains(r#""group":"Work""#), "{}", request);
        assert_eq!(
            toast_of(&app),
            (
                "Photos moved to Work".to_string(),
                crate::model::ToastKind::Success
            )
        );
        assert_eq!(group_of(&app, "photos").as_deref(), Some("Work"));
        assert!(app.model.ui.folder_group_menu.is_none());
        assert_eq!(app.model.selected_folder().unwrap().id, "photos");
    }

    #[tokio::test]
    async fn test_menu_apply_current_sends_nothing() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "photos");

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        assert!(app.model.ui.folder_group_menu.is_none());
        assert_eq!(captured_after_wait(&captured).await, None);
    }

    #[tokio::test]
    async fn test_no_group_toast() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "photos");

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;
        press(&mut app, crossterm::event::KeyCode::Down).await;
        press(&mut app, crossterm::event::KeyCode::Down).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let request = captured.lock().unwrap().clone().expect("PATCH sent");
        assert!(request.contains(r#""group":"""#), "{}", request);
        assert_eq!(toast_of(&app).0, "Photos removed from its group");
        assert_eq!(group_of(&app, "photos").as_deref(), Some(""));
    }

    async fn open_new_group_input(app: &mut App) {
        press(app, crossterm::event::KeyCode::Char('e')).await;
        for _ in 0..3 {
            press(app, crossterm::event::KeyCode::Down).await;
        }
        press(app, crossterm::event::KeyCode::Enter).await;
        assert!(app.model.ui.group_name_input.is_some(), "input open");
    }

    #[tokio::test]
    async fn test_new_group_input_flow() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "docs");
        open_new_group_input(&mut app).await;

        type_text(&mut app, "Clx").await;
        press(&mut app, crossterm::event::KeyCode::Backspace).await;
        type_text(&mut app, "ients jeq").await;
        for _ in 0..4 {
            press(&mut app, crossterm::event::KeyCode::Backspace).await;
        }
        assert!(
            !app.model.ui.should_quit,
            "q typed into the name, not a shortcut"
        );
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let request = captured.lock().unwrap().clone().expect("PATCH sent");
        assert!(request.contains(r#""group":"Clients""#), "{}", request);
        assert!(app.model.ui.group_name_input.is_none());
        assert_eq!(toast_of(&app).0, "Documents moved to Clients");
    }

    #[tokio::test]
    async fn test_new_group_empty_shows_hint() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "docs");
        open_new_group_input(&mut app).await;

        type_text(&mut app, "   ").await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let input = app.model.ui.group_name_input.as_ref().expect("still open");
        assert!(input.show_empty_hint);
        assert_eq!(captured_after_wait(&captured).await, None);
    }

    #[tokio::test]
    async fn test_new_group_reuses_existing_case() {
        let (url, captured) = spawn_capturing_stub().await;
        let mut app = app_with_groups(create_test_app_at(&url).0, "docs");
        open_new_group_input(&mut app).await;

        type_text(&mut app, "work").await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let request = captured.lock().unwrap().clone().expect("PATCH sent");
        assert!(request.contains(r#""group":"Work""#), "{}", request);
    }

    #[tokio::test]
    async fn test_new_group_esc_returns_to_menu() {
        let mut app = app_with_groups(create_test_app(), "docs");
        open_new_group_input(&mut app).await;

        press(&mut app, crossterm::event::KeyCode::Esc).await;

        assert!(app.model.ui.group_name_input.is_none());
        assert!(app.model.ui.folder_group_menu.is_some());
    }

    #[tokio::test]
    async fn test_set_group_failure_shows_error() {
        let mut app = app_with_groups(create_test_app(), "photos");

        press(&mut app, crossterm::event::KeyCode::Char('e')).await;
        press(&mut app, crossterm::event::KeyCode::Char('j')).await;
        press(&mut app, crossterm::event::KeyCode::Enter).await;

        let (message, kind) = toast_of(&app);
        assert!(message.starts_with("Failed to set group:"), "{}", message);
        assert_eq!(kind, crate::model::ToastKind::Error);
        assert_eq!(group_of(&app, "photos").as_deref(), Some("Personal"));
    }

    // ========================================
    // VERSION ON RECONNECT
    // ========================================

    fn system_status_ok() -> crate::services::api::ApiResponse {
        crate::services::api::ApiResponse::SystemStatusResult {
            status: Ok(crate::api::SystemStatus {
                my_id: "MLCMR7B".to_string(),
                uptime: 10,
                start_time: String::new(),
            }),
        }
    }

    fn version_requested(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
    ) -> bool {
        let mut found = false;
        while let Ok(req) = rx.try_recv() {
            if matches!(req, crate::services::api::ApiRequest::GetVersion) {
                found = true;
            }
        }
        found
    }

    #[tokio::test]
    async fn test_reconnect_requests_version() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.syncthing.version = Some("v2.0.16".to_string());
        app.model.syncthing.connection_state =
            crate::model::syncthing::ConnectionState::Connecting {
                attempt: 1,
                last_error: None,
                next_retry_secs: 5,
            };

        crate::handlers::api::handle_api_response(&mut app, system_status_ok());

        assert!(
            version_requested(&mut rx),
            "reconnect should refetch the version"
        );
    }

    #[tokio::test]
    async fn test_status_without_known_version_requests_it() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.syncthing.connection_state = crate::model::syncthing::ConnectionState::Connected;

        crate::handlers::api::handle_api_response(&mut app, system_status_ok());

        assert!(version_requested(&mut rx));
    }

    #[tokio::test]
    async fn test_connected_status_with_known_version_does_not_refetch() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![media_folder()];
        app.model.syncthing.version = Some("v2.1.7".to_string());
        app.model.syncthing.connection_state = crate::model::syncthing::ConnectionState::Connected;

        crate::handlers::api::handle_api_response(&mut app, system_status_ok());

        assert!(!version_requested(&mut rx));
    }

    #[tokio::test]
    async fn test_version_result_stores_version() {
        let mut app = create_test_app();
        crate::handlers::api::handle_api_response(
            &mut app,
            crate::services::api::ApiResponse::VersionResult {
                version: Ok("v2.1.7".to_string()),
            },
        );
        assert_eq!(app.model.syncthing.version.as_deref(), Some("v2.1.7"));
    }

    // ========================================
    // PAUSED FOLDER PREVIEW PANE
    // ========================================

    fn pausable_folder(id: &str, paused: bool) -> crate::api::Folder {
        serde_json::from_value(serde_json::json!({
            "id": id, "label": id, "path": "/data", "paused": paused, "type": "sendreceive"
        }))
        .unwrap()
    }

    /// Folder list with `media` selected and its root shown in the preview pane
    fn folder_list_previewing_media(app: &mut App, vault_paused: bool) {
        app.model.set_folders(vec![
            pausable_folder("media", false),
            pausable_folder("vault", vault_paused),
        ]);
        view_folder_root(app, "media", vec![file_item("a.mkv")]);
        app.model.navigation.focus_level = 0;
        app.model.navigation.folders_state_selection = Some(0);
    }

    #[tokio::test]
    async fn test_selecting_paused_folder_clears_preview_pane() {
        let mut app = create_test_app();
        folder_list_previewing_media(&mut app, true);

        press(&mut app, crossterm::event::KeyCode::Down).await;

        assert_eq!(app.model.selected_folder().unwrap().id, "vault");
        assert!(
            app.model.navigation.breadcrumb_trail.is_empty(),
            "media's files must not stay on screen for paused vault"
        );
    }

    #[tokio::test]
    async fn test_pausing_selected_folder_clears_its_pane() {
        let (url, _captured) = spawn_capturing_stub().await;
        let (mut app, _rx) = create_test_app_at(&url);
        folder_list_previewing_media(&mut app, false);
        app.model.ui.confirm_action = Some(crate::model::ConfirmAction::PauseResume {
            folder_id: "media".to_string(),
            label: "media".to_string(),
            is_paused: false,
        });

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        assert!(app.model.selected_folder().unwrap().paused);
        assert!(app.model.navigation.breadcrumb_trail.is_empty());
    }

    #[tokio::test]
    async fn test_resuming_folder_marks_it_unpaused() {
        let (url, _captured) = spawn_capturing_stub().await;
        let (mut app, _rx) = create_test_app_at(&url);
        app.model.set_folders(vec![pausable_folder("vault", true)]);
        app.model.navigation.folders_state_selection = Some(0);
        app.model.ui.confirm_action = Some(crate::model::ConfirmAction::PauseResume {
            folder_id: "vault".to_string(),
            label: "vault".to_string(),
            is_paused: true,
        });

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        assert!(!app.model.selected_folder().unwrap().paused);
    }

    // ========================================
    // LOCAL DELETIONS
    // ========================================

    /// Documents/manuals, receive-only, with router-setup.txt deleted locally and selected
    fn app_viewing_locally_deleted() -> App {
        let mut app = create_test_app();
        let folder: crate::api::Folder = serde_json::from_value(serde_json::json!({
            "id": "documents", "label": "Documents", "path": "/data/documents",
            "paused": false, "type": "receiveonly"
        }))
        .unwrap();
        app.model.set_folders(vec![folder]);
        view_folder_root(
            &mut app,
            "documents",
            vec![file_item("printer.txt"), file_item("router-setup.txt")],
        );
        let level = &mut app.model.navigation.breadcrumb_trail[0];
        level.folder_label = "Documents".to_string();
        level.prefix = Some("manuals/".to_string());
        level.selected_index = Some(1);
        level
            .file_sync_states
            .insert("printer.txt".to_string(), SyncState::Synced);
        let mut index = crate::model::syncthing::NeedIndex::default();
        crate::logic::sync_states::set_index_local_changed(
            &mut index,
            &[crate::api::LocalChangedFile {
                name: "manuals/router-setup.txt".to_string(),
                deleted: true,
            }],
            1,
        );
        app.model
            .syncthing
            .need_index
            .insert("documents".to_string(), index);
        app
    }

    #[tokio::test]
    async fn test_locally_deleted_file_has_its_own_icon_and_label() {
        let mut app = app_viewing_locally_deleted();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();

        let row = rows
            .iter()
            .find(|r| r.contains("router-setup.txt") && !r.contains("Selected"))
            .expect("listing row");
        assert!(row.contains("❌"), "{}", row);
        assert!(!row.contains("⛅"), "{}", row);
        assert!(
            rows.iter().any(|r| r.contains("Deleted locally")),
            "{:#?}",
            rows
        );
    }

    #[tokio::test]
    async fn test_locally_deleted_preview_explains_restore() {
        let mut app = app_viewing_locally_deleted();
        let item = file_item("router-setup.txt");

        app.fetch_file_info_and_content(
            "documents".to_string(),
            "manuals/router-setup.txt".to_string(),
            item,
        )
        .await;

        let popup = app.model.ui.file_info_popup.as_ref().unwrap();
        assert_eq!(
            popup.file_content.as_ref().unwrap_err(),
            crate::logic::sync_states::locally_deleted_message()
        );
    }

    #[tokio::test]
    async fn test_locally_deleted_image_preview_shows_restore_text_not_loading() {
        let mut app = app_viewing_locally_deleted();
        let index = app.model.syncthing.need_index.get_mut("documents").unwrap();
        crate::logic::sync_states::set_index_local_changed(
            index,
            &[crate::api::LocalChangedFile {
                name: "manuals/diagram.jpg".to_string(),
                deleted: true,
            }],
            1,
        );

        app.fetch_file_info_and_content(
            "documents".to_string(),
            "manuals/diagram.jpg".to_string(),
            file_item("diagram.jpg"),
        )
        .await;

        let popup = app.model.ui.file_info_popup.as_ref().unwrap();
        assert!(!popup.is_image);
        assert_eq!(
            popup.file_content.as_ref().unwrap_err(),
            crate::logic::sync_states::locally_deleted_message()
        );

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let screen: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_string())
            .collect();
        assert!(!screen.contains("Loading image"));
        assert!(screen.contains("Deleted on this device"));
    }

    // ========================================
    // UNDELETE
    // ========================================

    /// How the undelete stub answers scan requests
    #[derive(Clone)]
    enum ScanMode {
        Normal,
        /// Never answers any scan
        Hang,
        /// Closes the first scan's connection without a response
        DropFirst,
        /// Holds the first scan until the test notifies
        GateFirst(std::sync::Arc<tokio::sync::Notify>),
    }

    /// Stub that records each request in full; GET ignores answers with `patterns`
    async fn spawn_undelete_stub(patterns: &[&str], scans: ScanMode) -> (String, RequestLog) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = serde_json::json!({ "ignore": patterns }).to_string();
        let log: RequestLog = Default::default();
        let log_for_task = log.clone();
        let first_scan_seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let (body, log, scans, first_scan_seen) = (
                    body.clone(),
                    log_for_task.clone(),
                    scans.clone(),
                    first_scan_seen.clone(),
                );
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let mut n = sock.read(&mut buf).await.unwrap_or(0);
                    if let Ok(Ok(more)) = tokio::time::timeout(
                        std::time::Duration::from_millis(20),
                        sock.read(&mut buf[n..]),
                    )
                    .await
                    {
                        n += more;
                    }
                    let request = String::from_utf8_lossy(&buf[..n]).to_string();
                    log.lock().unwrap().push(request.clone());
                    if request.starts_with("POST /rest/db/scan") {
                        let first =
                            !first_scan_seen.swap(true, std::sync::atomic::Ordering::SeqCst);
                        match scans {
                            ScanMode::Hang => {
                                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                                return;
                            }
                            ScanMode::DropFirst if first => return,
                            ScanMode::GateFirst(gate) if first => gate.notified().await,
                            _ => {}
                        }
                    }
                    let payload = if request.starts_with("GET ") {
                        body.as_str()
                    } else {
                        ""
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{}", addr), log)
    }

    fn request_lines(log: &RequestLog) -> Vec<String> {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r.lines().next().unwrap_or("").to_string())
            .collect()
    }

    fn ignore_bodies(log: &RequestLog) -> Vec<String> {
        log.lock()
            .unwrap()
            .iter()
            .filter(|r| r.starts_with("POST /rest/db/ignores"))
            .map(|r| r.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
            .collect()
    }

    /// Handle finished undelete batches, as the main loop does, until none are running
    async fn finish_undeletes(app: &mut App) {
        while app.undelete_jobs.values().any(|job| job.running) {
            let done =
                tokio::time::timeout(std::time::Duration::from_secs(5), app.undelete_rx.recv())
                    .await
                    .expect("undelete batch should finish")
                    .expect("channel open");
            crate::handlers::undelete::handle_undelete_done(app, done);
        }
    }

    /// Documents/manuals with three locally deleted files, `router-setup.txt` selected
    fn app_with_deleted_files(url: &str) -> App {
        let mut app = app_viewing_locally_deleted();
        app.client = crate::SyncthingClient::new(url.to_string(), "k".to_string());
        let level = &mut app.model.navigation.breadcrumb_trail[0];
        level.items.push(file_item("wifi.txt"));
        level.items.push(file_item("zigbee.txt"));
        let index = app.model.syncthing.need_index.get_mut("documents").unwrap();
        crate::logic::sync_states::set_index_local_changed(
            index,
            &[
                crate::api::LocalChangedFile {
                    name: "manuals/router-setup.txt".to_string(),
                    deleted: true,
                },
                crate::api::LocalChangedFile {
                    name: "manuals/wifi.txt".to_string(),
                    deleted: true,
                },
                crate::api::LocalChangedFile {
                    name: "manuals/zigbee.txt".to_string(),
                    deleted: true,
                },
            ],
            1,
        );
        app
    }

    fn deleted_here(app: &App, path: &str) -> bool {
        crate::logic::sync_states::is_locally_deleted(
            app.model.syncthing.need_index.get("documents"),
            None,
            path,
        )
    }

    #[tokio::test]
    async fn test_undelete_returns_immediately_while_scan_hangs() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            press(&mut app, crossterm::event::KeyCode::Char('u')),
        )
        .await
        .expect("u must not wait for Syncthing");

        let toast = app.model.ui.toast.as_ref().expect("toast");
        assert!(
            toast.message.starts_with("Restoring router-setup.txt"),
            "{}",
            toast.message
        );
        assert!(
            !deleted_here(&app, "manuals/router-setup.txt"),
            "❌ clears right away"
        );
    }

    #[tokio::test]
    async fn test_undelete_batch_ignores_rescans_then_restores_ignores() {
        let (url, log) = spawn_undelete_stub(&["*.tmp"], ScanMode::Normal).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;
        finish_undeletes(&mut app).await;

        let lines = request_lines(&log);
        let kinds: Vec<&str> = lines
            .iter()
            .map(|l| {
                if l.starts_with("GET /rest/db/ignores") {
                    "get-ignores"
                } else if l.starts_with("POST /rest/db/ignores") {
                    "set-ignores"
                } else if l.starts_with("POST /rest/db/scan") && l.contains("sub=manuals") {
                    "scan-path"
                } else {
                    "other"
                }
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "get-ignores",
                "set-ignores",
                "scan-path",
                "set-ignores",
                "scan-path"
            ],
            "{:#?}",
            lines
        );
        let bodies = ignore_bodies(&log);
        assert!(
            bodies[0].contains("/manuals/router-setup.txt") && bodies[0].contains("*.tmp"),
            "{:?}",
            bodies
        );
        assert!(
            !bodies[1].contains("router-setup") && bodies[1].contains("*.tmp"),
            "{:?}",
            bodies
        );
        assert_eq!(
            app.model.ui.toast.as_ref().unwrap().message,
            "Restored router-setup.txt"
        );
        assert!(app.cache.pending_undelete_patterns().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_undeletes_during_a_running_batch_join_the_next_batch() {
        let gate = std::sync::Arc::new(tokio::sync::Notify::new());
        let (url, log) = spawn_undelete_stub(&[], ScanMode::GateFirst(gate.clone())).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await; // router-setup.txt
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        for idx in [2, 3] {
            app.model.navigation.breadcrumb_trail[0].selected_index = Some(idx);
            press(&mut app, crossterm::event::KeyCode::Char('u')).await; // wifi, zigbee
        }
        gate.notify_one();
        finish_undeletes(&mut app).await;

        let bodies = ignore_bodies(&log);
        assert_eq!(
            bodies.len(),
            4,
            "two batches, two ignore changes each: {:?}",
            bodies
        );
        assert!(
            bodies[0].contains("router-setup") && !bodies[0].contains("wifi"),
            "{:?}",
            bodies
        );
        assert!(
            bodies[2].contains("/manuals/wifi.txt") && bodies[2].contains("/manuals/zigbee.txt"),
            "{:?}",
            bodies
        );
        assert_eq!(
            app.model.ui.toast.as_ref().unwrap().message,
            "Restored 2 files"
        );
    }

    #[tokio::test]
    async fn test_undelete_failure_still_restores_ignores() {
        let (url, log) = spawn_undelete_stub(&["*.tmp"], ScanMode::DropFirst).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;
        finish_undeletes(&mut app).await;

        let bodies = ignore_bodies(&log);
        let last = bodies.last().expect("ignores set back");
        assert!(
            !last.contains("router-setup") && last.contains("*.tmp"),
            "{:?}",
            bodies
        );
        let toast = app.model.ui.toast.as_ref().expect("toast");
        assert!(
            toast
                .message
                .starts_with("Failed to restore router-setup.txt"),
            "{}",
            toast.message
        );
        assert_eq!(toast.kind, crate::model::ToastKind::Error);
        assert!(app.cache.pending_undelete_patterns().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_leftover_undelete_patterns_are_removed_at_startup() {
        let (url, log) =
            spawn_undelete_stub(&["/recipes/chili.md", "*.tmp"], ScanMode::Normal).await;
        let mut app = app_with_deleted_files(&url);
        app.cache
            .add_pending_undelete("documents", &["/recipes/chili.md".to_string()])
            .unwrap();

        app.remove_leftover_undelete_patterns().await;

        let bodies = ignore_bodies(&log);
        assert_eq!(bodies.len(), 1, "{:?}", bodies);
        assert!(
            !bodies[0].contains("chili") && bodies[0].contains("*.tmp"),
            "{:?}",
            bodies
        );
        assert!(app.cache.pending_undelete_patterns().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_undelete_on_other_file_warns_and_sends_nothing() {
        let (url, log) = spawn_undelete_stub(&[], ScanMode::Normal).await;
        let mut app = app_with_deleted_files(&url);
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(0); // printer.txt

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(log.lock().unwrap().is_empty());
        let toast = app.model.ui.toast.as_ref().expect("toast");
        assert_eq!(
            toast.message,
            "Only files deleted on this device can be undeleted"
        );
        assert_eq!(toast.kind, crate::model::ToastKind::Warning);
    }

    #[tokio::test]
    async fn test_delete_in_receive_only_folder_marks_row_deleted() {
        let mut app = app_viewing_locally_deleted();
        let dir = std::env::temp_dir().join(format!("stui-del-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("printer.txt");
        std::fs::write(&file, "x").unwrap();
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(0);
        app.model.ui.confirm_action = Some(crate::model::ConfirmAction::Delete {
            path: file.to_string_lossy().to_string(),
            name: "printer.txt".to_string(),
            is_dir: false,
        });

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        let _ = std::fs::remove_dir_all(&dir);
        let level = &app.model.navigation.breadcrumb_trail[0];
        assert!(
            level.items.iter().any(|i| i.name == "printer.txt"),
            "row stays"
        );
        assert!(
            deleted_here(&app, "manuals/printer.txt"),
            "row shows deleted locally"
        );
    }

    fn confirm_delete(app: &mut App, path: &std::path::Path, name: &str, is_dir: bool) {
        app.model.ui.confirm_action = Some(crate::model::ConfirmAction::Delete {
            path: path.to_string_lossy().to_string(),
            name: name.to_string(),
            is_dir,
        });
    }

    fn temp_file(tag: &str, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("stui-del-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, "x").unwrap();
        (dir, file)
    }

    #[tokio::test]
    async fn test_receive_only_delete_survives_status_with_zero_local_changes() {
        let mut app = app_viewing_locally_deleted();
        let (dir, file) = temp_file("zero", "printer.txt");
        app.model.navigation.breadcrumb_trail[0].selected_index = Some(0);
        confirm_delete(&mut app, &file, "printer.txt", false);
        press(&mut app, crossterm::event::KeyCode::Char('y')).await;
        let _ = std::fs::remove_dir_all(&dir);

        crate::handlers::api::handle_api_response(&mut app, status_result("documents", "idle"));

        assert!(
            deleted_here(&app, "manuals/printer.txt"),
            "a zero receive-only status must not drop the pending deletion"
        );
        assert!(
            app.model
                .pending
                .get("documents", "manuals/printer.txt")
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_receive_only_directory_delete_rolls_up_to_parent() {
        let mut app = app_viewing_locally_deleted();
        let mut child = app.model.navigation.breadcrumb_trail[0].clone();
        child.selected_index = Some(0);
        let mut root = child.clone();
        root.prefix = None;
        root.items = vec![dir_item("manuals")];
        root.file_sync_states =
            std::collections::HashMap::from([("manuals".to_string(), SyncState::Synced)]);
        root.selected_index = Some(0);
        app.model.navigation.breadcrumb_trail = vec![root, child];
        app.model.navigation.focus_level = 2;
        let (dir, file) = temp_file("rollup", "printer.txt");
        confirm_delete(&mut app, &file, "printer.txt", false);

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;
        let _ = std::fs::remove_dir_all(&dir);

        let root = &app.model.navigation.breadcrumb_trail[0];
        let display = crate::logic::display::level_item_display(
            &app.model.syncthing,
            root,
            &root.items[0],
            &app.model.pending,
        );
        assert_ne!(
            display,
            Some(crate::logic::display::ItemDisplay::State(SyncState::Synced))
        );
        assert!(display.is_some(), "parent shows a local-change state");
    }

    #[tokio::test]
    async fn test_normal_delete_row_stays_gone_on_stale_listing() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a"), file_item("b")]);
        let (dir, file) = temp_file("stale", "a");
        confirm_delete(&mut app, &file, "a", false);
        let sent = std::time::Instant::now() - Duration::from_millis(1);

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;
        let _ = std::fs::remove_dir_all(&dir);
        crate::handlers::api::handle_api_response(
            &mut app,
            crate::services::api::ApiResponse::BrowseResult {
                folder_id: "media".to_string(),
                prefix: None,
                items: Ok(vec![file_item("a"), file_item("b")]),
                requested_at: sent,
            },
        );

        let names: Vec<&str> = app.model.navigation.breadcrumb_trail[0]
            .items
            .iter()
            .map(|i| i.name.as_str())
            .collect();
        assert_eq!(names, vec!["b"]);
    }

    #[tokio::test]
    async fn test_failed_delete_shows_error_toast() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a")]);
        let missing = std::env::temp_dir().join(format!("stui-missing-{}", std::process::id()));
        confirm_delete(&mut app, &missing, "a", false);

        press(&mut app, crossterm::event::KeyCode::Char('y')).await;

        let toast = app.model.ui.toast.as_ref().expect("toast");
        assert_eq!(toast.kind, crate::model::ToastKind::Error);
        assert!(
            toast.message.starts_with("Failed to delete a: "),
            "{}",
            toast.message
        );
        assert_eq!(app.model.navigation.breadcrumb_trail[0].items.len(), 1);
    }

    // ========================================
    // FINISHED DOWNLOADS
    // ========================================

    fn level_state(app: &App, level: usize, name: &str) -> Option<SyncState> {
        app.model.navigation.breadcrumb_trail[level]
            .file_sync_states
            .get(name)
            .copied()
    }

    #[test]
    fn test_finished_download_marks_file_synced_at_once() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);

        handle_cache_invalidation(&mut app, item_finished("media", "a.mkv"));

        assert_eq!(level_state(&app, 0, "a.mkv"), Some(SyncState::Synced));
    }

    #[test]
    fn test_finished_download_in_subdirectory_marks_that_level() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);
        app.model.navigation.breadcrumb_trail[0].prefix = Some("clips/".to_string());

        handle_cache_invalidation(&mut app, item_finished("media", "clips/a.mkv"));
        handle_cache_invalidation(&mut app, item_finished("media", "other/a.mkv"));

        assert_eq!(level_state(&app, 0, "a.mkv"), Some(SyncState::Synced));
    }

    #[test]
    fn test_failed_download_or_finished_delete_keeps_state() {
        let mut app = create_test_app();
        view_folder_root(
            &mut app,
            "media",
            vec![file_item("a.mkv"), file_item("b.mkv")],
        );

        handle_cache_invalidation(&mut app, item_failed("media", "a.mkv"));
        handle_cache_invalidation(
            &mut app,
            CacheInvalidation::ItemFinished {
                folder_id: "media".to_string(),
                file_path: "b.mkv".to_string(),
                failed: false,
                deleted: true,
                timestamp: SystemTime::now(),
            },
        );

        assert_eq!(level_state(&app, 0, "a.mkv"), Some(SyncState::RemoteOnly));
        assert_eq!(level_state(&app, 0, "b.mkv"), Some(SyncState::RemoteOnly));
    }

    #[tokio::test]
    async fn test_undelete_shows_file_syncing_until_downloaded() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;

        assert!(
            app.model
                .syncthing
                .syncing_files
                .get("documents")
                .is_some_and(|s| s.contains("manuals/router-setup.txt")),
            "🔄 from the moment u is pressed"
        );
    }

    #[tokio::test]
    async fn test_failed_undelete_shows_file_deleted_again() {
        let (url, _log) = spawn_undelete_stub(&["*.tmp"], ScanMode::DropFirst).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;
        finish_undeletes(&mut app).await;

        assert!(
            !app.model
                .syncthing
                .syncing_files
                .get("documents")
                .is_some_and(|s| s.contains("manuals/router-setup.txt"))
        );
        assert!(deleted_here(&app, "manuals/router-setup.txt"));
    }

    #[tokio::test]
    async fn test_finished_download_stays_synced_through_relisting() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "media", vec![file_item("a.mkv")]);
        // A need list fetched mid-download still lists the file
        let at = mark_fetched(&mut app, "media");
        handle_api_response(&mut app, need_result("media", at, &[], &["a.mkv"]));

        handle_cache_invalidation(&mut app, item_finished("media", "a.mkv"));
        handle_api_response(
            &mut app,
            ApiResponse::BrowseResult {
                folder_id: "media".to_string(),
                prefix: None,
                items: Ok(vec![file_item("a.mkv")]),
                requested_at: std::time::Instant::now(),
            },
        );

        assert_eq!(
            displayed_states(&app).get("a.mkv"),
            Some(&SyncState::Synced),
            "the re-listing must not bring back the stale need entry"
        );
    }

    #[tokio::test]
    async fn test_undeleted_file_stays_syncing_through_need_refresh() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);
        press(&mut app, crossterm::event::KeyCode::Char('u')).await;

        // Syncthing hasn't started the download yet: nothing in progress
        let at = mark_fetched(&mut app, "documents");
        handle_api_response(&mut app, need_result("documents", at, &[], &[]));

        assert_eq!(
            displayed_states(&app).get("router-setup.txt"),
            Some(&SyncState::Syncing)
        );
    }

    #[test]
    fn test_finished_download_clears_stale_local_deletion() {
        // Mid-restore, Syncthing's local changes still listed the file as deleted
        let mut app = app_viewing_locally_deleted();

        handle_cache_invalidation(
            &mut app,
            item_finished("documents", "manuals/router-setup.txt"),
        );

        assert!(!deleted_here(&app, "manuals/router-setup.txt"));
        assert_eq!(
            displayed_states(&app).get("router-setup.txt"),
            Some(&SyncState::Synced)
        );
    }

    #[tokio::test]
    async fn test_undelete_records_restoring_pending_change() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);

        press(&mut app, crossterm::event::KeyCode::Char('u')).await;

        let change = app
            .model
            .pending
            .get("documents", "manuals/router-setup.txt");
        assert_eq!(
            change.map(|c| c.kind.clone()),
            Some(crate::model::pending::PendingKind::Restoring)
        );
    }

    #[tokio::test]
    async fn test_pause_during_restore_clears_syncing_icon() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);
        press(&mut app, crossterm::event::KeyCode::Char('u')).await;
        assert!(
            app.model
                .pending
                .get("documents", "manuals/router-setup.txt")
                .is_some()
        );

        handle_api_response(&mut app, status_result("documents", "paused"));

        assert!(
            app.model
                .pending
                .get("documents", "manuals/router-setup.txt")
                .is_none()
        );
        assert_ne!(
            displayed_states(&app).get("router-setup.txt"),
            Some(&SyncState::Syncing)
        );
    }

    #[tokio::test]
    async fn test_stale_local_changed_after_item_finished_does_not_show_deleted() {
        let (url, _log) = spawn_undelete_stub(&[], ScanMode::Hang).await;
        let mut app = app_with_deleted_files(&url);
        let before_undelete = std::time::Instant::now();
        press(&mut app, crossterm::event::KeyCode::Char('u')).await;
        assert!(
            app.model
                .pending
                .get("documents", "manuals/router-setup.txt")
                .is_some()
        );
        handle_cache_invalidation(
            &mut app,
            item_finished("documents", "manuals/router-setup.txt"),
        );
        app.model
            .syncthing
            .local_changed_requested
            .insert("documents".to_string(), 1);

        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "documents".to_string(),
                sequence: 1,
                files: Ok(vec![crate::api::LocalChangedFile {
                    name: "manuals/router-setup.txt".to_string(),
                    deleted: true,
                }]),
                requested_at: before_undelete,
            },
        );

        assert_eq!(
            displayed_items(&app)["router-setup.txt"],
            Some(crate::logic::display::ItemDisplay::State(SyncState::Synced))
        );
    }
}
