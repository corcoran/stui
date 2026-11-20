use crate::App;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use stui::DisplayMode;

use super::{
    breadcrumb, dialogs, folder_list, layout, legend, search, status_bar, system_bar, toast,
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
    let (
        breadcrumb_folder_label,
        breadcrumb_folder_id,
        breadcrumb_item_count,
        breadcrumb_selected_item,
    ) = if app.model.navigation.focus_level > 0 {
        let level_idx = app.model.navigation.focus_level - 1;
        if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
            let folder_label = Some(level.folder_label.clone());
            let folder_id = Some(level.folder_id.clone());
            // Use filtered_items if active, otherwise use all items (same as breadcrumb rendering)
            let display_items = level.filtered_items.as_ref().unwrap_or(&level.items);
            let item_count = Some(display_items.len());
            let syncing = app.model.syncthing.syncing_files.get(&level.folder_id);
            let selected_item = level.selected_index.and_then(|sel| {
                display_items.get(sel).map(|item| {
                    let sync_state = crate::logic::sync_states::item_sync_state(
                        level.file_sync_states.get(&item.name).copied(),
                        level.prefix.as_deref(),
                        item,
                        syncing,
                    );
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
            (folder_label, folder_id, item_count, selected_item)
        } else {
            (None, None, None, None)
        }
    } else {
        (None, None, None, None)
    };

    let pending_operations_count: usize = app
        .model
        .performance
        .pending_ignore_deletes
        .values()
        .map(|info| info.paths.len())
        .sum();

    // Calculate dynamic status bar height
    let out_of_sync_filter_active = app.model.ui.out_of_sync_filter.is_some();
    let out_of_sync_filter_progress = breadcrumb_folder_id
        .as_ref()
        .filter(|_| out_of_sync_filter_active)
        .and_then(|id| app.model.syncthing.need_paging.get(id))
        .and_then(crate::logic::need_paging::need_paging_progress);
    let status_height = status_bar::calculate_status_height(
        size.width,
        &app.icon_renderer,
        app.model.navigation.focus_level,
        &app.model.syncthing.folders,
        &app.model.syncthing.folder_statuses,
        app.model.navigation.folders_state_selection,
        breadcrumb_folder_label.clone(),
        breadcrumb_folder_id.clone(),
        breadcrumb_item_count,
        breadcrumb_selected_item.clone(),
        app.model.ui.sort_mode.as_str(),
        app.model.ui.sort_reverse,
        app.model.performance.last_load_time_ms,
        app.model.performance.cache_hit,
        pending_operations_count,
        out_of_sync_filter_active,
        out_of_sync_filter_progress,
        &app.model.ui.folder_activity,
        &app.model.syncthing.last_folder_updates,
        app.model.syncthing.connected_device_count,
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
    if layout_info.folders_visible
        && let Some(folders_area) = layout_info.folders_area
    {
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

        // In-flight transfers are overlaid here only; level.file_sync_states stays the base
        let sync_states = crate::logic::sync_states::overlay_syncing_states(
            &level.items,
            &level.file_sync_states,
            level.prefix.as_deref(),
            app.model.syncthing.syncing_files.get(&level.folder_id),
        );

        // Create temporary ListState for rendering
        let mut temp_state = ratatui::widgets::ListState::default();
        temp_state.select(level.selected_index);
        breadcrumb::render_breadcrumb_panel(
            f,
            area,
            &level.items,                  // Unfiltered source
            level.filtered_items.as_ref(), // Filtered view (if active)
            &sync_states,
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
                .map(|level| level.display_items().len()) // Use display_items to respect filtering
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
        breadcrumb_folder_id,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        app.model.ui.sort_mode.as_str(),
        app.model.ui.sort_reverse,
        app.model.performance.last_load_time_ms,
        app.model.performance.cache_hit,
        pending_operations_count,
        out_of_sync_filter_active,
        out_of_sync_filter_progress,
        &app.model.ui.folder_activity,
        &app.model.syncthing.last_folder_updates,
        app.model.syncthing.connected_device_count,
    );

    // Everything drawn from here on is an overlay; recede what is behind it
    maybe_dim_background(f.buffer_mut(), size, &app.model);

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
            crate::model::ConfirmAction::PauseResume {
                label, is_paused, ..
            } => {
                dialogs::render_pause_resume_confirmation(f, label, *is_paused);
            }
            crate::model::ConfirmAction::Rescan { folder_label, .. } => {
                dialogs::render_rescan_confirmation(f, folder_label);
            }
        }
    }

    // Render setup help dialog if active
    if app.model.ui.show_setup_help {
        // Get error message from connection state
        let error_message = match &app.model.syncthing.connection_state {
            crate::model::syncthing::ConnectionState::Disconnected { message, .. } => {
                message.as_str()
            }
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
        let my_device_id = app
            .model
            .syncthing
            .system_status
            .as_ref()
            .map(|s| s.my_id.as_str());
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

    // Render folder history modal (if active)
    if let Some(ref modal_state) = app.model.ui.folder_history_modal {
        crate::ui::folder_history::render_folder_history_modal(
            f,
            size,
            modal_state,
            &app.icon_renderer,
        );
    }

    // Render toast notification if active
    if let Some((message, _timestamp)) = &app.model.ui.toast_message {
        toast::render_toast(f, size, message);
    }
}

/// Style applied to everything behind an open modal.
/// Resets bg and strips every modifier except DIM so selection bars and bold/reversed
/// text recede along with the rest.
fn dim_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .bg(Color::Reset)
        .add_modifier(Modifier::DIM)
        .remove_modifier(Modifier::all() - Modifier::DIM)
}

/// Dim the background when a modal is open; no-op otherwise.
fn maybe_dim_background(buf: &mut Buffer, area: Rect, model: &crate::model::Model) {
    if crate::logic::ui::modal_open(model) {
        dim_background(buf, area);
    }
}

/// Dim every cell in `area`. Overlays drawn afterwards keep their own styling.
fn dim_background(buf: &mut Buffer, area: Rect) {
    buf.set_style(area, dim_style());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    // ========================================
    // DIM BACKGROUND
    // ========================================

    fn is_dimmed(buf: &Buffer, x: u16, y: u16) -> bool {
        let cell = &buf[(x, y)];
        cell.fg == Color::DarkGray
            && cell.bg == Color::Reset
            && cell.modifier.contains(Modifier::DIM)
    }

    #[test]
    fn test_dim_background_dims_every_cell_and_keeps_text() {
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "hello", Style::default().fg(Color::Green));
        dim_background(&mut buf, area);
        for y in 0..3 {
            for x in 0..10 {
                assert!(is_dimmed(&buf, x, y), "cell ({x},{y}) not dimmed");
            }
        }
        assert_eq!(buf[(0, 0)].symbol(), "h");
    }

    #[test]
    fn test_dim_background_overrides_highlight_bg_and_modifiers() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(
            0,
            0,
            "selected",
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        );
        dim_background(&mut buf, area);
        assert_eq!(buf[(0, 0)].bg, Color::Reset);
        assert!(!buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(0, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_popup_drawn_after_dim_keeps_normal_style() {
        let backend = TestBackend::new(30, 9);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let full = f.area();
                f.render_widget(
                    Paragraph::new("background text everywhere")
                        .style(Style::default().fg(Color::Cyan)),
                    full,
                );
                dim_background(f.buffer_mut(), full);
                let popup = Rect::new(10, 3, 10, 3);
                f.render_widget(Clear, popup);
                f.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Yellow)),
                    popup,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(is_dimmed(buf, 0, 0));
        assert!(is_dimmed(buf, 29, 8));
        assert!(!is_dimmed(buf, 10, 3));
        assert_eq!(buf[(10, 3)].fg, Color::Yellow);
        assert!(!buf[(10, 3)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_no_modal_leaves_buffer_untouched() {
        let model = crate::model::Model::new(false);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "text", Style::default().fg(Color::Green));
        maybe_dim_background(&mut buf, area, &model);
        assert_eq!(buf[(0, 0)].fg, Color::Green);
        assert!(!buf[(0, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_modal_open_dims_buffer_via_maybe_dim() {
        let mut model = crate::model::Model::new(false);
        model.ui.show_setup_help = true;
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "text", Style::default().fg(Color::Green));
        maybe_dim_background(&mut buf, area, &model);
        assert!(is_dimmed(&buf, 0, 0));
    }

    #[test]
    fn test_toast_only_does_not_dim_via_maybe_dim() {
        let mut model = crate::model::Model::new(false);
        model.ui.show_toast("hi".to_string());
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "text", Style::default().fg(Color::Green));
        maybe_dim_background(&mut buf, area, &model);
        assert_eq!(buf[(0, 0)].fg, Color::Green);
    }
}
