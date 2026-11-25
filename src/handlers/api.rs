//! API Response Handler
//!
//! Handles responses from the Syncthing REST API background service.
//! Processes browse results, file info, folder status, and connection stats.

use std::time::Instant;

use crate::App;
use crate::api::{BrowseItem, SyncState};
use crate::logic::pending::{Observation, Verdict};
use crate::model::pending::PendingKind;
use crate::model::syncthing::ConnectionState;
use crate::services::api::{ApiRequest, ApiResponse, Priority};

/// Handle API response from background service
///
/// Processes different types of API responses and updates app state accordingly.
///
/// Response types:
/// - BrowseResult: Directory listing for a folder
/// - FileInfoResult: Detailed sync state for a single file
/// - FolderStatusResult: Overall folder status (sequence, sync progress)
/// - RescanResult: Result of folder rescan operation
/// - SystemStatusResult: System info (uptime, device name)
/// - ConnectionStatsResult: Transfer statistics
/// - FolderNeed: Need list (replaces the folder's syncing set and need index)
/// - LocalChangedIndex: Receive-only local changes for the need index
pub fn handle_api_response(app: &mut App, response: ApiResponse) {
    match response {
        ApiResponse::BrowseResult {
            folder_id,
            prefix,
            items,
            requested_at,
        } => {
            // Mark browse as no longer loading
            let browse_key = format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
            app.model.performance.loading_browse.remove(&browse_key);

            let applied_key = (folder_id.clone(), prefix.clone());
            if app
                .model
                .syncthing
                .browse_applied_at
                .get(&applied_key)
                .is_some_and(|applied| requested_at < *applied)
            {
                crate::log_debug(&format!(
                    "DEBUG [BrowseResult]: Dropping listing older than the one shown for folder={} prefix={:?}",
                    folder_id, prefix
                ));
                return;
            }

            let Ok(mut items) = items else {
                // API call failed - only update state if we're not already in Connecting mode
                if !matches!(
                    app.model.syncthing.connection_state,
                    ConnectionState::Connecting { .. }
                ) {
                    let error = items.unwrap_err();
                    app.model.syncthing.connection_state = ConnectionState::Disconnected {
                        error_type: crate::logic::errors::classify_error(&error),
                        message: crate::logic::errors::format_error_message(&error),
                    };
                }
                return;
            };

            // Successful API call - mark as connected
            app.model.syncthing.connection_state = ConnectionState::Connected;

            // Check if this response is still relevant to current navigation
            // We allow caching for subdirectories of the current folder (prefetch),
            // but skip if we've navigated completely away from this folder
            let is_relevant = if app.model.navigation.breadcrumb_trail.is_empty() {
                false // No breadcrumb trail, nothing is relevant
            } else if app.model.navigation.focus_level == 0 {
                // At folder list - only accept browse results that match a breadcrumb in the trail
                // (e.g., when backing out from a folder with active search, we need to refresh the root)
                app.model
                    .navigation
                    .breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id && level.prefix == prefix)
            } else {
                // Check if this folder_id matches any level in our current breadcrumb trail
                // This allows prefetching subdirectories that aren't yet open
                app.model
                    .navigation
                    .breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id)
            };

            if !is_relevant {
                crate::log_debug(&format!(
                    "DEBUG [BrowseResult]: Skipping irrelevant response for folder={} prefix={:?} (navigated away)",
                    folder_id, prefix
                ));
                return; // Skip saving and UI updates for responses from folders we've left
            }
            app.model
                .syncthing
                .browse_applied_at
                .insert(applied_key, requested_at);

            // Get folder sequence for cache
            let folder_sequence = app
                .model
                .syncthing
                .folder_statuses
                .get(&folder_id)
                .map(|s| s.sequence)
                .unwrap_or(0);

            // Check if this folder has local changes and merge them synchronously
            let folder_status = app.model.syncthing.folder_statuses.get(&folder_id);
            let has_local_changes = crate::logic::folder::has_local_changes(folder_status);

            let mut local_item_names = Vec::new();

            if has_local_changes {
                crate::log_debug("DEBUG [BrowseResult]: Folder has local changes, fetching...");

                // Block to wait for local items synchronously
                let folder_id_clone = folder_id.clone();
                let prefix_clone = prefix.clone();
                let client = app.client.clone();

                // Use block_in_place to run async code synchronously
                let local_result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        client
                            .get_local_changed_items(&folder_id_clone, prefix_clone.as_deref())
                            .await
                    })
                });

                if let Ok(local_items) = local_result {
                    crate::log_debug(&format!(
                        "DEBUG [BrowseResult]: Fetched {} local items",
                        local_items.len()
                    ));

                    // Merge local items
                    for local_item in local_items {
                        if !items.iter().any(|i| i.name == local_item.name) {
                            crate::log_debug(&format!(
                                "DEBUG [BrowseResult]: Merging local item: {}",
                                local_item.name
                            ));
                            local_item_names.push(local_item.name.clone());
                            items.push(local_item);
                        }
                    }
                } else {
                    crate::log_debug("DEBUG [BrowseResult]: Failed to fetch local items");
                }
            }

            crate::log_debug(&format!(
                "DEBUG [BrowseResult]: folder={} prefix={:?} items_count={} focus_level={} breadcrumb_count={}",
                folder_id,
                prefix,
                items.len(),
                app.model.navigation.focus_level,
                app.model.navigation.breadcrumb_trail.len()
            ));

            observe_listing(app, &folder_id, prefix.as_deref(), &mut items, requested_at);

            // Save merged items to cache
            if let Err(e) =
                app.cache
                    .save_browse_items(&folder_id, prefix.as_deref(), &items, folder_sequence)
            {
                crate::log_debug(&format!(
                    "ERROR [BrowseResult]: Failed to save browse items to cache: {}",
                    e
                ));
            }

            // Update UI if this browse result matches current navigation
            // Find matching breadcrumb level (works for both root and non-root)
            let matching_level_idx = if prefix.is_none() {
                // Root level: match folder_id and no prefix
                app.model
                    .navigation
                    .breadcrumb_trail
                    .iter()
                    .position(|level| level.folder_id == folder_id && level.prefix.is_none())
            } else {
                // Non-root level: match folder_id and prefix
                app.model
                    .navigation
                    .breadcrumb_trail
                    .iter()
                    .position(|level| level.folder_id == folder_id && level.prefix == prefix)
            };

            if let Some(idx) = matching_level_idx {
                crate::log_debug(&format!(
                    "DEBUG [BrowseResult]: Updating level {} for folder={} prefix={:?}",
                    idx, folder_id, prefix
                ));

                let mut cached_states =
                    app.cached_sync_states(&folder_id, &items, prefix.as_deref());
                // The pending change covers these until confirmed; the cache may predate it
                cached_states.retain(|name, _| {
                    let path =
                        crate::logic::sync_states::breadcrumb_item_path(prefix.as_deref(), name);
                    app.model.pending.get(&folder_id, &path).is_none()
                });
                let derive_ctx =
                    crate::app::sync_states::derive_context(&app.model.syncthing, &folder_id);

                if let Some(level) = app.model.navigation.breadcrumb_trail.get_mut(idx) {
                    // Save currently selected item name BEFORE replacing items
                    let selected_name = level
                        .selected_index
                        .and_then(|sel_idx| level.display_items().get(sel_idx))
                        .map(|item| item.name.clone());

                    // Directories' own states: cached, else the previous own state
                    let mut direct_states =
                        crate::logic::sync_states::directory_states(&items, &cached_states);
                    for item in &items {
                        if let Some(state) = level.direct_states.get(&item.name)
                            && !direct_states.contains_key(&item.name)
                            && item.item_type == "FILE_INFO_TYPE_DIRECTORY"
                        {
                            direct_states.insert(item.name.clone(), *state);
                        }
                    }
                    let mut sync_states = crate::logic::sync_states::merge_level_states(
                        &level.file_sync_states,
                        cached_states,
                        &items,
                        &local_item_names,
                    );
                    crate::logic::sync_states::apply_derived_states(
                        &items,
                        prefix.as_deref(),
                        &mut sync_states,
                        &direct_states,
                        &derive_ctx,
                    );

                    crate::log_debug(&format!(
                        "DEBUG [BrowseResult]: Updating level {} with {} items (was {})",
                        idx,
                        items.len(),
                        level.items.len()
                    ));

                    // Update level
                    level.items = items.clone(); // Update unfiltered source
                    level.file_sync_states = sync_states;
                    level.direct_states = direct_states;

                    // DON'T touch filtered_items here - let the filter functions manage it

                    // Update directory states based on their children
                    app.update_directory_states(idx);

                    // Sort and restore selection using the saved name
                    app.sort_level_with_selection(idx, selected_name);

                    // Check if this is the current breadcrumb level
                    let is_current_level =
                        idx == app.model.navigation.focus_level.saturating_sub(1);

                    // Check if this BrowseResult is for the same folder as current level
                    // GUARD: Only check if we have a valid focus_level (not 0)
                    let current_level_folder = if app.model.navigation.focus_level > 0 {
                        app.model
                            .navigation
                            .breadcrumb_trail
                            .get(app.model.navigation.focus_level - 1)
                            .map(|l| l.folder_id.as_str())
                    } else {
                        None
                    };
                    let is_same_folder = current_level_folder == Some(&folder_id);

                    // CRITICAL: Re-apply search filter if active AND this is for the current folder
                    // Search uses cache.get_all_browse_items() which queries entire folder recursively,
                    // so we need to re-filter whenever ANY prefix in the folder gets cached (not just current level)
                    if !app.model.ui.search_query.is_empty() && is_same_folder {
                        app.apply_search_filter();
                    }

                    // Re-apply out-of-sync filter only for current level (it's level-specific)
                    if is_current_level && app.model.ui.out_of_sync_filter.is_some() {
                        app.apply_out_of_sync_filter();
                    }

                    // Request FileInfo for ALL items (no filtering, let API service deduplicate)
                    for item in &items {
                        let file_path = if let Some(ref pfx) = prefix {
                            format!("{}{}", pfx, item.name)
                        } else {
                            item.name.clone()
                        };

                        let sync_key = format!("{}:{}", folder_id, file_path);
                        if !app
                            .model
                            .performance
                            .loading_sync_states
                            .contains(&sync_key)
                        {
                            app.model.performance.loading_sync_states.insert(sync_key);
                            let _ = app.api_tx.send(ApiRequest::GetFileInfo {
                                folder_id: folder_id.clone(),
                                file_path,
                                priority: Priority::Medium,
                                requested_at: std::time::Instant::now(),
                            });
                        }
                    }
                }
            } else {
                // No matching breadcrumb level - this is a prefetch response
                // Browse items are already saved to cache, skip UI update
                crate::log_debug(&format!(
                    "DEBUG [BrowseResult]: No matching level for folder={} prefix={:?} (prefetch)",
                    folder_id, prefix
                ));

                // Continue prefetch if search is active (>= 2 chars)
                if !app.model.ui.search_query.is_empty() && app.model.ui.search_query.len() >= 2 {
                    crate::log_debug(&format!(
                        "DEBUG [BrowseResult]: Continuing prefetch from '{:?}'",
                        prefix
                    ));
                    // This will recursively queue subdirectories, but discovered_dirs prevents duplicates
                    app.prefetch_subdirectories_for_search(&folder_id, prefix.as_deref());

                    // Re-apply search filter to show newly cached items, but throttle to once per 300ms
                    // to prevent grinding the app to a halt when prefetching hundreds of directories
                    let elapsed = app.model.performance.last_search_filter_update.elapsed();
                    if elapsed.as_millis() >= 300 {
                        crate::log_debug(&format!(
                            "DEBUG [BrowseResult]: Updating search filter ({}ms since last update)",
                            elapsed.as_millis()
                        ));
                        app.model.performance.last_search_filter_update = std::time::Instant::now();
                        app.apply_search_filter();
                    } else {
                        crate::log_debug(&format!(
                            "DEBUG [BrowseResult]: Throttling search filter update (only {}ms since last)",
                            elapsed.as_millis()
                        ));
                    }
                }
            }
        }

        ApiResponse::FileInfoResult {
            folder_id,
            file_path,
            details,
            requested_at,
        } => {
            // Mark as no longer loading
            let sync_key = format!("{}:{}", folder_id, file_path);
            app.model.performance.loading_sync_states.remove(&sync_key);

            // An item event newer than the request already said more than this reply can
            let superseded = app
                .model
                .syncthing
                .path_events
                .get(&folder_id)
                .and_then(|events| events.get(file_path.trim_end_matches('/')))
                .is_some_and(|event| {
                    event.kind == crate::model::syncthing::PathEventKind::Finished
                        && event.at > requested_at
                });
            if superseded {
                crate::log_debug(&format!(
                    "DEBUG [FileInfoResult]: dropping reply older than ItemFinished folder={} path={}",
                    folder_id, file_path
                ));
                return;
            }

            let file_details = match details.as_ref() {
                Ok(details) => details,
                Err(error) => {
                    // API call failed - only update state if we're not already in Connecting mode
                    if !matches!(
                        app.model.syncthing.connection_state,
                        ConnectionState::Connecting { .. }
                    ) {
                        app.model.syncthing.connection_state = ConnectionState::Disconnected {
                            error_type: crate::logic::errors::classify_error(error),
                            message: crate::logic::errors::format_error_message(error),
                        };
                        crate::log_debug(&format!(
                            "DEBUG [FileInfoResult ERROR]: folder={} path={} error={:?}",
                            folder_id, file_path, error
                        ));
                    }
                    return;
                }
            };

            // Successful API call - mark as connected
            app.model.syncthing.connection_state = ConnectionState::Connected;

            let file_sequence = file_details
                .local
                .as_ref()
                .or(file_details.global.as_ref())
                .map(|f| f.sequence)
                .unwrap_or(0);

            let state = file_details.determine_sync_state();
            crate::log_debug(&format!(
                "DEBUG [FileInfoResult]: folder={} path={} state={:?} seq={}",
                folder_id, file_path, state, file_sequence
            ));

            let path = file_path.trim_end_matches('/');
            let confirmed_key = (folder_id.clone(), path.to_string());
            if app
                .model
                .syncthing
                .file_info_confirmed_at
                .get(&confirmed_key)
                .is_some_and(|confirmed| requested_at < *confirmed)
            {
                crate::log_debug(&format!(
                    "DEBUG [FileInfoResult]: dropping reply older than the one that confirmed a change folder={} path={}",
                    folder_id, file_path
                ));
                return;
            }
            if app.model.pending.get(&folder_id, path).is_some() {
                let verdict = app.observe_path(
                    &folder_id,
                    path,
                    Observation::FileInfo {
                        state,
                        requested_at,
                    },
                );
                if verdict == Verdict::Keep {
                    crate::log_debug(&format!(
                        "DEBUG [FileInfoResult]: pending change kept, not applying folder={} path={} state={:?}",
                        folder_id, file_path, state
                    ));
                    return;
                }
                if verdict == Verdict::Confirmed {
                    app.model
                        .syncthing
                        .file_info_confirmed_at
                        .insert(confirmed_key, requested_at);
                }
            }

            // Cached even when the folder is no longer on screen, so it isn't fetched again
            app.pending_sync_state_writes.push((
                folder_id.clone(),
                file_path.clone(),
                state,
                file_sequence,
            ));

            // Preview panels (folder list focused) count: their trail level is on screen
            let is_relevant = crate::logic::sync_states::is_file_info_relevant(
                &folder_id,
                app.model
                    .navigation
                    .breadcrumb_trail
                    .iter()
                    .map(|level| level.folder_id.as_str()),
            );
            if !is_relevant {
                crate::log_debug(&format!(
                    "DEBUG [FileInfoResult]: folder={} not in breadcrumb trail, cached only",
                    folder_id
                ));
                return;
            }

            // Update the level that lists this path as a direct child
            let mut matched_level = false;
            let derive_ctx =
                crate::app::sync_states::derive_context(&app.model.syncthing, &folder_id);
            for level in app.model.navigation.breadcrumb_trail.iter_mut() {
                if level.folder_id != folder_id {
                    continue;
                }
                let Some(item_name) =
                    crate::logic::sync_states::level_item_name(&file_path, level.prefix.as_deref())
                else {
                    continue;
                };
                matched_level = true;

                // FileInfo of a directory covers only its own entry; a worse aggregate wins
                let is_dir = level.items.iter().any(|item| {
                    item.name == item_name && item.item_type == "FILE_INFO_TYPE_DIRECTORY"
                });
                if is_dir {
                    level.direct_states.insert(item_name.to_string(), state);
                }
                let state = if is_dir {
                    let derived = crate::logic::sync_states::derived_state(
                        &file_path,
                        true,
                        &derive_ctx,
                        Some(state),
                    );
                    crate::logic::sync_states::resolve_directory_state(Some(state), derived)
                        .unwrap_or(state)
                } else {
                    state
                };

                let current_state = level.file_sync_states.get(item_name).copied();
                if current_state == Some(state) {
                    continue;
                }
                crate::log_debug(&format!(
                    "DEBUG [FileInfo]: Updating {} {:?} -> {:?}",
                    item_name, current_state, state
                ));
                level.file_sync_states.insert(item_name.to_string(), state);

                if state == SyncState::Ignored {
                    // translated_base_path already includes the full path to this directory level
                    let host_path = format!(
                        "{}/{}",
                        level.translated_base_path.trim_end_matches('/'),
                        item_name
                    );
                    let exists = std::path::Path::new(&host_path).exists();
                    level.ignored_exists.insert(item_name.to_string(), exists);
                } else {
                    level.ignored_exists.remove(item_name);
                }
            }
            if !matched_level {
                crate::log_debug(&format!(
                    "DEBUG [FileInfoResult]: no open level lists folder={} path={}",
                    folder_id, file_path
                ));
            }
        }

        ApiResponse::FolderStatusResult {
            folder_id,
            status,
            requested_at,
        } => {
            let Ok(status) = status else {
                // API call failed - only update state if we're not already in Connecting mode
                // (Background reconnection manages the Connecting state)
                if !matches!(
                    app.model.syncthing.connection_state,
                    ConnectionState::Connecting { .. }
                ) {
                    let error = status.unwrap_err();
                    let error_type = crate::logic::errors::classify_error(&error);
                    let message = crate::logic::errors::format_error_message(&error);

                    crate::log_debug(&format!(
                        "DEBUG [FolderStatusResult]: Error='{}' Type={:?} Message='{}'",
                        error, error_type, message
                    ));

                    app.model.syncthing.connection_state = ConnectionState::Disconnected {
                        error_type,
                        message,
                    };
                }
                return;
            };

            if app
                .model
                .syncthing
                .status_applied_at
                .get(&folder_id)
                .is_some_and(|applied| requested_at < *applied)
            {
                crate::log_debug(&format!(
                    "DEBUG [FolderStatusResult]: Dropping status older than the one shown for folder={}",
                    folder_id
                ));
                return;
            }
            app.model
                .syncthing
                .status_applied_at
                .insert(folder_id.clone(), requested_at);
            app.observe_folder_change(
                &folder_id,
                Observation::FolderStatus {
                    receive_only_items: status.receive_only_total_items,
                    requested_at,
                },
            );

            // Successful API call - mark as connected
            let was_disconnected = !matches!(
                app.model.syncthing.connection_state,
                ConnectionState::Connected
            );
            app.model.syncthing.connection_state = ConnectionState::Connected;

            // If we just reconnected, immediately fetch system status for responsive UI
            // SystemStatus will handle setting needs_folder_refresh if folders are empty
            if was_disconnected {
                let start = std::time::Instant::now();
                crate::log_debug(
                    "DEBUG [FolderStatusResult]: Just reconnected, requesting immediate system status",
                );
                let _ = app.api_tx.send(ApiRequest::GetSystemStatus);
                crate::log_debug(&format!(
                    "DEBUG [FolderStatusResult]: GetSystemStatus sent in {:?}",
                    start.elapsed()
                ));
            }

            let sequence = status.sequence;
            let receive_only_count = status.receive_only_total_items;

            // Check if sequence changed
            if let Some(&last_seq) = app.model.performance.last_known_sequences.get(&folder_id)
                && last_seq != sequence
            {
                crate::log_debug(&format!(
                    "DEBUG [FolderStatusResult]: Sequence changed from {} to {} for folder={}",
                    last_seq, sequence, folder_id
                ));

                // Sequence changed - invalidate cache and refresh
                app.invalidate_dir_child_states(&folder_id, None);
                app.invalidate_and_refresh_folder(&folder_id);
            }

            // Check if receive-only item count changed (indicates local-only files added/removed)
            if let Some(&last_count) = app
                .model
                .performance
                .last_known_receive_only_counts
                .get(&folder_id)
                && last_count != receive_only_count
            {
                crate::log_debug(&format!(
                    "DEBUG [FolderStatusResult]: receiveOnlyTotalItems changed from {} to {} for folder={}",
                    last_count, receive_only_count, folder_id
                ));

                // Trigger refresh for currently viewed directory
                if !app.model.navigation.breadcrumb_trail.is_empty()
                    && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
                {
                    for level in &mut app.model.navigation.breadcrumb_trail {
                        if level.folder_id == folder_id {
                            let browse_key =
                                format!("{}:{}", folder_id, level.prefix.as_deref().unwrap_or(""));
                            if !app.model.performance.loading_browse.contains(&browse_key) {
                                app.model.performance.loading_browse.insert(browse_key);

                                let _ = app.api_tx.send(ApiRequest::BrowseFolder {
                                    folder_id: folder_id.clone(),
                                    prefix: level.prefix.clone(),
                                    priority: Priority::High,
                                    requested_at: std::time::Instant::now(),
                                });

                                crate::log_debug(&format!(
                                    "DEBUG [FolderStatusResult]: Triggered browse refresh for prefix={:?}",
                                    level.prefix
                                ));
                            }
                        }
                    }
                }
            }

            // Update last known values
            app.model
                .performance
                .last_known_sequences
                .insert(folder_id.clone(), sequence);
            app.model
                .performance
                .last_known_receive_only_counts
                .insert(folder_id.clone(), receive_only_count);

            let prev_state = app
                .model
                .syncthing
                .folder_statuses
                .get(&folder_id)
                .map(|s| s.state.clone());

            // Save and use fresh status
            let _ = app.cache.save_folder_status(&folder_id, &status, sequence);
            app.model
                .syncthing
                .folder_statuses
                .insert(folder_id.clone(), status.clone());

            // After the insert: re-derived states must see this status
            app.schedule_need_index(&folder_id, prev_state.as_deref(), &status);

            // After the insert: a restarted need-list walk is tagged with the new sequence
            app.maybe_restart_need_list(&folder_id);
        }

        ApiResponse::RescanResult {
            folder_id,
            success,
            error,
        } => {
            let now = std::time::Instant::now();
            app.model.pending.start_deadlines(&folder_id, now);
            let ignore_changes: Vec<String> = app
                .model
                .pending
                .in_folder(&folder_id)
                .filter(|(_, change)| {
                    matches!(
                        change.kind,
                        PendingKind::Ignored { .. } | PendingKind::Unignored
                    ) && change.started <= now
                })
                .map(|(path, _)| path.to_string())
                .collect();
            for path in ignore_changes {
                app.model
                    .performance
                    .loading_sync_states
                    .insert(format!("{}:{}", folder_id, path));
                let _ = app.api_tx.send(ApiRequest::GetFileInfo {
                    folder_id: folder_id.clone(),
                    file_path: path,
                    priority: Priority::High,
                    requested_at: now,
                });
            }
            if success {
                crate::log_debug(&format!(
                    "DEBUG [RescanResult]: Successfully rescanned folder={}, requesting immediate status update",
                    folder_id
                ));

                // Immediately request folder status to detect sequence changes
                // This makes the rescan feel more responsive
                let _ = app.api_tx.send(ApiRequest::GetFolderStatus {
                    folder_id,
                    requested_at: std::time::Instant::now(),
                });
            } else {
                crate::log_debug(&format!(
                    "DEBUG [RescanResult ERROR]: Failed to rescan folder={} error={:?}",
                    folder_id, error
                ));
            }
        }

        ApiResponse::VersionResult { version } => {
            if let Ok(version) = version {
                app.model.syncthing.version = Some(version);
            }
        }

        ApiResponse::SystemStatusResult { status } => {
            match status {
                Ok(sys_status) => {
                    crate::log_debug(&format!(
                        "DEBUG [SystemStatusResult]: Received system status, uptime={}",
                        sys_status.uptime
                    ));

                    // Store system status first (needed for DevicesResult handler)
                    app.model.syncthing.system_status = Some(sys_status);

                    // Check if we just reconnected
                    let was_disconnected = !matches!(
                        app.model.syncthing.connection_state,
                        ConnectionState::Connected
                    );

                    // Syncthing may have been upgraded while stui was disconnected
                    if was_disconnected || app.model.syncthing.version.is_none() {
                        let _ = app.api_tx.send(ApiRequest::GetVersion);
                    }

                    // Always fetch devices list on successful system status (first poll after reconnection)
                    // This ensures we have fresh device list and update device name (in case it changed in GUI)
                    // Only fetch if we don't already have devices, to avoid unnecessary API calls
                    if app.model.syncthing.devices.is_empty()
                        || app.model.syncthing.device_name.is_none()
                    {
                        crate::log_debug(
                            "DEBUG [SystemStatusResult]: Requesting devices list to update device name",
                        );
                        let _ = app.api_tx.send(ApiRequest::GetDevices);
                    }

                    // If we're connected but have no folders, fetch them
                    // Note: We don't check was_disconnected because FolderStatus might have set
                    // connection state to Connected before SystemStatus arrives
                    // We also don't check show_setup_help because user might have dismissed it
                    if app.model.syncthing.folders.is_empty() {
                        crate::log_debug(&format!(
                            "DEBUG [SystemStatusResult]: Connected with no folders, setting fetch flag (was_disconnected={} show_setup_help={})",
                            was_disconnected, app.model.ui.show_setup_help
                        ));
                        app.model.ui.needs_folder_refresh = true;
                    } else {
                        crate::log_debug(&format!(
                            "DEBUG [SystemStatusResult]: NOT setting flag - folders.len()={} (was_disconnected={})",
                            app.model.syncthing.folders.len(),
                            was_disconnected
                        ));
                    }

                    // Successful API call - mark as connected
                    app.model.syncthing.connection_state = ConnectionState::Connected;
                }
                Err(e) => {
                    // SystemStatus is semi-critical: if we're idle and it fails, this likely indicates disconnection
                    // However, if we're already in Connecting state (reconnection active), don't override it
                    if !matches!(
                        app.model.syncthing.connection_state,
                        ConnectionState::Connecting { .. }
                    ) {
                        let error_type = crate::logic::errors::classify_error(&e);
                        let message = crate::logic::errors::format_error_message(&e);

                        app.model.syncthing.connection_state = ConnectionState::Disconnected {
                            error_type,
                            message: message.clone(),
                        };

                        crate::log_debug(&format!(
                            "DEBUG [SystemStatusResult]: Connection lost - {}",
                            message
                        ));
                    } else {
                        // Already reconnecting, don't override
                        crate::log_debug(&format!(
                            "DEBUG [SystemStatusResult ERROR]: {} (reconnection active, state unchanged)",
                            e
                        ));
                    }
                }
            }
        }

        ApiResponse::ConnectionStatsResult { stats } => match stats {
            Ok(conn_stats) => {
                // Update current stats with new data
                app.model.syncthing.connection_stats = Some(conn_stats.clone());

                // Calculate transfer rates if we have previous stats
                if let Some((prev_stats, prev_instant)) = &app.model.syncthing.last_connection_stats
                {
                    let elapsed = prev_instant.elapsed().as_secs_f64();
                    if elapsed > 0.0 {
                        let in_delta = (conn_stats.total.in_bytes_total as i64
                            - prev_stats.total.in_bytes_total as i64)
                            .max(0) as f64;
                        let out_delta = (conn_stats.total.out_bytes_total as i64
                            - prev_stats.total.out_bytes_total as i64)
                            .max(0) as f64;
                        let in_rate = in_delta / elapsed;
                        let out_rate = out_delta / elapsed;

                        // Store the calculated rates for UI display
                        app.model.syncthing.last_transfer_rates = Some((in_rate, out_rate));
                    }

                    // Update baseline every ~10 seconds to prevent drift
                    if elapsed > 10.0 {
                        app.model.syncthing.last_connection_stats =
                            Some((conn_stats, Instant::now()));
                    }
                } else {
                    // First fetch, store as baseline
                    app.model.syncthing.last_connection_stats = Some((conn_stats, Instant::now()));
                    // Initialize rates to zero on first fetch
                    app.model.syncthing.last_transfer_rates = Some((0.0, 0.0));
                }

                // Successful API call - mark as connected
                app.model.syncthing.connection_state = ConnectionState::Connected;
            }
            Err(e) => {
                // NOTE: Connection stats failures don't mark the entire connection as Disconnected
                // This is a non-critical endpoint - other API calls may still be working
                // Log the error but don't change connection state
                crate::log_debug(&format!(
                    "DEBUG [ConnectionStatsResult ERROR]: {} (non-critical, connection state unchanged)",
                    e
                ));
            }
        },

        ApiResponse::DevicesResult { devices } => match devices {
            Ok(devices_list) => {
                crate::log_debug(&format!(
                    "DEBUG [DevicesResult]: Received {} devices",
                    devices_list.len()
                ));

                // Update devices list in model
                app.model.syncthing.devices = devices_list.clone();

                // Extract and update device name if we don't have it yet
                if app.model.syncthing.device_name.is_none() {
                    if let Some(sys_status) = &app.model.syncthing.system_status {
                        let my_id = &sys_status.my_id;
                        if let Some(device) = devices_list.iter().find(|d| &d.id == my_id) {
                            app.model.syncthing.device_name = Some(device.name.clone());
                            // Cache device name for next startup
                            let _ = app.cache.save_device_name(&device.name);
                            crate::log_debug(&format!(
                                "DEBUG [DevicesResult]: Set device name to '{}'",
                                device.name
                            ));
                        } else {
                            crate::log_debug(&format!(
                                "DEBUG [DevicesResult]: Could not find device with my_id={}",
                                my_id
                            ));
                        }
                    } else {
                        crate::log_debug(
                            "DEBUG [DevicesResult]: No system status available yet, cannot extract device name",
                        );
                    }
                }

                // Successful API call - mark as connected
                app.model.syncthing.connection_state = ConnectionState::Connected;
            }
            Err(e) => {
                // Device list fetch failed - log but don't change connection state
                // This is non-critical for display purposes
                crate::log_debug(&format!(
                    "DEBUG [DevicesResult ERROR]: {} (non-critical, connection state unchanged)",
                    e
                ));
            }
        },

        ApiResponse::NeededFiles {
            folder_id,
            generation,
            sequence,
            page,
            perpage,
            response,
        } => {
            let filter_activated =
                app.continue_need_list(&folder_id, generation, sequence, page, perpage, response);
            if filter_activated {
                // Clear the "Loading out-of-sync files..." toast
                app.model.ui.dismiss_toast();
            }
        }

        ApiResponse::FolderNeed {
            folder_id,
            sequence,
            requested_at,
            need,
            ignore_patterns,
        } => {
            // Dropped when sent before the folder was cleared (it would resurrect the syncing
            // set) or when the index already holds a newer response
            let accepted = crate::logic::sync_states::accept_need_response(
                app.model.syncthing.need_fetches.get(&folder_id),
                app.model
                    .syncthing
                    .need_index
                    .get(&folder_id)
                    .and_then(|index| index.requested_at),
                requested_at,
            );
            if !accepted {
                crate::log_debug(&format!(
                    "DEBUG [FolderNeed]: dropping superseded response folder={} seq={}",
                    folder_id, sequence
                ));
                return;
            }
            match need {
                Ok(need) => {
                    let progress: Vec<String> =
                        crate::logic::sync_states::syncing_paths_from_need(&need)
                            .into_iter()
                            .collect();
                    let syncing = crate::logic::sync_states::syncing_from_need(
                        &progress,
                        requested_at,
                        app.model.syncthing.path_events.get(&folder_id),
                    );
                    crate::log_debug(&format!(
                        "DEBUG [FolderNeed]: folder={} seq={} in_progress={}",
                        folder_id,
                        sequence,
                        syncing.len()
                    ));
                    let mut syncing = syncing;
                    syncing.extend(
                        app.model
                            .pending
                            .restoring_in(&folder_id)
                            .map(str::to_string),
                    );
                    app.model
                        .syncthing
                        .syncing_files
                        .insert(folder_id.clone(), syncing);

                    // Counts from the status the request was made for; a newer status means
                    // this list is already stale and a refetch is queued
                    let complete = app
                        .model
                        .syncthing
                        .folder_statuses
                        .get(&folder_id)
                        .is_some_and(|status| {
                            status.sequence == sequence
                                && crate::logic::sync_states::need_entry_count(&need)
                                    >= status.need_total_items
                        });
                    let index = app
                        .model
                        .syncthing
                        .need_index
                        .entry(folder_id.clone())
                        .or_default();
                    crate::logic::sync_states::set_index_need(
                        index,
                        &need,
                        sequence,
                        complete,
                        ignore_patterns.ok(),
                    );
                    index.requested_at = Some(requested_at);
                    app.refresh_derived_states(&folder_id);
                }
                Err(e) => {
                    crate::log_debug(&format!(
                        "DEBUG [FolderNeed ERROR]: folder={} error={}",
                        folder_id, e
                    ));
                    // The next poll retries after the refresh interval
                    if let Some(fetch) = app.model.syncthing.need_fetches.get_mut(&folder_id)
                        && fetch.at == requested_at
                    {
                        fetch.failed = true;
                    }
                }
            }
        }

        ApiResponse::SystemLogResult { .. } => {}

        ApiResponse::LocalChangedIndex {
            folder_id,
            sequence,
            requested_at,
            files,
        } => {
            if app.model.syncthing.local_changed_requested.get(&folder_id) != Some(&sequence) {
                crate::log_debug(&format!(
                    "DEBUG [LocalChangedIndex]: dropping superseded response folder={} seq={}",
                    folder_id, sequence
                ));
                return;
            }
            match files {
                Ok(files) => {
                    crate::log_debug(&format!(
                        "DEBUG [LocalChangedIndex]: folder={} seq={} files={}",
                        folder_id,
                        sequence,
                        files.len()
                    ));
                    let index = app
                        .model
                        .syncthing
                        .need_index
                        .entry(folder_id.clone())
                        .or_default();
                    crate::logic::sync_states::set_index_local_changed(index, &files, sequence);
                    // A download that finished after the request already undid its local change
                    if let Some(events) = app.model.syncthing.path_events.get(&folder_id) {
                        for (path, event) in events {
                            if event.kind == crate::model::syncthing::PathEventKind::Finished
                                && event.succeeded
                                && event.at > requested_at
                            {
                                crate::logic::sync_states::forget_local_change(index, path);
                            }
                        }
                    }
                    let pending: Vec<String> = app
                        .model
                        .pending
                        .in_folder(&folder_id)
                        .map(|(path, _)| path.to_string())
                        .collect();
                    for path in pending {
                        let deleted = files
                            .iter()
                            .any(|f| f.deleted && f.name.trim_end_matches('/') == path);
                        app.observe_path(
                            &folder_id,
                            &path,
                            Observation::LocalChanged {
                                deleted,
                                requested_at,
                            },
                        );
                    }
                    if let Some(index) = app.model.syncthing.need_index.get_mut(&folder_id) {
                        crate::logic::pending::reapply_to_index(
                            index,
                            app.model
                                .pending
                                .in_folder(&folder_id)
                                .map(|(path, change)| (path, &change.kind)),
                        );
                    }
                    app.refresh_derived_states(&folder_id);
                }
                Err(e) => {
                    crate::log_debug(&format!(
                        "DEBUG [LocalChangedIndex ERROR]: folder={} error={}",
                        folder_id, e
                    ));
                    // The next poll retries after the refresh interval
                    app.model
                        .syncthing
                        .local_changed_failed_at
                        .insert(folder_id.clone(), Instant::now());
                }
            }
        }

        ApiResponse::LocalChanged {
            folder_id,
            file_paths,
        } => {
            // Cache the response
            if let Err(e) = app.cache.cache_local_changed_files(&folder_id, &file_paths) {
                crate::log_debug(&format!(
                    "Failed to cache local changed files for {}: {}",
                    folder_id, e
                ));
            }

            // If filter is active for this folder, re-apply it with updated data
            if let Some(_filter_state) = &app.model.ui.out_of_sync_filter {
                let current_folder_id = app
                    .model
                    .navigation
                    .breadcrumb_trail
                    .first()
                    .map(|level| &level.folder_id);

                if current_folder_id == Some(&folder_id) {
                    // Re-sort and re-filter to pick up new local changed files
                    app.sort_all_levels();
                    app.apply_out_of_sync_filter();
                }
            }
        }
    }
}

/// Judge a listing of `prefix` against the pending changes of paths directly in it, and drop
/// rows whose removal is still pending
fn observe_listing(
    app: &mut App,
    folder_id: &str,
    prefix: Option<&str>,
    items: &mut Vec<BrowseItem>,
    requested_at: Instant,
) {
    let listed_here: Vec<(String, String)> = app
        .model
        .pending
        .in_folder(folder_id)
        .filter_map(|(path, _)| {
            crate::logic::sync_states::name_in_level(prefix, path)
                .map(|name| (path.to_string(), name.to_string()))
        })
        .collect();
    for (path, name) in &listed_here {
        let present = items.iter().any(|item| &item.name == name);
        app.observe_path(
            folder_id,
            path,
            Observation::Listed {
                present,
                requested_at,
            },
        );
    }
    items.retain(|item| {
        let path = crate::logic::sync_states::breadcrumb_item_path(prefix, &item.name);
        app.model
            .pending
            .get(folder_id, &path)
            .is_none_or(|change| change.kind != PendingKind::Removed)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::events::handle_cache_invalidation;
    use crate::logic::display::ItemDisplay;
    use crate::model::pending::PendingFolderKind;
    use crate::model::pending::{PATH_DEADLINE, REVERT_DEADLINE};
    use crate::test_support::{
        create_test_app, create_test_app_with_api_rx, file_details, file_item, folder_status,
        item_finished, status_result, view_folder_root,
    };
    use std::time::Duration;

    fn status_with_sequence(folder: &str, sequence: u64, requested_at: Instant) -> ApiResponse {
        let mut status = folder_status("idle");
        status.sequence = sequence;
        ApiResponse::FolderStatusResult {
            folder_id: folder.to_string(),
            status: Ok(status),
            requested_at,
        }
    }

    fn listing(folder: &str, names: &[&str], requested_at: Instant) -> ApiResponse {
        ApiResponse::BrowseResult {
            folder_id: folder.to_string(),
            prefix: None,
            items: Ok(names.iter().map(|n| file_item(n)).collect()),
            requested_at,
        }
    }

    fn shown_names(app: &App) -> Vec<String> {
        app.model.navigation.breadcrumb_trail[0]
            .items
            .iter()
            .map(|i| i.name.clone())
            .collect()
    }

    // ========================================
    // OUT-OF-ORDER REPLIES
    // ========================================

    #[test]
    fn test_older_folder_status_reply_is_dropped() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);

        handle_api_response(&mut app, status_with_sequence("f", 7, t1));
        while rx.try_recv().is_ok() {}
        handle_api_response(&mut app, status_with_sequence("f", 3, t0));

        assert_eq!(app.model.syncthing.folder_statuses["f"].sequence, 7);
        assert_eq!(app.model.performance.last_known_sequences["f"], 7);
        let queued: Vec<ApiRequest> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            !queued
                .iter()
                .any(|r| matches!(r, ApiRequest::BrowseFolder { .. })),
            "dropped reply queued a browse: {:?}",
            queued
        );
    }

    #[test]
    fn test_newer_folder_status_reply_is_applied() {
        let mut app = create_test_app();
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);

        handle_api_response(&mut app, status_with_sequence("f", 3, t0));
        handle_api_response(&mut app, status_with_sequence("f", 7, t1));

        assert_eq!(app.model.syncthing.folder_statuses["f"].sequence, 7);
        assert_eq!(app.model.syncthing.status_applied_at["f"], t1);
    }

    #[test]
    fn test_older_browse_reply_is_dropped_and_clears_loading() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![]);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);

        handle_api_response(&mut app, listing("f", &["a", "b"], t1));
        app.model
            .performance
            .loading_browse
            .insert("f:".to_string());
        handle_api_response(&mut app, listing("f", &["a", "b", "c"], t0));

        assert_eq!(shown_names(&app), vec!["a", "b"]);
        assert!(!app.model.performance.loading_browse.contains("f:"));
    }

    #[test]
    fn test_newer_browse_reply_is_applied() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![]);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);

        handle_api_response(&mut app, listing("f", &["a", "b"], t0));
        handle_api_response(&mut app, listing("f", &["a", "b", "c"], t1));

        assert_eq!(shown_names(&app), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_browse_order_is_tracked_per_prefix() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![]);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);

        let sub = |at| ApiResponse::BrowseResult {
            folder_id: "f".to_string(),
            prefix: Some("sub/".to_string()),
            items: Ok(vec![file_item("x")]),
            requested_at: at,
        };
        handle_api_response(&mut app, sub(t1));
        handle_api_response(&mut app, listing("f", &["a"], t0));

        assert_eq!(shown_names(&app), vec!["a"]);
    }

    // ========================================
    // PENDING LOCAL CHANGES
    // ========================================

    /// An instant strictly before anything recorded after this call
    fn before_now() -> Instant {
        Instant::now() - Duration::from_millis(1)
    }

    fn displayed(app: &App, name: &str) -> Option<ItemDisplay> {
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

    fn file_info(path: &str, state: SyncState, requested_at: Instant) -> ApiResponse {
        ApiResponse::FileInfoResult {
            folder_id: "f".to_string(),
            file_path: path.to_string(),
            details: Box::new(Ok(file_details(state))),
            requested_at,
        }
    }

    fn cache_write_queued(app: &App, path: &str) -> bool {
        app.pending_sync_state_writes
            .iter()
            .any(|(folder, p, ..)| folder == "f" && p == path)
    }

    fn local_changed_index(sequence: u64, deleted: &[&str], requested_at: Instant) -> ApiResponse {
        ApiResponse::LocalChangedIndex {
            folder_id: "f".to_string(),
            sequence,
            requested_at,
            files: Ok(deleted
                .iter()
                .map(|n| crate::api::LocalChangedFile {
                    name: n.to_string(),
                    deleted: true,
                })
                .collect()),
        }
    }

    #[test]
    fn test_pending_ignored_survives_stale_file_info() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        let sent = before_now();
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(&mut app, file_info("a", SyncState::Synced, sent));

        assert_eq!(
            displayed(&app, "a"),
            Some(ItemDisplay::Ignored { exists: true })
        );
        assert!(app.model.pending.get("f", "a").is_some());
        assert_eq!(
            app.model.navigation.breadcrumb_trail[0].file_sync_states["a"],
            SyncState::RemoteOnly
        );
        assert!(!cache_write_queued(&app, "a"));
    }

    #[test]
    fn test_pending_ignored_survives_fresh_unconfirming_file_info() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(&mut app, file_info("a", SyncState::Synced, Instant::now()));

        assert_eq!(
            displayed(&app, "a"),
            Some(ItemDisplay::Ignored { exists: true })
        );
        assert!(app.model.pending.get("f", "a").is_some());
        assert!(!cache_write_queued(&app, "a"));
    }

    #[test]
    fn test_pending_ignored_confirmed_by_fresh_file_info_clears_change() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(&mut app, file_info("a", SyncState::Ignored, Instant::now()));

        assert!(app.model.pending.get("f", "a").is_none());
        assert_eq!(
            app.model.navigation.breadcrumb_trail[0].file_sync_states["a"],
            SyncState::Ignored
        );
        assert!(cache_write_queued(&app, "a"));
        // /data/f/a doesn't exist on this machine
        assert_eq!(
            displayed(&app, "a"),
            Some(ItemDisplay::Ignored { exists: false })
        );
    }

    #[test]
    fn test_pending_deleted_locally_survives_same_sequence_local_changed_reply() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.model
            .syncthing
            .local_changed_requested
            .insert("f".to_string(), 5);
        let sent = before_now();
        app.record_pending("f", "a", PendingKind::DeletedLocally);

        handle_api_response(&mut app, local_changed_index(5, &[], sent));

        assert_eq!(
            app.model.syncthing.need_index["f"].local_changed.get("a"),
            Some(&SyncState::RemoteOnly)
        );
        assert!(app.model.pending.get("f", "a").is_some());
        assert_eq!(displayed(&app, "a"), Some(ItemDisplay::LocallyDeleted));
    }

    #[test]
    fn test_pending_deleted_locally_confirmed_by_fresh_local_changed_reply() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.model
            .syncthing
            .local_changed_requested
            .insert("f".to_string(), 5);
        app.record_pending("f", "a", PendingKind::DeletedLocally);

        handle_api_response(&mut app, local_changed_index(5, &["a"], Instant::now()));

        assert!(app.model.pending.get("f", "a").is_none());
        assert_eq!(displayed(&app, "a"), Some(ItemDisplay::LocallyDeleted));
    }

    #[test]
    fn test_zero_receive_only_status_keeps_pending_local_deletion_in_index() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending("f", "a", PendingKind::DeletedLocally);

        handle_api_response(&mut app, status_result("f", "idle"));

        assert_eq!(
            app.model.syncthing.need_index["f"].local_changed.get("a"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(displayed(&app, "a"), Some(ItemDisplay::LocallyDeleted));
    }

    #[test]
    fn test_pending_removed_row_stays_hidden_on_stale_listing() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("b")]);
        let sent = before_now();
        app.record_pending("f", "a", PendingKind::Removed);

        handle_api_response(&mut app, listing("f", &["a", "b"], sent));

        assert_eq!(shown_names(&app), vec!["b"]);
        assert!(app.model.pending.get("f", "a").is_some());
    }

    #[test]
    fn test_pending_removed_confirmed_by_fresh_listing_without_it() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("b")]);
        app.record_pending("f", "a", PendingKind::Removed);

        handle_api_response(&mut app, listing("f", &["a", "b"], Instant::now()));
        assert_eq!(shown_names(&app), vec!["b"]);
        assert!(app.model.pending.get("f", "a").is_some());

        handle_api_response(&mut app, listing("f", &["b"], Instant::now()));
        assert!(app.model.pending.get("f", "a").is_none());
    }

    #[test]
    fn test_pending_path_keeps_in_memory_state_over_cache_on_listing() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.cache
            .save_sync_state("f", "a", SyncState::Synced, 1)
            .unwrap();
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(&mut app, listing("f", &["a"], Instant::now()));

        assert_eq!(
            app.model.navigation.breadcrumb_trail[0].file_sync_states["a"],
            SyncState::RemoteOnly
        );
    }

    #[test]
    fn test_rescan_result_starts_deadline_and_expiry_requests_file_info() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(
            &mut app,
            ApiResponse::RescanResult {
                folder_id: "f".to_string(),
                success: true,
                error: None,
            },
        );
        let deadline = app.model.pending.get("f", "a").unwrap().deadline;
        assert!(deadline.is_some());
        while rx.try_recv().is_ok() {}

        app.expire_pending(Instant::now() + PATH_DEADLINE);

        assert!(app.model.pending.get("f", "a").is_none());
        let queued: Vec<ApiRequest> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            queued.iter().any(|r| matches!(
                r,
                ApiRequest::GetFileInfo { folder_id, file_path, priority: Priority::High, .. }
                    if folder_id == "f" && file_path == "a"
            )),
            "{:?}",
            queued
        );
        assert!(app.model.performance.loading_sync_states.contains("f:a"));
    }

    #[test]
    fn test_rescan_result_refetches_pending_ignore_paths() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );
        app.record_pending("f", "b", PendingKind::Unignored);
        app.record_pending("f", "c", PendingKind::Removed);
        app.record_pending("g", "d", PendingKind::Unignored);
        while rx.try_recv().is_ok() {}

        handle_api_response(
            &mut app,
            ApiResponse::RescanResult {
                folder_id: "f".to_string(),
                success: true,
                error: None,
            },
        );

        let mut fetched: Vec<(String, String)> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|r| match r {
                ApiRequest::GetFileInfo {
                    folder_id,
                    file_path,
                    priority: Priority::High,
                    ..
                } => Some((folder_id, file_path)),
                _ => None,
            })
            .collect();
        fetched.sort();
        assert_eq!(
            fetched,
            vec![
                ("f".to_string(), "a".to_string()),
                ("f".to_string(), "b".to_string())
            ]
        );
        let loading = &app.model.performance.loading_sync_states;
        assert!(loading.contains("f:a") && loading.contains("f:b"));
        assert!(!loading.contains("f:c") && !loading.contains("g:d"));
    }

    #[test]
    fn test_expired_removed_change_relists_its_level() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(&mut app, "f", vec![file_item("b")]);
        app.model.navigation.breadcrumb_trail[0].prefix = Some("dir/".to_string());
        app.record_pending("f", "dir/a", PendingKind::Removed);
        app.model.pending.start_deadlines("f", Instant::now());
        while rx.try_recv().is_ok() {}

        app.expire_pending(Instant::now() + PATH_DEADLINE);

        let queued: Vec<ApiRequest> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            queued.iter().any(|r| matches!(
                r,
                ApiRequest::BrowseFolder { folder_id, prefix, priority: Priority::High, .. }
                    if folder_id == "f" && prefix.as_deref() == Some("dir/")
            )),
            "{:?}",
            queued
        );
        assert!(app.model.performance.loading_browse.contains("f:dir/"));
    }

    #[test]
    fn test_failed_rescan_also_starts_deadline() {
        let mut app = create_test_app();
        app.record_pending("f", "a", PendingKind::Removed);

        handle_api_response(
            &mut app,
            ApiResponse::RescanResult {
                folder_id: "f".to_string(),
                success: false,
                error: Some(anyhow::anyhow!("boom")),
            },
        );

        assert!(app.model.pending.get("f", "a").unwrap().deadline.is_some());
    }

    #[test]
    fn test_expiry_refetches_only_open_level_paths() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending("f", "a", PendingKind::Unignored);
        app.record_pending("f", "sub/b", PendingKind::Unignored);
        app.record_pending("g", "c", PendingKind::Unignored);
        let now = Instant::now();
        app.model.pending.start_deadlines("f", now);
        app.model.pending.start_deadlines("g", now);
        while rx.try_recv().is_ok() {}

        app.expire_pending(now + PATH_DEADLINE);

        assert_eq!(app.model.pending.in_folder("f").count(), 0);
        assert_eq!(app.model.pending.in_folder("g").count(), 0);
        let fetched: Vec<(String, String)> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|r| match r {
                ApiRequest::GetFileInfo {
                    folder_id,
                    file_path,
                    ..
                } => Some((folder_id, file_path)),
                _ => None,
            })
            .collect();
        assert_eq!(fetched, vec![("f".to_string(), "a".to_string())]);
        let loading = &app.model.performance.loading_sync_states;
        assert!(loading.contains("f:a"));
        assert!(!loading.contains("f:sub/b") && !loading.contains("g:c"));
    }

    #[test]
    fn test_expired_folder_change_refreshes_folder_statuses() {
        let (mut app, mut rx) = create_test_app_with_api_rx();
        app.model.syncthing.folders = vec![
            serde_json::from_value(serde_json::json!({
                "id": "f",
                "label": "F",
                "path": "/data/f",
                "paused": false,
                "type": "receiveonly",
            }))
            .expect("valid Folder"),
        ];
        let now = Instant::now();
        app.model
            .pending
            .record_folder("f", PendingFolderKind::Reverting, now);
        app.model.pending.folder_mut("f").unwrap().deadline = Some(now);

        app.expire_pending(now);

        assert!(app.model.pending.folder("f").is_none());
        let queued: Vec<ApiRequest> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            queued.iter().any(
                |r| matches!(r, ApiRequest::GetFolderStatus { folder_id, .. } if folder_id == "f")
            ),
            "{:?}",
            queued
        );
    }

    #[test]
    fn test_fresh_zero_receive_only_status_confirms_revert() {
        let mut app = create_test_app();
        app.model
            .pending
            .record_folder("f", PendingFolderKind::Reverting, Instant::now());

        let stale = before_now() - Duration::from_millis(5);
        handle_api_response(
            &mut app,
            crate::test_support::status_result_at("f", "idle", stale),
        );
        assert!(app.model.pending.folder("f").is_some());

        handle_api_response(&mut app, status_result("f", "idle"));
        assert!(app.model.pending.folder("f").is_none());
    }

    #[test]
    fn test_pause_contradicts_restoring_and_revert() {
        let mut app = create_test_app();
        app.record_pending("f", "a", PendingKind::Restoring);
        app.record_pending("f", "b", PendingKind::Removed);
        app.model
            .pending
            .record_folder("f", PendingFolderKind::Reverting, Instant::now());

        let mut status = folder_status("paused");
        status.receive_only_total_items = 3;
        handle_api_response(
            &mut app,
            ApiResponse::FolderStatusResult {
                folder_id: "f".to_string(),
                status: Ok(status),
                requested_at: Instant::now(),
            },
        );

        assert!(app.model.pending.get("f", "a").is_none());
        assert!(app.model.pending.get("f", "b").is_some());
        assert!(app.model.pending.folder("f").is_none());
    }

    #[test]
    fn test_item_finished_confirms_restoring() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending("f", "a", PendingKind::Restoring);
        assert_eq!(
            displayed(&app, "a"),
            Some(ItemDisplay::State(SyncState::Syncing))
        );

        handle_cache_invalidation(&mut app, item_finished("f", "a"));

        assert!(app.model.pending.get("f", "a").is_none());
        assert_eq!(
            displayed(&app, "a"),
            Some(ItemDisplay::State(SyncState::Synced))
        );
    }

    // ========================================
    // REVERT
    // ========================================

    /// Answers every request with an empty 200 until the test ends
    async fn spawn_ok_stub() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                });
            }
        });
        format!("http://{}", addr)
    }

    fn changed(name: &str, deleted: bool) -> crate::api::LocalChangedFile {
        crate::api::LocalChangedFile {
            name: name.to_string(),
            deleted,
        }
    }

    fn reverting_changes() -> Vec<crate::api::LocalChangedFile> {
        vec![
            changed("gone", true),
            changed("edited", false),
            changed("new", false),
        ]
    }

    fn revert_app(url: &str) -> App {
        let (mut app, _rx) = crate::test_support::create_test_app_at(url);
        view_folder_root(
            &mut app,
            "f",
            vec![file_item("gone"), file_item("edited"), file_item("new")],
        );
        let mut index = crate::model::syncthing::NeedIndex::default();
        crate::logic::sync_states::set_index_local_changed(&mut index, &reverting_changes(), 5);
        app.model
            .syncthing
            .need_index
            .insert("f".to_string(), index);
        app.model
            .syncthing
            .local_changed_requested
            .insert("f".to_string(), 5);
        app
    }

    fn all_syncing(app: &App) -> bool {
        ["gone", "edited", "new"]
            .iter()
            .all(|n| displayed(app, n) == Some(ItemDisplay::State(SyncState::Syncing)))
    }

    #[tokio::test]
    async fn test_revert_marks_local_changes_pending() {
        let url = spawn_ok_stub().await;
        let mut app = revert_app(&url);

        let before = Instant::now();
        app.revert_folder_changes("f", &reverting_changes())
            .await
            .unwrap();
        let after = Instant::now();

        let kind = |p: &str| app.model.pending.get("f", p).map(|c| c.kind.clone());
        assert_eq!(kind("gone"), Some(PendingKind::Restoring));
        assert_eq!(kind("edited"), Some(PendingKind::Reverting));
        assert_eq!(kind("new"), Some(PendingKind::Reverting));
        let folder = app.model.pending.folder("f").unwrap();
        assert_eq!(folder.kind, PendingFolderKind::Reverting);
        let deadline = folder.deadline.expect("deadline set after the request");
        assert!(deadline >= before + REVERT_DEADLINE);
        assert!(deadline <= after + REVERT_DEADLINE);
        assert!(all_syncing(&app));
        assert!(app.model.syncthing.need_index["f"].local_changed.is_empty());
    }

    #[tokio::test]
    async fn test_revert_stale_replies_do_not_flip_icons() {
        let url = spawn_ok_stub().await;
        let mut app = revert_app(&url);
        let sent = before_now();
        app.revert_folder_changes("f", &reverting_changes())
            .await
            .unwrap();

        handle_api_response(&mut app, file_info("gone", SyncState::RemoteOnly, sent));
        handle_api_response(&mut app, file_info("edited", SyncState::Synced, sent));
        handle_api_response(&mut app, file_info("new", SyncState::Synced, sent));
        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "f".to_string(),
                sequence: 5,
                requested_at: sent,
                files: Ok(reverting_changes()),
            },
        );

        assert!(all_syncing(&app));
        assert_eq!(app.model.pending.in_folder("f").count(), 3);
        assert!(app.model.syncthing.need_index["f"].local_changed.is_empty());
    }

    #[tokio::test]
    async fn test_revert_confirmed_by_zero_local_changes_status() {
        let url = spawn_ok_stub().await;
        let mut app = revert_app(&url);
        let sent = before_now();
        app.revert_folder_changes("f", &reverting_changes())
            .await
            .unwrap();

        handle_api_response(
            &mut app,
            crate::test_support::status_result_at("f", "idle", sent),
        );
        assert!(app.model.pending.folder("f").is_some());

        handle_api_response(&mut app, status_result("f", "idle"));
        assert!(app.model.pending.folder("f").is_none());
    }

    #[tokio::test]
    async fn test_revert_path_changes_expire_with_the_folder() {
        let url = spawn_ok_stub().await;
        let (mut app, _rx) = crate::test_support::create_test_app_at(&url);
        view_folder_root(
            &mut app,
            "f",
            vec![crate::test_support::dir_item("d"), file_item("edited")],
        );
        let changes = vec![changed("d/x", false), changed("edited", false)];
        let mut index = crate::model::syncthing::NeedIndex::default();
        crate::logic::sync_states::set_index_local_changed(&mut index, &changes, 5);
        app.model
            .syncthing
            .need_index
            .insert("f".to_string(), index);
        app.revert_folder_changes("f", &changes).await.unwrap();
        assert_eq!(
            displayed(&app, "d"),
            Some(ItemDisplay::State(SyncState::Syncing))
        );

        app.expire_pending(Instant::now() + REVERT_DEADLINE);

        assert!(app.model.pending.folder("f").is_none());
        assert!(
            !app.model
                .pending
                .in_folder("f")
                .any(|(_, c)| c.kind == PendingKind::Reverting)
        );
        assert_ne!(
            displayed(&app, "d"),
            Some(ItemDisplay::State(SyncState::Syncing))
        );
    }

    #[tokio::test]
    async fn test_revert_failure_drops_changes() {
        let mut app = revert_app("http://127.0.0.1:9");
        app.refresh_derived_states("f");
        let before: Vec<_> = ["gone", "edited", "new"]
            .iter()
            .map(|n| displayed(&app, n))
            .collect();
        assert_eq!(before[0], Some(ItemDisplay::LocallyDeleted));

        let result = app.revert_folder_changes("f", &reverting_changes()).await;

        assert!(result.is_err());
        assert_eq!(app.model.pending.in_folder("f").count(), 0);
        assert!(app.model.pending.folder("f").is_none());
        let after: Vec<_> = ["gone", "edited", "new"]
            .iter()
            .map(|n| displayed(&app, n))
            .collect();
        assert_eq!(after, before);
        let index = &app.model.syncthing.need_index["f"];
        assert_eq!(
            index.local_changed.get("gone"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            index.local_changed.get("edited"),
            Some(&SyncState::LocalOnly)
        );
        assert_eq!(index.local_changed.get("new"), Some(&SyncState::LocalOnly));
    }

    #[tokio::test]
    async fn test_revert_updates_parent_directory_at_once() {
        let url = spawn_ok_stub().await;
        let (mut app, _rx) = crate::test_support::create_test_app_at(&url);
        view_folder_root(&mut app, "f", vec![crate::test_support::dir_item("d")]);
        let changes = vec![changed("d/x", false)];
        let mut index = crate::model::syncthing::NeedIndex::default();
        crate::logic::sync_states::set_index_local_changed(&mut index, &changes, 5);
        app.model
            .syncthing
            .need_index
            .insert("f".to_string(), index);
        app.refresh_derived_states("f");
        let before = displayed(&app, "d");
        assert_eq!(before, Some(ItemDisplay::State(SyncState::LocalOnly)));

        app.revert_folder_changes("f", &changes).await.unwrap();

        assert_eq!(
            displayed(&app, "d"),
            Some(ItemDisplay::State(SyncState::Syncing))
        );
    }

    #[tokio::test]
    async fn test_stale_local_changed_after_revert_finished_does_not_show_local_change() {
        let url = spawn_ok_stub().await;
        let (mut app, _rx) = crate::test_support::create_test_app_at(&url);
        view_folder_root(
            &mut app,
            "f",
            vec![crate::test_support::dir_item("d"), file_item("edited")],
        );
        let changes = vec![changed("d/x", false), changed("edited", false)];
        let mut index = crate::model::syncthing::NeedIndex::default();
        crate::logic::sync_states::set_index_local_changed(&mut index, &changes, 5);
        app.model
            .syncthing
            .need_index
            .insert("f".to_string(), index);
        app.model
            .syncthing
            .local_changed_requested
            .insert("f".to_string(), 5);
        let sent = before_now();
        app.revert_folder_changes("f", &changes).await.unwrap();
        handle_cache_invalidation(&mut app, item_finished("f", "d/x"));
        handle_cache_invalidation(&mut app, item_finished("f", "edited"));
        assert_eq!(app.model.pending.in_folder("f").count(), 0);

        handle_api_response(
            &mut app,
            ApiResponse::LocalChangedIndex {
                folder_id: "f".to_string(),
                sequence: 5,
                requested_at: sent,
                files: Ok(changes),
            },
        );

        let shown = crate::test_support::displayed_items(&app);
        assert_eq!(shown["edited"], Some(ItemDisplay::State(SyncState::Synced)));
        assert_ne!(shown["d"], Some(ItemDisplay::State(SyncState::LocalOnly)));
    }

    #[test]
    fn test_reverting_added_file_confirmed_by_delete_event() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("new")]);
        app.record_pending("f", "new", PendingKind::Reverting);

        handle_cache_invalidation(
            &mut app,
            crate::services::events::CacheInvalidation::ItemFinished {
                folder_id: "f".to_string(),
                file_path: "new".to_string(),
                failed: false,
                deleted: true,
                timestamp: std::time::SystemTime::now(),
            },
        );

        assert!(app.model.pending.get("f", "new").is_none());
    }

    // ========================================
    // CACHE PRECEDENCE AND WRITE-THROUGH
    // ========================================

    fn queued_write(app: &App, path: &str) -> Option<SyncState> {
        app.pending_sync_state_writes
            .iter()
            .rev()
            .find(|(folder, p, ..)| folder == "f" && p == path)
            .map(|(_, _, state, _)| *state)
    }

    #[test]
    fn test_reopened_level_shows_pending_state_not_cache() {
        let mut app = create_test_app();
        let items = vec![file_item("a"), file_item("b")];
        app.cache
            .save_sync_state("f", "sub/a", SyncState::Synced, 0)
            .unwrap();
        app.cache
            .save_sync_state("f", "sub/b", SyncState::Synced, 0)
            .unwrap();
        app.record_pending(
            "f",
            "sub/a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        let (states, _) = app.load_sync_states_from_cache("f", &items, Some("sub/"));

        assert!(!states.contains_key("a"));
        assert_eq!(states.get("b"), Some(&SyncState::Synced));
    }

    #[test]
    fn test_confirmed_change_written_through_to_cache() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.cache
            .save_sync_state("f", "a", SyncState::Synced, 0)
            .unwrap();
        app.record_pending(
            "f",
            "a",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
        );

        handle_api_response(&mut app, file_info("a", SyncState::Ignored, Instant::now()));

        assert!(app.model.pending.get("f", "a").is_none());
        assert_eq!(queued_write(&app, "a"), Some(SyncState::Ignored));
    }

    #[test]
    fn test_item_finished_synced_written_through_to_cache() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);

        handle_cache_invalidation(&mut app, item_finished("f", "a"));

        assert_eq!(queued_write(&app, "a"), Some(SyncState::Synced));
    }

    #[test]
    fn test_item_finished_confirming_restore_written_through_to_cache() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);
        app.record_pending("f", "a", PendingKind::Restoring);

        handle_cache_invalidation(&mut app, item_finished("f", "a"));

        assert!(app.model.pending.get("f", "a").is_none());
        assert_eq!(queued_write(&app, "a"), Some(SyncState::Synced));
    }

    #[test]
    fn test_failed_item_finished_writes_nothing() {
        let mut app = create_test_app();
        view_folder_root(&mut app, "f", vec![file_item("a")]);

        handle_cache_invalidation(&mut app, crate::test_support::item_failed("f", "a"));

        assert_eq!(queued_write(&app, "a"), None);
    }
}
