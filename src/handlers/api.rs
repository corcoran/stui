//! API Response Handler
//!
//! Handles responses from the Syncthing REST API background service.
//! Processes browse results, file info, folder status, and connection stats.

use std::time::Instant;

use crate::api::SyncState;
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
            app.model.loading_browse.remove(&browse_key);

            let Ok(mut items) = items else {
                return; // Silently ignore errors
            };

            // Check if this response is still relevant to current navigation
            // We allow caching for subdirectories of the current folder (prefetch),
            // but skip if we've navigated completely away from this folder
            let is_relevant = if app.model.focus_level == 0 {
                false // At folder list, no browse results are relevant
            } else if app.model.breadcrumb_trail.is_empty() {
                false // No breadcrumb trail, nothing is relevant
            } else {
                // Check if this folder_id matches any level in our current breadcrumb trail
                // This allows prefetching subdirectories that aren't yet open
                app.model.breadcrumb_trail
                    .iter()
                    .any(|level| level.folder_id == folder_id)
            };

            if !is_relevant {
                crate::log_debug(&format!("DEBUG [BrowseResult]: Skipping irrelevant response for folder={} prefix={:?} (navigated away)", folder_id, prefix));
                return; // Skip saving and UI updates for responses from folders we've left
            }

            // Get folder sequence for cache
            let folder_sequence = app
                .model.folder_statuses
                .get(&folder_id)
                .map(|s| s.sequence)
                .unwrap_or(0);

            // Check if this folder has local changes and merge them synchronously
            let has_local_changes = app
                .model.folder_statuses
                .get(&folder_id)
                .map(|s| s.receive_only_total_items > 0)
                .unwrap_or(false);

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

            crate::log_debug(&format!("DEBUG [BrowseResult]: folder={} prefix={:?} focus_level={} breadcrumb_count={}",
                               folder_id, prefix, app.model.focus_level, app.model.breadcrumb_trail.len()));

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
                app.model.breadcrumb_trail
                    .iter()
                    .position(|level| level.folder_id == folder_id && level.prefix.is_none())
            } else {
                // Non-root level: match folder_id and prefix
                app.model.breadcrumb_trail
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

                if let Some(level) = app.model.breadcrumb_trail.get_mut(idx) {
                    // Save currently selected item name BEFORE replacing items
                    let selected_name = level
                        .selected_index
                        
                        .and_then(|sel_idx| level.items.get(sel_idx))
                        .map(|item| item.name.clone());

                    // Start with cached states (no preservation logic)
                    let mut sync_states = cached_states;

                    // Merge in local-only items
                    for local_item_name in &local_item_names {
                        sync_states.insert(local_item_name.clone(), SyncState::LocalOnly);
                    }

                    // Update level
                    level.items = items.clone();
                    level.file_sync_states = sync_states;

                    // Update directory states based on their children
                    app.update_directory_states(idx);

                    // Sort and restore selection using the saved name
                    app.sort_level_with_selection(idx, selected_name);

                    // Request FileInfo for ALL items (no filtering, let API service deduplicate)
                    for item in &items {
                        let file_path = if let Some(ref pfx) = prefix {
                            format!("{}{}", pfx, item.name)
                        } else {
                            item.name.clone()
                        };

                        let sync_key = format!("{}:{}", folder_id, file_path);
                        if !app.model.loading_sync_states.contains(&sync_key) {
                            app.model.loading_sync_states.insert(sync_key);
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
            }
        }

        ApiResponse::FileInfoResult {
            folder_id,
            file_path,
            details,
        } => {
            // Mark as no longer loading
            let sync_key = format!("{}:{}", folder_id, file_path);
            app.model.loading_sync_states.remove(&sync_key);

            let Ok(file_details) = details else {
                crate::log_debug(&format!(
                    "DEBUG [FileInfoResult ERROR]: folder={} path={} error={:?}",
                    folder_id, file_path, details.err()
                ));
                return; // Silently ignore errors
            };

            // Check if this response is still relevant to current navigation
            let is_relevant = if app.model.focus_level == 0 {
                false // At folder list, no file info is relevant
            } else if app.model.breadcrumb_trail.is_empty() {
                false // No breadcrumb trail, nothing is relevant
            } else {
                // Check if this folder_id matches any level in our current breadcrumb trail
                app.model.breadcrumb_trail
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
            for (_level_idx, level) in app.model.breadcrumb_trail.iter_mut().enumerate() {
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
                return; // Silently ignore errors
            };

            let sequence = status.sequence;
            let receive_only_count = status.receive_only_total_items;

            // Check if sequence changed
            if let Some(&last_seq) = app.model.last_known_sequences.get(&folder_id) {
                if last_seq != sequence {
                    // Log HashMap size BEFORE any operations
                    if !app.model.breadcrumb_trail.is_empty()
                        && app.model.breadcrumb_trail[0].folder_id == folder_id
                    {
                        crate::log_bug(&format!(
                            "seq_change: BEFORE operations - HashMap has {} states",
                            app.model.breadcrumb_trail[0].file_sync_states.len()
                        ));
                    }

                    crate::log_bug(&format!(
                        "seq_change: {} {}->{}",
                        folder_id, last_seq, sequence
                    ));
                    let _ = app.cache.invalidate_folder(&folder_id);

                    // Log HashMap size AFTER cache invalidation
                    if !app.model.breadcrumb_trail.is_empty()
                        && app.model.breadcrumb_trail[0].folder_id == folder_id
                    {
                        crate::log_bug(&format!(
                            "seq_change: after cache.invalidate - HashMap has {} states",
                            app.model.breadcrumb_trail[0].file_sync_states.len()
                        ));
                    }

                    // Clear discovered directories for this folder (so they get re-discovered with new sequence)
                    app.model.discovered_dirs
                        .retain(|key| !key.starts_with(&format!("{}:", folder_id)));

                    // Log HashMap size after discovered_dirs.retain
                    if !app.model.breadcrumb_trail.is_empty()
                        && app.model.breadcrumb_trail[0].folder_id == folder_id
                    {
                        crate::log_bug(&format!(
                            "seq_change: after discovered_dirs.retain - HashMap has {} states",
                            app.model.breadcrumb_trail[0].file_sync_states.len()
                        ));
                    }
                }
            }

            // Check if receive-only item count changed (indicates local-only files added/removed)
            if let Some(&last_count) = app.model.last_known_receive_only_counts.get(&folder_id) {
                if last_count != receive_only_count {
                    crate::log_debug(&format!("DEBUG [FolderStatusResult]: receiveOnlyTotalItems changed from {} to {} for folder={}", last_count, receive_only_count, folder_id));

                    // Trigger refresh for currently viewed directory
                    if !app.model.breadcrumb_trail.is_empty()
                        && app.model.breadcrumb_trail[0].folder_id == folder_id
                    {
                        for level in &mut app.model.breadcrumb_trail {
                            if level.folder_id == folder_id {
                                let browse_key = format!(
                                    "{}:{}",
                                    folder_id,
                                    level.prefix.as_deref().unwrap_or("")
                                );
                                if !app.model.loading_browse.contains(&browse_key) {
                                    app.model.loading_browse.insert(browse_key);

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
            app.model.last_known_sequences
                .insert(folder_id.clone(), sequence);
            app.model.last_known_receive_only_counts
                .insert(folder_id.clone(), receive_only_count);

            // Save and use fresh status
            let _ = app.cache.save_folder_status(&folder_id, &status, sequence);
            app.model.folder_statuses.insert(folder_id, status);
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
                app.model.system_status = Some(sys_status);
            }
            Err(e) => {
                crate::log_debug(&format!("DEBUG [SystemStatusResult ERROR]: {}", e));
            }
        },

        ApiResponse::ConnectionStatsResult { stats } => match stats {
            Ok(conn_stats) => {
                // Update current stats with new data
                app.model.connection_stats = Some(conn_stats.clone());

                // Calculate transfer rates if we have previous stats
                if let Some((prev_stats, prev_instant)) = &app.model.last_connection_stats {
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
                        app.model.last_transfer_rates = Some((in_rate, out_rate));
                    }

                    // Update baseline every ~10 seconds to prevent drift
                    if elapsed > 10.0 {
                        app.model.last_connection_stats = Some((conn_stats, Instant::now()));
                    }
                } else {
                    // First fetch, store as baseline
                    app.model.last_connection_stats = Some((conn_stats, Instant::now()));
                    // Initialize rates to zero on first fetch
                    app.model.last_transfer_rates = Some((0.0, 0.0));
                }
            }
            Err(e) => {
                crate::log_debug(&format!("DEBUG [ConnectionStatsResult ERROR]: {}", e));
            }
        },
    }
}
