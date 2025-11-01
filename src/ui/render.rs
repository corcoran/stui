use crate::App;
use ratatui::Frame;

use super::{
    breadcrumb::{self, DisplayMode},
    dialogs, folder_list, layout, legend, status_bar, system_bar, toast,
};

/// Main render function - orchestrates all UI rendering
/// This replaces the large terminal.draw() closure in main.rs
pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Calculate layout
    let has_breadcrumbs = !app.model.navigation.breadcrumb_trail.is_empty();
    let layout_info = layout::calculate_layout(size, app.model.navigation.breadcrumb_trail.len(), has_breadcrumbs);

    // Render system info bar at the top
    let (total_files, total_dirs, total_bytes) = app.get_local_state_summary();
    system_bar::render_system_bar(
        f,
        layout_info.system_area,
        app.model.syncthing.system_status.as_ref(),
        app.model.syncthing.device_name.as_deref(),
        (total_files, total_dirs, total_bytes),
        app.model.syncthing.last_transfer_rates,
    );

    // Render folders pane if visible
    if layout_info.folders_visible {
        if let Some(folders_area) = layout_info.folders_area {
            // Create temporary ListState for rendering
            let mut temp_state = ratatui::widgets::ListState::default();
            temp_state.select(app.model.navigation.folders_state_selection);
            folder_list::render_folder_list(
                f,
                folders_area,
                &app.model.syncthing.folders,
                &app.model.syncthing.folder_statuses,
                app.model.syncthing.statuses_loaded,
                &mut temp_state,
                app.model.navigation.focus_level == 0,
                &app.icon_renderer,
                &app.model.syncthing.last_folder_updates,
            );
            // Sync back the selection (though folder_list doesn't usually modify it)
            app.model.navigation.folders_state_selection = temp_state.selected();
        }
    }

    // Render breadcrumb levels
    let mut breadcrumb_idx = 0;
    for (idx, level) in app.model.navigation.breadcrumb_trail.iter_mut().enumerate() {
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
            parts
                .last()
                .map(|s| s.to_string())
                .unwrap_or_else(|| level.folder_label.clone())
        } else {
            level.folder_label.clone()
        };

        let is_focused = app.model.navigation.focus_level == idx + 1;
        // All ancestor breadcrumbs should remain highlighted when drilling deeper
        let is_parent_selected = app.model.navigation.focus_level > idx + 1;

        // Convert DisplayMode from main.rs to ui::breadcrumb::DisplayMode
        let display_mode = match app.model.ui.display_mode {
            crate::DisplayMode::Off => DisplayMode::Off,
            crate::DisplayMode::TimestampOnly => DisplayMode::TimestampOnly,
            crate::DisplayMode::TimestampAndSize => DisplayMode::TimestampAndSize,
        };

        // Syncing states now come from ItemStarted events (no override needed)

        // Create temporary ListState for rendering
        let mut temp_state = ratatui::widgets::ListState::default();
        temp_state.select(level.selected_index);
        breadcrumb::render_breadcrumb_panel(
            f,
            area,
            &level.items,
            &level.file_sync_states,
            &level.ignored_exists,
            &mut temp_state,
            &title,
            is_focused,
            is_parent_selected,
            display_mode,
            &app.icon_renderer,
            &level.translated_base_path,
            level.prefix.as_deref(),
        );
        // Sync back the selection
        level.selected_index = temp_state.selected();

        breadcrumb_idx += 1;
    }

    // Render hotkey legend if there's space
    if let Some(legend_area) = layout_info.legend_area {
        // Check if restore is available (only in breadcrumbs with local changes)
        let can_restore = if !app.model.navigation.breadcrumb_trail.is_empty() {
            let folder_id = &app.model.navigation.breadcrumb_trail[0].folder_id;
            let folder_status = app.model.syncthing.folder_statuses.get(folder_id);
            crate::logic::folder::should_show_restore_button(
                app.model.navigation.focus_level,
                folder_status,
            )
        } else {
            false
        };

        legend::render_legend(
            f,
            legend_area,
            app.model.ui.vim_mode,
            app.model.navigation.focus_level,
            can_restore,
            app.open_command.is_some(),
        );
    }

    // Render status bar at the bottom
    let (breadcrumb_folder_label, breadcrumb_item_count, breadcrumb_selected_item) =
        if app.model.navigation.focus_level > 0 {
            let level_idx = app.model.navigation.focus_level - 1;
            if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
                let folder_label = Some(level.folder_label.as_str());
                let item_count = Some(level.items.len());
                let selected_item = level.selected_index.and_then(|sel| {
                    level.items.get(sel).map(|item| {
                        let sync_state = level.file_sync_states.get(&item.name).copied();
                        let is_ignored = sync_state == Some(crate::api::SyncState::Ignored);
                        let exists = if is_ignored {
                            level.ignored_exists.get(&item.name).copied()
                        } else {
                            None
                        };
                        (
                            item.name.as_str(),
                            item.item_type.as_str(),
                            sync_state,
                            exists,
                        )
                    })
                });
                (folder_label, item_count, selected_item)
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

    // Calculate pending operations count (total paths across all folders)
    let pending_operations_count: usize = app
        .model.performance.pending_ignore_deletes
        .values()
        .map(|info| info.paths.len())
        .sum();

    status_bar::render_status_bar(
        f,
        layout_info.status_area,
        app.model.navigation.focus_level,
        &app.model.syncthing.folders,
        &app.model.syncthing.folder_statuses,
        app.model.navigation.folders_state_selection,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        app.model.ui.sort_mode.as_str(),
        app.model.ui.sort_reverse,
        app.model.performance.last_load_time_ms,
        app.model.performance.cache_hit,
        pending_operations_count,
    );

    // Render confirmation dialogs if active
    if let Some((_folder_id, changed_files)) = &app.model.ui.confirm_revert {
        dialogs::render_revert_confirmation(f, changed_files);
    }

    if let Some((_host_path, display_name, is_dir)) = &app.model.ui.confirm_delete {
        dialogs::render_delete_confirmation(f, display_name, *is_dir);
    }

    if let Some(pattern_state) = &mut app.model.ui.pattern_selection {
        // Create temporary ListState for rendering
        let mut temp_state = ratatui::widgets::ListState::default();
        temp_state.select(pattern_state.selected_index);
        dialogs::render_pattern_selection(f, &pattern_state.patterns, &mut temp_state);
        // Sync back the selection
        pattern_state.selected_index = temp_state.selected();
    }

    // Render file info popup if active
    if let Some(state) = &mut app.model.ui.file_info_popup {
        let my_device_id = app.model.syncthing.system_status.as_ref().map(|s| s.my_id.as_str());
        dialogs::render_file_info(
            f,
            state,
            &app.model.syncthing.devices,
            my_device_id,
            &app.icon_renderer,
            app.model.ui.image_font_size,
            &mut app.image_state_map,
        );
    }

    // Render toast notification if active
    if let Some((message, _timestamp)) = &app.model.ui.toast_message {
        toast::render_toast(f, size, message);
    }
}
