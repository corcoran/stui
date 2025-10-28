# File Preview Pop-up Feature

## Overview
Add a '?' hotkey that displays a detailed file information pop-up with two columns:
- **Left column (35 chars fixed width)**: File metadata
- **Right column (remaining width)**: File content preview with line wrapping and scrollbar

The popup will occupy 90% of terminal width and 90% of terminal height.

## UI/UX Design

### Layout
```
┌─────────────────────────────────────────────────────────────────────┐
│ File Information - /path/to/file.txt                    [Esc: Close] │
├──────────────────┬──────────────────────────────────────────────────┤
│ Metadata (25%)   │ Preview (75%)                                    │
│                  │                                                  │
│ Name: file.txt   │ This is the file content with line wrapping     │
│ Size: 1.2 KB     │ enabled so that long lines don't overflow       │
│ Modified: ...    │ horizontally. The text will wrap naturally      │
│ State: Synced    │ within the preview column boundaries.           │
│ Ignored: No      │                                                  │
│ Exists: Yes      │ For binary files, we'll show a message or       │
│ Permissions: ... │ attempt text extraction using 'strings'-like    │
│ Modified By: ... │ logic to prevent binary insanity.               │
│ ...              │                                                  │
│                  │ [scrollable if content exceeds height]          │
└──────────────────┴──────────────────────────────────────────────────┘
```

### Behavior
- **Trigger**: Press '?' when a file/folder is selected (focus_level > 0)
- **Close**: Press Esc or '?' again to dismiss
- **Updates**: Manual only - close and reopen to refresh
- **Scrolling** (full vim support):
  - ↑/↓ or j/k: Scroll up/down by 1 line
  - PgUp/PgDn: Scroll up/down by 10 lines (page)
  - Ctrl-d / Ctrl-u: Half-page down/up (10 lines)
  - Ctrl-f / Ctrl-b: Full-page down/up (20 lines)
  - gg: Jump to top
  - G: Jump to bottom
  - Visual scrollbar appears when content exceeds viewport height

## Implementation Details

### 1. Data Structures (`src/main.rs`)

Add to App struct (around line 100):
```rust
pub struct App {
    // ... existing fields ...
    pub show_file_info: Option<FileInfoPopupState>,
}

pub struct FileInfoPopupState {
    pub folder_id: String,
    pub file_path: String,
    pub browse_item: BrowseItem,
    pub file_details: Option<FileDetails>,
    pub file_content: Result<String, String>, // Ok(content) or Err(error message)
    pub exists_on_disk: bool,
    pub is_binary: bool,
}
```

### 2. Keybinding Handler (`src/main.rs`)

Add in event loop (after line 2944, near 't' keybinding):
```rust
KeyCode::Char('?') if self.focus_level > 0 => {
    if self.show_file_info.is_some() {
        // Toggle off
        self.show_file_info = None;
    } else {
        // Get selected item
        if let Some(level) = self.breadcrumb_trail.get(self.focus_level) {
            if let Some(selected_idx) = level.state.selected() {
                if let Some(item) = level.items.get(selected_idx) {
                    // Construct full path
                    let file_path = if let Some(prefix) = &level.prefix {
                        format!("{}{}", prefix, item.name)
                    } else {
                        item.name.clone()
                    };

                    // Spawn async task to fetch data
                    self.fetch_file_info_and_content(
                        level.folder_id.clone(),
                        file_path,
                        item.clone(),
                    );
                }
            }
        }
    }
}

// Also handle Esc when popup is open
KeyCode::Esc => {
    if self.show_file_info.is_some() {
        self.show_file_info = None;
    } else {
        // ... existing Esc behavior ...
    }
}
```

### 3. Data Fetching Method (`src/main.rs`)

Add new method to App impl:
```rust
impl App {
    fn fetch_file_info_and_content(
        &mut self,
        folder_id: String,
        file_path: String,
        browse_item: BrowseItem,
    ) {
        let client = self.client.clone();
        let config = self.config.clone();
        let folder_id_clone = folder_id.clone();
        let file_path_clone = file_path.clone();

        // Initialize popup state with loading message
        self.show_file_info = Some(FileInfoPopupState {
            folder_id: folder_id.clone(),
            file_path: file_path.clone(),
            browse_item,
            file_details: None,
            file_content: Err("Loading...".to_string()),
            exists_on_disk: false,
            is_binary: false,
        });

        tokio::spawn(async move {
            // 1. Fetch file details from API
            let file_details = client.get_file_info(&folder_id_clone, &file_path_clone).await;

            // 2. Read file content from disk
            let (file_content, exists_on_disk, is_binary) =
                read_file_content(&config, &folder_id_clone, &file_path_clone).await;

            // 3. Update state (need channel communication back to main thread)
            // TODO: Implement via existing api_service channel or new mechanism
        });
    }
}
```

### 4. File Reading Logic (`src/main.rs` or new `src/file_reader.rs`)

```rust
async fn read_file_content(
    config: &Config,
    folder_id: &str,
    file_path: &str,
) -> (Result<String, String>, bool, bool) {
    const MAX_SIZE: u64 = 20 * 1024 * 1024; // 20MB
    const BINARY_CHECK_SIZE: usize = 8192; // First 8KB

    // Translate container path to host path
    let host_path = translate_path_to_host(config, folder_id, file_path);

    // Check if file exists
    let exists = tokio::fs::metadata(&host_path).await.is_ok();
    if !exists {
        return (Err("File not found on disk".to_string()), false, false);
    }

    // Check file size
    match tokio::fs::metadata(&host_path).await {
        Ok(metadata) => {
            if metadata.len() > MAX_SIZE {
                return (
                    Err(format!("File too large ({}) - max 20MB", format_bytes(metadata.len()))),
                    true,
                    false
                );
            }

            // Read file content
            match tokio::fs::read(&host_path).await {
                Ok(bytes) => {
                    // Check if binary (null bytes in first 8KB)
                    let check_size = std::cmp::min(bytes.len(), BINARY_CHECK_SIZE);
                    let is_binary = bytes[..check_size].contains(&0);

                    if is_binary {
                        // Attempt text extraction (similar to 'strings' command)
                        let extracted = extract_text_from_binary(&bytes);
                        (Ok(extracted), true, true)
                    } else {
                        // Try to decode as UTF-8
                        match String::from_utf8(bytes) {
                            Ok(content) => (Ok(content), true, false),
                            Err(_) => {
                                // Try lossy conversion
                                let content = String::from_utf8_lossy(&bytes).to_string();
                                (Ok(content), true, true)
                            }
                        }
                    }
                }
                Err(e) => (Err(format!("Failed to read file: {}", e)), true, false),
            }
        }
        Err(e) => (Err(format!("Failed to get file info: {}", e)), exists, false),
    }
}

fn extract_text_from_binary(bytes: &[u8]) -> String {
    // Extract printable ASCII strings (similar to 'strings' command)
    let mut result = String::new();
    let mut current_string = String::new();
    const MIN_STRING_LENGTH: usize = 4;

    for &byte in bytes {
        if byte >= 32 && byte <= 126 || byte == b'\n' || byte == b'\t' {
            current_string.push(byte as char);
        } else {
            if current_string.len() >= MIN_STRING_LENGTH {
                result.push_str(&current_string);
                result.push('\n');
            }
            current_string.clear();
        }
    }

    if current_string.len() >= MIN_STRING_LENGTH {
        result.push_str(&current_string);
    }

    if result.is_empty() {
        result = "[Binary file - no readable text found]".to_string();
    }

    result
}

fn translate_path_to_host(config: &Config, folder_id: &str, file_path: &str) -> PathBuf {
    // Use existing path_map logic from config
    // Find folder config, get path, apply path_map translation
    // (Reference existing implementation in delete/open commands)
}
```

### 5. Render Function (`src/ui/dialogs.rs`)

Add new function (after existing dialog functions):
```rust
use ratatui::text::{Line, Span};
use ratatui::widgets::Wrap;

pub fn render_file_info(
    f: &mut Frame,
    state: &FileInfoPopupState,
    theme: &IconTheme,
) {
    // Calculate centered area (80% width, 90% height)
    let area = f.area();
    let popup_width = (area.width as f32 * 0.8) as u16;
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
    f.render_widget(Clear, popup_area);

    // Split into two columns (25% / 75%)
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(75),
        ])
        .split(popup_area);

    // Render metadata column
    render_metadata_column(f, columns[0], state, theme);

    // Render preview column
    render_preview_column(f, columns[1], state);
}

fn render_metadata_column(
    f: &mut Frame,
    area: Rect,
    state: &FileInfoPopupState,
    theme: &IconTheme,
) {
    let mut lines = vec![];

    // Name
    lines.push(Line::from(vec![
        Span::styled("Name: ", Style::default().fg(Color::Yellow)),
        Span::raw(&state.browse_item.name),
    ]));

    // Size
    lines.push(Line::from(vec![
        Span::styled("Size: ", Style::default().fg(Color::Yellow)),
        Span::raw(format_bytes(state.browse_item.size)),
    ]));

    // Modified time
    lines.push(Line::from(vec![
        Span::styled("Modified: ", Style::default().fg(Color::Yellow)),
        Span::raw(&state.browse_item.mod_time),
    ]));

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

            lines.push(Line::from(vec![
                Span::styled("Sequence: ", Style::default().fg(Color::Yellow)),
                Span::raw(local.sequence.to_string()),
            ]));

            if !local.modified_by.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Modified By: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&local.modified_by[..std::cmp::min(12, local.modified_by.len())]),
                ]));
            }
        }

        lines.push(Line::from(""));

        // Global state comparison
        if let Some(global) = &details.global {
            lines.push(Line::from(Span::styled(
                "Global State:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            )));

            lines.push(Line::from(vec![
                Span::styled("  Sequence: ", Style::default().fg(Color::Yellow)),
                Span::raw(global.sequence.to_string()),
            ]));

            // Check if out of sync
            if let Some(local) = &details.local {
                if local.sequence != global.sequence {
                    lines.push(Line::from(Span::styled(
                        "  ⚠️ Out of sync!",
                        Style::default().fg(Color::Red)
                    )));
                }
            }
        }

        lines.push(Line::from(""));

        // Device availability
        if !details.availability.is_empty() {
            lines.push(Line::from(Span::styled(
                "Available on:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            )));

            for device in &details.availability {
                lines.push(Line::from(format!("  • {}", &device.id[..12])));
            }
        }
    }

    lines.push(Line::from(""));

    // Disk status
    lines.push(Line::from(vec![
        Span::styled("Exists on Disk: ", Style::default().fg(Color::Yellow)),
        Span::raw(if state.exists_on_disk { "Yes" } else { "No" }),
    ]));

    if state.is_binary {
        lines.push(Line::from(Span::styled(
            "⚠️ Binary file",
            Style::default().fg(Color::Magenta)
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Metadata")
        )
        .style(Style::default().bg(Color::Black).fg(Color::White));

    f.render_widget(paragraph, area);
}

fn render_preview_column(
    f: &mut Frame,
    area: Rect,
    state: &FileInfoPopupState,
) {
    let content = match &state.file_content {
        Ok(text) => text.clone(),
        Err(msg) => format!("Error: {}", msg),
    };

    // IMPORTANT: Enable text wrapping with Wrap { trim: false }
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Preview")
        )
        .style(Style::default().bg(Color::Black).fg(Color::White))
        .wrap(Wrap { trim: false }); // Enable line wrapping!

    f.render_widget(paragraph, area);
}
```

### 6. Update Render Pipeline (`src/ui/render.rs`)

Add after pattern selection dialog (around line 191):
```rust
// Render file info popup
if let Some(state) = &app.show_file_info {
    dialogs::render_file_info(f, state, &icon_theme);
}
```

### 7. Update Legend (`src/ui/legend.rs`)

Add '?' key to breadcrumb view (around line 76):
```rust
if app.focus_level > 0 {
    // ... existing keys ...
    keys.push(("?", "Info"));
    // ... rest of keys ...
}
```

## State Management Considerations

### Async Communication
- **Challenge**: Tokio task needs to update App state after fetching data
- **Solution Options**:
  1. Use existing `api_service` channel architecture (add new request/response types)
  2. Create dedicated channel for file info updates
  3. Store result in Arc<Mutex<>> shared state

**Recommended**: Extend `ApiRequest`/`ApiResponse` enums:
```rust
// In src/main.rs or src/api.rs
pub enum ApiRequest {
    // ... existing variants ...
    GetFileInfoAndContent {
        folder_id: String,
        file_path: String,
        browse_item: BrowseItem,
    },
}

pub enum ApiResponse {
    // ... existing variants ...
    FileInfoAndContent {
        state: FileInfoPopupState,
    },
}
```

### Reactive Updates
- **Current approach**: Manual only - user closes and reopens popup to refresh
- **Future enhancement**: Listen to Syncthing events for the specific file and auto-update
- **Implementation note**: When event-driven updates are added, subscribe to:
  - `ItemStarted` / `ItemFinished` for the file
  - `LocalIndexUpdated` with filename match
  - Re-fetch data and update `show_file_info` state

## Testing Checklist

- [ ] Works with text files (various encodings)
- [ ] Works with binary files (shows extracted text or error)
- [ ] Handles large files (>20MB shows error)
- [ ] Handles missing files (deleted between selection and popup)
- [ ] Path mapping works correctly (container → host)
- [ ] Line wrapping works in preview column
- [ ] Popup centers correctly on different terminal sizes
- [ ] Esc closes popup
- [ ] '?' toggles popup on/off
- [ ] Metadata displays all available fields
- [ ] Shows correct sync state (local vs global)
- [ ] Shows device availability
- [ ] Handles API errors gracefully
- [ ] Works with files in subdirectories
- [ ] Works with files in root of folder

## Future Enhancements

1. **Scrolling**: Add scroll support for tall content (PgUp/PgDn, j/k in vim mode)
2. **Image preview**: Detect image files and render using terminal graphics protocols (Kitty, iTerm2, Sixel)
3. **Syntax highlighting**: Use syntect or similar for code files
4. **Diff view**: Show local vs global differences
5. **Event-driven updates**: Auto-refresh on file changes
6. **Configurable layout**: Allow users to adjust column proportions
7. **Export**: Save preview to file or copy to clipboard
8. **Search within preview**: Filter/highlight text in preview column

## References

- Existing dialog patterns: `src/ui/dialogs.rs`
- Format utilities: `src/utils.rs::format_bytes()` (consolidated utility function)
- Path mapping: See delete/open command implementations in `src/main.rs`
- API client: `src/api.rs::SyncthingClient::get_file_info()`
- Ratatui wrapping: https://docs.rs/ratatui/latest/ratatui/widgets/struct.Wrap.html

## Code Cleanup

As part of this implementation, duplicate `format_bytes` functions were consolidated:
- Created `src/utils.rs` module for shared utility functions
- Moved `format_bytes()` from `src/ui/status_bar.rs` to `src/utils.rs`
- Removed duplicate implementations from `src/main.rs` and `src/ui/dialogs.rs`
- All usages now reference `utils::format_bytes()`

## Recent Improvements

### UI Layout Enhancements
- **Metadata column**: Changed from 25% percentage-based to 35-char fixed width
  - Prevents filename wrapping in most cases
  - Allows preview to use maximum available space
- **Popup width**: Increased from 80% to 90% of terminal width for better content visibility

### Scrolling & Navigation
- **Scrollbar**: Added visual scrollbar on right edge of preview pane
  - Appears automatically when content exceeds viewport height
  - Shows scroll position with ↑/↓ indicators and █ thumb
  - Smart line counting considers text wrapping for accurate scrollbar sizing
- **Full vim keybinding support**:
  - j/k: Line-by-line scrolling
  - Ctrl-d/Ctrl-u: Half-page scrolling
  - Ctrl-f/Ctrl-b: Full-page scrolling
  - gg: Jump to top
  - G: Jump to bottom
  - Also supports arrow keys and PgUp/PgDn for non-vim users

### Metadata Improvements
- **Removed fields**: Sequence and Blocks (too technical, not user-facing)
- **Better sync status**: Replaced "Global State: Sequence" with user-friendly status:
  - "✅ In Sync" (green) - Local and global versions match
  - "⚠️ Behind (needs update)" (yellow) - Local is older than global
  - "⚠️ Ahead (local changes)" (yellow) - Local has changes not yet synced
- **Device names**: Shows human-readable device names instead of device IDs
  - "Modified By" shows device name (e.g., "MyLaptop" instead of "52FR4F4-...")
  - "Available on (connected)" shows device names with current device filtered out
  - Only shows connected/online devices (offline devices won't appear)
  - Falls back to device ID if name lookup fails
  - Shows "Only this device" when no other devices have the file
- **Visual improvements**:
  - Uses terminal default background colors (respects user's terminal theme)
  - Clean, consistent appearance across metadata and preview columns
