//! File operation methods
//!
//! User actions that interact with files and external commands:
//! - Delete files/directories
//! - Rescan folders
//! - Restore deleted files (receive-only folders)
//! - Open files/directories with external commands
//! - Copy paths to clipboard

use crate::model::TerminalRequest;
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
        match self.selected_host_path() {
            Some(host_path) => self.open_host_path(host_path),
            None => OpenOutcome::NothingToOpen,
        }
    }

    /// Opens the file shown in the preview popup
    pub(crate) fn open_previewed_file(&mut self) -> OpenOutcome {
        match self.previewed_host_path() {
            Some(host_path) => self.open_host_path(host_path),
            None => OpenOutcome::NothingToOpen,
        }
    }

    /// Host path of the previewed file: its folder's path plus the file, mapped through
    /// `path_map` like preview reads
    fn previewed_host_path(&self) -> Option<String> {
        let popup = self.model.ui.file_info_popup.as_ref()?;
        let folder = self.model.syncthing.get_folder(&popup.folder_id)?;
        let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), popup.file_path);
        Some(
            self.path_map
                .iter()
                .find_map(|(container, host)| {
                    container_path
                        .strip_prefix(container.as_str())
                        .map(|rest| format!("{}{}", host, rest))
                })
                .unwrap_or(container_path),
        )
    }

    fn open_host_path(&mut self, host_path: String) -> OpenOutcome {
        // Also keeps an editor from creating a missing file inside the synced folder
        if !std::path::Path::new(&host_path).exists() {
            log_debug(&format!("open: Path does not exist: {}", host_path));
            return OpenOutcome::NothingToOpen;
        }
        match logic::open::open_with(
            &self.open_env,
            self.open_text_in_editor,
            file_kind(&host_path),
        ) {
            logic::open::OpenWith::Editor => return self.queue_in_editor(host_path),
            logic::open::OpenWith::Blocked => return OpenOutcome::BinaryNeedsDesktop,
            logic::open::OpenWith::Desktop => {}
        }
        let Some(open_cmd) = self.open_command.clone() else {
            return OpenOutcome::NotConfigured;
        };
        match spawn_detached(&open_cmd, "open_command", &host_path) {
            Ok(()) => {
                log_debug(&format!("open_command: spawned {} {}", open_cmd, host_path));
                OpenOutcome::Opened(host_path)
            }
            Err(outcome) => outcome,
        }
    }

    /// Queues the editor with `host_path` as its last argument
    fn queue_in_editor(&mut self, host_path: String) -> OpenOutcome {
        let Some(command) = self.editor_command.clone() else {
            return OpenOutcome::NoEditor;
        };
        let (program, mut args) =
            match logic::command::parse_command_with(&command, is_program_file) {
                Ok(Some(parsed)) => parsed,
                Ok(None) => {
                    return OpenOutcome::Invalid("Invalid editor: empty command".to_string());
                }
                Err(e) => {
                    return OpenOutcome::Invalid(format!("Invalid editor: {}", e));
                }
            };
        args.push(host_path);
        self.model
            .ui
            .terminal_requests
            .push(TerminalRequest::RunInTerminal { program, args });
        OpenOutcome::QueuedInTerminal
    }

    pub(crate) fn open_web_ui(&mut self) -> WebUiOutcome {
        let url = self.base_url.clone();
        let Some(open_cmd) = self.open_command.clone() else {
            return WebUiOutcome::NotConfigured;
        };
        match spawn_detached(&open_cmd, "open_command", &url) {
            Ok(()) => WebUiOutcome::Opened(url),
            Err(OpenOutcome::Failed { command, error }) => WebUiOutcome::Failed { command, error },
            Err(OpenOutcome::Invalid(message)) => WebUiOutcome::Invalid(message),
            Err(_) => WebUiOutcome::NotConfigured,
        }
    }

    /// `clipboard_command`, unless it is unset or blank (then copies go through OSC 52)
    pub(crate) fn configured_clipboard_command(&self) -> Option<String> {
        self.clipboard_command
            .clone()
            .filter(|command| !command.trim().is_empty())
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
        let outcome = match self.configured_clipboard_command() {
            Some(command) => run_clipboard_command(&command, &text),
            None => {
                self.model
                    .ui
                    .terminal_requests
                    .push(TerminalRequest::Copy(text.clone()));
                CopyOutcome::SentToTerminal(text)
            }
        };
        // Always logged, not just in debug mode: clipboard failures are otherwise silent
        let line = match &outcome {
            CopyOutcome::Copied(text) => format!("Copied to clipboard: {}", text),
            CopyOutcome::Failed { command, error } => {
                format!("ERROR: clipboard command '{}' failed: {}", command, error)
            }
            CopyOutcome::Invalid(message) => format!("ERROR: {}", message),
            CopyOutcome::SentToTerminal(text) => {
                format!("Sent to the terminal clipboard: {}", text)
            }
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
    /// Queued as OSC 52 for the terminal, because no clipboard_command is set
    SentToTerminal(String),
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
    /// Queued for the main loop to run inside the terminal
    QueuedInTerminal,
    /// The editor is needed, but no `editor`, $VISUAL or $EDITOR is set
    NoEditor,
    /// A binary file, and no desktop to open it on
    BinaryNeedsDesktop,
}

/// Result of `o` in the folder list; the keyboard handler turns it into a toast
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebUiOutcome {
    Opened(String),
    Failed {
        command: String,
        error: String,
    },
    /// Full message: `Invalid <setting>: <error>`
    Invalid(String),
    NotConfigured,
}

/// How long `c` waits for a clipboard command to report success or failure
const CLIPBOARD_COMMAND_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs a configured clipboard command with `text` on stdin and waits up to
/// `CLIPBOARD_COMMAND_WAIT` for it to exit
pub(crate) fn run_clipboard_command(command: &str, text: &str) -> CopyOutcome {
    use std::io::Write;
    let (program, args) = match logic::command::parse_command_with(command, is_program_file) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            return CopyOutcome::Invalid("Invalid clipboard_command: empty command".to_string());
        }
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
    // Most tools exit or fork once the text is set; one still running after the wait has
    // its text and keeps running in the background, so stui doesn't freeze on it
    let deadline = std::time::Instant::now() + CLIPBOARD_COMMAND_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return CopyOutcome::Copied(text.to_string()),
            Ok(Some(status)) => return failed(format!("exited with {}", status)),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => return CopyOutcome::Copied(text.to_string()),
            Err(e) => return failed(e.to_string()),
        }
    }
}

/// Kind of the file at `path`: a directory, text (a regular file that is not an image and
/// whose first 8 KB `logic::file::detect_encoding` decodes, the preview's decision), or
/// binary
fn file_kind(path: &str) -> logic::open::FileKind {
    use logic::open::FileKind;
    use std::io::Read;
    if std::path::Path::new(path).is_dir() {
        return FileKind::Directory;
    }
    if logic::file::is_image_file(path) {
        return FileKind::Binary;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return FileKind::Binary;
    };
    let mut head = Vec::with_capacity(8192);
    if file.take(8192).read_to_end(&mut head).is_err() {
        return FileKind::Binary;
    }
    if logic::file::detect_encoding(path, &head).is_some() {
        FileKind::Text
    } else {
        FileKind::Binary
    }
}

/// Whether a configured command string names an existing file, so it is used whole
/// (Windows paths keep their backslashes)
fn is_program_file(command: &str) -> bool {
    std::path::Path::new(command).is_file()
}

/// Starts a configured opener with `target` as its last argument, without waiting
fn spawn_detached(command: &str, setting: &str, target: &str) -> Result<(), OpenOutcome> {
    let (program, args) = match logic::command::parse_command_with(command, is_program_file) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use logic::open::FileKind;

    fn kind_of(name: &str, bytes: &[u8]) -> FileKind {
        let dir = std::env::temp_dir().join(format!("stui-file-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        file_kind(path.to_str().unwrap())
    }

    #[test]
    fn test_utf16_with_bom_is_text() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("hello".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(kind_of("utf16.txt", &bytes), FileKind::Text);
    }

    #[test]
    fn test_nul_bytes_without_art_extension_are_binary() {
        assert_eq!(kind_of("data.bin", b"ab\0cd"), FileKind::Binary);
    }

    #[test]
    fn test_nfo_with_nul_bytes_is_text() {
        assert_eq!(kind_of("release.nfo", b"ab\0cd"), FileKind::Text);
    }
}
