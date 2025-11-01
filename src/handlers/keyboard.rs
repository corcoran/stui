//! Keyboard Input Handler
//!
//! Handles all keyboard input and user interactions.
//! This is the largest handler, processing ~60 different key combinations.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

use crate::api::SyncState;
use crate::model;
use crate::App;

/// Handle keyboard input
///
/// Processes all keyboard events and dispatches to appropriate actions.
pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
        // Update last user action timestamp for idle detection
        app.model.performance.last_user_action = Instant::now();

        // Handle confirmation prompts first
        if let Some((folder_id, _)) = &app.model.ui.confirm_revert {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - revert the folder
                    let folder_id = folder_id.clone();
                    app.model.ui.confirm_revert = None;
                    let _ = app.client.revert_folder(&folder_id).await;

                    // Refresh statuses in background (non-blocking)
                    app.refresh_folder_statuses_nonblocking();

                    return Ok(());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled
                    app.model.ui.confirm_revert = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while prompt is showing
                    return Ok(());
                }
            }
        }

        // Handle delete confirmation prompt
        if let Some((host_path, _name, is_dir)) = &app.model.ui.confirm_delete {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - delete the file/directory
                    let host_path = host_path.clone();
                    let is_dir = *is_dir;
                    app.model.ui.confirm_delete = None;

                    // Perform deletion
                    let delete_result = if is_dir {
                        std::fs::remove_dir_all(&host_path)
                    } else {
                        std::fs::remove_file(&host_path)
                    };

                    if delete_result.is_ok() {
                        // Get current folder info for cache invalidation
                        if app.model.navigation.focus_level > 0 && !app.model.navigation.breadcrumb_trail.is_empty() {
                            let level_idx = app.model.navigation.focus_level - 1;

                            // Extract all needed data first (immutable borrow)
                            let deletion_info =
                                if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
                                    let selected_idx = level.selected_index;
                                    selected_idx.and_then(|idx| {
                                        level.items.get(idx).map(|item| {
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

                                // Invalidate cache for this file/directory
                                if is_dir {
                                    let _ = app.cache.invalidate_directory(&folder_id, &file_path);
                                } else {
                                    let _ =
                                        app.cache.invalidate_single_file(&folder_id, &file_path);
                                }

                                // Immediately remove from current view (mutable borrow)
                                if let Some(level) = app.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                                    // Remove from items
                                    if idx < level.items.len() {
                                        level.items.remove(idx);
                                    }
                                    // Remove from sync states
                                    level.file_sync_states.remove(&item_name);

                                    // Adjust selection
                                    let new_selection = if level.items.is_empty() {
                                        None
                                    } else if idx >= level.items.len() {
                                        Some(level.items.len() - 1)
                                    } else {
                                        Some(idx)
                                    };
                                    level.selected_index = new_selection;
                                }

                                // Invalidate browse cache for this directory
                                let browse_key =
                                    format!("{}:{}", folder_id, prefix.as_deref().unwrap_or(""));
                                app.model.performance.loading_browse.remove(&browse_key);
                            }
                        }

                        // Trigger rescan after successful deletion
                        let _ = app.rescan_selected_folder();
                    }
                    // TODO: Show error message if deletion fails

                    return Ok(());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled
                    app.model.ui.confirm_delete = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while prompt is showing
                    return Ok(());
                }
            }
        }

        // Handle pause/resume confirmation prompt
        if let Some((folder_id, _folder_label, is_paused)) = &app.model.ui.confirm_pause_resume {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed - pause/resume the folder
                    let folder_id = folder_id.clone();
                    let is_paused = *is_paused;
                    app.model.ui.confirm_pause_resume = None;

                    // Toggle pause state
                    let new_paused_state = !is_paused;
                    match app.client.set_folder_paused(&folder_id, new_paused_state).await {
                        Ok(_) => {
                            let action = if new_paused_state { "paused" } else { "resumed" };
                            app.model.ui.show_toast(format!("Folder {}", action));

                            // Immediately reload folder list to update paused state
                            match app.client.get_folders().await {
                                Ok(folders) => {
                                    app.model.syncthing.folders = folders;
                                }
                                Err(e) => {
                                    crate::log_debug(&format!("Failed to reload folders after pause/resume: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            app.model.ui.show_toast(format!("Failed to pause/resume folder: {}", e));
                        }
                    }

                    return Ok(());
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled
                    app.model.ui.confirm_pause_resume = None;
                    return Ok(());
                }
                _ => {
                    // Ignore other keys while prompt is showing
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
                        crate::log_bug("pattern_selection: updated .stignore");

                        // Immediately show as Unknown to give user feedback
                        if app.model.navigation.focus_level > 0 && app.model.navigation.focus_level <= app.model.navigation.breadcrumb_trail.len() {
                            let level_idx = app.model.navigation.focus_level - 1;
                            if let Some(level) = app.model.navigation.breadcrumb_trail.get_mut(level_idx) {
                                level
                                    .file_sync_states
                                    .insert(item_name.clone(), SyncState::Unknown);

                                // Update ignored_exists (file is no longer ignored) - do it inline to avoid borrow issues
                                level.ignored_exists.remove(&item_name);

                                // Don't add optimistic update for unignore - final state is unpredictable
                                crate::log_bug(&format!(
                                    "pattern_selection: cleared {} state (un-ignoring), no optimistic update",
                                    item_name
                                ));
                            }
                        }

                        // Wait for Syncthing to process .stignore change before rescanning
                        crate::log_bug("pattern_selection: waiting 200ms for Syncthing to process .stignore change");
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                        // Trigger rescan - ItemStarted/ItemFinished events will update state
                        // Also fetch file info after delay as fallback (for files that don't need syncing)
                        crate::log_bug(&format!(
                            "pattern_selection: calling rescan for folder={}",
                            folder_id
                        ));
                        app.client.rescan_folder(&folder_id).await?;
                        crate::log_bug("pattern_selection: rescan completed");

                        let folder_id_clone = folder_id.clone();
                        let api_tx = app.api_tx.clone();
                        let item_name_clone = item_name.clone();

                        let file_path_for_api = if app.model.navigation.focus_level > 0
                            && app.model.navigation.focus_level <= app.model.navigation.breadcrumb_trail.len()
                        {
                            let level_idx = app.model.navigation.focus_level - 1;
                            if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
                                if let Some(ref prefix) = level.prefix {
                                    format!("{}/{}", prefix.trim_matches('/'), &item_name_clone)
                                } else {
                                    item_name_clone.clone()
                                }
                            } else {
                                item_name_clone.clone()
                            }
                        } else {
                            item_name_clone
                        };

                        tokio::spawn(async move {
                            // Wait longer for ItemStarted event to potentially fire
                            // Syncthing needs time to discover file, calculate hashes, start transfer
                            crate::log_bug("pattern_selection: waiting 3 seconds for ItemStarted event");
                            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                            crate::log_bug(&format!(
                                "pattern_selection: requesting file info for {}",
                                file_path_for_api
                            ));
                            // Fetch file info as fallback
                            let _ = api_tx.send(crate::services::api::ApiRequest::GetFileInfo {
                                folder_id: folder_id_clone,
                                file_path: file_path_for_api,
                                priority: crate::services::api::Priority::Medium,
                            });
                        });
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
                            app.model.ui.show_toast(format!("Folder type changed to {}", type_display));

                            // Immediately reload folder list to update folder type
                            match app.client.get_folders().await {
                                Ok(folders) => {
                                    app.model.syncthing.folders = folders;
                                }
                                Err(e) => {
                                    crate::log_debug(&format!("Failed to reload folders after type change: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            app.model.ui.show_toast(format!("Failed to change folder type: {}", e));
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
                KeyCode::Esc | KeyCode::Char('?') => {
                    // Close popup and trigger sixel cleanup if it was an image (terminal.clear once)
                    if popup_state.is_image {
                        app.model.ui.sixel_cleanup_frames = 1;
                    }
                    app.model.ui.file_info_popup = None;
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

        match key.code {
            KeyCode::Char('q') => app.model.ui.should_quit = true,
            KeyCode::Char('r') => {
                // Rescan the selected/current folder
                let _ = app.rescan_selected_folder();
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
                let _ = app.toggle_ignore().await;
            }
            KeyCode::Char('I') => {
                // Flush pending writes before destructive operation
                app.flush_pending_db_writes();
                // Ignore file AND delete from disk
                let _ = app.ignore_and_delete().await;
            }
            KeyCode::Char('o') => {
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
                    app.model.ui.folder_type_selection = Some(crate::model::types::FolderTypeSelectionState {
                        folder_id: folder.id.clone(),
                        folder_label: label,
                        current_type: folder.folder_type.clone(),
                        selected_index,
                    });
                }
            }
            KeyCode::Char('c') if app.model.navigation.focus_level > 0 => {
                // Copy file/directory path (breadcrumbs only)
                let _ = app.copy_to_clipboard();
            }
            KeyCode::Char('p') if app.model.navigation.focus_level == 0 => {
                // Pause/resume folder (only in folder view)
                if let Some(folder) = app.model.selected_folder() {
                    let label = folder.label.clone().unwrap_or_else(|| folder.id.clone());
                    app.model.ui.confirm_pause_resume = Some((
                        folder.id.clone(),
                        label,
                        folder.paused,
                    ));
                }
            }
            KeyCode::Char('s') => {
                // Cycle through sort modes
                app.cycle_sort_mode();
            }
            KeyCode::Char('S') => {
                // Toggle reverse sort order
                app.toggle_sort_reverse();
            }
            KeyCode::Char('t') => {
                // Cycle through display modes: Off -> TimestampOnly -> TimestampAndSize -> Off
                app.model.ui.display_mode = crate::logic::ui::cycle_display_mode(app.model.ui.display_mode);
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
                    if let Some(level) = app.model.navigation.breadcrumb_trail.get(app.model.navigation.focus_level - 1) {
                        if let Some(selected_idx) = level.selected_index {
                            if let Some(item) = level.items.get(selected_idx) {
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
                    app.enter_directory().await?;
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
