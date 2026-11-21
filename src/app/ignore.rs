//! Ignore pattern management
//!
//! Methods for managing .stignore patterns:
//! - Toggle ignore state (add/remove patterns)
//! - Ignore and delete (immediate action)

use crate::logic::ignore::{ScanScope, scan_scope_for_pattern};
use crate::logic::pending::Observation;
use crate::model::pending::PendingKind;
use crate::{App, SyncState, log_debug, logic, model};
use anyhow::Result;
use std::path::PathBuf;

/// What `toggle_ignore` did, so the key handler can give feedback.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IgnoreToggle {
    Unchanged,
    Ignored,
    /// Holds the item name.
    Unignored(String),
    /// Several patterns match; the pattern selection menu is open.
    ChoosePattern,
    /// Un-ignore refused: this pending deletion covers the item.
    BlockedByPendingDelete(PathBuf),
}

/// Undeletes for one folder: whether a batch is running, and the paths for the next batch
#[derive(Debug, Default)]
pub(crate) struct UndeleteJob {
    pub running: bool,
    pub pending: Vec<String>,
}

/// Result of one undelete batch, sent back to the main loop
#[derive(Debug)]
pub(crate) struct UndeleteDone {
    pub folder_id: String,
    pub paths: Vec<String>,
    pub patterns: Vec<String>,
    pub result: Result<(), String>,
    /// Whether the ignore list holds none of `patterns` afterwards
    pub ignores_restored: bool,
}

/// One undelete batch. Syncthing has no per-file revert, so this ignores the paths and
/// rescans them (Syncthing forgets the local deletions), then puts the ignore list back and
/// rescans again (Syncthing downloads the files). The ignore list is put back even when a
/// step fails. Rescans wait while the folder syncs, which is why this runs in the background.
async fn run_undelete_batch(
    client: crate::SyncthingClient,
    folder_id: String,
    paths: Vec<String>,
    patterns: Vec<String>,
) -> UndeleteDone {
    let original: Result<Vec<String>, String> = client
        .get_ignore_patterns(&folder_id)
        .await
        .map(|current| {
            current
                .into_iter()
                .filter(|p| !patterns.contains(p))
                .collect()
        })
        .map_err(|e| e.to_string());
    let (result, ignores_restored) = match original {
        Err(e) => (Err(e), true),
        Ok(original) => {
            let mut with_paths = patterns.clone();
            with_paths.extend(original.iter().cloned());
            let forget = async {
                client.set_ignore_patterns(&folder_id, with_paths).await?;
                client.rescan_paths(&folder_id, &paths).await
            }
            .await;
            let restore = client.set_ignore_patterns(&folder_id, original).await;
            let ignores_restored = restore.is_ok();
            let result = match (forget, restore) {
                (Ok(()), Ok(())) => client.rescan_paths(&folder_id, &paths).await,
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            (result.map_err(|e| e.to_string()), ignores_restored)
        }
    };
    UndeleteDone {
        folder_id,
        paths,
        patterns,
        result,
        ignores_restored,
    }
}

impl App {
    /// Undelete `path` in `folder_id`: its local deletion is dropped from the index at once,
    /// and it joins the folder's next batch if one is already running.
    pub(crate) fn queue_undelete(&mut self, folder_id: &str, path: String) {
        self.record_pending(folder_id, &path, PendingKind::Restoring);
        // Shown syncing until Syncthing reports the download finished
        self.model
            .syncthing
            .syncing_files
            .entry(folder_id.to_string())
            .or_default()
            .insert(path.clone());
        let job = self.undelete_jobs.entry(folder_id.to_string()).or_default();
        if job.running {
            if !job.pending.contains(&path) {
                job.pending.push(path);
            }
        } else {
            self.start_undelete_batch(folder_id.to_string(), vec![path]);
        }
    }

    fn start_undelete_batch(&mut self, folder_id: String, paths: Vec<String>) {
        let patterns: Vec<String> = paths.iter().map(|p| format!("/{}", p)).collect();
        let _ = self.cache.add_pending_undelete(&folder_id, &patterns);
        self.undelete_jobs
            .entry(folder_id.clone())
            .or_default()
            .running = true;
        let client = self.client.clone();
        let tx = self.undelete_tx.clone();
        tokio::spawn(async move {
            let done = run_undelete_batch(client, folder_id, paths, patterns).await;
            let _ = tx.send(done);
        });
    }

    /// Show a failed batch's files as deleted locally again instead of syncing
    pub(crate) fn mark_undelete_failed(&mut self, done: &UndeleteDone) {
        for path in &done.paths {
            if let Some(set) = self.model.syncthing.syncing_files.get_mut(&done.folder_id) {
                set.remove(path);
            }
            self.observe_path(&done.folder_id, path, Observation::ActionFailed);
            self.record_pending(&done.folder_id, path, PendingKind::DeletedLocally);
        }
    }

    /// Clear the record of a finished batch and start the folder's next batch, if any
    pub(crate) fn finish_undelete_batch(&mut self, done: &UndeleteDone) {
        if done.ignores_restored {
            let _ = self
                .cache
                .remove_pending_undelete(&done.folder_id, &done.patterns);
        }
        let next = match self.undelete_jobs.get_mut(&done.folder_id) {
            Some(job) if !job.pending.is_empty() => std::mem::take(&mut job.pending),
            _ => {
                self.undelete_jobs.remove(&done.folder_id);
                return;
            }
        };
        self.start_undelete_batch(done.folder_id.clone(), next);
    }

    /// Remove undelete patterns a previous run recorded but didn't take back out of the
    /// ignore list (stui exited or Syncthing was unreachable). Only those exact patterns go.
    pub(crate) async fn remove_leftover_undelete_patterns(&mut self) {
        let Ok(pending) = self.cache.pending_undelete_patterns() else {
            return;
        };
        for (folder_id, patterns) in pending {
            let Ok(current) = self.client.get_ignore_patterns(&folder_id).await else {
                continue;
            };
            let kept: Vec<String> = current
                .iter()
                .filter(|p| !patterns.contains(p))
                .cloned()
                .collect();
            if kept.len() != current.len()
                && self
                    .client
                    .set_ignore_patterns(&folder_id, kept)
                    .await
                    .is_err()
            {
                continue;
            }
            let _ = self.cache.remove_pending_undelete(&folder_id, &patterns);
        }
    }

    pub(crate) async fn toggle_ignore(&mut self) -> Result<IgnoreToggle> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return Ok(IgnoreToggle::Unchanged);
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(IgnoreToggle::Unchanged);
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();
        let prefix = level.prefix.clone();

        // Get selected item (respects filtered view if active)
        let selected_idx = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(IgnoreToggle::Unchanged),
        };

        let item = match level.display_items().get(selected_idx) {
            Some(item) => item,
            None => return Ok(IgnoreToggle::Unchanged),
        };
        let item_name = item.name.clone(); // Clone for later use
        let sync_state = level
            .file_sync_states
            .get(&item.name)
            .copied()
            .unwrap_or(SyncState::Unknown);

        // Build the relative path from folder root
        let relative_path = if let Some(ref prefix) = prefix {
            format!("{}{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        // Get current ignore patterns
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;

        if sync_state == SyncState::Ignored {
            if self
                .model
                .pending
                .blocks_unignore(&folder_id, &relative_path)
            {
                log_debug(&format!(
                    "Blocked un-ignore: {} is pending deletion",
                    relative_path
                ));
                return Ok(IgnoreToggle::BlockedByPendingDelete(PathBuf::from(
                    &relative_path,
                )));
            }

            // File is ignored - find matching patterns and remove them
            let file_path = format!("/{}", relative_path);
            let matching_patterns = logic::ignore::find_matching_patterns(&patterns, &file_path);

            if matching_patterns.is_empty() {
                return Ok(IgnoreToggle::Unchanged); // No patterns match (shouldn't happen)
            }

            if matching_patterns.len() == 1 {
                // Only one pattern - remove it directly
                let pattern_to_remove = &matching_patterns[0];
                let scope = scan_scope_for_pattern(pattern_to_remove, &relative_path);
                let updated_patterns: Vec<String> = patterns
                    .into_iter()
                    .filter(|p| p != pattern_to_remove)
                    .collect();

                self.client
                    .set_ignore_patterns(&folder_id, updated_patterns)
                    .await?;

                let file_path_for_api = if let Some(ref prefix) = prefix {
                    format!("{}/{}", prefix.trim_matches('/'), item_name)
                } else {
                    item_name.clone()
                };
                self.record_pending(&folder_id, &file_path_for_api, PendingKind::Unignored);
                self.finish_unignore(&folder_id, file_path_for_api, scope);
                return Ok(IgnoreToggle::Unignored(item_name));
            } else {
                // Multiple patterns match - show selection menu
                self.model.ui.pattern_selection = Some(model::PatternSelectionState {
                    folder_id,
                    item_name,
                    patterns: matching_patterns,
                    selected_index: Some(0),
                });
                return Ok(IgnoreToggle::ChoosePattern);
            }
        } else {
            // File is not ignored - add it to ignore
            let new_pattern = format!("/{}", relative_path);

            // Check if pattern already exists
            if patterns.contains(&new_pattern) {
                return Ok(IgnoreToggle::Unchanged);
            }

            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern.clone());

            self.client
                .set_ignore_patterns(&folder_id, updated_patterns)
                .await?;

            // Immediately mark as ignored in UI
            let mut exists = false;
            if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                level
                    .file_sync_states
                    .insert(item_name.clone(), SyncState::Ignored);

                // Update ignored_exists (file is now ignored) - do it inline to avoid borrow issues
                // translated_base_path already includes the full path to this directory level
                let host_path = format!(
                    "{}/{}",
                    level.translated_base_path.trim_end_matches('/'),
                    item_name
                );
                exists = std::path::Path::new(&host_path).exists();
                level.ignored_exists.insert(item_name.clone(), exists);
            }
            self.record_pending(
                &folder_id,
                &relative_path,
                PendingKind::Ignored {
                    exists,
                    deleting: false,
                },
            );

            self.rescan_paths(
                &folder_id,
                scan_scope_for_pattern(&new_pattern, &relative_path)
                    .subs()
                    .to_vec(),
            );
        }

        Ok(IgnoreToggle::Ignored)
    }

    /// Rescan after an unignore. The item shows as loading until a FileInfo reply says it is
    /// no longer ignored; the RescanResult handler requests that FileInfo once the scan returns.
    pub(crate) fn finish_unignore(&self, folder_id: &str, file_path: String, scope: ScanScope) {
        log_debug(&format!(
            "DEBUG [finish_unignore]: folder={} path={}",
            folder_id, file_path
        ));
        self.rescan_paths(folder_id, scope.subs().to_vec());
    }

    pub(crate) async fn ignore_and_delete(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0
            || self.model.navigation.breadcrumb_trail.is_empty()
        {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];
        let folder_id = level.folder_id.clone();

        // Get selected item (respects filtered view if active)
        let selected_idx = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(()),
        };

        let item = match level.display_items().get(selected_idx) {
            Some(item) => item,
            None => return Ok(()),
        };
        let item_name = item.name.clone(); // Clone for later use

        // Build paths
        let relative_path = if let Some(ref prefix) = level.prefix {
            format!("{}{}", prefix, item.name)
        } else {
            item.name.clone()
        };

        // Note: translated_base_path already includes full directory path (with prefix),
        // so we only append the item name (not relative_path which duplicates the prefix)
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

        // Check if file exists on disk
        if !std::path::Path::new(&host_path).exists() {
            // File doesn't exist, just add to ignore
            let patterns = self.client.get_ignore_patterns(&folder_id).await?;
            let new_pattern = format!("/{}", relative_path);

            if !patterns.contains(&new_pattern) {
                let mut updated_patterns = patterns;
                updated_patterns.insert(0, new_pattern);

                self.client
                    .set_ignore_patterns(&folder_id, updated_patterns)
                    .await?;

                if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                    level
                        .file_sync_states
                        .insert(item_name.clone(), SyncState::Ignored);

                    // Update ignored_exists (file is now ignored)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Ignored);
                }
                self.record_pending(
                    &folder_id,
                    &relative_path,
                    PendingKind::Ignored {
                        exists: false,
                        deleting: true,
                    },
                );

                self.rescan_paths(
                    &folder_id,
                    scan_scope_for_pattern(&format!("/{}", relative_path), &relative_path)
                        .subs()
                        .to_vec(),
                );
            }
            return Ok(());
        }

        // File exists - add to ignore first, then delete
        let patterns = self.client.get_ignore_patterns(&folder_id).await?;
        let new_pattern = format!("/{}", relative_path);

        if !patterns.contains(&new_pattern) {
            let mut updated_patterns = patterns;
            updated_patterns.insert(0, new_pattern);
            self.client
                .set_ignore_patterns(&folder_id, updated_patterns)
                .await?;
        }

        // Recorded before the delete so un-ignore is blocked until Syncthing confirms
        self.record_pending(
            &folder_id,
            &relative_path,
            PendingKind::Ignored {
                exists: false,
                deleting: true,
            },
        );

        // Now delete the file
        let is_dir = std::path::Path::new(&host_path).is_dir();
        let delete_result = if is_dir {
            std::fs::remove_dir_all(&host_path)
        } else {
            std::fs::remove_file(&host_path)
        };

        match delete_result {
            Ok(()) => {
                // Verify file is actually gone
                if std::path::Path::new(&host_path).exists() {
                    log_debug(&format!(
                        "Warning: File still exists after deletion: {}",
                        host_path
                    ));
                } else {
                    log_debug(&format!("Successfully deleted file: {}", host_path));
                }

                if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                    level
                        .file_sync_states
                        .insert(item_name.clone(), SyncState::Ignored);

                    // Update ignored_exists (file is now ignored and deleted)
                    self.update_ignored_exists_for_file(level_idx, &item_name, SyncState::Ignored);
                }

                self.rescan_paths(
                    &folder_id,
                    scan_scope_for_pattern(&format!("/{}", relative_path), &relative_path)
                        .subs()
                        .to_vec(),
                );
            }
            Err(e) => {
                log_debug(&format!("Failed to delete file: {} - {}", host_path, e));
                self.model.pending.remove(&folder_id, &relative_path);
                return Err(e.into());
            }
        }

        Ok(())
    }
}
