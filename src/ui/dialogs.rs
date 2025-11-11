use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

use super::icons::IconRenderer;
use crate::model::FileInfoPopupState;
use crate::utils;
use crate::{api::Device, ImagePreviewState};

/// Render the revert confirmation dialog (for restoring deleted files in receive-only folders)
pub fn render_revert_confirmation(f: &mut Frame, changed_files: &[String]) {
    let file_list = changed_files
        .iter()
        .take(5)
        .map(|file| format!("  - {}", file))
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
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

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
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

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
    let prompt_area = Rect {
        x: (area.width.saturating_sub(prompt_width)) / 2,
        y: (area.height.saturating_sub(prompt_height)) / 2,
        width: prompt_width,
        height: prompt_height,
    };

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

/// Render the file information popup with metadata and preview
pub fn render_file_info(
    f: &mut Frame,
    state: &mut FileInfoPopupState,
    devices: &[Device],
    my_device_id: Option<&str>,
    icon_renderer: &IconRenderer,
    image_font_size: Option<(u16, u16)>,
    image_state_map: &mut std::collections::HashMap<String, crate::ImagePreviewState>,
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
    );

    // Render preview column
    render_preview_column(f, columns[1], state, image_font_size, image_state_map);
}

fn render_metadata_column(
    f: &mut Frame,
    area: Rect,
    state: &mut FileInfoPopupState,
    devices: &[Device],
    my_device_id: Option<&str>,
    icon_renderer: &IconRenderer,
    image_state_map: &mut std::collections::HashMap<String, crate::ImagePreviewState>,
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
        Span::raw(&state.browse_item.mod_time),
    ]));

    // Image resolution (if this is an image with loaded metadata)
    if state.is_image {
        if let Some(
            crate::ImagePreviewState::Ready { metadata, .. }
            | crate::ImagePreviewState::Failed { metadata },
        ) = image_state_map.get(&state.file_path)
        {
            if let Some((width, height)) = metadata.dimensions {
                lines.push(Line::from(vec![
                    Span::styled("Resolution: ", Style::default().fg(Color::Yellow)),
                    Span::raw(format!("{}x{}", width, height)),
                ]));
            }
        }
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
                let device_name = devices
                    .iter()
                    .find(|d| d.id == local.modified_by)
                    .map(|d| d.name.as_str())
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
        if let Some(local) = &details.local {
            if let Some(global) = &details.global {
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
            "⚠️  Binary file",
            Style::default().fg(Color::Magenta),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Metadata"),
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
                                .border_style(Style::default().fg(Color::Cyan))
                                .title("Preview"),
                        )
                        .style(Style::default().fg(Color::Yellow));
                    f.render_widget(paragraph, area);
                }
                ImagePreviewState::Ready { .. } => {
                    // Get mutable reference to protocol for rendering
                    if let Some(ImagePreviewState::Ready {
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

                        let render_rect = if let (Some((img_w, img_h)), Some((font_w, font_h))) =
                            (metadata.dimensions, image_font_size)
                        {
                            // Calculate how many cells the image needs (protocol's "desired" size)
                            let desired_width = ((img_w as f32) / (font_w as f32)).ceil() as u16;
                            let desired_height = ((img_h as f32) / (font_h as f32)).ceil() as u16;

                            // Fit desired size to available area (Resize::Fit logic)
                            let (fit_width, fit_height) = if desired_width <= inner_area.width
                                && desired_height <= inner_area.height
                            {
                                // Image fits - use desired size
                                (desired_width, desired_height)
                            } else {
                                // Image doesn't fit - scale down maintaining aspect ratio
                                let desired_aspect = desired_width as f32 / desired_height as f32;
                                let area_aspect =
                                    inner_area.width as f32 / inner_area.height as f32;

                                if desired_aspect > area_aspect {
                                    // Width constrained
                                    let width = inner_area.width;
                                    let height = (width as f32 / desired_aspect) as u16;
                                    (width, height)
                                } else {
                                    // Height constrained
                                    let height = inner_area.height;
                                    let width = (height as f32 * desired_aspect) as u16;
                                    (width, height)
                                }
                            };

                            // Center the fitted rect
                            let x_offset = (inner_area.width.saturating_sub(fit_width)) / 2;
                            let y_offset = (inner_area.height.saturating_sub(fit_height)) / 2;

                            Rect {
                                x: inner_area.x + x_offset,
                                y: inner_area.y + y_offset,
                                width: fit_width,
                                height: fit_height,
                            }
                        } else {
                            // No dimensions or font_size - use full area
                            inner_area
                        };

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
                        f.render_stateful_widget(image, render_rect, protocol);
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

        // Parse ANSI codes using our custom safe parser
        // Only handles SGR (color/style) codes, ignores cursor movement/screen clearing
        let parsed_text = crate::logic::file::parse_ansi_to_text(&content_str);

        // Count total lines WITHOUT wrapping (ANSI art needs fixed-width display)
        // Each line in the parsed text is a single display line, no wrapping calculation needed
        let total_lines = parsed_text.lines.len();

        // Viewport height (visible lines)
        let viewport_height = area.height.saturating_sub(2) as usize; // -2 for borders

        // Calculate max scroll position (can't scroll past the last line)
        let max_scroll = total_lines.saturating_sub(viewport_height);

        // Clamp scroll offset to valid range
        let clamped_scroll = (state.scroll_offset as usize).min(max_scroll) as u16;

        // Write the clamped value back to state to prevent offset drift
        state.scroll_offset = clamped_scroll;

        // Check if this is an ANSI art file (by extension or content)
        // Auto-detect ANSI codes in any file, not just .ans/.asc
        let is_ansi_art = {
            let path_lower = state.file_path.to_lowercase();
            let has_ansi_extension = path_lower.ends_with(".ans") || path_lower.ends_with(".asc");
            let has_ansi_content = crate::logic::file::contains_ansi_codes(content_str.as_bytes());
            has_ansi_extension || has_ansi_content
        };

        // Create a fixed 80-character width container for ANSI art only
        // ANSI art is often designed for 80 columns
        use ratatui::layout::{Constraint, Direction, Layout};

        let preview_area = if is_ansi_art {
            let fixed_width = 82u16; // 80 chars + 2 for borders
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

        // For ANSI art, enable wrapping to preserve 80-column layout
        // Non-ANSI art can also wrap safely
        // NOTE: Don't set .style() on the Paragraph - it would override ANSI colors
        let paragraph = Paragraph::new(parsed_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title("Preview (↑↓/j/k, ^d/^u, ^f/^b, gg/G, PgUp/PgDn)"),
            )
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((clamped_scroll, 0));

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
    let dialog_area = Rect {
        x: (area.width.saturating_sub(dialog_width)) / 2,
        y: (area.height.saturating_sub(dialog_height)) / 2,
        width: dialog_width,
        height: dialog_height,
    };

    f.render_widget(Clear, dialog_area);
    f.render_widget(paragraph, dialog_area);
}
