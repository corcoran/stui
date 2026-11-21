//! File operation methods
//!
//! User actions that interact with files and external commands:
//! - Delete files/directories
//! - Rescan folders
//! - Restore deleted files (receive-only folders)
//! - Open files/directories with external commands
//! - Copy paths to clipboard

use crate::{App, log_debug, logic, services};
use anyhow::Result;

impl App {
    /// Get folder ID and label for rescan operation
    /// Works in both folder list view and breadcrumb view
    pub(crate) fn get_rescan_folder_info(&self) -> Option<(String, String)> {
        if self.model.navigation.focus_level == 0 {
            // Folder list view - get selected folder
            let selected = self.model.navigation.folders_state_selection?;
            let folder = self.model.syncthing.folders.get(selected)?;
            Some((
                folder.id.clone(),
                folder.label.clone().unwrap_or_else(|| folder.id.clone()),
            ))
        } else {
            // Breadcrumb view - get current folder from trail
            if self.model.navigation.breadcrumb_trail.is_empty() {
                return None;
            }
            let level = &self.model.navigation.breadcrumb_trail[0];
            Some((level.folder_id.clone(), level.folder_label.clone()))
        }
    }

    pub(crate) fn rescan_selected_folder(&mut self) -> Result<()> {
        // Get the folder ID using helper method
        let (folder_id, _) = self
            .get_rescan_folder_info()
            .ok_or_else(|| anyhow::anyhow!("No folder selected"))?;

        log_debug(&format!(
            "DEBUG [rescan_selected_folder]: Requesting rescan for folder={}",
            folder_id
        ));

        // Trigger rescan via non-blocking API
        let _ = self.api_tx.send(services::api::ApiRequest::RescanFolder {
            folder_id,
            paths: Vec::new(),
        });

        Ok(())
    }

    /// Fire-and-forget scan of only `paths` (folder-relative) via the API service. An empty
    /// `paths` scans the whole folder. Callers that must wait for the scan to finish call
    /// `SyncthingClient::rescan_paths` directly.
    pub(crate) fn rescan_paths(&self, folder_id: &str, paths: Vec<String>) {
        log_debug(&format!(
            "DEBUG [rescan_paths]: folder={} paths={:?}",
            folder_id, paths
        ));
        let _ = self.api_tx.send(services::api::ApiRequest::RescanFolder {
            folder_id: folder_id.to_string(),
            paths,
        });
    }

    /// Force refresh a folder - invalidate cache immediately and trigger rescan
    /// Unlike normal rescan, this doesn't wait for sequence change to invalidate cache
    pub(crate) fn force_refresh_folder(&mut self, folder_id: &str) -> Result<()> {
        log_debug(&format!(
            "DEBUG [force_refresh_folder]: Force refreshing folder={}",
            folder_id
        ));

        // Step 1: Invalidate cache and refresh breadcrumbs immediately
        self.invalidate_and_refresh_folder(folder_id);

        // Step 2: Trigger Syncthing rescan
        let _ = self.api_tx.send(services::api::ApiRequest::RescanFolder {
            folder_id: folder_id.to_string(),
            paths: Vec::new(),
        });

        Ok(())
    }

    /// Invalidate cache and refresh breadcrumbs for a folder
    /// Used by both force_refresh_folder and FolderStatusResult handler
    pub(crate) fn invalidate_and_refresh_folder(&mut self, folder_id: &str) {
        log_debug(&format!(
            "DEBUG [invalidate_and_refresh_folder]: Invalidating cache for folder={}",
            folder_id
        ));

        // Invalidate cache
        let _ = self.cache.invalidate_folder(folder_id);
        // A need-list walk keeps its rows current through restarts and sweeps
        if !self.model.syncthing.need_paging.contains_key(folder_id) {
            let _ = self.cache.invalidate_out_of_sync_categories(folder_id);
        }

        // Clear discovered directories (force re-discovery)
        self.model
            .performance
            .discovered_dirs
            .retain(|key| !key.starts_with(&format!("{}:", folder_id)));

        // Refresh breadcrumbs if currently viewing this folder
        if !self.model.navigation.breadcrumb_trail.is_empty()
            && self.model.navigation.breadcrumb_trail[0].folder_id == folder_id
        {
            log_debug(&format!(
                "DEBUG [invalidate_and_refresh_folder]: Refreshing {} breadcrumb levels for folder={}",
                self.model.navigation.breadcrumb_trail.len(),
                folder_id
            ));

            for (idx, level) in self.model.navigation.breadcrumb_trail.iter().enumerate() {
                if level.folder_id == folder_id {
                    let browse_key =
                        format!("{}:{}", folder_id, level.prefix.as_deref().unwrap_or(""));

                    log_debug(&format!(
                        "DEBUG [invalidate_and_refresh_folder]: Level {}: prefix={:?} loading_browse.contains={}",
                        idx,
                        level.prefix,
                        self.model.performance.loading_browse.contains(&browse_key)
                    ));

                    if !self.model.performance.loading_browse.contains(&browse_key) {
                        self.model.performance.loading_browse.insert(browse_key);

                        let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                            folder_id: folder_id.to_string(),
                            prefix: level.prefix.clone(),
                            priority: services::api::Priority::High,
                            requested_at: std::time::Instant::now(),
                        });

                        log_debug(&format!(
                            "DEBUG [invalidate_and_refresh_folder]: Sent BrowseFolder request for prefix={:?}",
                            level.prefix
                        ));
                    }
                }
            }
        }
    }

    pub(crate) async fn restore_selected_file(&mut self) -> Result<()> {
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

        let folder_id = self.model.navigation.breadcrumb_trail[level_idx]
            .folder_id
            .clone();

        // Check if this is a receive-only folder with local changes
        if logic::folder::has_local_changes(self.model.syncthing.folder_statuses.get(&folder_id)) {
            // Receive-only folder with local changes - fetch the list of changed files
            let changed_files = self
                .client
                .get_local_changed_entries(&folder_id)
                .await
                .unwrap_or_else(|_| Vec::new());

            // Show confirmation prompt with file list
            self.model.ui.confirm_action = Some(crate::model::ConfirmAction::Revert {
                folder_id,
                changed_files,
            });
            return Ok(());
        }

        // Not receive-only or no local changes - just rescan
        self.client.rescan_folder(&folder_id).await?;

        // Refresh statuses in background (non-blocking)
        self.refresh_folder_statuses_nonblocking();

        Ok(())
    }

    pub(crate) async fn delete_file(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if !logic::folder::can_delete_file(
            self.model.navigation.focus_level,
            self.model.navigation.breadcrumb_trail.is_empty(),
        ) {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];

        // Get selected item (respects filtered view if active)
        let selected_idx = match level.selected_index {
            Some(idx) => idx,
            None => {
                return Ok(());
            }
        };

        let item = match level.display_items().get(selected_idx) {
            Some(item) => item,
            None => {
                return Ok(());
            }
        };

        // Build the full host path
        // Note: translated_base_path already includes full directory path (with prefix),
        // so we only append the item name (not relative_path which duplicates the prefix)
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

        // Check if file exists on disk
        if !std::path::Path::new(&host_path).exists() {
            return Ok(()); // Nothing to delete
        }

        // Check if it's a directory
        let is_dir = std::path::Path::new(&host_path).is_dir();

        // Show confirmation prompt
        self.model.ui.confirm_action = Some(crate::model::ConfirmAction::Delete {
            path: host_path,
            name: item.name.clone(),
            is_dir,
        });

        Ok(())
    }

    /// Host path of the item selected in the focused breadcrumb level
    fn selected_host_path(&self) -> Option<String> {
        let level_idx = self.model.navigation.focus_level.checked_sub(1)?;
        let level = self.model.navigation.breadcrumb_trail.get(level_idx)?;
        let item = level.display_items().get(level.selected_index?)?;
        Some(format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        ))
    }

    pub(crate) fn open_selected_item(&mut self) -> OpenOutcome {
        let Some(open_cmd) = self.open_command.clone() else {
            return OpenOutcome::NotConfigured;
        };
        let Some(host_path) = self.selected_host_path() else {
            return OpenOutcome::NothingToOpen;
        };
        if !std::path::Path::new(&host_path).exists() {
            log_debug(&format!(
                "open_selected_item: Path does not exist: {}",
                host_path
            ));
            return OpenOutcome::NothingToOpen;
        }
        match spawn_detached(&open_cmd, "open_command", &host_path) {
            Ok(()) => {
                log_debug(&format!("open_command: spawned {} {}", open_cmd, host_path));
                OpenOutcome::Opened(host_path)
            }
            Err(outcome) => outcome,
        }
    }

    pub(crate) fn open_syncthing_web_ui(&mut self) -> OpenOutcome {
        let Some(open_cmd) = self.open_command.clone() else {
            return OpenOutcome::NotConfigured;
        };
        let url = self.base_url.clone();
        match spawn_detached(&open_cmd, "open_command", &url) {
            Ok(()) => OpenOutcome::Opened(url),
            Err(outcome) => outcome,
        }
    }

    pub(crate) fn copy_to_clipboard(&mut self) -> CopyOutcome {
        let text = if self.model.navigation.focus_level == 0 {
            self.model
                .navigation
                .folders_state_selection
                .and_then(|selected| self.model.syncthing.folders.get(selected))
                .map(|folder| folder.id.clone())
        } else {
            self.selected_host_path()
        };
        let Some(text) = text else {
            return CopyOutcome::NothingToCopy;
        };
        let outcome = match &self.clipboard_command {
            Some(command) => run_clipboard_command(command, &text),
            None => CopyOutcome::NotConfigured,
        };
        // Always logged, not just in debug mode: clipboard failures are otherwise silent
        let line = match &outcome {
            CopyOutcome::Copied(text) => format!("Copied to clipboard: {}", text),
            CopyOutcome::Failed { command, error } => {
                format!("ERROR: clipboard command '{}' failed: {}", command, error)
            }
            CopyOutcome::Invalid(message) => format!("ERROR: {}", message),
            CopyOutcome::NotConfigured => "No clipboard_command configured".to_string(),
            CopyOutcome::NothingToCopy => return outcome,
        };
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::utils::get_debug_log_path())
            .and_then(|mut f| writeln!(f, "{}", line));
        outcome
    }
}

/// Result of copying with `c`; the keyboard handler turns it into a toast
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopyOutcome {
    Copied(String),
    Failed {
        command: String,
        error: String,
    },
    /// Full message: `Invalid <setting>: <error>`
    Invalid(String),
    NotConfigured,
    NothingToCopy,
}

/// Result of opening with `o`; the keyboard handler turns it into a toast
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenOutcome {
    Opened(String),
    Failed {
        command: String,
        error: String,
    },
    /// Full message: `Invalid <setting>: <error>`
    Invalid(String),
    NotConfigured,
    NothingToOpen,
}

/// Runs a configured clipboard command with `text` on stdin and waits for it to exit
pub(crate) fn run_clipboard_command(command: &str, text: &str) -> CopyOutcome {
    use std::io::Write;
    let (program, args) = match logic::command::parse_command(command) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return CopyOutcome::NotConfigured,
        Err(e) => return CopyOutcome::Invalid(format!("Invalid clipboard_command: {}", e)),
    };
    let failed = |error: String| CopyOutcome::Failed {
        command: command.to_string(),
        error,
    };
    let mut child = match std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return failed(e.to_string()),
    };
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(text.as_bytes())
    {
        return failed(e.to_string());
    }
    match child.wait() {
        Ok(status) if status.success() => CopyOutcome::Copied(text.to_string()),
        Ok(status) => failed(format!("exited with {}", status)),
        Err(e) => failed(e.to_string()),
    }
}

/// Starts a configured opener with `target` as its last argument, without waiting
fn spawn_detached(command: &str, setting: &str, target: &str) -> Result<(), OpenOutcome> {
    let (program, args) = match logic::command::parse_command(command) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return Err(OpenOutcome::NotConfigured),
        Err(e) => return Err(OpenOutcome::Invalid(format!("Invalid {}: {}", setting, e))),
    };
    std::process::Command::new(&program)
        .args(&args)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| OpenOutcome::Failed {
            command: command.to_string(),
            error: e.to_string(),
        })
}
