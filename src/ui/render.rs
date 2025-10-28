use ratatui::Frame;
use crate::App;

use super::{
    breadcrumb::{self, DisplayMode},
    dialogs, folder_list, layout, legend, status_bar, system_bar, toast,
};

/// Main render function - orchestrates all UI rendering
/// This replaces the large terminal.draw() closure in main.rs
pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.size();

    // Calculate layout
    let has_breadcrumbs = !app.breadcrumb_trail.is_empty();
    let layout_info = layout::calculate_layout(size, app.breadcrumb_trail.len(), has_breadcrumbs);

    // Render system info bar at the top
    let (total_files, total_dirs, total_bytes) = app.get_local_state_summary();
    system_bar::render_system_bar(
        f,
        layout_info.system_area,
        app.system_status.as_ref(),
        app.device_name.as_deref(),
        (total_files, total_dirs, total_bytes),
        app.last_transfer_rates,
    );

    // Render folders pane if visible
    if layout_info.folders_visible {
        if let Some(folders_area) = layout_info.folders_area {
            folder_list::render_folder_list(
                f,
                folders_area,
                &app.folders,
                &app.folder_statuses,
                app.statuses_loaded,
                &mut app.folders_state,
                app.focus_level == 0,
                &app.icon_renderer,
                &app.last_folder_updates,
            );
        }
    }

    // Render breadcrumb levels
    let mut breadcrumb_idx = 0;
    for (idx, level) in app.breadcrumb_trail.iter_mut().enumerate() {
        if idx + 1 < layout_info.start_pane {
            continue; // Skip panes that are off-screen to the left
        }

        if breadcrumb_idx >= layout_info.breadcrumb_areas.len() {
            break; // No more areas to render into
        }

        let area = layout_info.breadcrumb_areas[breadcrumb_idx];

        // Determine title
        let title = if let Some(ref prefix) = level.prefix {
            // Show last part of path
            let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();
            parts.last().map(|s| s.to_string()).unwrap_or_else(|| level.folder_label.clone())
        } else {
            level.folder_label.clone()
        };

        let is_focused = app.focus_level == idx + 1;
        // All ancestor breadcrumbs should remain highlighted when drilling deeper
        let is_parent_selected = app.focus_level > idx + 1;

        // Convert DisplayMode from main.rs to ui::breadcrumb::DisplayMode
        let display_mode = match app.display_mode {
            crate::DisplayMode::Off => DisplayMode::Off,
            crate::DisplayMode::TimestampOnly => DisplayMode::TimestampOnly,
            crate::DisplayMode::TimestampAndSize => DisplayMode::TimestampAndSize,
        };

        // Check if any items in this level are currently syncing and update their state
        for item in &level.items {
            let item_path = if let Some(ref prefix) = level.prefix {
                format!("{}/{}", prefix.trim_end_matches('/'), item.name)
            } else {
                item.name.clone()
            };
            let sync_key = format!("{}:{}", level.folder_id, item_path);

            if app.syncing_files.contains(&sync_key) {
                // Override state to Syncing if this file is actively syncing
                level.file_sync_states.insert(item.name.clone(), crate::api::SyncState::Syncing);
            }
        }

        breadcrumb::render_breadcrumb_panel(
            f,
            area,
            &level.items,
            &level.file_sync_states,
            &mut level.state,
            &title,
            is_focused,
            is_parent_selected,
            display_mode,
            &app.icon_renderer,
            &level.translated_base_path,
            level.prefix.as_deref(),
        );

        breadcrumb_idx += 1;
    }

    // Render hotkey legend if there's space
    if let Some(legend_area) = layout_info.legend_area {
        let has_breadcrumbs = !app.breadcrumb_trail.is_empty();

        // Check if restore is available (only in breadcrumbs with local changes)
        let can_restore = if app.focus_level > 0 && has_breadcrumbs {
            // Get the folder ID from the breadcrumb trail
            let folder_id = &app.breadcrumb_trail[0].folder_id;
            // Check if the folder has local changes to restore
            app.folder_statuses.get(folder_id)
                .map(|status| status.receive_only_total_items > 0)
                .unwrap_or(false)
        } else {
            false
        };

        legend::render_legend(
            f,
            legend_area,
            app.vim_mode,
            app.focus_level,
            can_restore,
            app.open_command.is_some(),
        );
    }

    // Render status bar at the bottom
    let (breadcrumb_folder_label, breadcrumb_item_count, breadcrumb_selected_item) = if app.focus_level > 0 {
        let level_idx = app.focus_level - 1;
        if let Some(level) = app.breadcrumb_trail.get(level_idx) {
            let folder_label = Some(level.folder_label.as_str());
            let item_count = Some(level.items.len());
            let selected_item = level.state.selected().and_then(|sel| {
                level.items.get(sel).map(|item| {
                    let sync_state = level.file_sync_states.get(&item.name).copied();
                    let is_ignored = sync_state == Some(crate::api::SyncState::Ignored);
                    let exists = if is_ignored {
                        level.ignored_exists.get(&item.name).copied()
                    } else {
                        None
                    };
                    (item.name.as_str(), item.item_type.as_str(), sync_state, exists)
                })
            });
            (folder_label, item_count, selected_item)
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    status_bar::render_status_bar(
        f,
        layout_info.status_area,
        app.focus_level,
        &app.folders,
        &app.folder_statuses,
        app.folders_state.selected(),
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        app.sort_mode.as_str(),
        app.sort_reverse,
        app.last_load_time_ms,
        app.cache_hit,
    );

    // Render confirmation dialogs if active
    if let Some((_folder_id, changed_files)) = &app.confirm_revert {
        dialogs::render_revert_confirmation(f, changed_files);
    }

    if let Some((_host_path, display_name, is_dir)) = &app.confirm_delete {
        dialogs::render_delete_confirmation(f, display_name, *is_dir);
    }

    if let Some((_folder_id, _item_name, patterns, state)) = &mut app.pattern_selection {
        dialogs::render_pattern_selection(f, patterns, state);
    }

    // Render toast notification if active
    if let Some((message, _timestamp)) = &app.toast_message {
        toast::render_toast(f, size, message);
    }
}
