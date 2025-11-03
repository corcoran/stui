//! File operation methods
//!
//! User actions that interact with files and external commands:
//! - Delete files/directories
//! - Rescan folders
//! - Restore deleted files (receive-only folders)
//! - Open files/directories with external commands
//! - Copy paths to clipboard

use crate::{App, services, logic, log_debug};
use anyhow::Result;
use std::time::Instant;

impl App {
    pub(crate) fn rescan_selected_folder(&mut self) -> Result<()> {
        // Get the folder ID to rescan
        let folder_id = if self.model.navigation.focus_level == 0 {
            // On folder list - rescan selected folder
            if let Some(selected) = self.model.navigation.folders_state_selection {
                if let Some(folder) = self.model.syncthing.folders.get(selected) {
                    folder.id.clone()
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        } else {
            // In a breadcrumb level - rescan the current folder
            if !self.model.navigation.breadcrumb_trail.is_empty() {
                self.model.navigation.breadcrumb_trail[0].folder_id.clone()
            } else {
                return Ok(());
            }
        };

        log_debug(&format!(
            "DEBUG [rescan_selected_folder]: Requesting rescan for folder={}",
            folder_id
        ));

        // Trigger rescan via non-blocking API
        let _ = self
            .api_tx
            .send(services::api::ApiRequest::RescanFolder { folder_id });

        Ok(())
    }

    pub(crate) async fn restore_selected_file(&mut self) -> Result<()> {
        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0 || self.model.navigation.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        let folder_id = self.model.navigation.breadcrumb_trail[level_idx].folder_id.clone();

        // Check if this is a receive-only folder with local changes
        if logic::folder::has_local_changes(self.model.syncthing.folder_statuses.get(&folder_id)) {
            // Receive-only folder with local changes - fetch the list of changed files
            let changed_files = self
                .client
                .get_local_changed_files(&folder_id)
                .await
                .unwrap_or_else(|_| Vec::new());

            // Show confirmation prompt with file list
            self.model.ui.confirm_revert = Some((folder_id, changed_files));
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

        // Get selected item
        let selected = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(()),
        };

        if selected >= level.items.len() {
            return Ok(());
        }

        let item = &level.items[selected];

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
        self.model.ui.confirm_delete = Some((host_path, item.name.clone(), is_dir));

        Ok(())
    }

    pub(crate) fn open_selected_item(&mut self) -> Result<()> {
        // Check if open_command is configured
        let Some(ref open_cmd) = self.open_command else {
            self.model.ui.toast_message = Some((
                "Error: open_command not configured".to_string(),
                Instant::now(),
            ));
            return Ok(());
        };

        // Only works when focused on a breadcrumb level (not folder list)
        if self.model.navigation.focus_level == 0 || self.model.navigation.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if level_idx >= self.model.navigation.breadcrumb_trail.len() {
            return Ok(());
        }

        let level = &self.model.navigation.breadcrumb_trail[level_idx];

        // Get selected item
        let selected = match level.selected_index {
            Some(idx) => idx,
            None => return Ok(()),
        };

        if selected >= level.items.len() {
            return Ok(());
        }

        let item = &level.items[selected];

        // Build the full host path
        // Note: translated_base_path already includes the full path to this directory level
        let host_path = format!(
            "{}/{}",
            level.translated_base_path.trim_end_matches('/'),
            item.name
        );

        // Check if file/directory exists on disk before trying to open
        if !std::path::Path::new(&host_path).exists() {
            log_debug(&format!(
                "open_selected_item: Path does not exist: {}",
                host_path
            ));
            return Ok(()); // Nothing to open
        }

        // Execute command in background (spawn, don't wait for completion)
        // This allows GUI apps and editors to open without blocking the TUI
        let result = std::process::Command::new(open_cmd)
            .arg(&host_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(_child) => {
                // Log in debug mode
                if crate::DEBUG_MODE.load(std::sync::atomic::Ordering::Relaxed) {
                    log_debug(&format!("open_command: spawned {} {}", open_cmd, host_path));
                }
                // Show toast notification with full path
                let toast_msg = format!("Opened: {}", host_path);
                self.model.ui.toast_message = Some((toast_msg, Instant::now()));
            }
            Err(e) => {
                log_debug(&format!(
                    "Failed to execute open_command '{}': {}",
                    open_cmd, e
                ));
                // Show error toast
                let toast_msg = format!("Error: Failed to open with '{}'", open_cmd);
                self.model.ui.toast_message = Some((toast_msg, Instant::now()));
            }
        }

        Ok(())
    }

    pub(crate) fn open_syncthing_web_ui(&mut self) -> Result<()> {
        // Check if open_command is configured
        let Some(ref open_cmd) = self.open_command else {
            self.model.ui.show_toast("Error: open_command not configured".to_string());
            return Ok(());
        };

        // Spawn command to open Syncthing web UI
        let result = std::process::Command::new(open_cmd)
            .arg(&self.base_url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(_child) => {
                self.model.ui.show_toast(format!("Opening Syncthing: {}", self.base_url));
            }
            Err(e) => {
                self.model.ui.show_toast(format!("Failed to open: {}", e));
            }
        }

        Ok(())
    }

    pub(crate) fn copy_to_clipboard(&mut self) -> Result<()> {
        let text_to_copy = if self.model.navigation.focus_level == 0 {
            // In folder list - copy folder ID
            if let Some(selected) = self.model.navigation.folders_state_selection {
                if let Some(folder) = self.model.syncthing.folders.get(selected) {
                    Some(folder.id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            // In breadcrumbs - copy file/directory path (mapped host path)
            if self.model.navigation.breadcrumb_trail.is_empty() {
                return Ok(());
            }

            let level_idx = self.model.navigation.focus_level - 1;
            if level_idx >= self.model.navigation.breadcrumb_trail.len() {
                return Ok(());
            }

            let level = &self.model.navigation.breadcrumb_trail[level_idx];

            // Get selected item
            let selected = match level.selected_index {
                Some(idx) => idx,
                None => return Ok(()),
            };

            if selected >= level.items.len() {
                return Ok(());
            }

            let item = &level.items[selected];

            // Build the full host path
            // Note: translated_base_path already includes the full path to this directory level
            let host_path = format!(
                "{}/{}",
                level.translated_base_path.trim_end_matches('/'),
                item.name
            );

            Some(host_path)
        };

        // Copy to clipboard if we have text
        if let Some(text) = text_to_copy {
            // Always log clipboard operations (not just in debug mode) since they can fail silently
            use std::io::Write;
            let log_file = std::path::Path::new("/tmp/synctui-debug.log");

            if let Some(ref clipboard_cmd) = self.clipboard_command {
                // Use user-configured clipboard command (text sent via stdin)
                // Spawn in background and write to stdin without waiting
                let result = std::process::Command::new(clipboard_cmd)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .and_then(|mut child| {
                        if let Some(mut stdin) = child.stdin.take() {
                            stdin.write_all(text.as_bytes())?;
                            // Close stdin to signal EOF
                            drop(stdin);
                        }
                        Ok(())
                    });

                match result {
                    Ok(_) => {
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(log_file)
                            .and_then(|mut f| {
                                writeln!(f, "Copied to clipboard via {}: {}", clipboard_cmd, text)
                            });
                        // Show toast notification with full path
                        let toast_msg = format!("Copied to clipboard: {}", text);
                        self.model.ui.toast_message = Some((toast_msg, Instant::now()));
                    }
                    Err(e) => {
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(log_file)
                            .and_then(|mut f| {
                                writeln!(
                                    f,
                                    "ERROR: Failed to execute clipboard command '{}': {}",
                                    clipboard_cmd, e
                                )
                            });
                        // Show error toast
                        let toast_msg = format!("Error: Failed to copy with '{}'", clipboard_cmd);
                        self.model.ui.toast_message = Some((toast_msg, Instant::now()));
                    }
                }
            } else {
                // No clipboard command configured - log message
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file)
                    .and_then(|mut f| {
                        writeln!(f, "No clipboard_command configured - set clipboard_command in config.yaml")
                    });
                // Show error toast
                self.model.ui.toast_message = Some((
                    "Error: clipboard_command not configured".to_string(),
                    Instant::now(),
                ));
            }
        }

        Ok(())
    }
}
