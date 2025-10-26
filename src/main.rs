use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::{collections::HashMap, fs, io, time::{Duration, Instant}};

mod api;
mod config;

use api::{BrowseItem, Folder, FolderStatus, SyncthingClient};
use config::Config;

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[derive(Clone)]
struct BreadcrumbLevel {
    folder_id: String,
    folder_label: String,
    folder_path: String,  // Cache the folder's container path
    prefix: Option<String>,
    items: Vec<BrowseItem>,
    state: ListState,
    translated_base_path: String,  // Cached translated base path for this level
}

struct App {
    client: SyncthingClient,
    folders: Vec<Folder>,
    folders_state: ListState,
    folder_statuses: HashMap<String, FolderStatus>,
    statuses_loaded: bool,
    last_status_update: Instant,
    path_map: HashMap<String, String>,
    breadcrumb_trail: Vec<BreadcrumbLevel>,
    focus_level: usize, // 0 = folders, 1+ = breadcrumb levels
    should_quit: bool,
}

impl App {
    fn translate_path(&self, folder: &Folder, relative_path: &str) -> String {
        // Get the full container path
        let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), relative_path);

        // Try to map container path to host path using path_map
        for (container_prefix, host_prefix) in &self.path_map {
            if container_path.starts_with(container_prefix) {
                let remainder = container_path.strip_prefix(container_prefix).unwrap_or("");
                return format!("{}{}", host_prefix.trim_end_matches('/'), remainder);
            }
        }

        // If no mapping found, return container path
        container_path
    }

    async fn new(config: Config) -> Result<Self> {
        let client = SyncthingClient::new(config.base_url.clone(), config.api_key.clone());
        let folders = client.get_folders().await?;

        let mut app = App {
            client,
            folders,
            folders_state: ListState::default(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            last_status_update: Instant::now(),
            path_map: config.path_map,
            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            should_quit: false,
        };

        if !app.folders.is_empty() {
            app.folders_state.select(Some(0));
            app.load_root_level().await?;
        }

        // Load folder statuses asynchronously
        app.load_folder_statuses().await;

        Ok(app)
    }

    async fn load_folder_statuses(&mut self) {
        for folder in &self.folders {
            if let Ok(status) = self.client.get_folder_status(&folder.id).await {
                self.folder_statuses.insert(folder.id.clone(), status);
            }
        }
        self.statuses_loaded = true;
        self.last_status_update = Instant::now();
    }

    async fn check_and_update_statuses(&mut self) {
        // Auto-refresh every 5 seconds
        if self.last_status_update.elapsed() >= Duration::from_secs(5) {
            self.load_folder_statuses().await;
        }
    }

    async fn load_root_level(&mut self) -> Result<()> {
        if let Some(selected) = self.folders_state.selected() {
            if let Some(folder) = self.folders.get(selected) {
                // Don't try to browse paused folders
                if folder.paused {
                    // Stay on folder list, don't enter the folder
                    return Ok(());
                }

                let items = self.client.browse_folder(&folder.id, None).await?;

                let mut state = ListState::default();
                if !items.is_empty() {
                    state.select(Some(0));
                }

                // Compute translated base path once
                let translated_base_path = self.translate_path(folder, "");

                self.breadcrumb_trail = vec![BreadcrumbLevel {
                    folder_id: folder.id.clone(),
                    folder_label: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
                    folder_path: folder.path.clone(),
                    prefix: None,
                    items,
                    state,
                    translated_base_path,
                }];
                self.focus_level = 1;
            }
        }
        Ok(())
    }

    async fn enter_directory(&mut self) -> Result<()> {
        if self.focus_level == 0 || self.breadcrumb_trail.is_empty() {
            return Ok(());
        }

        let level_idx = self.focus_level - 1;
        if level_idx >= self.breadcrumb_trail.len() {
            return Ok(());
        }

        let current_level = &self.breadcrumb_trail[level_idx];
        if let Some(selected_idx) = current_level.state.selected() {
            if let Some(item) = current_level.items.get(selected_idx) {
                // Only enter if it's a directory
                if item.item_type != "FILE_INFO_TYPE_DIRECTORY" {
                    return Ok(());
                }

                let folder_id = current_level.folder_id.clone();
                let folder_label = current_level.folder_label.clone();
                let folder_path = current_level.folder_path.clone();

                // Build new prefix
                let new_prefix = if let Some(ref prefix) = current_level.prefix {
                    format!("{}{}/", prefix, item.name)
                } else {
                    format!("{}/", item.name)
                };

                let items = self.client.browse_folder(&folder_id, Some(&new_prefix)).await?;

                let mut state = ListState::default();
                if !items.is_empty() {
                    state.select(Some(0));
                }

                // Compute translated base path once for this level
                let full_relative_path = new_prefix.trim_end_matches('/');
                let container_path = format!("{}/{}", folder_path.trim_end_matches('/'), full_relative_path);

                // Map to host path
                let translated_base_path = self.path_map.iter()
                    .find_map(|(container_prefix, host_prefix)| {
                        container_path.strip_prefix(container_prefix.as_str())
                            .map(|remainder| format!("{}{}", host_prefix.trim_end_matches('/'), remainder))
                    })
                    .unwrap_or(container_path);

                // Truncate breadcrumb trail to current level + 1
                self.breadcrumb_trail.truncate(level_idx + 1);

                // Add new level
                self.breadcrumb_trail.push(BreadcrumbLevel {
                    folder_id,
                    folder_label,
                    folder_path,
                    prefix: Some(new_prefix),
                    items,
                    state,
                    translated_base_path,
                });

                self.focus_level += 1;
            }
        }

        Ok(())
    }

    fn go_back(&mut self) {
        if self.focus_level > 1 {
            self.breadcrumb_trail.pop();
            self.focus_level -= 1;
        } else if self.focus_level == 1 {
            self.focus_level = 0;
        }
    }

    fn next_item(&mut self) {
        if self.focus_level == 0 {
            // Navigate folders
            let i = match self.folders_state.selected() {
                Some(i) => {
                    if i >= self.folders.len() - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.folders_state.select(Some(i));
        } else {
            // Navigate current breadcrumb level
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => {
                        if i >= level.items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    fn previous_item(&mut self) {
        if self.focus_level == 0 {
            // Navigate folders
            let i = match self.folders_state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.folders.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.folders_state.select(Some(i));
        } else {
            // Navigate current breadcrumb level
            let level_idx = self.focus_level - 1;
            if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
                if level.items.is_empty() {
                    return;
                }
                let i = match level.state.selected() {
                    Some(i) => {
                        if i == 0 {
                            level.items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                level.state.select(Some(i));
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => {
                // Manual refresh of folder statuses
                self.load_folder_statuses().await;
            }
            KeyCode::Left | KeyCode::Backspace => {
                self.go_back();
            }
            KeyCode::Right | KeyCode::Enter => {
                if self.focus_level == 0 {
                    self.load_root_level().await?;
                } else {
                    self.enter_directory().await?;
                }
            }
            KeyCode::Up => {
                self.previous_item();
            }
            KeyCode::Down => {
                self.next_item();
            }
            _ => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config_str = fs::read_to_string("config.yaml")?;
    let config: Config = serde_yaml::from_str(&config_str)?;

    // Initialize app
    let mut app = App::new(config).await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app with error handler
    let result = run_app(&mut terminal, &mut app).await;

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Return result after cleanup
    result
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let size = f.size();

            // Create main layout: content area + status bar at bottom
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),      // Content area
                    Constraint::Length(3),   // Status bar (3 lines: top border, text, bottom border)
                ])
                .split(size);

            let content_area = main_chunks[0];
            let status_area = main_chunks[1];

            // Calculate how many panes we need (folders + breadcrumb levels)
            let num_panes = 1 + app.breadcrumb_trail.len();

            // Determine visible panes based on terminal width
            let min_pane_width = 20;
            let max_visible_panes = (content_area.width / min_pane_width).max(2) as usize;

            // Calculate which panes to show (prioritize right side)
            let start_pane = if num_panes > max_visible_panes {
                num_panes - max_visible_panes
            } else {
                0
            };

            let visible_panes = num_panes.min(max_visible_panes);

            // Create equal-width constraints for visible panes
            let constraints: Vec<Constraint> = (0..visible_panes)
                .map(|_| Constraint::Ratio(1, visible_panes as u32))
                .collect();

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(content_area);

            let mut chunk_idx = 0;

            // Render folders pane if visible
            if start_pane == 0 {
                let folders_items: Vec<ListItem> = app
                    .folders
                    .iter()
                    .map(|folder| {
                        let display_name = folder.label.as_ref().unwrap_or(&folder.id);

                        // Determine status icon
                        let icon = if !app.statuses_loaded {
                            "🔍 " // Loading
                        } else if folder.paused {
                            "⏸  " // Paused
                        } else if let Some(status) = app.folder_statuses.get(&folder.id) {
                            if status.state == "" || status.state == "paused" {
                                "⏸  " // Paused (empty state means paused)
                            } else if status.state == "syncing" {
                                "🔄 " // Syncing
                            } else if status.need_total_items > 0 {
                                "⚠️ " // Out of sync
                            } else if status.state == "idle" {
                                "✅ " // Synced
                            } else if status.state.starts_with("sync") {
                                "🔄 " // Any sync variant
                            } else if status.state == "scanning" {
                                "🔍 " // Scanning
                            } else {
                                "❓ " // Unknown state
                            }
                        } else {
                            "❌ " // Error fetching status
                        };

                        ListItem::new(Span::raw(format!("{}{}", icon, display_name)))
                    })
                    .collect();

                let is_focused = app.focus_level == 0;
                let folders_list = List::new(folders_items)
                    .block(
                        Block::default()
                            .title("Folders")
                            .borders(Borders::ALL)
                            .border_style(if is_focused {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Gray)
                            }),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("> ");

                f.render_stateful_widget(folders_list, chunks[chunk_idx], &mut app.folders_state);
                chunk_idx += 1;
            }

            // Render breadcrumb levels
            for (idx, level) in app.breadcrumb_trail.iter_mut().enumerate() {
                if idx + 1 < start_pane {
                    continue; // Skip panes that are off-screen to the left
                }

                let items: Vec<ListItem> = level
                    .items
                    .iter()
                    .map(|item| {
                        let icon = match item.item_type.as_str() {
                            "FILE_INFO_TYPE_DIRECTORY" => "📁 ",
                            "FILE_INFO_TYPE_FILE" => "📄 ",
                            _ => "❓ ",
                        };
                        ListItem::new(Span::raw(format!("{}{}", icon, item.name)))
                    })
                    .collect();

                let title = if let Some(ref prefix) = level.prefix {
                    // Show last part of path
                    let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();
                    parts.last().map(|s| s.to_string()).unwrap_or_else(|| level.folder_label.clone())
                } else {
                    level.folder_label.clone()
                };

                let is_focused = app.focus_level == idx + 1;
                let list = List::new(items)
                    .block(
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_style(if is_focused {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::Gray)
                            }),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("> ");

                if chunk_idx < chunks.len() {
                    f.render_stateful_widget(list, chunks[chunk_idx], &mut level.state);
                    chunk_idx += 1;
                }
            }

            // Render status bar at the bottom with columns
            let status_line = if app.focus_level == 0 {
                // Show selected folder status
                if let Some(selected) = app.folders_state.selected() {
                    if let Some(folder) = app.folders.get(selected) {
                        let folder_name = folder.label.as_ref().unwrap_or(&folder.id);
                        if folder.paused {
                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                "Paused",
                                "-",
                                "-",
                                "-"
                            )
                        } else if let Some(status) = app.folder_statuses.get(&folder.id) {
                            let state_display = if status.state.is_empty() { "paused" } else { &status.state };
                            let in_sync = status.global_total_items.saturating_sub(status.need_total_items);
                            let items_display = format!("{}/{}", in_sync, status.global_total_items);
                            let need_display = if status.need_total_items > 0 {
                                format!("{} items ({}) ", status.need_total_items, format_bytes(status.need_bytes))
                            } else {
                                "Up to date ".to_string()
                            };

                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                state_display,
                                format_bytes(status.global_bytes),
                                items_display,
                                need_display
                            )
                        } else {
                            format!("{:<25} │ {:>15} │ {:>15} │ {:>15} │ {:>20}",
                                format!("Folder: {}", folder_name),
                                "Loading...",
                                "-",
                                "-",
                                "-"
                            )
                        }
                    } else {
                        "No folder selected".to_string()
                    }
                } else {
                    "No folder selected".to_string()
                }
            } else {
                // Show current item in breadcrumb trail
                let level_idx = app.focus_level - 1;
                if let Some(level) = app.breadcrumb_trail.get(level_idx) {
                    if let Some(selected) = level.state.selected() {
                        if let Some(item) = level.items.get(selected) {
                            let item_type = match item.item_type.as_str() {
                                "FILE_INFO_TYPE_DIRECTORY" => "Directory",
                                "FILE_INFO_TYPE_FILE" => "File",
                                _ => "Item",
                            };

                            // Use cached translated base path and append item name
                            let full_path = format!("{}/{}",
                                level.translated_base_path.trim_end_matches('/'),
                                item.name
                            );

                            format!("{}: {}  |  Path: {}",
                                item_type,
                                item.name,
                                full_path
                            )
                        } else {
                            "No item selected".to_string()
                        }
                    } else {
                        "No item selected".to_string()
                    }
                } else {
                    "".to_string()
                }
            };

            let status_bar = Paragraph::new(Line::from(Span::raw(status_line)))
                .block(Block::default().borders(Borders::ALL).title("Status"))
                .style(Style::default().fg(Color::White));

            f.render_widget(status_bar, status_area);
        })?;

        if app.should_quit {
            break;
        }

        // Check for periodic status updates
        app.check_and_update_statuses().await;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await?;
            }
        }
    }

    Ok(())
}
