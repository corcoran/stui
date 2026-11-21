//! Keyboard Input Handler
//!
//! Handles all keyboard input and user interactions.
//! This is the largest handler, processing ~60 different key combinations.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

use crate::App;
use crate::model::{self, ConfirmAction};

/// Check if folder history modal should load more files
///
/// Returns true if user is near bottom (within 10 items) and more files are available
fn should_load_more_history(modal: &crate::model::types::FolderHistoryModal) -> bool {
    let near_bottom = modal.selected_index >= modal.entries.len().saturating_sub(10);
    near_bottom && !modal.loading && modal.has_more
}

/// Set the menu's folder to `group`, then regroup the list and report the outcome.
/// Choosing the folder's current group sends nothing.
async fn apply_folder_group(
    app: &mut App,
    menu: &crate::model::types::FolderGroupMenuState,
    group: String,
) {
    if group == menu.current_group {
        return;
    }
    match app.client.set_folder_group(&menu.folder_id, &group).await {
        Ok(()) => {
            app.model.set_folder_group(&menu.folder_id, &group);
            if group.is_empty() {
                app.model
                    .ui
                    .show_success(format!("{} removed from its group", menu.folder_label));
            } else {
                app.model
                    .ui
                    .show_success(format!("{} moved to {}", menu.folder_label, group));
            }
        }
        Err(e) => app
            .model
            .ui
            .show_error(format!("Failed to set group: {}", e)),
    }
}

/// Handle keyboard input
///
/// Processes all keyboard events and dispatches to appropriate actions.
/// The Ignored icon stays until FileInfo returns, so this is the immediate feedback.
fn unignore_toast(item_name: &str) -> String {
    format!("Unignored {}, rescanning...", item_name)
}

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Update last user action timestamp for idle detection
    app.model.performance.last_user_action = Instant::now();

    // Handle setup help dialog first (shown when no cache and connection fails)
    if app.model.ui.show_setup_help {
        match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                // Close dialog and let automatic background retry continue
                app.model.ui.show_setup_help = false;
                return Ok(());
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                // Copy config path to clipboard
                if let Some(clipboard_cmd) = &app.clipboard_command {
                    let path = app.model.ui.config_path.clone();
                    // Execute clipboard command via stdin
                    use std::io::Write;
                    use std::process::{Command, Stdio};

                    let result = Command::new("sh")
                        .arg("-c")
                        .arg(clipboard_cmd)
                        .stdin(Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            if let Some(mut stdin) = child.stdin.take() {
                                stdin.write_all(path.as_bytes())?;
                            }
                            child.wait()
                        });

                    match result {
                        Ok(_) => {
                            app.model
                                .ui
                                .show_success("Config path copied to clipboard".to_string());
                        }
                        Err(e) => {
                            app.model.ui.show_error(format!("Failed to copy: {}", e));
                        }
                    }
                } else {
                    app.model
                        .ui
                        .show_warning("No clipboard command configured".to_string());
                }
                return Ok(());
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                // Quit app
                app.model.ui.should_quit = true;
                return Ok(());
            }
            _ => {
                // Ignore other keys while setup help is showing
                return Ok(());
            }
        }
    }

    // Handle unified confirmation prompts first
    if let Some(action) = app.model.ui.confirm_action.clone() {
        // Handle Rescan separately (needs y/f/n instead of y/n)
        if let ConfirmAction::Rescan {
            folder_id,
            folder_label,
        } = action
        {
            match key.code {
                KeyCode::Char('y') => {
                    // Normal rescan
                    app.model.ui.confirm_action = None;
                    let _ = app.rescan_selected_folder();
                    app.model
                        .ui
                        .show_info(format!("Rescanning {}...", folder_label));
                }
                KeyCode::Char('f') => {
                    // Force refresh
                    app.model.ui.confirm_action = None;
                    let _ = app.force_refresh_folder(&folder_id);
                    app.model
                        .ui
                        .show_info(format!("Force refreshing {}...", folder_label));
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    // Cancel
                    app.model.ui.confirm_action = None;
                }
                _ => {} // Ignore other keys while dialog is open
            }
            return Ok(());
        }

        // Handle other confirmation dialogs (y/n only)
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // User confirmed - execute the action
                app.model.ui.confirm_action = None;

                match action {
                    ConfirmAction::Revert { folder_id, .. } => {
                        // Revert receive-only folder
                        let _ = app.client.revert_folder(&folder_id).await;
                        app.refresh_folder_statuses_nonblocking();
                    }
                    ConfirmAction::Delete { path, is_dir, .. } => {
                        // Delete file or directory
                        let delete_result = if is_dir {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        };

                        if delete_result.is_ok() {
                            let mut deleted: Option<(String, String)> = None;
                            // Get current folder info for cache invalidation
                            if app.model.navigation.focus_level > 0
                                && !app.model.navigation.breadcrumb_trail.is_empty()
                            {
                                let level_idx = app.model.navigation.focus_level - 1;

                                // Extract all needed data first (immutable borrow)
                                let deletion_info = if let Some(level) =
                                    app.model.navigation.breadcrumb_trail.get(level_idx)
                                {
                                    let selected_idx = level.selected_index;
                                    selected_idx.and_then(|idx| {
                                        level.display_items().get(idx).map(|item| {
                                            (
                                                level.folder_id.clone(),
                                                item.name.clone(),
                                                level.prefix.clone(),
                                                idx,
                                            )
                                        })
                                    })
                                } else {
                                    None
                                };

                                // Now do the mutations
                                if let Some((folder_id, item_name, prefix, idx)) = deletion_info {
                                    // Build file path for cache invalidation
                                    let file_path = if let Some(ref prefix) = prefix {
                                        format!("{}{}", prefix, item_name)
                                    } else {
                                        item_name.clone()
                                    };

                                    deleted = Some((folder_id.clone(), file_path.clone()));

                                    // Invalidate cache for this file/directory
                                    if is_dir {
                                        let _ =
                                            app.cache.invalidate_directory(&folder_id, &file_path);
                                    } else {
                                        let _ = app
                                            .cache
                                            .invalidate_single_file(&folder_id, &file_path);
                                    }

                                    let receive_only =
                                        app.model.syncthing.folders.iter().any(|f| {
                                            f.id == folder_id && f.folder_type == "receiveonly"
                                        });
                                    if receive_only {
                                        // The file stays on the other devices, so the row stays,
                                        // shown as deleted locally
                                        crate::logic::sync_states::record_local_deletion(
                                            app.model
                                                .syncthing
                                                .need_index
                                                .entry(folder_id.clone())
                                                .or_default(),
                                            &file_path,
                                        );
                                    } else if let Some(level) =
                                        app.model.navigation.breadcrumb_trail.get_mut(level_idx)
                                    {
                                        // Remove from items by name (not by index, which is relative to filtered list)
                                        level.items.retain(|item| item.name != item_name);

                                        // Remove from filtered_items by name (if filter is active)
                                        if let Some(ref mut filtered) = level.filtered_items {
                                            filtered.retain(|item| item.name != item_name);
                                        }

                                        // Remove from sync states
                                        level.file_sync_states.remove(&item_name);
                                        level.direct_states.remove(&item_name);

                                        // Adjust selection based on display_items() (respects filter)
                                        let display_len = level.display_items().len();
                                        let new_selection = if display_len == 0 {
                                            None
                                        } else if idx >= display_len {
                                            Some(display_len - 1)
                                        } else {
                                            Some(idx)
                                        };
                                        level.selected_index = new_selection;
                                    }

                                    // Invalidate browse cache for this directory
                                    let browse_key = format!(
                                        "{}:{}",
                                        folder_id,
                                        prefix.as_deref().unwrap_or("")
                                    );
                                    app.model.performance.loading_browse.remove(&browse_key);
                                }
                            }

                            // Scan the deleted path so Syncthing records the deletion
                            match deleted {
                                Some((folder_id, path)) => {
                                    app.rescan_paths(&folder_id, vec![path]);
                                }
                                None => {
                                    let _ = app.rescan_selected_folder();
                                }
                            }
                        }
                        // TODO: Show error message if deletion fails
                    }
                    ConfirmAction::IgnoreDelete { .. } => {
                        // Not implemented yet - this variant is never set
                    }
                    ConfirmAction::PauseResume {
                        folder_id,
                        is_paused,
                        ..
                    } => {
                        // Toggle pause state
                        let new_paused_state = !is_paused;
                        match app
                            .client
                            .set_folder_paused(&folder_id, new_paused_state)
                            .await
                        {
                            Ok(_) => {
                                let action = if new_paused_state {
                                    "paused"
                                } else {
                                    "resumed"
                                };
                                app.model.ui.show_success(format!("Folder {}", action));
                                app.model.set_folder_paused(&folder_id, new_paused_state);

                                // Clears the pane of a now-paused folder, or loads a resumed one
                                if app.model.navigation.focus_level == 0
                                    && app
                                        .model
                                        .selected_folder()
                                        .is_some_and(|f| f.id == folder_id)
                                {
                                    let _ = app.load_root_level(true).await;
                                }

                                // Immediately reload folder list to update paused state
                                match app.client.get_folders().await {
                                    Ok(folders) => {
                                        app.model.set_folders(folders);
                                    }
                                    Err(e) => {
                                        crate::log_debug(&format!(
                                            "Failed to reload folders after pause/resume: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                app.model
                                    .ui
                                    .show_error(format!("Failed to pause/resume folder: {}", e));
                            }
                        }
                    }
                    ConfirmAction::Rescan { .. } => {
                        // Handled above (needs y/f/n instead of just y/n)
                        unreachable!("Rescan should be handled earlier")
                    }
                }

                return Ok(());
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                // User cancelled
                app.model.ui.confirm_action = None;
                return Ok(());
            }
            _ => {
                // Ignore other keys while prompt is showing
                return Ok(());
            }
        }
    }

    // ========================================
    // FOLDER HISTORY MODAL HANDLERS
    // ========================================

    // Handle folder history modal (process before other keys)
    if let Some(ref mut modal_state) = app.model.ui.folder_history_modal {
        match key.code {
            KeyCode::Enter => {
                // Get selected file path and folder ID before closing modal
                if let Some(entry) = modal_state.entries.get(modal_state.selected_index) {
                    let file_path = entry.file_path.clone();
                    let folder_id = modal_state.folder_id.clone();

                    // Close modal first for cleaner UX
                    app.close_folder_history_modal();

                    // Navigate back to folder view if we're in breadcrumbs
                    while app.model.navigation.focus_level > 0 {
                        app.go_back();
                    }

                    // Enter the folder to establish breadcrumb level 1
                    match app.enter_folder(&folder_id).await {
                        Ok(()) => {
                            // Now jump to the file within the folder
                            if let Err(e) = app.jump_to_file(&file_path).await {
                                app.model
                                    .ui
                                    .show_error(format!("Failed to navigate: {}", e));
                            }
                        }
                        Err(e) => {
                            app.model
                                .ui
                                .show_error(format!("Failed to enter folder: {}", e));
                        }
                    }
                }
                return Ok(());
            }
            KeyCode::Esc => {
                app.close_folder_history_modal();
                return Ok(());
            }
            KeyCode::Char('u')
                if key.modifiers.contains(KeyModifiers::CONTROL) && app.model.ui.vim_mode =>
            {
                // Half page up (Ctrl-u in vim mode) - must be before plain 'u'
                modal_state.selected_index = modal_state.selected_index.saturating_sub(5);
                return Ok(());
            }
            KeyCode::Char('u') => {
                // Toggle - close modal with same key
                app.close_folder_history_modal();
                return Ok(());
            }
            KeyCode::Up | KeyCode::Char('k') if app.model.ui.vim_mode => {
                // Navigate up in history list
                if modal_state.selected_index > 0 {
                    modal_state.selected_index -= 1;
                }
                return Ok(());
            }
            KeyCode::Down | KeyCode::Char('j') if app.model.ui.vim_mode => {
                // Navigate down in history list
                if modal_state.selected_index + 1 < modal_state.entries.len() {
                    modal_state.selected_index += 1;
                }

                // Auto-load more if near bottom
                if should_load_more_history(modal_state) {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::Up if !app.model.ui.vim_mode => {
                // Navigate up (non-vim mode)
                if modal_state.selected_index > 0 {
                    modal_state.selected_index -= 1;
                }
                return Ok(());
            }
            KeyCode::Down if !app.model.ui.vim_mode => {
                // Navigate down (non-vim mode)
                if modal_state.selected_index + 1 < modal_state.entries.len() {
                    modal_state.selected_index += 1;
                }

                // Auto-load more if near bottom
                if should_load_more_history(modal_state) {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::PageUp => {
                // Page up (10 items)
                modal_state.selected_index = modal_state.selected_index.saturating_sub(10);
                return Ok(());
            }
            KeyCode::PageDown => {
                // Page down (10 items)
                modal_state.selected_index = (modal_state.selected_index + 10)
                    .min(modal_state.entries.len().saturating_sub(1));

                // Auto-load more if near bottom
                if should_load_more_history(modal_state) {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::Home | KeyCode::Char('g')
                if app.model.ui.vim_mode
                    && app.model.ui.vim_command_state
                        == crate::model::VimCommandState::WaitingForSecondG =>
            {
                // Jump to first (gg in vim mode)
                modal_state.selected_index = 0;
                app.model.ui.vim_command_state = crate::model::VimCommandState::None;
                return Ok(());
            }
            KeyCode::Home if !app.model.ui.vim_mode => {
                // Jump to first (non-vim mode)
                modal_state.selected_index = 0;
                return Ok(());
            }
            KeyCode::End | KeyCode::Char('G') if app.model.ui.vim_mode => {
                // Jump to last (G in vim mode)
                if !modal_state.entries.is_empty() {
                    modal_state.selected_index = modal_state.entries.len() - 1;
                }

                // Immediate load if at end and more available (fast jump)
                if modal_state.has_more && !modal_state.loading {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::End if !app.model.ui.vim_mode => {
                // Jump to last (non-vim mode)
                if !modal_state.entries.is_empty() {
                    modal_state.selected_index = modal_state.entries.len() - 1;
                }

                // Immediate load if at end and more available (fast jump)
                if modal_state.has_more && !modal_state.loading {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL) && app.model.ui.vim_mode =>
            {
                // Half page down (Ctrl-d in vim mode)
                modal_state.selected_index = (modal_state.selected_index + 5)
                    .min(modal_state.entries.len().saturating_sub(1));

                // Auto-load more if near bottom
                if should_load_more_history(modal_state) {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::Char('f')
                if key.modifiers.contains(KeyModifiers::CONTROL) && app.model.ui.vim_mode =>
            {
                // Full page down (Ctrl-f in vim mode)
                modal_state.selected_index = (modal_state.selected_index + 10)
                    .min(modal_state.entries.len().saturating_sub(1));

                // Auto-load more if near bottom
                if should_load_more_history(modal_state) {
                    let _ = app.load_next_history_batch().await;
                }

                return Ok(());
            }
            KeyCode::Char('b')
                if key.modifiers.contains(KeyModifiers::CONTROL) && app.model.ui.vim_mode =>
            {
                // Full page up (Ctrl-b in vim mode)
                modal_state.selected_index = modal_state.selected_index.saturating_sub(10);
                return Ok(());
            }
            KeyCode::Char('g') if app.model.ui.vim_mode => {
                // First g in gg sequence
                app.model.ui.vim_command_state = crate::model::VimCommandState::WaitingForSecondG;
                return Ok(());
            }
            _ => {
                // Ignore other keys while modal is open
                return Ok(());
            }
        }
    }

    // Handle pattern selection menu
    if let Some(pattern_state) = &mut app.model.ui.pattern_selection {
        match key.code {
            KeyCode::Up => {
                let selected = pattern_state.selected_index.unwrap_or(0);
                if selected > 0 {
                    pattern_state.selected_index = Some(selected - 1);
                }
                return Ok(());
            }
            KeyCode::Down => {
                let selected = pattern_state.selected_index.unwrap_or(0);
                if selected < pattern_state.patterns.len() - 1 {
                    pattern_state.selected_index = Some(selected + 1);
                }
                return Ok(());
            }
            KeyCode::Enter => {
                // Remove the selected pattern
                let selected = pattern_state.selected_index.unwrap_or(0);
                if selected < pattern_state.patterns.len() {
                    let pattern_to_remove = pattern_state.patterns[selected].clone();
                    let folder_id = pattern_state.folder_id.clone();
                    let item_name = pattern_state.item_name.clone();
                    app.model.ui.pattern_selection = None;

                    // Get all patterns and remove the selected one
                    let all_patterns = app.client.get_ignore_patterns(&folder_id).await?;
                    let updated_patterns: Vec<String> = all_patterns
                        .into_iter()
                        .filter(|p| p != &pattern_to_remove)
                        .collect();

                    app.client
                        .set_ignore_patterns(&folder_id, updated_patterns)
                        .await?;

                    if app.model.navigation.focus_level > 0
                        && app.model.navigation.focus_level
                            <= app.model.navigation.breadcrumb_trail.len()
                    {
                        let level_idx = app.model.navigation.focus_level - 1;
                        let file_path = crate::logic::sync_states::breadcrumb_item_path(
                            app.model.navigation.breadcrumb_trail[level_idx]
                                .prefix
                                .as_deref(),
                            &item_name,
                        );
                        let scope = crate::logic::ignore::scan_scope_for_pattern(
                            &pattern_to_remove,
                            &file_path,
                        );
                        app.finish_unignore(&folder_id, file_path, scope);
                        app.model.ui.show_success(unignore_toast(&item_name));
                    }
                }
                return Ok(());
            }
            KeyCode::Esc => {
                // Cancel
                app.model.ui.pattern_selection = None;
                return Ok(());
            }
            _ => {
                // Ignore other keys while menu is showing
                return Ok(());
            }
        }
    }

    // Handle new group name input (before the menu it was opened from)
    if let Some(input) = &mut app.model.ui.group_name_input {
        match key.code {
            KeyCode::Esc => {
                let menu = input.menu.clone();
                app.model.ui.group_name_input = None;
                app.model.ui.folder_group_menu = Some(menu);
            }
            KeyCode::Enter => {
                let existing =
                    crate::logic::folder_groups::group_names(&app.model.syncthing.folders);
                match crate::logic::folder_groups::resolve_group_name(&input.text, &existing) {
                    Some(name) => {
                        let menu = input.menu.clone();
                        app.model.ui.group_name_input = None;
                        apply_folder_group(app, &menu, name).await;
                    }
                    None => input.show_empty_hint = true,
                }
            }
            KeyCode::Backspace => {
                input.text.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.text.push(c);
                input.show_empty_hint = false;
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle folder group menu
    if let Some(menu) = &mut app.model.ui.folder_group_menu {
        use crate::model::types::GroupMenuOption;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                menu.selected_index = menu.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if menu.selected_index + 1 < menu.options.len() {
                    menu.selected_index += 1;
                }
            }
            KeyCode::Esc => app.model.ui.folder_group_menu = None,
            KeyCode::Enter => {
                let menu = menu.clone();
                app.model.ui.folder_group_menu = None;
                match menu.options.get(menu.selected_index) {
                    Some(GroupMenuOption::NewGroup) => {
                        app.model.ui.group_name_input =
                            Some(crate::model::types::GroupNameInputState {
                                menu,
                                text: String::new(),
                                show_empty_hint: false,
                            });
                    }
                    Some(GroupMenuOption::Group(name)) => {
                        let name = name.clone();
                        apply_folder_group(app, &menu, name).await;
                    }
                    Some(GroupMenuOption::NoGroup) => {
                        apply_folder_group(app, &menu, String::new()).await
                    }
                    None => {}
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle folder type selection menu
    if let Some(type_state) = &mut app.model.ui.folder_type_selection {
        match key.code {
            KeyCode::Up => {
                if type_state.selected_index > 0 {
                    type_state.selected_index -= 1;
                }
                return Ok(());
            }
            KeyCode::Down => {
                if type_state.selected_index < 2 {
                    // 3 options: 0, 1, 2
                    type_state.selected_index += 1;
                }
                return Ok(());
            }
            KeyCode::Enter => {
                // Apply the selected folder type
                let selected_idx = type_state.selected_index;
                let folder_id = type_state.folder_id.clone();
                app.model.ui.folder_type_selection = None;

                let new_type = match selected_idx {
                    0 => "sendonly",
                    1 => "sendreceive",
                    2 => "receiveonly",
                    _ => "sendreceive", // Fallback
                };

                // Call API to change folder type
                match app.client.set_folder_type(&folder_id, new_type).await {
                    Ok(_) => {
                        let type_display = match new_type {
                            "sendonly" => "Send Only",
                            "sendreceive" => "Send & Receive",
                            "receiveonly" => "Receive Only",
                            _ => new_type,
                        };
                        app.model
                            .ui
                            .show_success(format!("Folder type changed to {}", type_display));

                        // Immediately reload folder list to update folder type
                        match app.client.get_folders().await {
                            Ok(folders) => {
                                app.model.set_folders(folders);
                            }
                            Err(e) => {
                                crate::log_debug(&format!(
                                    "Failed to reload folders after type change: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        app.model
                            .ui
                            .show_error(format!("Failed to change folder type: {}", e));
                    }
                }

                return Ok(());
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                // Cancel
                app.model.ui.folder_type_selection = None;
                return Ok(());
            }
            _ => {
                // Ignore other keys while menu is showing
                return Ok(());
            }
        }
    }

    // Handle file info popup
    if let Some(popup_state) = &mut app.model.ui.file_info_popup {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Left | KeyCode::Backspace => {
                // Close popup and trigger sixel cleanup if it was an image (terminal.clear once)
                if popup_state.is_image {
                    app.model.ui.sixel_cleanup_frames = 1;
                }
                app.model.ui.file_info_popup = None;
                return Ok(());
            }
            KeyCode::Char('J') | KeyCode::Char('K') | KeyCode::Down | KeyCode::Up
                if match key.code {
                    KeyCode::Char(_) => app.model.ui.vim_mode,
                    _ => key.modifiers.contains(KeyModifiers::SHIFT),
                } =>
            {
                let forward = matches!(key.code, KeyCode::Char('J') | KeyCode::Down);
                let was_image = popup_state.is_image;
                app.model.ui.vim_command_state = model::VimCommandState::None;
                if app.preview_adjacent_file(forward).await && was_image {
                    app.model.ui.sixel_cleanup_frames = 1;
                }
                return Ok(());
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Scroll down
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(1);
                return Ok(());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // Scroll up
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(1);
                return Ok(());
            }
            KeyCode::PageDown => {
                // Scroll down by page (10 lines)
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(10);
                return Ok(());
            }
            KeyCode::PageUp => {
                // Scroll up by page (10 lines)
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(10);
                return Ok(());
            }
            // Vim keybindings for scrolling
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-d: Half page down
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(10);
                return Ok(());
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-u: Half page up
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(10);
                return Ok(());
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-f: Full page down
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_add(20);
                return Ok(());
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-b: Full page up
                popup_state.scroll_offset = popup_state.scroll_offset.saturating_sub(20);
                return Ok(());
            }
            KeyCode::Char('g') => {
                // First 'g' in 'gg' sequence - need to track this
                if app.model.ui.vim_command_state == model::VimCommandState::WaitingForSecondG {
                    // This is the second 'g' - go to top
                    popup_state.scroll_offset = 0;
                    app.model.ui.vim_command_state = model::VimCommandState::None;
                } else {
                    // First 'g' - wait for second one
                    app.model.ui.vim_command_state = model::VimCommandState::WaitingForSecondG;
                }
                return Ok(());
            }
            KeyCode::Char('r') => {
                if crate::logic::file::has_rendered_view(&popup_state.file_path) {
                    popup_state.raw_view = !popup_state.raw_view;
                    popup_state.scroll_offset = 0;
                }
                app.model.ui.vim_command_state = model::VimCommandState::None;
                return Ok(());
            }
            KeyCode::Char('G') => {
                // Go to bottom (set to a very large number, will be clamped by rendering)
                popup_state.scroll_offset = u16::MAX;
                app.model.ui.vim_command_state = model::VimCommandState::None;
                return Ok(());
            }
            _ => {
                // Reset 'gg' sequence on any other key
                app.model.ui.vim_command_state = model::VimCommandState::None;
                // Ignore other keys while popup is showing
                return Ok(());
            }
        }
    }

    // Handle search input mode (process before other keys)
    // Only process these keys if actively typing (search_mode = true)
    if app.model.ui.search_mode {
        match key.code {
            KeyCode::Esc => {
                // Exit search mode and clear query
                app.clear_search(None); // No toast, user explicitly pressed Esc
                // Reload all breadcrumb levels without filter (for fresh data)
                app.refresh_all_breadcrumbs().await?;
                return Ok(());
            }
            KeyCode::Enter => {
                // Accept search and exit input mode (keep filtering active)
                crate::log_debug(&format!(
                    "DEBUG [keyboard]: Enter pressed in search mode, query='{}', keeping filter active",
                    app.model.ui.search_query
                ));
                app.model.ui.search_mode = false;
                return Ok(());
            }
            KeyCode::Backspace => {
                // Remove last character
                app.model.ui.search_query.pop();

                // If query is now empty, exit search mode
                if app.model.ui.search_query.is_empty() {
                    app.model.ui.search_mode = false;
                    app.model.ui.search_origin_level = None;
                    // Immediately clear filtered_items for current breadcrumb
                    if app.model.navigation.focus_level > 0 {
                        let level_idx = app.model.navigation.focus_level - 1;
                        if let Some(level) =
                            app.model.navigation.breadcrumb_trail.get_mut(level_idx)
                        {
                            level.filtered_items = None;
                        }
                    }
                    app.refresh_current_breadcrumb().await?;
                } else {
                    // Re-filter current breadcrumb
                    app.apply_search_filter();
                }
                return Ok(());
            }
            KeyCode::Char(c) => {
                // Add character to query
                app.model.ui.search_query.push(c);

                // When query reaches 2 characters, trigger recursive prefetch
                if app.model.ui.search_query.len() == 2
                    && let Some(level) = app
                        .model
                        .navigation
                        .breadcrumb_trail
                        .get(app.model.navigation.focus_level.saturating_sub(1))
                {
                    let folder_id = level.folder_id.clone();
                    let prefix = level.prefix.clone();

                    crate::log_debug(&format!(
                        "DEBUG [keyboard]: Search query reached 2 chars, starting prefetch for folder '{}' prefix '{:?}'",
                        folder_id, prefix
                    ));

                    // Clear previous prefetch tracking for new search
                    app.model.performance.discovered_dirs.clear();

                    // Start prefetch from current location
                    app.prefetch_subdirectories_for_search(&folder_id, prefix.as_deref());
                }

                // Re-filter current breadcrumb in real-time (only applies filter if >= 2 chars)
                app.apply_search_filter();
                return Ok(());
            }
            _ => {
                // Ignore other keys in search mode
                return Ok(());
            }
        }
    }

    // Handle Esc in breadcrumbs - clear search if active (even when not in search_mode)
    if key.code == KeyCode::Esc {
        crate::log_debug(&format!(
            "DEBUG [keyboard]: Esc pressed - focus_level={}, search_query='{}', search_mode={}",
            app.model.navigation.focus_level, app.model.ui.search_query, app.model.ui.search_mode
        ));

        if app.model.navigation.focus_level > 0 && !app.model.ui.search_query.is_empty() {
            crate::log_debug("DEBUG [keyboard]: Clearing search...");
            app.clear_search(None); // No toast, user explicitly pressed Esc
            // Reload all breadcrumb levels without filter (for fresh data)
            app.refresh_all_breadcrumbs().await?;
            return Ok(());
        }
    }

    match key.code {
        KeyCode::Char('q') => app.model.ui.should_quit = true,
        KeyCode::Char('r') => {
            // Show rescan confirmation dialog
            if let Some((folder_id, folder_label)) = app.get_rescan_folder_info() {
                app.model.ui.confirm_action = Some(ConfirmAction::Rescan {
                    folder_id,
                    folder_label,
                });
            }
        }
        KeyCode::Char('u') if app.model.navigation.focus_level > 0 => {
            let selection = app.model.current_level().and_then(|level| {
                level.selected_item().map(|item| {
                    (
                        level.folder_id.clone(),
                        crate::logic::sync_states::breadcrumb_item_path(
                            level.prefix.as_deref(),
                            &item.name,
                        ),
                        item.name.clone(),
                        crate::logic::display::level_item_display(
                            &app.model.syncthing,
                            level,
                            item,
                        ),
                    )
                })
            });
            match selection {
                Some((
                    folder_id,
                    path,
                    name,
                    Some(crate::logic::display::ItemDisplay::LocallyDeleted),
                )) => {
                    app.queue_undelete(&folder_id, path);
                    let busy = app
                        .model
                        .syncthing
                        .folder_statuses
                        .get(&folder_id)
                        .is_some_and(|s| s.state != "idle");
                    let label = app
                        .model
                        .syncthing
                        .folders
                        .iter()
                        .find(|f| f.id == folder_id)
                        .map(|f| f.label.clone().unwrap_or_else(|| f.id.clone()))
                        .unwrap_or(folder_id);
                    if busy {
                        app.model.ui.show_success(format!(
                            "Restoring {} once {} finishes syncing…",
                            name, label
                        ));
                    } else {
                        app.model.ui.show_success(format!("Restoring {}…", name));
                    }
                }
                _ => app
                    .model
                    .ui
                    .show_warning("Only files deleted on this device can be undeleted"),
            }
        }
        KeyCode::Char('R') => {
            // Restore selected file (if remote-only/deleted locally)
            let _ = app.restore_selected_file().await;
        }
        // Vim keybindings with Ctrl modifiers (check before 'd' and other letters)
        KeyCode::Char('d')
            if app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.half_page_down(20).await; // Use reasonable default, will be more precise with frame height
        }
        KeyCode::Char('u')
            if app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.half_page_up(20).await;
        }
        KeyCode::Char('f')
            if !app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            // Ctrl-F: Enter search mode (normal mode only - vim uses / instead)
            app.enter_search_mode();
        }
        KeyCode::Char('f')
            if app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.page_down(40).await;
        }
        KeyCode::Char('b')
            if app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.page_up(40).await;
        }
        KeyCode::Char('d') => {
            // Flush pending writes before destructive operation
            app.flush_pending_db_writes();
            // Delete file from disk (with confirmation)
            let _ = app.delete_file().await;
        }
        KeyCode::Char('i') => {
            // Toggle ignore state (add or remove from .stignore)
            match app.toggle_ignore().await {
                Ok(crate::app::ignore::IgnoreToggle::Unignored(item_name)) => {
                    app.model.ui.show_success(unignore_toast(&item_name));
                }
                Ok(crate::app::ignore::IgnoreToggle::BlockedByPendingDelete(path)) => {
                    app.model.ui.show_warning(format!(
                        "Cannot un-ignore: deletion in progress for {}",
                        path.display()
                    ));
                }
                _ => {}
            }
        }
        KeyCode::Char('I') => {
            // Flush pending writes before destructive operation
            app.flush_pending_db_writes();
            // Ignore file AND delete from disk
            let _ = app.ignore_and_delete().await;
        }
        KeyCode::Char('o') if app.model.navigation.focus_level == 0 => {
            // Open Syncthing web UI in browser
            let _ = app.open_syncthing_web_ui();
        }
        KeyCode::Char('o') if app.model.navigation.focus_level > 0 => {
            // Open file/directory with configured command
            let _ = app.open_selected_item();
        }
        KeyCode::Char('c') if app.model.navigation.focus_level == 0 => {
            // Change folder type (only in folder view)
            if let Some(folder) = app.model.selected_folder() {
                let label = folder.label.clone().unwrap_or_else(|| folder.id.clone());
                // Determine initial selection index based on current type
                let selected_index = match folder.folder_type.as_str() {
                    "sendonly" => 0,
                    "sendreceive" => 1,
                    "receiveonly" => 2,
                    _ => 1, // Default to Send & Receive
                };
                app.model.ui.folder_type_selection =
                    Some(crate::model::types::FolderTypeSelectionState {
                        folder_id: folder.id.clone(),
                        folder_label: label,
                        current_type: folder.folder_type.clone(),
                        selected_index,
                    });
            }
        }
        KeyCode::Char('e') if app.model.navigation.focus_level == 0 => {
            if let Some(folder) = app.model.selected_folder() {
                if crate::logic::folder_groups::groups_supported(&app.model.syncthing.folders) {
                    let current_group = folder.group.clone().unwrap_or_default();
                    let options = crate::logic::folder_groups::group_menu_options(
                        &app.model.syncthing.folders,
                    );
                    let current_option = if current_group.is_empty() {
                        crate::model::types::GroupMenuOption::NoGroup
                    } else {
                        crate::model::types::GroupMenuOption::Group(current_group.clone())
                    };
                    let selected_index = options
                        .iter()
                        .position(|o| *o == current_option)
                        .unwrap_or(0);
                    app.model.ui.folder_group_menu =
                        Some(crate::model::types::FolderGroupMenuState {
                            folder_id: folder.id.clone(),
                            folder_label: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
                            current_group,
                            options,
                            selected_index,
                        });
                } else {
                    let message = crate::logic::folder_groups::groups_unsupported_message(
                        app.model.syncthing.version.as_deref(),
                    );
                    app.model.ui.show_warning(message);
                }
            }
        }
        KeyCode::Char('c') if app.model.navigation.focus_level > 0 => {
            // Copy file/directory path (breadcrumbs only)
            let _ = app.copy_to_clipboard();
        }
        KeyCode::Char('f') if app.model.navigation.focus_level > 0 => {
            // Toggle out-of-sync filter (only in breadcrumb view)
            app.activate_out_of_sync_filter();
        }
        KeyCode::Char('u') if app.model.navigation.focus_level == 0 => {
            // Open folder update history modal (folder view only)
            if let Some(folder) = app.model.selected_folder() {
                let folder_id = folder.id.clone();
                let label = folder.label.clone().unwrap_or_else(|| folder.id.clone());
                app.open_folder_history_modal(&folder_id, &label).await;
            }
        }
        KeyCode::Char('p') if app.model.navigation.focus_level == 0 => {
            // Pause/resume folder (only in folder view)
            if let Some(folder) = app.model.selected_folder() {
                let label = folder.label.clone().unwrap_or_else(|| folder.id.clone());
                app.model.ui.confirm_action = Some(crate::model::ConfirmAction::PauseResume {
                    folder_id: folder.id.clone(),
                    label,
                    is_paused: folder.paused,
                });
            }
        }
        KeyCode::Char('s') => {
            // Cycle through sort modes
            app.cycle_sort_mode();
            // Show toast with new sort mode
            app.model
                .ui
                .show_info(format!("Sort: {}", app.model.ui.sort_mode.as_str()));
        }
        KeyCode::Char('S') => {
            // Toggle reverse sort order
            app.toggle_sort_reverse();
            // Show toast with new sort direction
            let direction = if app.model.ui.sort_reverse {
                "descending"
            } else {
                "ascending"
            };
            app.model.ui.show_info(format!(
                "Sort: {} ({})",
                app.model.ui.sort_mode.as_str(),
                direction
            ));
        }
        KeyCode::Char('t') => {
            // Cycle through display modes: Off -> TimestampOnly -> TimestampAndSize -> Off
            app.model.ui.display_mode =
                crate::logic::ui::cycle_display_mode(app.model.ui.display_mode);
        }
        KeyCode::Char('/') if app.model.ui.vim_mode => {
            // /: Enter search mode (vim mode only)
            app.enter_search_mode();
        }
        KeyCode::Char('?') if app.model.navigation.focus_level > 0 => {
            // Toggle file information popup
            if let Some(popup_state) = &app.model.ui.file_info_popup {
                // Close popup and trigger sixel cleanup if it was an image (terminal.clear once)
                if popup_state.is_image {
                    app.model.ui.sixel_cleanup_frames = 1;
                }
                app.model.ui.file_info_popup = None;
            } else {
                // Open popup for selected item
                if let Some(level) = app
                    .model
                    .navigation
                    .breadcrumb_trail
                    .get(app.model.navigation.focus_level - 1)
                    && let Some(selected_idx) = level.selected_index
                    && let Some(item) = level.display_items().get(selected_idx)
                {
                    // Construct full path
                    let file_path = if let Some(prefix) = &level.prefix {
                        format!("{}{}", prefix, item.name)
                    } else {
                        item.name.clone()
                    };

                    // Fetch file info and content (await since it's async)
                    app.fetch_file_info_and_content(
                        level.folder_id.clone(),
                        file_path,
                        item.clone(),
                    )
                    .await;
                }
            }
        }
        // Vim keybindings
        KeyCode::Char('h') if app.model.ui.vim_mode => {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.go_back();
        }
        KeyCode::Char('j') if app.model.ui.vim_mode => {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.next_item().await;
        }
        KeyCode::Char('k') if app.model.ui.vim_mode => {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.previous_item().await;
        }
        KeyCode::Char('l') if app.model.ui.vim_mode => {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            if app.model.navigation.focus_level == 0 {
                app.load_root_level(false).await?; // Not preview - actually enter folder
            } else {
                app.enter_directory().await?;
            }
        }
        KeyCode::Char('g') if app.model.ui.vim_mode => {
            let (new_state, should_jump) = crate::logic::ui::next_vim_command_state(
                app.model.ui.vim_command_state,
                true, // 'g' key pressed
            );
            app.model.ui.vim_command_state = new_state;
            if should_jump {
                app.jump_to_first().await;
            }
        }
        KeyCode::Char('G') if app.model.ui.vim_mode => {
            app.model.ui.vim_command_state = model::VimCommandState::None;
            app.jump_to_last().await;
        }
        // Standard navigation keys (not advertised)
        KeyCode::PageDown => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.page_down(40).await;
        }
        KeyCode::PageUp => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.page_up(40).await;
        }
        KeyCode::Home => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.jump_to_first().await;
        }
        KeyCode::End => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.jump_to_last().await;
        }
        KeyCode::Left | KeyCode::Backspace => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            // Flush before navigation to save state
            app.flush_pending_db_writes();
            app.go_back();
        }
        KeyCode::Right | KeyCode::Enter => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            // Flush before navigation to save state
            app.flush_pending_db_writes();
            if app.model.navigation.focus_level == 0 {
                app.load_root_level(false).await?; // Not preview - actually enter folder
            } else {
                // Check if selected item is a file - if so, show preview instead of navigating
                if let Some(level) = app
                    .model
                    .navigation
                    .breadcrumb_trail
                    .get(app.model.navigation.focus_level - 1)
                    && let Some(selected_idx) = level.selected_index
                    && let Some(item) = level.display_items().get(selected_idx)
                {
                    if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                        // File - show preview (same logic as '?' key)
                        let file_path = if let Some(prefix) = &level.prefix {
                            format!("{}{}", prefix, item.name)
                        } else {
                            item.name.clone()
                        };

                        app.fetch_file_info_and_content(
                            level.folder_id.clone(),
                            file_path,
                            item.clone(),
                        )
                        .await;
                    } else {
                        // Directory - navigate into it
                        app.enter_directory().await?;
                    }
                }
            }
        }
        KeyCode::Up => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.previous_item().await;
        }
        KeyCode::Down => {
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
            app.next_item().await;
        }
        _ => {
            // Reset last_key_was_g on any other key
            if app.model.ui.vim_mode {
                app.model.ui.vim_command_state = model::VimCommandState::None;
            }
        }
    }
    Ok(())
}
