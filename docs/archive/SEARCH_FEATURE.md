# Search Feature Implementation Guide

## Overview

This document describes the implementation plan for the real-time search feature in Synctui. The search allows users to filter files and directories with wildcard support, persisting across navigation until manually cleared.

## User Requirements

### Trigger Keys
- **Normal Mode**: `Ctrl-F`
- **Vim Mode**: `/`

### UI Behavior
- Input box appears above "Hotkeys" legend with prompt: `Match: `
- Search happens in real-time as user types
- Wildcard support: `*` matches any sequence of characters
- Case-insensitive matching
- Shows match count in title: `Search (3 matches)`

### Search Scope
- **Primary Goal**: Search ALL files/folders in current folder recursively (all cached data)
- **Fallback**: If recursive search is too slow (>50ms), limit to current directory only
- Matches files and directories at any depth in the folder tree

### Navigation Behavior
- Search query persists when navigating between directories
- Search automatically applies to new directory when entered
- Only clears when:
  - User hits `Backspace` enough to empty the query (input box disappears)
  - User hits `Esc` (explicit cancel)

### Filtering Display Logic
When searching for a file deep in hierarchy (e.g., `/Movies/Photos/jeff-1.txt` with query "jeff"):
- **At root level**: Show only `Movies/` (collapsed, indicates match exists inside)
- **In Movies/**: Show only `Photos/` breadcrumb (collapsed, indicates match exists inside)
- **In Movies/Photos/**: Show `jeff-1.txt` file (actual match visible)

### Empty Query Behavior
- Empty query = no filtering (show all items normally)

## Architecture Overview

Following the Elm Architecture pattern used throughout Synctui:

```
┌─────────────────────────────────────────────────────────────┐
│                      User Input (Ctrl-F / /)                │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│          handlers/keyboard.rs - Input Processing            │
│  • Capture keystrokes (chars, backspace, enter, esc)       │
│  • Update Model.ui.search_query                             │
│  • Call App.apply_search_filter()                           │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│               main.rs - Orchestration                        │
│  • apply_search_filter() - filters current breadcrumb       │
│  • Queries cache for recursive search (Phase 2)             │
│  • Updates Model.navigation.breadcrumb_trail items          │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│          logic/search.rs - Pure Business Logic              │
│  • search_matches(query, path) - wildcard matching          │
│  • filter_items(items, query) - filter list                 │
│  • count_matches() - calculate match count                  │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│            ui/search.rs - Rendering                          │
│  • render_search_input() - draws input box                  │
│  • Shows query, match count, cursor                         │
│  • Cyan border when active, gray when inactive              │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Core Search Infrastructure (MVP)

**Objective**: Get basic search working with current directory filtering only.

#### 1.1 State Management (`src/model/ui.rs`)

Add to `UiModel` struct:

```rust
/// Search state
pub search_mode: bool,              // Whether search input is active (receiving keystrokes)
pub search_query: String,           // Current search query
```

Update methods:
- `has_modal()`: Add check for `search_mode`
- `close_all_modals()`: Clear search state (`search_mode = false`, `search_query.clear()`)

**Rationale**: Search is a modal UI state similar to popups/dialogs, so it belongs in `UiModel`.

#### 1.2 Pure Search Logic (`src/logic/search.rs` - NEW FILE)

Create pure, testable functions:

```rust
/// Match a search query against a file path using wildcard patterns
///
/// # Pattern Rules
/// - "*" matches any sequence of characters within a path component
/// - Matches are case-insensitive
/// - Matches file name or any part of the path
///
/// # Examples
/// ```
/// assert!(search_matches("jeff", "jeff-1.txt"));
/// assert!(search_matches("*jeff*", "my-jeff-file.txt"));
/// assert!(search_matches("photos", "/Movies/Photos/jeff-1.txt"));
/// ```
pub fn search_matches(query: &str, file_path: &str) -> bool {
    if query.is_empty() {
        return true; // Empty query matches everything
    }

    let query_lower = query.to_lowercase();
    let path_lower = file_path.to_lowercase();

    // Try glob pattern matching first
    if let Ok(pattern) = glob::Pattern::new(&query_lower) {
        // Match against full path
        if pattern.matches(&path_lower) {
            return true;
        }

        // Match against each path component
        for component in path_lower.split('/') {
            if pattern.matches(component) {
                return true;
            }
        }
    }

    // Fallback: simple substring match (if glob pattern is invalid)
    path_lower.contains(&query_lower)
}

/// Filter a list of BrowseItems by search query
///
/// # Arguments
/// - `items`: List of items to filter
/// - `query`: Search query with optional wildcards
/// - `prefix`: Optional path prefix for building full paths
///
/// # Returns
/// Filtered list containing only matching items
pub fn filter_items(
    items: &[BrowseItem],
    query: &str,
    prefix: Option<&str>,
) -> Vec<BrowseItem> {
    if query.is_empty() {
        return items.to_vec();
    }

    items.iter()
        .filter(|item| {
            let full_path = match prefix {
                Some(p) => format!("{}/{}", p.trim_matches('/'), item.name),
                None => item.name.clone(),
            };
            search_matches(query, &full_path)
        })
        .cloned()
        .collect()
}

/// Count matches in a list of items
pub fn count_matches(
    items: &[BrowseItem],
    query: &str,
    prefix: Option<&str>,
) -> usize {
    if query.is_empty() {
        return items.len();
    }

    items.iter()
        .filter(|item| {
            let full_path = match prefix {
                Some(p) => format!("{}/{}", p.trim_matches('/'), item.name),
                None => item.name.clone(),
            };
            search_matches(query, &full_path)
        })
        .count()
}
```

**Testing Strategy** (add to same file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_matches_all() {
        assert!(search_matches("", "any-file.txt"));
    }

    #[test]
    fn test_exact_match() {
        assert!(search_matches("jeff", "jeff-1.txt"));
        assert!(!search_matches("jeff", "john.txt"));
    }

    #[test]
    fn test_wildcard_prefix() {
        assert!(search_matches("jeff*", "jeff-1.txt"));
        assert!(search_matches("jeff*", "jeff.txt"));
        assert!(!search_matches("jeff*", "my-jeff.txt"));
    }

    #[test]
    fn test_wildcard_suffix() {
        assert!(search_matches("*jeff", "my-jeff"));
        assert!(search_matches("*.txt", "file.txt"));
    }

    #[test]
    fn test_wildcard_contains() {
        assert!(search_matches("*jeff*", "my-jeff-file.txt"));
        assert!(search_matches("*jeff*", "jeff.txt"));
        assert!(search_matches("*jeff*", "file-jeff"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(search_matches("JEFF", "jeff-1.txt"));
        assert!(search_matches("jeff", "JEFF-1.TXT"));
        assert!(search_matches("JeFf", "jEfF.txt"));
    }

    #[test]
    fn test_path_component_matching() {
        assert!(search_matches("photos", "/Movies/Photos/jeff-1.txt"));
        assert!(search_matches("movies", "/Movies/Photos/jeff-1.txt"));
        assert!(search_matches("*photos*", "/Movies/Photos/jeff-1.txt"));
    }

    #[test]
    fn test_substring_fallback() {
        // If glob pattern fails, fall back to substring
        assert!(search_matches("jeff", "my-jeff-file.txt"));
    }
}
```

**Rationale**:
- Use existing `glob` crate (already used for `.stignore` patterns)
- Pure functions = easy to test
- Follows existing pattern in `src/logic/`

#### 1.3 Keyboard Input Handling (`src/handlers/keyboard.rs`)

Add search mode handling at the **top** of `handle_key()` function (after existing modal handlers):

```rust
// Handle search input mode (process before other keys)
if app.model.ui.search_mode {
    match key.code {
        KeyCode::Esc => {
            // Exit search mode and clear query
            app.model.ui.search_mode = false;
            app.model.ui.search_query.clear();
            // Reload current breadcrumb without filter
            app.refresh_current_breadcrumb().await?;
            return Ok(());
        }
        KeyCode::Enter => {
            // Accept search and exit input mode (keep filtering active)
            app.model.ui.search_mode = false;
            return Ok(());
        }
        KeyCode::Backspace => {
            // Remove last character
            app.model.ui.search_query.pop();

            // If query is now empty, exit search mode
            if app.model.ui.search_query.is_empty() {
                app.model.ui.search_mode = false;
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
            // Re-filter current breadcrumb in real-time
            app.apply_search_filter();
            return Ok(());
        }
        _ => {
            // Ignore other keys in search mode
            return Ok(());
        }
    }
}

// ... existing modal handlers (confirm dialogs, etc.) ...

// Add search triggers in main match block:
match key.code {
    // ... existing keybindings ...

    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
        // Ctrl-F: Enter search mode (unless in vim mode - vim uses / instead)
        if !app.model.ui.vim_mode {
            app.model.ui.search_mode = true;
            app.model.ui.search_query.clear();
        }
    }
    KeyCode::Char('/') if app.model.ui.vim_mode => {
        // /: Enter search mode (vim mode only)
        app.model.ui.search_mode = true;
        app.model.ui.search_query.clear();
    }

    // ... rest of existing keybindings ...
}
```

**Rationale**:
- Search mode is modal (like file info popup), so process it first
- `Backspace` auto-exits when query becomes empty (per user requirement)
- Filter happens on every keystroke (real-time)

#### 1.4 App Orchestration Methods (`src/main.rs`)

Add helper methods to `App` implementation:

```rust
impl App {
    /// Apply search filter to current breadcrumb level (current directory only)
    fn apply_search_filter(&mut self) {
        // Don't filter folder list (only breadcrumbs)
        if self.model.navigation.focus_level == 0 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            // Get original items from cache
            // (This is simplified - actual implementation will fetch from cache)
            let original_items = level.items.clone(); // TODO: Fetch from cache instead

            // Filter items based on search query
            let query = &self.model.ui.search_query;
            let filtered = crate::logic::search::filter_items(
                &original_items,
                query,
                level.prefix.as_deref(),
            );

            // Update items with filtered list
            level.items = filtered;

            // Reset selection to first item (if any matches)
            level.selected_index = if level.items.is_empty() {
                None
            } else {
                Some(0)
            };

            level.scroll_offset = 0;
        }
    }

    /// Refresh current breadcrumb without search filter (restore original items)
    async fn refresh_current_breadcrumb(&mut self) -> Result<()> {
        if self.model.navigation.focus_level == 0 {
            return Ok(());
        }

        let level_idx = self.model.navigation.focus_level - 1;
        if let Some(level) = self.model.navigation.breadcrumb_trail.get(level_idx) {
            let folder_id = level.folder_id.clone();
            let prefix = level.prefix.clone();

            // Re-fetch from cache or API (without filter)
            // This will restore the original unfiltered items
            self.load_breadcrumb_level(&folder_id, prefix.as_deref()).await?;
        }

        Ok(())
    }
}
```

**Key Insight**: We need to preserve original items somewhere. Options:
1. **Store in BreadcrumbLevel**: Add `original_items: Option<Vec<BrowseItem>>` field
2. **Refetch from cache**: Always query cache for original (cleaner, but requires cache hit)

**Recommendation**: Option 2 (refetch from cache) - cleaner, avoids state duplication.

#### 1.5 UI Rendering - Search Input Box (`src/ui/search.rs` - NEW FILE)

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render search input box above legend
///
/// # Arguments
/// - `f`: Ratatui frame
/// - `area`: Rectangular area to render in
/// - `query`: Current search query
/// - `active`: Whether input is actively receiving keystrokes
/// - `match_count`: Number of matches found (None if not calculated)
pub fn render_search_input(
    f: &mut Frame,
    area: Rect,
    query: &str,
    active: bool,
    match_count: Option<usize>,
) {
    // Build title with match count
    let title = if active {
        match match_count {
            Some(count) => format!("Search ({} matches) - Esc to cancel", count),
            None => "Search - Esc to cancel".to_string(),
        }
    } else {
        "Search (Ctrl-F / /)".to_string()
    };

    let border_color = if active { Color::Cyan } else { Color::Gray };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(border_color));

    // Build input line with cursor
    let cursor_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::SLOW_BLINK);

    let input_line = if active {
        Line::from(vec![
            Span::raw("Match: "),
            Span::raw(query),
            Span::styled("█", cursor_style), // Blinking cursor
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("Match: {}", query),
                Style::default().fg(Color::Gray)
            ),
        ])
    };

    let paragraph = Paragraph::new(vec![input_line])
        .block(block)
        .style(Style::default());

    f.render_widget(paragraph, area);
}

/// Calculate required height for search input box (always 3 lines)
pub fn calculate_search_height() -> u16 {
    3 // Top border + input line + bottom border
}
```

**Rationale**:
- Follows existing UI module pattern (separate file per component)
- Blinking cursor for visual feedback
- Match count in title (per user requirement)

#### 1.6 Layout Integration (`src/ui/layout.rs`)

Update `LayoutInfo` struct:

```rust
pub struct LayoutInfo {
    pub system_area: Rect,
    pub folders_area: Option<Rect>,
    pub breadcrumb_areas: Vec<Rect>,
    pub search_area: Option<Rect>,  // NEW
    pub legend_area: Option<Rect>,
    pub status_area: Rect,
    pub start_pane: usize,
    pub folders_visible: bool,
}
```

Update `calculate_layout()` function signature and implementation:

```rust
pub fn calculate_layout(
    terminal_size: Rect,
    num_breadcrumb_levels: usize,
    has_breadcrumbs: bool,
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    status_height: u16,
    search_visible: bool,  // NEW parameter
) -> LayoutInfo {
    let legend_height = super::legend::calculate_legend_height(
        vim_mode,
        focus_level,
        can_restore,
        has_open_command,
        search_visible,  // Pass through
    );

    let search_height = if search_visible { 3 } else { 0 };  // NEW

    // Main vertical layout: System → Content → Search → Legend → Status
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                     // System bar
            Constraint::Min(3),                        // Content area
            Constraint::Length(search_height),         // Search input (NEW)
            Constraint::Length(legend_height),         // Legend
            Constraint::Length(status_height),         // Status bar
        ])
        .split(terminal_size);

    let system_area = main_chunks[0];
    let content_area = main_chunks[1];
    let search_area = if search_visible { Some(main_chunks[2]) } else { None };
    let legend_area = Some(if search_visible { main_chunks[3] } else { main_chunks[2] });
    let status_area = if search_visible { main_chunks[4] } else { main_chunks[3] };

    // ... rest of layout logic for content area ...

    LayoutInfo {
        system_area,
        folders_area,
        breadcrumb_areas,
        search_area,  // NEW
        legend_area,
        status_area,
        start_pane,
        folders_visible,
    }
}
```

**Rationale**:
- Search area conditionally appears based on visibility
- Inserted between content and legend (per user requirement)
- Dynamic height (0 when hidden, 3 when visible)

#### 1.7 Main Render Function (`src/ui/render.rs`)

Update render function to include search:

```rust
pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.size();

    // Determine if search should be visible
    let search_visible = app.model.ui.search_mode || !app.model.ui.search_query.is_empty();

    // Calculate layout
    let layout_info = layout::calculate_layout(
        size,
        app.model.navigation.breadcrumb_trail.len(),
        has_breadcrumbs,
        app.model.ui.vim_mode,
        app.model.navigation.focus_level,
        can_restore,
        app.open_command.is_some(),
        status_height,
        search_visible,  // NEW parameter
    );

    // ... existing rendering (system bar, folders, breadcrumbs) ...

    // Render search input if visible
    if let Some(search_area) = layout_info.search_area {
        let match_count = if app.model.navigation.focus_level > 0 {
            let level_idx = app.model.navigation.focus_level - 1;
            app.model.navigation.breadcrumb_trail
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
        );
    }

    // ... existing rendering (legend, status bar, dialogs) ...
}
```

#### 1.8 Legend Updates (`src/ui/legend.rs`)

Update function signature and add search keys:

```rust
pub fn build_legend_paragraph(
    vim_mode: bool,
    focus_level: usize,
    can_restore: bool,
    has_open_command: bool,
    search_active: bool,  // NEW parameter
) -> Paragraph<'static> {
    let mut hotkey_spans = vec![];

    // Navigation keys (always visible)
    hotkey_spans.extend(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::raw(":Navigate  "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw(":Open  "),
    ]);

    // Search key - only show in breadcrumb view (focus_level > 0)
    if focus_level > 0 {
        if search_active {
            // Show escape hint when search input is active
            hotkey_spans.extend(vec![
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(":Exit Search  "),
            ]);
        } else {
            // Show search trigger
            let search_key = if vim_mode { "/" } else { "^F" };
            hotkey_spans.extend(vec![
                Span::styled(search_key, Style::default().fg(Color::Yellow)),
                Span::raw(":Search  "),
            ]);
        }
    }

    // ... rest of hotkeys ...
}
```

Update all call sites in `render.rs` to pass `search_active` parameter.

### Phase 2: Recursive Search (All Files in Folder)

**Objective**: Search all cached files in the entire folder tree, not just current directory.

#### 2.1 Cache Method for Recursive Search (`src/cache.rs`)

Add method to query all items in a folder across all prefixes:

```rust
/// Search all browse items for a folder (recursive)
///
/// Returns list of (full_path, BrowseItem) tuples matching the query.
/// Only returns items from cache if folder_sequence matches.
pub fn search_browse_items(
    &self,
    folder_id: &str,
    folder_sequence: u64,
    query: &str,
) -> Result<Vec<(String, BrowseItem)>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    // Query all cached items for this folder
    let mut stmt = self.conn.prepare(
        "SELECT prefix, name, item_type, mod_time, size
         FROM browse_cache
         WHERE folder_id = ?1 AND folder_sequence = ?2"
    )?;

    let rows = stmt.query_map(params![folder_id, folder_sequence as i64], |row| {
        let prefix: String = row.get(0)?;
        let name: String = row.get(1)?;
        let item = BrowseItem {
            name: name.clone(),
            item_type: row.get(2)?,
            mod_time: row.get(3)?,
            size: row.get(4)?,
        };

        // Build full path
        let full_path = if prefix.is_empty() {
            name
        } else {
            format!("{}/{}", prefix, name)
        };

        Ok((full_path, item))
    })?;

    // Filter by search query
    let mut results = vec![];
    for row in rows {
        let (full_path, item) = row?;
        if crate::logic::search::search_matches(query, &full_path) {
            results.push((full_path, item));
        }
    }

    Ok(results)
}
```

#### 2.2 Update App Method for Recursive Filtering

Update `apply_search_filter()` in `src/main.rs`:

```rust
impl App {
    /// Apply search filter to current breadcrumb level
    ///
    /// If query is not empty, searches all cached items in folder recursively
    /// and filters current level to show only parent paths containing matches.
    fn apply_search_filter(&mut self) {
        // Don't filter folder list
        if self.model.navigation.focus_level == 0 {
            return;
        }

        let level_idx = self.model.navigation.focus_level - 1;
        let level = match self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
            Some(l) => l,
            None => return,
        };

        let query = &self.model.ui.search_query;

        // Empty query = no filter
        if query.is_empty() {
            // TODO: Restore original items from cache
            return;
        }

        // Get folder sequence for cache validation
        let folder_sequence = self.model.syncthing.folder_statuses
            .get(&level.folder_id)
            .map(|status| status.sequence)
            .unwrap_or(0);

        // Search all cached items recursively
        let all_matches = match self.cache_db.search_browse_items(
            &level.folder_id,
            folder_sequence,
            query,
        ) {
            Ok(matches) => matches,
            Err(e) => {
                debug!("Cache search failed: {}", e);
                // Fallback to current level only
                let filtered = crate::logic::search::filter_items(
                    &level.items,
                    query,
                    level.prefix.as_deref(),
                );
                level.items = filtered;
                return;
            }
        };

        // Filter current level items to show only those with matching descendants
        let current_prefix = level.prefix.as_deref().unwrap_or("");
        let current_depth = if current_prefix.is_empty() {
            0
        } else {
            current_prefix.split('/').count()
        };

        let filtered_items: Vec<BrowseItem> = level.items
            .iter()
            .filter(|item| {
                let item_path = if current_prefix.is_empty() {
                    item.name.clone()
                } else {
                    format!("{}/{}", current_prefix, item.name)
                };

                // Check if item itself matches
                if crate::logic::search::search_matches(query, &item_path) {
                    return true;
                }

                // Check if any descendant matches
                all_matches.iter().any(|(full_path, _)| {
                    full_path.starts_with(&format!("{}/", item_path))
                })
            })
            .cloned()
            .collect();

        level.items = filtered_items;
        level.selected_index = if level.items.is_empty() {
            None
        } else {
            Some(0)
        };
        level.scroll_offset = 0;
    }
}
```

#### 2.3 Performance Optimization

If recursive search is too slow (>50ms for typical dataset):

**Option A: Debouncing**
```rust
// In Model.ui
pub search_debounce_timer: Option<Instant>,
pub search_debounce_ms: u64,  // Default 200ms

// In keyboard handler
KeyCode::Char(c) => {
    app.model.ui.search_query.push(c);
    app.model.ui.search_debounce_timer = Some(Instant::now());
    // Don't filter immediately
}

// In main event loop
if let Some(timer) = app.model.ui.search_debounce_timer {
    if timer.elapsed() > Duration::from_millis(app.model.ui.search_debounce_ms) {
        app.apply_search_filter();
        app.model.ui.search_debounce_timer = None;
    }
}
```

**Option B: Async Search**
- Move search to background task (similar to API service)
- Show loading indicator while searching
- Cancel previous search if new keystroke arrives

**Recommendation**: Try recursive search without optimization first. Only add debouncing if measurements show it's needed.

### Phase 3: Polish & Edge Cases

#### 3.1 Edge Cases to Handle

1. **Search in folder view (focus_level == 0)**
   - Should ignore search trigger (only works in breadcrumbs)
   - OR: Search folder names/labels? (TBD)

2. **No matches found**
   - Display empty list (not crash)
   - Show message in status bar: "No matches found"

3. **Navigation while searching**
   - Search persists to new directory
   - Re-apply filter to new breadcrumb level
   - Update match count

4. **Modal dialogs while searching**
   - Keep search visible but inactive
   - Search input doesn't receive keystrokes while modal is open

5. **Very long query strings**
   - Truncate display in input box if too wide
   - Show ellipsis: "Match: verylongque..."

#### 3.2 Status Bar Integration

Show search feedback in status bar when no matches:

```rust
// In status_bar.rs
if app.model.ui.search_mode || !app.model.ui.search_query.is_empty() {
    if app.model.navigation.focus_level > 0 {
        let level = &app.model.navigation.breadcrumb_trail[app.model.navigation.focus_level - 1];
        if level.items.is_empty() {
            // Show "no matches" message
            status_spans.push(Span::styled(
                " | No matches found",
                Style::default().fg(Color::Yellow),
            ));
        }
    }
}
```

#### 3.3 Debug Logging

Add performance logging (when `--debug` flag is enabled):

```rust
use std::time::Instant;

let start = Instant::now();
let filtered = filter_items(&items, query, prefix);
let elapsed = start.elapsed();

debug!("Search filter took {:?} for {} items → {} matches",
       elapsed, items.len(), filtered.len());
```

## Testing Strategy

### Unit Tests

**File: `src/logic/search.rs`**

Comprehensive test coverage (see Phase 1.2 for examples):
- ✅ Empty query matches all
- ✅ Exact matching
- ✅ Wildcard patterns (*, prefix, suffix, contains)
- ✅ Case-insensitive matching
- ✅ Path component matching
- ✅ Substring fallback

**File: `src/model/ui.rs`**

Test search state:
```rust
#[test]
fn test_has_modal_includes_search() {
    let mut ui = UiModel::default();
    ui.search_mode = true;
    assert!(ui.has_modal());
}

#[test]
fn test_close_all_modals_clears_search() {
    let mut ui = UiModel::default();
    ui.search_mode = true;
    ui.search_query = "test".to_string();
    ui.close_all_modals();
    assert!(!ui.search_mode);
    assert!(ui.search_query.is_empty());
}
```

### Integration Tests (Manual)

**Test Checklist:**

1. **Enter Search Mode**
   - [ ] Press Ctrl-F in normal mode → input appears with cyan border
   - [ ] Press / in vim mode → input appears with cyan border
   - [ ] Cursor blinks in input box
   - [ ] Ctrl-F in vim mode does nothing (vim uses /)
   - [ ] / in normal mode does nothing (normal uses Ctrl-F)

2. **Search Input**
   - [ ] Type characters → appears in "Match: " prompt
   - [ ] Type characters → items filter in real-time
   - [ ] Match count updates in title: "Search (3 matches)"
   - [ ] Case-insensitive: "JEFF" matches "jeff-1.txt"
   - [ ] Wildcard: "*jeff*" matches "my-jeff-file.txt"
   - [ ] Path matching: "photos" matches "/Movies/Photos/file.txt"

3. **Backspace Behavior**
   - [ ] Press Backspace → removes last character
   - [ ] Items update after each backspace
   - [ ] Backspace to empty query → search input disappears
   - [ ] Backspace to empty → all items restored

4. **Exit Search Mode**
   - [ ] Press Esc → search cleared, all items restored
   - [ ] Press Enter → search accepted, filtering persists, input deactivates

5. **Navigation During Search**
   - [ ] Enter directory while searching → search persists
   - [ ] Search applies to new directory
   - [ ] Match count updates for new directory
   - [ ] Go back while searching → search persists

6. **Edge Cases**
   - [ ] Empty query → shows all items (no filtering)
   - [ ] No matches → shows empty list (not crash)
   - [ ] No matches → status bar shows "No matches found"
   - [ ] Very long query → truncates in input box
   - [ ] Modal dialog open → search input inactive but visible

7. **Legend Updates**
   - [ ] Folder view: No search key shown
   - [ ] Breadcrumb view: Shows "^F:Search" (or "/:Search" in vim)
   - [ ] Search active: Shows "Esc:Exit Search"

8. **Performance**
   - [ ] Filtering 100 items takes <10ms
   - [ ] Filtering 1000 items takes <50ms
   - [ ] UI remains responsive during filtering
   - [ ] No lag when typing rapidly

### Performance Benchmarks

Test with different dataset sizes:
- **Small**: 50 files → should be instant (<5ms)
- **Medium**: 500 files → should be fast (<20ms)
- **Large**: 5000 files → acceptable (<100ms)
- **Huge**: 50,000 files → may need debouncing

If recursive search exceeds 50ms, implement debouncing (Phase 2.3).

## Implementation Checklist

### Phase 1: Core Search (Current Directory)

- [ ] Add search state to `Model.ui` (`search_mode`, `search_query`)
- [ ] Create `src/logic/search.rs` with pure functions + tests
- [ ] Add keyboard handlers in `src/handlers/keyboard.rs`
- [ ] Add `apply_search_filter()` to `App` in `src/main.rs`
- [ ] Create `src/ui/search.rs` for input rendering
- [ ] Update `src/ui/layout.rs` to include search area
- [ ] Update `src/ui/render.rs` to render search input
- [ ] Update `src/ui/legend.rs` with search keys
- [ ] Update `src/ui/mod.rs` to include search module
- [ ] Run unit tests: `cargo test search`
- [ ] Manual testing with checklist
- [ ] Debug logging for performance measurement

### Phase 2: Recursive Search (All Files)

- [ ] Add `search_browse_items()` to `src/cache.rs`
- [ ] Update `apply_search_filter()` for recursive search
- [ ] Test with large datasets (1000+ files)
- [ ] Measure performance (target <50ms)
- [ ] Add debouncing if needed
- [ ] Update tests

### Phase 3: Polish

- [ ] Handle all edge cases (no matches, long queries, modals)
- [ ] Status bar integration ("No matches found")
- [ ] Performance optimization if needed
- [ ] Comprehensive manual testing
- [ ] Update CLAUDE.md with search documentation

## Files to Modify

### New Files (2)
- `src/logic/search.rs` - Pure search functions + tests
- `src/ui/search.rs` - Search input rendering

### Modified Files (7)
- `src/model/ui.rs` - Add search state
- `src/handlers/keyboard.rs` - Add search input handling
- `src/main.rs` - Add search orchestration methods
- `src/ui/layout.rs` - Add search area to layout
- `src/ui/render.rs` - Call search rendering
- `src/ui/legend.rs` - Add search keys
- `src/ui/mod.rs` - Include search module

### Optional (Phase 2)
- `src/cache.rs` - Add recursive search method

## Estimated Effort

- **Phase 1 (Core)**: 400 lines, 3-4 hours
- **Phase 2 (Recursive)**: 200 lines, 2 hours
- **Phase 3 (Polish)**: 100 lines, 1 hour
- **Total**: ~700 lines, 6-7 hours

## Dependencies

No new dependencies needed:
- ✅ `glob` crate already in `Cargo.toml` (used for ignore patterns)
- ✅ Ratatui widgets sufficient for input rendering

## Performance Targets

- **Filter time**: <10ms for 1000 items
- **Recursive search**: <50ms for typical dataset
- **Debounce threshold**: 200ms (if needed)
- **Memory**: Minimal overhead (no duplicate item storage)

## Design Decisions Summary

Based on user requirements:

1. ✅ **Search Persistence**: Query persists across navigation until manually cleared
2. ✅ **Recursive Scope**: Search all files in folder (Phase 2), fallback to current directory if slow
3. ✅ **Empty Query**: Shows all items (no filtering)
4. ✅ **Match Count**: Display in search box title
5. ✅ **Real-Time Filtering**: Updates on every keystroke
6. ✅ **Wildcard Support**: Uses `glob` crate for * patterns
7. ✅ **Auto-Exit**: Backspace to empty query closes search input

## Future Enhancements (Not in Scope)

- Search history (save recent queries)
- Regex support (advanced pattern matching)
- Search across multiple folders simultaneously
- Saved search presets
- Filter by file type (combine with existing features)
- Search result export