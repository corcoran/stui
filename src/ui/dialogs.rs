use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

/// Columns a preview popup pane spends on its borders and horizontal padding
const PANE_CHROME: u16 = 4;

use super::icons::IconRenderer;
use crate::api::LocalChangedFile;
use crate::model::FileInfoPopupState;
use crate::utils;
use crate::{ImagePreviewState, api::Device};

/// Create a centered rectangle within the given area
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: (area.width.saturating_sub(width)) / 2,
        y: (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Render the revert confirmation dialog (for restoring deleted files in receive-only folders)
pub fn render_revert_confirmation(f: &mut Frame, changed_files: &[LocalChangedFile]) {
    let file_list = changed_files
        .iter()
        .take(5)
        .map(|file| format!("  - {}", file.name))
        .collect::<Vec<_>>()
        .join("\n");

    let more_text = if changed_files.len() > 5 {
        format!("\n  ... and {} more", changed_files.len() - 5)
    } else {
        String::new()
    };

    let prompt_text = format!(
        "Revert folder to restore deleted files?\n\n\
        WARNING: This will remove {} local change(s):\n{}{}\n\n\
        Continue? (y/n)",
        changed_files.len(),
        file_list,
        more_text
    );

    // Center the prompt - adjust height based on number of files shown
    let area = f.area();
    let prompt_width = 60;
    let base_height = 10;
    let file_lines = changed_files.len().min(5);
    let prompt_height = base_height + file_lines as u16;
    let prompt_area = centered_rect(area, prompt_width, prompt_height);

    let prompt = Paragraph::new(prompt_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm Revert")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(prompt, prompt_area);
}

/// Render the delete confirmation dialog
pub fn render_delete_confirmation(f: &mut Frame, display_name: &str, is_dir: bool) {
    let item_type = if is_dir { "directory" } else { "file" };

    let prompt_text = format!(
        "Delete {} from disk?\n\n\
        {}: {}\n\n\
        WARNING: This action cannot be undone!\n\n\
        Continue? (y/n)",
        item_type,
        if is_dir { "Directory" } else { "File" },
        display_name
    );

    // Center the prompt
    let area = f.area();
    let prompt_width = 50;
    let prompt_height = 11;
    let prompt_area = centered_rect(area, prompt_width, prompt_height);

    let prompt = Paragraph::new(prompt_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirm Delete")
                .border_style(Style::default().fg(Color::Red)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(prompt, prompt_area);
}

/// Render pause/resume folder confirmation dialog
pub fn render_pause_resume_confirmation(f: &mut Frame, folder_label: &str, is_paused: bool) {
    let action = if is_paused { "Resume" } else { "Pause" };
    let action_lower = if is_paused { "resume" } else { "pause" };

    let prompt_text = format!(
        "{} folder?\n\n\
        Folder: {}\n\n\
        This will {} syncing for this folder.\n\n\
        Continue? (y/n)",
        action, folder_label, action_lower
    );

    // Center the prompt
    let area = f.area();
    let prompt_width = 50;
    let prompt_height = 10;
    let prompt_area = centered_rect(area, prompt_width, prompt_height);

    let border_color = if is_paused {
        Color::Green
    } else {
        Color::Yellow
    };

    let prompt = Paragraph::new(prompt_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Confirm {}", action))
                .border_style(Style::default().fg(border_color)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(prompt, prompt_area);
}

/// Render the pattern selection menu (for removing ignore patterns)
pub fn render_pattern_selection(f: &mut Frame, patterns: &[String], state: &mut ListState) {
    let menu_items: Vec<ListItem> = patterns
        .iter()
        .map(|pattern| {
            ListItem::new(Span::raw(pattern.clone())).style(Style::default().fg(Color::White))
        })
        .collect();

    // Center the menu
    let area = f.area();
    let menu_width = 60;
    let menu_height = (patterns.len() as u16 + 6).min(20); // +6 for borders and instructions
    let menu_area = Rect {
        x: (area.width.saturating_sub(menu_width)) / 2,
        y: (area.height.saturating_sub(menu_height)) / 2,
        width: menu_width,
        height: menu_height,
    };

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Select Pattern to Remove (↑↓ to navigate, Enter to remove, Esc to cancel)")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("► ");

    f.render_widget(ratatui::widgets::Clear, menu_area);
    f.render_stateful_widget(menu, menu_area, state);
}

/// Render the folder type selection menu
pub fn render_folder_type_selection(
    f: &mut Frame,
    folder_label: &str,
    current_type: &str,
    state: &mut ListState,
) {
    let types = [
        ("Send Only", "sendonly"),
        ("Send & Receive", "sendreceive"),
        ("Receive Only", "receiveonly"),
    ];

    let menu_items: Vec<ListItem> = types
        .iter()
        .map(|(display_name, api_name)| {
            let mut style = Style::default().fg(Color::White);
            // Highlight current type
            if *api_name == current_type {
                style = style.add_modifier(Modifier::ITALIC).fg(Color::Cyan);
            }
            ListItem::new(Span::styled(*display_name, style))
        })
        .collect();

    // Center the menu
    let area = f.area();
    let menu_width = 50;
    let menu_height = 11; // 3 items + borders + title + instructions
    let menu_area = Rect {
        x: (area.width.saturating_sub(menu_width)) / 2,
        y: (area.height.saturating_sub(menu_height)) / 2,
        width: menu_width,
        height: menu_height,
    };

    let title = format!("Change Folder Type: {}", folder_label);
    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom("↑↓ to navigate, Enter to select, Esc to cancel")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("► ");

    f.render_widget(ratatui::widgets::Clear, menu_area);
    f.render_stateful_widget(menu, menu_area, state);
}

fn centered_menu_area(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Folder group menu: existing groups, "No group" and "New group…"
pub fn render_folder_group_menu(f: &mut Frame, menu: &crate::model::types::FolderGroupMenuState) {
    use crate::model::types::GroupMenuOption;
    let items: Vec<ListItem> = menu
        .options
        .iter()
        .map(|option| {
            let (label, is_current) = match option {
                GroupMenuOption::Group(name) => (name.clone(), *name == menu.current_group),
                GroupMenuOption::NoGroup => ("No group".to_string(), menu.current_group.is_empty()),
                GroupMenuOption::NewGroup => ("New group…".to_string(), false),
            };
            if is_current {
                ListItem::new(Span::styled(
                    format!("{}  (current)", label),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                ))
            } else {
                ListItem::new(Span::styled(label, Style::default().fg(Color::White)))
            }
        })
        .collect();

    let height = (menu.options.len() as u16).saturating_add(2).max(5);
    let area = centered_menu_area(f.area(), 50, height);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Folder group: {}", menu.folder_label))
                .title_bottom("↑↓ select, Enter apply, Esc cancel")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("► ");
    let mut state = ListState::default();
    state.select(Some(menu.selected_index));
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_stateful_widget(list, area, &mut state);
}

/// One-line input for a new group name
pub fn render_group_name_input(f: &mut Frame, input: &crate::model::types::GroupNameInputState) {
    let area = centered_menu_area(f.area(), 50, 4);
    let mut lines = vec![Line::from(format!("{}█", input.text))];
    if input.show_empty_hint {
        lines.push(Line::from(Span::styled(
            "Name can't be empty",
            Style::default().fg(Color::Yellow),
        )));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("New group for {}", input.menu.folder_label))
            .title_bottom("Enter save, Esc back")
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(paragraph, area);
}

/// Text shown in place of file content. The locally deleted explanation is guidance, not an
/// error, so it has no "Error:" prefix.
fn preview_message_text(msg: &str) -> String {
    if msg == crate::logic::sync_states::locally_deleted_message() {
        msg.to_string()
    } else {
        format!("Error: {}", msg)
    }
}

/// Render the file information popup with metadata and preview
#[allow(clippy::too_many_arguments)]
pub fn render_file_info(
    f: &mut Frame,
    state: &mut FileInfoPopupState,
    devices: &[Device],
    my_device_id: Option<&str>,
    icon_renderer: &IconRenderer,
    image_font_size: Option<(u16, u16)>,
    image_state_map: &mut std::collections::HashMap<String, crate::ImagePreviewState>,
    vim_mode: bool,
) {
    // Calculate centered area (90% width, 90% height)
    let area = f.area();
    let popup_width = (area.width as f32 * 0.9) as u16;
    let popup_height = (area.height as f32 * 0.9) as u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    // Clear background
    f.render_widget(ratatui::widgets::Clear, popup_area);

    // Split into two columns - metadata gets reasonable fixed width, preview gets the rest
    // Use 35 chars for metadata (should fit most filenames without wrapping)
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(35),
            Constraint::Min(50), // Preview gets at least 50 chars
        ])
        .split(popup_area);

    // Render metadata column
    render_metadata_column(
        f,
        columns[0],
        state,
        devices,
        my_device_id,
        icon_renderer,
        image_state_map,
        vim_mode,
    );

    // Render preview column
    render_preview_column(f, columns[1], state, image_font_size, image_state_map);
}

#[allow(clippy::too_many_arguments)]
fn render_metadata_column(
    f: &mut Frame,
    area: Rect,
    state: &mut FileInfoPopupState,
    devices: &[Device],
    my_device_id: Option<&str>,
    icon_renderer: &IconRenderer,
    image_state_map: &mut std::collections::HashMap<String, crate::ImagePreviewState>,
    vim_mode: bool,
) {
    let mut lines = vec![];

    // Name
    lines.push(Line::from(vec![
        Span::styled("Name: ", Style::default().fg(Color::Yellow)),
        Span::raw(&state.browse_item.name),
    ]));

    // Type
    let item_type = if state.browse_item.item_type == "directory" {
        "Directory"
    } else {
        "File"
    };
    lines.push(Line::from(vec![
        Span::styled("Type: ", Style::default().fg(Color::Yellow)),
        Span::raw(item_type),
    ]));

    // Size
    lines.push(Line::from(vec![
        Span::styled("Size: ", Style::default().fg(Color::Yellow)),
        Span::raw(utils::format_bytes(state.browse_item.size)),
    ]));

    // Modified time
    lines.push(Line::from(vec![
        Span::styled("Modified: ", Style::default().fg(Color::Yellow)),
        Span::raw(crate::logic::formatting::format_datetime(
            &state.browse_item.mod_time,
        )),
    ]));

    // Image resolution (if this is an image with loaded metadata)
    if state.is_image
        && let Some(
            crate::ImagePreviewState::Ready { metadata, .. }
            | crate::ImagePreviewState::Failed { metadata },
        ) = image_state_map.get(&state.file_path)
        && let Some((width, height)) = metadata.dimensions
    {
        lines.push(Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}x{}", width, height)),
        ]));
    }

    lines.push(Line::from(""));

    // File details from API
    if let Some(details) = &state.file_details {
        // Local state
        if let Some(local) = &details.local {
            lines.push(Line::from(vec![
                Span::styled("State (Local): ", Style::default().fg(Color::Yellow)),
                Span::raw(if local.deleted { "Deleted" } else { "Present" }),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Ignored: ", Style::default().fg(Color::Yellow)),
                Span::raw(if local.ignored { "Yes" } else { "No" }),
            ]));

            if !local.permissions.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Permissions: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&local.permissions),
                ]));
            }

            if !local.modified_by.is_empty() {
                // Try to find the device name
                let device_name =
                    crate::logic::formatting::device_name_by_short_id(devices, &local.modified_by)
                        .unwrap_or_else(|| {
                            &local.modified_by[..std::cmp::min(12, local.modified_by.len())]
                        });

                lines.push(Line::from(vec![
                    Span::styled("Modified By: ", Style::default().fg(Color::Yellow)),
                    Span::raw(device_name),
                ]));
            }
        }

        lines.push(Line::from(""));

        // Sync status comparison (more user-friendly than sequence numbers)
        if let Some(local) = &details.local
            && let Some(global) = &details.global
        {
            let (status_text, sync_state) = if local.sequence == global.sequence {
                ("In Sync", crate::api::SyncState::Synced)
            } else if local.sequence < global.sequence {
                ("Behind (needs update)", crate::api::SyncState::OutOfSync)
            } else {
                ("Ahead (local changes)", crate::api::SyncState::OutOfSync)
            };

            // Get icon span from icon_renderer (returns [file_icon, status_icon])
            let icon_spans = icon_renderer.item_with_sync_state(false, sync_state);

            let mut status_spans = vec![Span::styled(
                "Sync Status: ",
                Style::default().fg(Color::Yellow),
            )];
            // Add just the status icon (second element, skip the file icon)
            if icon_spans.len() > 1 {
                status_spans.push(icon_spans[1].clone());
            }
            status_spans.push(Span::raw(status_text));

            lines.push(Line::from(status_spans));
        }

        lines.push(Line::from(""));

        // Device availability (only shows connected/online devices)
        let other_devices: Vec<_> = details
            .availability
            .iter()
            .filter(|d| Some(d.id.as_str()) != my_device_id) // Filter out current device
            .collect();

        if !other_devices.is_empty() {
            lines.push(Line::from(Span::styled(
                "Available on (connected):",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for device_avail in other_devices {
                // Try to find the device name
                let device_name = devices
                    .iter()
                    .find(|d| d.id == device_avail.id)
                    .map(|d| d.name.as_str())
                    .unwrap_or_else(|| {
                        &device_avail.id[..std::cmp::min(12, device_avail.id.len())]
                    });

                lines.push(Line::from(format!("  • {}", device_name)));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "Available on: Only this device",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "API details not available",
            Style::default().fg(Color::Gray),
        )));
    }

    lines.push(Line::from(""));

    // Disk status
    lines.push(Line::from(vec![
        Span::styled("Exists on Disk: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            if state.exists_on_disk { "Yes" } else { "No" },
            if state.exists_on_disk {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            },
        ),
    ]));

    if state.is_binary {
        lines.push(Line::from(Span::styled(
            format!("{} Binary file", icon_renderer.render_warning()),
            Style::default().fg(Color::Magenta),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1))
                .border_style(Style::default().fg(Color::Cyan))
                .title("Metadata")
                .title_bottom(if vim_mode {
                    "J/K: next/prev · o: open"
                } else {
                    "Shift+↑↓: next/prev · o: open"
                }),
        )
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_preview_column(
    f: &mut Frame,
    area: Rect,
    state: &mut FileInfoPopupState,
    image_font_size: Option<(u16, u16)>,
    image_state_map: &mut std::collections::HashMap<String, crate::ImagePreviewState>,
) {
    // Check if this is an image with an image state
    if state.is_image {
        if let Some(image_state) = image_state_map.get(&state.file_path) {
            match image_state {
                ImagePreviewState::Loading => {
                    // Show loading message
                    let paragraph = Paragraph::new("Loading image...")
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .padding(Padding::horizontal(1))
                                .border_style(Style::default().fg(Color::Cyan))
                                .title("Preview"),
                        )
                        .style(Style::default().fg(Color::Yellow));
                    f.render_widget(paragraph, area);
                }
                ImagePreviewState::Ready { .. } => {
                    // Get mutable reference to protocol for rendering
                    if let Some(&mut ImagePreviewState::Ready {
                        ref mut protocol,
                        ref metadata,
                    }) = image_state_map.get_mut(&state.file_path)
                    {
                        // Create bordered block with dimensions in title
                        let title = if let Some((w, h)) = metadata.dimensions {
                            format!("Preview (Image | {}x{})", w, h)
                        } else {
                            "Preview (Image)".to_string()
                        };

                        let block = Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(title);

                        // Render block first
                        f.render_widget(block, area);

                        // Render image with proper centering using font_size
                        let inner_area = area.inner(Margin {
                            horizontal: 1,
                            vertical: 1,
                        });

                        let render_rect = crate::logic::layout::fit_image_rect(
                            inner_area,
                            metadata.dimensions,
                            image_font_size,
                        );

                        // Adaptive filter for protocol's final resize
                        // Pre-downscale already handled most of the work, so we can use faster filters
                        let filter = if let Some((img_w, img_h)) = metadata.dimensions {
                            // Calculate if this is still a large downscale
                            let width_ratio = img_w as f32 / render_rect.width as f32;
                            let height_ratio = img_h as f32 / render_rect.height as f32;
                            let max_ratio = width_ratio.max(height_ratio);

                            if max_ratio > 3.0 {
                                // Still large downscale: use Triangle for speed
                                image::imageops::FilterType::Triangle
                            } else {
                                // Moderate/small: use Lanczos3 for quality
                                image::imageops::FilterType::Lanczos3
                            }
                        } else {
                            // No dimensions: use CatmullRom as balanced default
                            image::imageops::FilterType::CatmullRom
                        };

                        let image = ratatui_image::StatefulImage::new()
                            .resize(ratatui_image::Resize::Fit(Some(filter)));
                        f.render_stateful_widget(image, render_rect, protocol.as_mut());
                    }
                }
                ImagePreviewState::Failed { metadata } => {
                    // Show image &metadata as fallback
                    render_image_metadata(f, area, metadata);
                }
            }
        } else {
            // Image but no state yet - show loading
            let paragraph = Paragraph::new("Loading image...")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .padding(Padding::horizontal(1))
                        .border_style(Style::default().fg(Color::Cyan))
                        .title("Preview"),
                )
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(paragraph, area);
        }
    } else {
        // Original text preview logic
        let content_str = match &state.file_content {
            Ok(text) => text.clone(),
            Err(msg) => format!("Error: {}", msg),
        };

        let art = state.art.filter(|_| state.file_content.is_ok());
        let is_ansi_art = art.is_some();
        let canvas_width: u16 = art.map_or(crate::logic::file::DEFAULT_ART_WIDTH, |a| a.width);

        // Art gets a box as wide as its canvas
        use ratatui::layout::{Constraint, Direction, Layout};

        let preview_area = if is_ansi_art {
            let fixed_width = canvas_width + PANE_CHROME;
            if area.width > fixed_width {
                // Center the container horizontally
                let horizontal_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(0),              // Left padding
                        Constraint::Length(fixed_width), // Fixed width for ANSI art
                        Constraint::Min(0),              // Right padding
                    ])
                    .split(area);
                horizontal_chunks[1]
            } else {
                // Terminal too narrow, use full width
                area
            }
        } else {
            // Not ANSI art - use full width
            area
        };

        let inner_width = preview_area.width.saturating_sub(PANE_CHROME);
        let view = crate::logic::file::rendered_view(&state.file_path)
            .filter(|_| state.file_content.is_ok());
        let archive = state
            .archive
            .as_ref()
            .filter(|_| state.file_content.is_ok());
        let parsed_text = if let Err(msg) = &state.file_content {
            // Plain text so the paragraph word-wraps it
            ratatui::text::Text::from(preview_message_text(msg))
        } else if let Some(listing) = archive {
            crate::logic::archive::listing_text(listing)
        } else if let (Some(view), false) = (view, state.raw_view) {
            crate::logic::formatted::render(view, &content_str, inner_width)
        } else {
            crate::logic::file::parse_ansi_to_text(&content_str, canvas_width)
        };

        // Art and tables are not wrapped, so each parsed row is one screen row
        let wrap = !is_ansi_art
            && view.is_none_or(|v| {
                state.raw_view
                    || state.file_content.is_err()
                    || crate::logic::formatted::wraps(v, &content_str)
            });
        let total_lines = if !wrap {
            parsed_text.lines.len()
        } else {
            crate::logic::layout::wrapped_line_count(&parsed_text, inner_width)
        };
        let viewport_height = area.height.saturating_sub(2) as usize; // -2 for borders
        let max_scroll = total_lines.saturating_sub(viewport_height);
        let clamped_scroll = (state.scroll_offset as usize).min(max_scroll) as u16;
        // Write the clamped value back to state to prevent offset drift
        state.scroll_offset = clamped_scroll;

        let scroll_keys = "↑↓/j/k, ^d/^u, ^f/^b, gg/G, PgUp/PgDn";
        let title = match (view, state.raw_view) {
            _ if archive.is_some() => format!("Preview (Archive | {})", scroll_keys),
            (Some(v), false) => format!("Preview ({}, r: raw | {})", v.label(), scroll_keys),
            (Some(v), true) => {
                format!("Preview (Raw, r: {} | {})", v.rendered_word(), scroll_keys)
            }
            (None, _) => format!("Preview ({})", scroll_keys),
        };

        // Spans patch over this base style, so the art's own colors still win
        let base_style = if is_ansi_art {
            Style::default()
                .fg(crate::logic::file::ANSI_ART_FG)
                .bg(crate::logic::file::ANSI_ART_BG)
        } else {
            Style::default()
        };
        let mut paragraph = Paragraph::new(parsed_text)
            .style(base_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::horizontal(1))
                    .border_style(Style::default().fg(Color::Cyan).bg(Color::Reset))
                    .title(title),
            )
            .scroll((clamped_scroll, 0));
        if wrap {
            paragraph = paragraph.wrap(ratatui::widgets::Wrap { trim: false });
        }

        f.render_widget(paragraph, preview_area);

        // Render scrollbar if content is longer than viewport
        if total_lines > viewport_height {
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll).position(clamped_scroll as usize);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"))
                .track_symbol(Some("│"))
                .thumb_symbol("█");

            f.render_stateful_widget(
                scrollbar,
                preview_area.inner(Margin {
                    horizontal: 0,
                    vertical: 1,
                }), // Keep scrollbar inside borders
                &mut scrollbar_state,
            );
        }
    }
}

fn render_image_metadata(f: &mut Frame, area: Rect, metadata: &crate::ImageMetadata) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Image preview unavailable",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // Format
    if let Some(format) = &metadata.format {
        lines.push(Line::from(vec![
            Span::styled("Format: ", Style::default().fg(Color::Cyan)),
            Span::raw(format),
        ]));
    }

    // Dimensions
    if let Some((width, height)) = metadata.dimensions {
        lines.push(Line::from(vec![
            Span::styled("Dimensions: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{}x{}", width, height)),
        ]));
    }

    // File size
    if metadata.file_size > 0 {
        lines.push(Line::from(vec![
            Span::styled("File Size: ", Style::default().fg(Color::Cyan)),
            Span::raw(utils::format_bytes(metadata.file_size)),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1))
                .border_style(Style::default().fg(Color::Cyan))
                .title("Preview (Metadata Only)"),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(paragraph, area);
}

/// Render the setup help dialog (shown when no cache and connection fails)
pub fn render_setup_help(f: &mut Frame, error_message: &str, config_path: &str) {
    let lines = vec![
        Line::from(Span::styled(
            "Cannot connect to Syncthing API",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Error: {}", error_message),
            Style::default().fg(Color::Red),
        )),
        Line::from(""),
        Line::from("Please check:"),
        Line::from("  • Is Syncthing running?"),
        Line::from("  • Is the API URL correct?"),
        Line::from("  • Is the API key valid?"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Config: ", Style::default().fg(Color::Cyan)),
            Span::raw(config_path),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[r] ", Style::default().fg(Color::Green)),
            Span::raw("Retry    "),
            Span::styled("[c] ", Style::default().fg(Color::Green)),
            Span::raw("Copy config path    "),
            Span::styled("[q] ", Style::default().fg(Color::Green)),
            Span::raw("Quit"),
        ]),
    ];

    // Center the dialog
    let area = f.area();
    let prompt_width = 70;
    let prompt_height = 18;
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Connection Failed - Setup Help")
                .border_style(Style::default().fg(Color::Red)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, prompt_area);
    f.render_widget(paragraph, prompt_area);
}

/// Render the rescan confirmation dialog
pub fn render_rescan_confirmation(f: &mut Frame, folder_label: &str) {
    use ratatui::widgets::Clear;

    let text = vec![
        Line::from(format!("Rescan folder \"{}\"?", folder_label)),
        Line::from(""),
        Line::from("(y) Rescan - Ask Syncthing to scan for changes"),
        Line::from("(f) Force Refresh - Clear cache + rescan"),
        Line::from("(n) Cancel"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Rescan Folder ");

    let paragraph = Paragraph::new(text).block(block);

    // Center the dialog
    let area = f.area();
    let dialog_width = 52;
    let dialog_height = 9;
    let dialog_area = centered_rect(area, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);
    f.render_widget(paragraph, dialog_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn art_style(width: u16) -> crate::logic::file::ArtStyle {
        crate::logic::file::ArtStyle { width }
    }

    fn popup(path: &str, content: &str, raw_view: bool) -> FileInfoPopupState {
        FileInfoPopupState {
            folder_id: "f".to_string(),
            file_path: path.to_string(),
            browse_item: crate::api::BrowseItem {
                name: path.to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
                mod_time: String::new(),
                size: 0,
            },
            file_details: None,
            file_content: Ok(content.to_string()),
            exists_on_disk: true,
            is_binary: false,
            is_image: false,
            art: None,
            archive: None,
            scroll_offset: 0,
            raw_view,
        }
    }

    fn render_rows(state: &mut FileInfoPopupState, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut images = std::collections::HashMap::new();
        terminal
            .draw(|f| render_preview_column(f, f.area(), state, None, &mut images))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    // ========================================
    // ARCHIVE PREVIEW
    // ========================================

    fn listing() -> crate::logic::archive::ArchiveListing {
        use crate::logic::archive::*;
        ArchiveListing {
            kind: ArchiveKind::TarGz,
            total_files: 1,
            total_size: 12,
            packed_size: None,
            stopped_early: None,
            entries: vec![ArchiveEntry {
                path: "notes.txt".into(),
                size: 12,
                modified: Some("2026-10-02 12:01".into()),
                is_dir: false,
            }],
        }
    }

    #[test]
    fn test_archive_preview_shows_listing() {
        let mut state = popup("snap.tar.gz", "", false);
        state.archive = Some(listing());
        let rows = render_rows(&mut state, 120, 8);
        assert!(rows[0].contains("Preview (Archive |"), "{}", rows[0]);
        assert!(rows[1].contains("tar.gz · 1 file · 12 B"), "{:#?}", rows);
        assert!(
            rows[3].contains("2026-10-02 12:01") && rows[3].contains("notes.txt"),
            "{:#?}",
            rows
        );
    }

    #[test]
    fn test_archive_error_shows_message() {
        let mut state = popup("bad.zip", "", false);
        state.file_content = Err("Couldn't read archive: invalid Zip archive".into());
        state.archive = None;
        let rows = render_rows(&mut state, 120, 8);
        assert!(
            rows.iter().any(|r| r.contains("Couldn't read archive")),
            "{:#?}",
            rows
        );
    }

    // ========================================
    // TEXT PREVIEW
    // ========================================

    #[test]
    fn test_markdown_preview_renders_markup() {
        let mut state = popup("README.md", "*em* text", false);
        let rows = render_rows(&mut state, 120, 8);
        assert!(rows[1].contains("em text"), "{:?}", rows[1]);
        assert!(!rows[1].contains("*em*"));
    }

    #[test]
    fn test_markdown_preview_raw_view_shows_source() {
        let mut state = popup("README.md", "*em* text", true);
        let rows = render_rows(&mut state, 120, 8);
        assert!(rows[1].contains("*em* text"), "{:?}", rows[1]);
    }

    #[test]
    fn test_markdown_preview_title_names_raw_toggle() {
        let mut rendered = popup("README.md", "text", false);
        assert!(render_rows(&mut rendered, 120, 8)[0].contains("r: raw"));
        let mut raw = popup("README.md", "text", true);
        assert!(render_rows(&mut raw, 120, 8)[0].contains("r: rendered"));
    }

    #[test]
    fn test_plain_preview_title_has_no_raw_toggle() {
        let mut state = popup("notes.txt", "text", false);
        assert!(!render_rows(&mut state, 120, 8)[0].contains("r: "));
    }

    #[test]
    fn test_preview_scroll_limit_counts_wrapped_lines() {
        let content = format!("{}END", "word ".repeat(400));
        let mut state = popup("notes.txt", &content, false);
        state.scroll_offset = u16::MAX;
        let rows = render_rows(&mut state, 40, 10);
        assert!(
            rows.iter().any(|r| r.contains("END")),
            "scrolling to the bottom must reach the last wrapped line: {:?}",
            rows
        );
    }

    // ========================================
    // FILE STEPPING HINT
    // ========================================

    fn render_popup_rows(
        state: &mut FileInfoPopupState,
        vim_mode: bool,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        use crate::ui::icons::{IconMode, IconTheme};
        let icons = IconRenderer::new(IconMode::Emoji, IconTheme::default());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut images = std::collections::HashMap::new();
        terminal
            .draw(|f| render_file_info(f, state, &[], None, &icons, None, &mut images, vim_mode))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    fn shows(rows: &[String], text: &str) -> bool {
        rows.iter().any(|r| r.contains(text))
    }

    #[test]
    fn test_file_info_names_shift_arrows_without_vim_mode() {
        let mut text = popup("notes.txt", "text", false);
        let rows = render_popup_rows(&mut text, false, 120, 30);
        assert!(shows(&rows, "Shift+↑↓: next/prev · o: open"), "{:?}", rows);
        assert!(!shows(&rows, "J/K"));

        let mut image = popup("cat.jpg", "", false);
        image.is_image = true;
        let rows = render_popup_rows(&mut image, false, 120, 30);
        assert!(shows(&rows, "Shift+↑↓: next/prev · o: open"), "{:?}", rows);
    }

    #[test]
    fn test_file_info_names_jk_in_vim_mode() {
        let mut text = popup("notes.txt", "text", false);
        let rows = render_popup_rows(&mut text, true, 120, 30);
        assert!(shows(&rows, "J/K: next/prev · o: open"), "{:?}", rows);
    }

    // ========================================
    // MODIFIED BY
    // ========================================

    #[test]
    fn test_modified_by_short_id_shows_device_name() {
        use crate::ui::icons::{IconMode, IconTheme};
        let mut state = popup("notes.txt", "text", false);
        state.file_details = Some(crate::api::FileDetails {
            local: Some(
                serde_json::from_value(serde_json::json!({"modifiedBy": "3QFSJG3"})).unwrap(),
            ),
            global: None,
            availability: vec![],
        });
        let devices = vec![crate::api::Device {
            id: "3QFSJG3-7K2LR5B-DRDC4JV-OTJNFIQ-3BJ7XBI-ZUIIZXR-WGBITZK-2DLWAQJ".to_string(),
            name: "nas".to_string(),
        }];
        let icons = IconRenderer::new(IconMode::Emoji, IconTheme::default());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        let mut images = std::collections::HashMap::new();
        terminal
            .draw(|f| {
                render_file_info(
                    f,
                    &mut state,
                    &devices,
                    None,
                    &icons,
                    None,
                    &mut images,
                    false,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        assert!(shows(&rows, "Modified By: nas"), "{:?}", rows);
    }

    // ========================================
    // FOLDER GROUP MENU
    // ========================================

    #[test]
    fn test_group_menu_renders_title_options_and_current() {
        use crate::model::types::{FolderGroupMenuState, GroupMenuOption as O};
        let menu = FolderGroupMenuState {
            folder_id: "projects".to_string(),
            folder_label: "Projects".to_string(),
            current_group: "Work".to_string(),
            options: vec![
                O::Group("Personal".into()),
                O::Group("Work".into()),
                O::NoGroup,
                O::NewGroup,
            ],
            selected_index: 1,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| render_folder_group_menu(f, &menu))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        for text in [
            "Folder group: Projects",
            "Work  (current)",
            "No group",
            "New group…",
        ] {
            assert!(shows(&rows, text), "{:?} missing in {:?}", text, rows);
        }
    }

    // ========================================
    // PREVIEW MESSAGES
    // ========================================

    fn message_rows(message: &str) -> Vec<String> {
        let mut state = popup("manuals/router-setup.txt", "", false);
        state.file_content = Err(message.to_string());
        render_rows(&mut state, 60, 12)
            .into_iter()
            .map(|r| {
                r.trim_matches(|c: char| c == '│' || c.is_whitespace())
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn test_locally_deleted_message_is_word_wrapped_without_error_prefix() {
        let message = crate::logic::sync_states::locally_deleted_message();
        let rows = message_rows(message);
        assert!(!rows.iter().any(|r| r.contains("Error:")), "{:#?}", rows);
        let body: Vec<&str> = rows[1..]
            .iter()
            .map(String::as_str)
            .filter(|r| !r.is_empty() && !r.starts_with('└'))
            .collect();
        assert_eq!(body.join(" "), message.replace('\n', " "), "{:#?}", rows);
    }

    #[test]
    fn test_other_preview_errors_keep_prefix() {
        let rows = message_rows("File not found on disk");
        assert!(
            rows.iter()
                .any(|r| r.contains("Error: File not found on disk")),
            "{:#?}",
            rows
        );
    }

    // ========================================
    // PADDING
    // ========================================

    #[test]
    fn test_preview_text_is_padded_one_column_from_each_border() {
        // 20 columns: 2 borders + 2 padding leave 16 for text
        let mut state = popup("notes.txt", "abcdefghijklmnopq", false);
        let rows = render_rows(&mut state, 20, 6);
        assert_eq!(rows[1], "│ abcdefghijklmnop │", "{:#?}", rows);
        assert!(rows[2].starts_with("│ q"), "{:#?}", rows);
    }

    #[test]
    fn test_metadata_is_padded_one_column_from_the_border() {
        let mut state = popup("notes.txt", "text", false);
        let icons = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        let mut images = std::collections::HashMap::new();
        terminal
            .draw(|f| render_file_info(f, &mut state, &[], None, &icons, None, &mut images, false))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        assert!(
            rows.iter().any(|r| r.contains("│ Name: notes.txt")),
            "{:#?}",
            rows
        );
    }

    fn render_cell_style(state: &mut FileInfoPopupState, x: u16, y: u16) -> Style {
        let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
        let mut images = std::collections::HashMap::new();
        terminal
            .draw(|f| render_preview_column(f, f.area(), state, None, &mut images))
            .unwrap();
        terminal.backend().buffer()[(x, y)].style()
    }

    #[test]
    fn test_ansi_art_defaults_to_light_grey_on_black() {
        use crate::logic::file::{ANSI_ART_BG, ANSI_ART_FG};
        let mut state = popup("art.ans", "plain\x1b[31mred", false);
        state.art = Some(art_style(80));
        let text_cell = render_cell_style(&mut state, 2, 1);
        assert_eq!(text_cell.fg, Some(ANSI_ART_FG));
        assert_eq!(text_cell.bg, Some(ANSI_ART_BG));
        let empty_cell = render_cell_style(&mut state, 20, 3);
        assert_eq!(empty_cell.bg, Some(ANSI_ART_BG));
    }

    #[test]
    fn test_ansi_art_border_keeps_terminal_background() {
        let mut state = popup("art.ans", "plain", false);
        state.art = Some(art_style(80));
        for (x, y) in [(0, 0), (0, 2), (39, 2), (20, 5), (3, 0)] {
            let cell = render_cell_style(&mut state, x, y);
            assert_eq!(cell.bg, Some(Color::Reset), "cell ({}, {})", x, y);
        }
    }

    #[test]
    fn test_ansi_art_keeps_its_own_colors() {
        let mut state = popup("art.ans", "\x1b[31mred", false);
        state.art = Some(art_style(80));
        let cell = render_cell_style(&mut state, 2, 1);
        assert_eq!(cell.fg, Some(crate::logic::file::cga_color(1)));
    }

    #[test]
    fn test_art_style_decides_art_rendering() {
        use crate::logic::file::ANSI_ART_BG;
        let mut named_ans = popup("art.ans", "plain", false);
        assert_ne!(
            render_cell_style(&mut named_ans, 2, 1).bg,
            Some(ANSI_ART_BG)
        );
        let mut nfo_text = popup("notes.txt", "plain", false);
        nfo_text.art = Some(art_style(80));
        assert_eq!(render_cell_style(&mut nfo_text, 2, 1).bg, Some(ANSI_ART_BG));
    }

    #[test]
    fn test_error_text_is_never_drawn_as_art() {
        use crate::logic::file::ANSI_ART_BG;
        let mut state = popup("art.ans", "", false);
        state.file_content = Err("File not found on disk".to_string());
        state.art = Some(art_style(80));
        assert_ne!(render_cell_style(&mut state, 2, 1).bg, Some(ANSI_ART_BG));
    }

    #[test]
    fn test_plain_text_preview_keeps_terminal_colors() {
        let mut state = popup("notes.txt", "plain", false);
        let cell = render_cell_style(&mut state, 2, 1);
        assert_eq!(cell.fg, Some(Color::Reset));
        assert_eq!(cell.bg, Some(Color::Reset));
    }

    #[test]
    fn test_preview_hint_mentions_open() {
        let mut state = popup("notes.txt", "text", false);
        let icons = IconRenderer::new(
            crate::ui::icons::IconMode::Emoji,
            crate::ui::icons::IconTheme::default(),
        );
        for vim_mode in [false, true] {
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            let mut images = std::collections::HashMap::new();
            terminal
                .draw(|f| {
                    render_file_info(
                        f,
                        &mut state,
                        &[],
                        None,
                        &icons,
                        None,
                        &mut images,
                        vim_mode,
                    )
                })
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            let screen: String = (0..buf.area.height)
                .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
                .map(|(x, y)| buf[(x, y)].symbol().to_string())
                .collect();
            assert!(
                screen.contains("o: open"),
                "vim_mode {}: {}",
                vim_mode,
                screen
            );
        }
    }

    #[test]
    fn test_art_rows_are_cut_off_not_wrapped() {
        let line1 = format!("A{}", "a".repeat(60));
        let line2 = format!("B{}", "b".repeat(60));
        let mut state = popup("art.nfo", &format!("{}\n{}", line1, line2), false);
        state.art = Some(art_style(80));
        let rows = render_rows(&mut state, 40, 8); // inner width 36
        assert!(
            rows[1].starts_with(&format!("│ A{}", "a".repeat(35))),
            "{:#?}",
            rows
        );
        assert!(rows[2].starts_with("│ B"), "{:#?}", rows);
    }

    #[test]
    fn test_art_scroll_counts_rows() {
        let text = (0..3)
            .map(|i| format!("{}{}", i, "z".repeat(99)))
            .collect::<Vec<_>>()
            .join("\n");
        let mut state = popup("art.nfo", &text, false);
        state.art = Some(art_style(100));
        let rows = render_rows(&mut state, 40, 8); // 6 visible rows, 3 art rows
        assert!(
            !rows[1..].iter().any(|r| r.contains('↓')),
            "no scrollbar: {:#?}",
            rows
        );
    }

    #[test]
    fn test_art_box_uses_canvas_width() {
        let mut state = popup("wide.ans", "x", false);
        state.art = Some(art_style(132));
        let rows = render_rows(&mut state, 200, 6);
        let top: Vec<char> = rows[0].chars().collect();
        let left = top.iter().position(|c| *c == '┌').unwrap();
        let right = top.iter().position(|c| *c == '┐').unwrap();
        assert_eq!(right - left + 1, 132 + 4);
    }

    #[test]
    fn test_plain_text_still_wraps() {
        let mut state = popup("notes.txt", &"w".repeat(50), false);
        let rows = render_rows(&mut state, 40, 8);
        assert!(rows[2].starts_with("│ w"), "{:#?}", rows);
    }

    #[test]
    fn test_json_preview_is_formatted_with_title() {
        let mut state = popup("config.json", r#"{"a":1}"#, false);
        let rows = render_rows(&mut state, 120, 8);
        assert!(rows[0].contains("Preview (JSON, r: raw"), "{}", rows[0]);
        assert!(rows[2].contains("\"a\": 1"), "{:#?}", rows);
    }

    #[test]
    fn test_json_raw_view_shows_source() {
        let mut state = popup("config.json", r#"{"a":1}"#, true);
        let rows = render_rows(&mut state, 120, 8);
        assert!(
            rows[0].contains("Preview (Raw, r: formatted"),
            "{}",
            rows[0]
        );
        assert!(rows[1].contains(r#"{"a":1}"#), "{:#?}", rows);
    }

    #[test]
    fn test_csv_preview_is_a_table() {
        let mut state = popup("people.csv", "name,age\nAda,36\n", false);
        let rows = render_rows(&mut state, 120, 8);
        assert!(rows[0].contains("Preview (CSV, r: raw"), "{}", rows[0]);
        assert!(rows[1].contains("name │ age"), "{:#?}", rows);
    }

    #[test]
    fn test_wide_csv_is_cut_off_not_wrapped() {
        let header = (0..20).map(|i| format!("col{:02}", i)).collect::<Vec<_>>();
        let row = (0..20).map(|i| format!("v{:02}", i)).collect::<Vec<_>>();
        let csv = format!(
            "{}\n{}\n{}\n",
            header.join(","),
            row.join(","),
            row.join(",")
        );
        let mut state = popup("wide.csv", &csv, false);
        let rows = render_rows(&mut state, 60, 10);
        assert!(rows[1].contains("co"), "{:#?}", rows);
        assert!(rows[2].starts_with("│ ─"), "{:#?}", rows);
        assert!(rows[3].contains('v'), "{:#?}", rows);
        assert!(rows[4].contains('v'), "{:#?}", rows);
        assert!(
            rows[5].trim_matches(|c| c == '│' || c == ' ').is_empty(),
            "{:#?}",
            rows
        );
    }

    #[test]
    fn test_error_content_has_plain_title() {
        let mut state = popup("a.json", "", false);
        state.file_content = Err("missing".to_string());
        let rows = render_rows(&mut state, 120, 8);
        assert!(!rows[0].contains("r: "), "{}", rows[0]);
    }
}
