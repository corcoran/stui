//! API Response Handler
//!
//! Handles responses from the Syncthing REST API background service.
//! Processes browse results, file info, folder status, and connection stats.

use std::time::Instant;

use crate::api::SyncState;
use crate::model;
use crate::model::syncthing::ConnectionState;
use crate::services::api::{ApiRequest, ApiResponse, Priority};
use crate::App;

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
pub fn handle_api_response(app: &mut App, response: ApiResponse) {
    match response {
        ApiResponse::BrowseResult {
            folder_id,
            prefix,
            items,
        } => {
            // Mark browse as no longer loading
            let browse_key = format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
            app.model.performance.loading_browse.remove(&browse_key);

            let Ok(mut items) = items else {
                // API call failed - only update state if we're not already in Connecting mode
                if !matches!(app.model.syncthing.connection_state, ConnectionState::Connecting { .. }) {
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
                app.model.navigation.breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id && level.prefix == prefix)
            } else {
                // Check if this folder_id matches any level in our current breadcrumb trail
                // This allows prefetching subdirectories that aren't yet open
                app.model.navigation.breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id)
            };

            if !is_relevant {
                crate::log_debug(&format!("DEBUG [BrowseResult]: Skipping irrelevant response for folder={} prefix={:?} (navigated away)", folder_id, prefix));
                return; // Skip saving and UI updates for responses from folders we've left
            }

            // Get folder sequence for cache
            let folder_sequence = app
                .model.syncthing.folder_statuses
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
                folder_id, prefix, items.len(), app.model.navigation.focus_level, app.model.navigation.breadcrumb_trail.len()
            ));

            // Save merged items to cache
            if let Err(e) = app.cache.save_browse_items(
                &folder_id,
                prefix.as_deref(),
                &items,
                folder_sequence,
            ) {
                crate::log_debug(&format!(
                    "ERROR [BrowseResult]: Failed to save browse items to cache: {}",
                    e
                ));
            }

            // Update UI if this browse result matches current navigation
            // Find matching breadcrumb level (works for both root and non-root)
            let matching_level_idx = if prefix.is_none() {
                // Root level: match folder_id and no prefix
                app.model.navigation.breadcrumb_trail
                    .iter()
                    .position(|level| level.folder_id == folder_id && level.prefix.is_none())
            } else {
                // Non-root level: match folder_id and prefix
                app.model.navigation.breadcrumb_trail
                    .iter()
                    .position(|level| level.folder_id == folder_id && level.prefix == prefix)
            };

            if let Some(idx) = matching_level_idx {
                crate::log_debug(&format!(
                    "DEBUG [BrowseResult]: Updating level {} for folder={} prefix={:?}",
                    idx, folder_id, prefix
                ));

                // Load cached states (for instant display, will be replaced by FileInfo)
                let cached_states =
                    app.load_sync_states_from_cache(&folder_id, &items, prefix.as_deref());

                if let Some(level) = app.model.navigation.breadcrumb_trail.get_mut(idx) {
                    // Save currently selected item name BEFORE replacing items
                    let selected_name = level
                        .selected_index

                        .and_then(|sel_idx| level.display_items().get(sel_idx))
                        .map(|item| item.name.clone());

                    // Start with cached states (no preservation logic)
                    let mut sync_states = cached_states;

                    // Merge in local-only items
                    for local_item_name in &local_item_names {
                        sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                    }

                    crate::log_debug(&format!(
                        "DEBUG [BrowseResult]: Updating level {} with {} items (was {})",
                        idx, items.len(), level.items.len()
                    ));

                    // Update level
                    level.items = items.clone();  // Update unfiltered source
                    level.file_sync_states = sync_states;

                    // DON'T touch filtered_items here - let the filter functions manage it

                    // Update directory states based on their children
                    app.update_directory_states(idx);

                    // Sort and restore selection using the saved name
                    app.sort_level_with_selection(idx, selected_name);

                    // Only re-apply filters if this is the CURRENT breadcrumb level
                    // This prevents cascading filter re-applications during prefetch
                    let is_current_level = idx == app.model.navigation.focus_level.saturating_sub(1);

                    if is_current_level {
                        // Re-apply search filter if active
                        if !app.model.ui.search_query.is_empty() {
                            app.apply_search_filter();
                        }

                        // Re-apply out-of-sync filter if active
                        if app.model.ui.out_of_sync_filter.is_some() {
                            app.apply_out_of_sync_filter();
                        }
                    }

                    // Request FileInfo for ALL items (no filtering, let API service deduplicate)
                    for item in &items {
                        let file_path = if let Some(ref pfx) = prefix {
                            format!("{}{}", pfx, item.name)
                        } else {
                            item.name.clone()
                        };

                        let sync_key = format!("{}:{}", folder_id, file_path);
                        if !app.model.performance.loading_sync_states.contains(&sync_key) {
                            app.model.performance.loading_sync_states.insert(sync_key);
                            let _ = app.api_tx.send(ApiRequest::GetFileInfo {
                                folder_id: folder_id.clone(),
                                file_path,
                                priority: Priority::Medium,
                            });
                        }
                    }
                }
            } else {
                // No matching breadcrumb level - this is a prefetch response
                // Browse items are already saved to cache, skip UI update
                crate::log_debug(&format!("DEBUG [BrowseResult]: No matching level for folder={} prefix={:?} (prefetch)",
                                   folder_id, prefix));

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
        } => {
            // Mark as no longer loading
            let sync_key = format!("{}:{}", folder_id, file_path);
            app.model.performance.loading_sync_states.remove(&sync_key);

            let Ok(file_details) = details else {
                // API call failed - only update state if we're not already in Connecting mode
                if !matches!(app.model.syncthing.connection_state, ConnectionState::Connecting { .. }) {
                    let error = details.unwrap_err();
                    app.model.syncthing.connection_state = ConnectionState::Disconnected {
                        error_type: crate::logic::errors::classify_error(&error),
                        message: crate::logic::errors::format_error_message(&error),
                    };
                    crate::log_debug(&format!(
                        "DEBUG [FileInfoResult ERROR]: folder={} path={} error={:?}",
                        folder_id, file_path, error
                    ));
                }
                return;
            };

            // Successful API call - mark as connected
            app.model.syncthing.connection_state = ConnectionState::Connected;

            // Check if this response is still relevant to current navigation
            let is_relevant = if app.model.navigation.focus_level == 0 {
                false // At folder list, no file info is relevant
            } else if app.model.navigation.breadcrumb_trail.is_empty() {
                false // No breadcrumb trail, nothing is relevant
            } else {
                // Check if this folder_id matches any level in our current breadcrumb trail
                app.model.navigation.breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id)
            };

            if !is_relevant {
                crate::log_debug(&format!("DEBUG [FileInfoResult]: Skipping irrelevant response for folder={} path={}", folder_id, file_path));
                return; // Skip saving and UI updates for irrelevant responses
            }

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

            // Queue for batched write instead of immediate write
            app.pending_sync_state_writes.push((
                folder_id.clone(),
                file_path.clone(),
                state,
                file_sequence,
            ));

            // Update UI if this file is visible in current level
            let mut updated = false;
            for (_level_idx, level) in app.model.navigation.breadcrumb_trail.iter_mut().enumerate() {
                if level.folder_id == folder_id {
                    // Check if this file path belongs to this level
                    let level_prefix = level.prefix.as_deref().unwrap_or("");
                    crate::log_debug(&format!(
                        "DEBUG [FileInfoResult UI update]: checking level with prefix={:?}",
                        level_prefix
                    ));
                    if file_path.starts_with(level_prefix) {
                        let item_name =
                            file_path.strip_prefix(level_prefix).unwrap_or(&file_path);

                        // Get current state for tracking changes
                        let current_state = level.file_sync_states.get(item_name).copied();

                        // Update state if it changed
                        if current_state != Some(state) {
                            crate::log_debug(&format!(
                                "DEBUG [FileInfo]: Updating {} {:?} -> {:?}",
                                item_name, current_state, state
                            ));
                            level.file_sync_states.insert(item_name.to_string(), state);

                            // Update ignored_exists - do it inline to avoid borrow issues
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

                            updated = true;
                        }
                    } else {
                        crate::log_debug(&format!("DEBUG [FileInfoResult UI update]: NO MATCH - file_path={} doesn't start with level_prefix={}", file_path, level_prefix));
                    }
                }
            }
            if !updated {
                crate::log_debug(&format!("DEBUG [FileInfoResult UI update]: WARNING - No matching level found for folder={} path={}", folder_id, file_path));
            }
        }

        ApiResponse::FolderStatusResult { folder_id, status } => {
            let Ok(status) = status else {
                // API call failed - only update state if we're not already in Connecting mode
                // (Background reconnection manages the Connecting state)
                if !matches!(app.model.syncthing.connection_state, ConnectionState::Connecting { .. }) {
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
                crate::log_debug("DEBUG [FolderStatusResult]: Just reconnected, requesting immediate system status");
                let _ = app.api_tx.send(ApiRequest::GetSystemStatus);
                crate::log_debug(&format!("DEBUG [FolderStatusResult]: GetSystemStatus sent in {:?}", start.elapsed()));
            }

            let sequence = status.sequence;
            let receive_only_count = status.receive_only_total_items;

            // Check if sequence changed
            if let Some(&last_seq) = app.model.performance.last_known_sequences.get(&folder_id) {
                if last_seq != sequence {
                    crate::log_debug(&format!(
                        "DEBUG [FolderStatusResult]: Sequence changed from {} to {} for folder={}",
                        last_seq, sequence, folder_id
                    ));

                    // Sequence changed - invalidate cache and refresh
                    app.invalidate_and_refresh_folder(&folder_id);
                }
            }

            // Check if receive-only item count changed (indicates local-only files added/removed)
            if let Some(&last_count) = app.model.performance.last_known_receive_only_counts.get(&folder_id) {
                if last_count != receive_only_count {
                    crate::log_debug(&format!("DEBUG [FolderStatusResult]: receiveOnlyTotalItems changed from {} to {} for folder={}", last_count, receive_only_count, folder_id));

                    // Trigger refresh for currently viewed directory
                    if !app.model.navigation.breadcrumb_trail.is_empty()
                        && app.model.navigation.breadcrumb_trail[0].folder_id == folder_id
                    {
                        for level in &mut app.model.navigation.breadcrumb_trail {
                            if level.folder_id == folder_id {
                                let browse_key = format!(
                                    "{}:{}",
                                    folder_id,
                                    level.prefix.as_deref().unwrap_or("")
                                );
                                if !app.model.performance.loading_browse.contains(&browse_key) {
                                    app.model.performance.loading_browse.insert(browse_key);

                                    let _ = app.api_tx.send(ApiRequest::BrowseFolder {
                                        folder_id: folder_id.clone(),
                                        prefix: level.prefix.clone(),
                                        priority: Priority::High,
                                    });

                                    crate::log_debug(&format!("DEBUG [FolderStatusResult]: Triggered browse refresh for prefix={:?}", level.prefix));
                                }
                            }
                        }
                    }
                }
            }

            // Update last known values
            app.model.performance.last_known_sequences
                .insert(folder_id.clone(), sequence);
            app.model.performance.last_known_receive_only_counts
                .insert(folder_id.clone(), receive_only_count);

            // Save and use fresh status
            let _ = app.cache.save_folder_status(&folder_id, &status, sequence);
            app.model.syncthing.folder_statuses.insert(folder_id, status);
        }

        ApiResponse::RescanResult {
            folder_id,
            success,
            error,
        } => {
            if success {
                crate::log_debug(&format!("DEBUG [RescanResult]: Successfully rescanned folder={}, requesting immediate status update", folder_id));

                // Immediately request folder status to detect sequence changes
                // This makes the rescan feel more responsive
                let _ = app
                    .api_tx
                    .send(ApiRequest::GetFolderStatus { folder_id });
            } else {
                crate::log_debug(&format!(
                    "DEBUG [RescanResult ERROR]: Failed to rescan folder={} error={:?}",
                    folder_id, error
                ));
            }
        }

        ApiResponse::SystemStatusResult { status } => match status {
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

                // Always fetch devices list on successful system status (first poll after reconnection)
                // This ensures we have fresh device list and update device name (in case it changed in GUI)
                // Only fetch if we don't already have devices, to avoid unnecessary API calls
                if app.model.syncthing.devices.is_empty() || app.model.syncthing.device_name.is_none() {
                    crate::log_debug("DEBUG [SystemStatusResult]: Requesting devices list to update device name");
                    let _ = app.api_tx.send(ApiRequest::GetDevices);
                }

                // If we're connected but have no folders, fetch them
                // Note: We don't check was_disconnected because FolderStatus might have set
                // connection state to Connected before SystemStatus arrives
                // We also don't check show_setup_help because user might have dismissed it
                if app.model.syncthing.folders.is_empty() {
                    crate::log_debug(&format!(
                        "DEBUG [SystemStatusResult]: Connected with no folders, setting fetch flag (was_disconnected={} show_setup_help={})",
                        was_disconnected,
                        app.model.ui.show_setup_help
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
                if !matches!(app.model.syncthing.connection_state, ConnectionState::Connecting { .. }) {
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
        },

        ApiResponse::ConnectionStatsResult { stats } => match stats {
            Ok(conn_stats) => {
                // Update current stats with new data
                app.model.syncthing.connection_stats = Some(conn_stats.clone());

                // Calculate transfer rates if we have previous stats
                if let Some((prev_stats, prev_instant)) = &app.model.syncthing.last_connection_stats {
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
                        app.model.syncthing.last_connection_stats = Some((conn_stats, Instant::now()));
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
                crate::log_debug(&format!("DEBUG [ConnectionStatsResult ERROR]: {} (non-critical, connection state unchanged)", e));
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
                        crate::log_debug("DEBUG [DevicesResult]: No system status available yet, cannot extract device name");
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

        ApiResponse::NeededFiles { folder_id, response } => {
            // Cache the response
            if let Err(e) = app.cache.cache_needed_files(&folder_id, &response) {
                crate::log_debug(&format!("Failed to cache needed files for {}: {}", folder_id, e));
            }

            // Get breakdown from cache
            match app.cache.get_folder_sync_breakdown(&folder_id) {
                Ok(breakdown) => {
                    // Update summary state if open
                    if let Some(summary) = &mut app.model.ui.out_of_sync_summary {
                        summary.breakdowns.insert(folder_id.clone(), breakdown);
                        summary.loading.remove(&folder_id);
                    }
                }
                Err(e) => {
                    crate::log_debug(&format!("Failed to get breakdown for {}: {}", folder_id, e));
                }
            }

            // If in breadcrumb view for this folder, apply/reapply filter now that data is ready
            if app.model.navigation.focus_level > 0 {
                let level_idx = app.model.navigation.focus_level - 1;
                if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
                    if level.folder_id == folder_id {
                        if app.model.ui.out_of_sync_filter.is_none() {
                            // First time: activate filter
                            app.model.ui.out_of_sync_filter = Some(model::types::OutOfSyncFilterState {
                                origin_level: app.model.navigation.focus_level,
                                last_refresh: std::time::SystemTime::now(),
                            });

                            // Apply filter
                            app.apply_out_of_sync_filter();

                            // Clear loading toast
                            app.model.ui.toast_message = None;
                        } else {
                            // Filter already active: re-apply with fresh cache data
                            // This handles the case where cache was invalidated and just refreshed
                            app.apply_out_of_sync_filter();
                        }
                    }
                }
            }
        },

        ApiResponse::LocalChanged { folder_id, file_paths } => {
            // Cache the response
            if let Err(e) = app.cache.cache_local_changed_files(&folder_id, &file_paths) {
                crate::log_debug(&format!("Failed to cache local changed files for {}: {}", folder_id, e));
            }

            // If filter is active for this folder, re-apply it with updated data
            if let Some(_filter_state) = &app.model.ui.out_of_sync_filter {
                let current_folder_id = app.model.navigation.breadcrumb_trail
                    .get(0)
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
