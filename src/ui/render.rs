use crate::App;
use ratatui::Frame;
use synctui::DisplayMode;

use super::{
    breadcrumb, dialogs, folder_list, layout, legend, out_of_sync_summary, search, status_bar,
    system_bar, toast,
};

/// Main render function - orchestrates all UI rendering
/// This replaces the large terminal.draw() closure in main.rs
pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Calculate layout (with legend parameters for dynamic height)
    let has_breadcrumbs = !app.model.navigation.breadcrumb_trail.is_empty();

    // Check if restore is available (needed for legend height calculation)
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

    // Calculate status bar data (needed for status height calculation)
    // Clone strings to avoid borrowing issues
    let (breadcrumb_folder_label, breadcrumb_item_count, breadcrumb_selected_item) =
        if app.model.navigation.focus_level > 0 {
            let level_idx = app.model.navigation.focus_level - 1;
            if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
                let folder_label = Some(level.folder_label.clone());
                // Use filtered_items if active, otherwise use all items (same as breadcrumb rendering)
                let display_items = level.filtered_items.as_ref().unwrap_or(&level.items);
                let item_count = Some(display_items.len());
                let selected_item = level.selected_index.and_then(|sel| {
                    display_items.get(sel).map(|item| {
                        let sync_state = level.file_sync_states.get(&item.name).copied();
                        let is_ignored = sync_state == Some(crate::api::SyncState::Ignored);
                        let exists = if is_ignored {
                            level.ignored_exists.get(&item.name).copied()
                        } else {
                            None
                        };
                        (
                            item.name.clone(),
                            item.item_type.clone(),
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

    let pending_operations_count: usize = app
        .model.performance.pending_ignore_deletes
        .values()
        .map(|info| info.paths.len())
        .sum();

    // Calculate dynamic status bar height
    let out_of_sync_filter_active = app.model.ui.out_of_sync_filter.is_some();
    let status_height = status_bar::calculate_status_height(
        size.width,
        &app.icon_renderer,
        app.model.navigation.focus_level,
        &app.model.syncthing.folders,
        &app.model.syncthing.folder_statuses,
        app.model.navigation.folders_state_selection,
        breadcrumb_folder_label.clone(),
        breadcrumb_item_count,
        breadcrumb_selected_item.clone(),
        app.model.ui.sort_mode.as_str(),
        app.model.ui.sort_reverse,
        app.model.performance.last_load_time_ms,
        app.model.performance.cache_hit,
        pending_operations_count,
        out_of_sync_filter_active,
    );

    // Determine if search should be visible
    let search_visible = app.model.ui.search_mode || !app.model.ui.search_query.is_empty();

    let layout_info = layout::calculate_layout(
        size,
        app.model.navigation.breadcrumb_trail.len(),
        has_breadcrumbs,
        app.model.ui.vim_mode,
        app.model.navigation.focus_level,
        can_restore,
        app.open_command.is_some(),
        status_height,
        search_visible,
    );

    // Render system info bar at the top
    let (total_files, total_dirs, total_bytes) = app.get_local_state_summary();
    system_bar::render_system_bar(
        f,
        layout_info.system_area,
        &app.model.syncthing.connection_state,
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
            &level.items,                       // Unfiltered source
            level.filtered_items.as_ref(),      // Filtered view (if active)
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

    // Render search input if visible
    if let Some(search_area) = layout_info.search_area {
        let match_count = if app.model.navigation.focus_level > 0 {
            let level_idx = app.model.navigation.focus_level - 1;
            app.model
                .navigation
                .breadcrumb_trail
                .get(level_idx)
                .map(|level| level.items.len())
        } else {
            None
        };

        search::render_search_input(
            f,
            search_area,
            &app.model.ui.search_query,
            app.model.ui.search_mode,
            match_count,
            app.model.ui.vim_mode,
        );
    }

    // Render hotkey legend if there's space
    if let Some(legend_area) = layout_info.legend_area {
        legend::render_legend(
            f,
            legend_area,
            app.model.ui.vim_mode,
            app.model.navigation.focus_level,
            can_restore,
            app.open_command.is_some(),
            app.model.ui.search_mode,
            !app.model.ui.search_query.is_empty(),
        );
    }

    // Render status bar at the bottom (using data calculated earlier for height)
    status_bar::render_status_bar(
        f,
        layout_info.status_area,
        &app.icon_renderer,
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
        out_of_sync_filter_active,
    );

    // Render confirmation dialogs if active
    if let Some(action) = &app.model.ui.confirm_action {
        match action {
            crate::model::ConfirmAction::Revert { changed_files, .. } => {
                dialogs::render_revert_confirmation(f, changed_files);
            }
            crate::model::ConfirmAction::Delete { name, is_dir, .. } => {
                dialogs::render_delete_confirmation(f, name, *is_dir);
            }
            crate::model::ConfirmAction::IgnoreDelete { name, is_dir, .. } => {
                // Not implemented - would render ignore+delete confirmation
                dialogs::render_delete_confirmation(f, name, *is_dir);
            }
            crate::model::ConfirmAction::PauseResume { label, is_paused, .. } => {
                dialogs::render_pause_resume_confirmation(f, label, *is_paused);
            }
        }
    }

    // Render setup help dialog if active
    if app.model.ui.show_setup_help {
        // Get error message from connection state
        let error_message = match &app.model.syncthing.connection_state {
            crate::model::syncthing::ConnectionState::Disconnected { message, .. } => message.as_str(),
            _ => "Unknown error",
        };
        dialogs::render_setup_help(f, error_message, &app.model.ui.config_path);
    }

    if let Some(pattern_state) = &mut app.model.ui.pattern_selection {
        // Create temporary ListState for rendering
        let mut temp_state = ratatui::widgets::ListState::default();
        temp_state.select(pattern_state.selected_index);
        dialogs::render_pattern_selection(f, &pattern_state.patterns, &mut temp_state);
        // Sync back the selection
        pattern_state.selected_index = temp_state.selected();
    }

    if let Some(type_state) = &mut app.model.ui.folder_type_selection {
        // Create temporary ListState for rendering
        let mut temp_state = ratatui::widgets::ListState::default();
        temp_state.select(Some(type_state.selected_index));
        dialogs::render_folder_type_selection(
            f,
            &type_state.folder_label,
            &type_state.current_type,
            &mut temp_state,
        );
        // Sync back the selection (though it's managed by our own index)
        if let Some(selected) = temp_state.selected() {
            type_state.selected_index = selected;
        }
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

    // Render out-of-sync summary modal if active
    if let Some(summary_state) = &app.model.ui.out_of_sync_summary {
        out_of_sync_summary::render_out_of_sync_summary(
            f,
            size,
            &app.model.syncthing.folders,
            summary_state,
            &app.icon_renderer,
        );
    }

    // Render toast notification if active
    if let Some((message, _timestamp)) = &app.model.ui.toast_message {
        toast::render_toast(f, size, message);
    }
}
