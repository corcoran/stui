# Status Bar State Indicators Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add visual state indicators with icons to both folder and file/directory status bars, matching Syncthing web UI terminology.

**Architecture:** Extend `src/ui/status_bar.rs` with folder state mapping function, update `build_status_line()` to use `IconRenderer` for rendering state icons, reorder folder status fields, and add file/directory state display.

**Tech Stack:** Rust, Ratatui, existing `IconRenderer` (emoji/nerdfont toggle)

---

## Prerequisites

**Files to understand:**
- `src/ui/status_bar.rs` - Status bar rendering logic
- `src/ui/icons.rs` - Icon rendering system (already supports FolderState and SyncState)
- `src/api.rs:42-50` - SyncState enum definition
- Design doc: `docs/plans/2025-11-09-status-bar-state-indicators-design.md`

**Key concepts:**
- `FolderState` enum: Loading, Paused, Syncing, OutOfSync, Synced, Scanning, Unknown, Error
- `SyncState` enum: Synced, OutOfSync, LocalOnly, RemoteOnly, Ignored, Syncing, Unknown
- `IconRenderer::status_icon()` returns `Span<'static>` with colored icon
- Status bar has two modes: focus_level == 0 (folders) and focus_level > 0 (files/dirs)

---

## Task 1: Add Folder State Mapping Function

**Goal:** Create pure function to map Syncthing API state strings to FolderState enum + display labels.

**Files:**
- Modify: `src/ui/status_bar.rs` (add after line 11, before `build_status_paragraph`)

**Step 1: Write the test module**

Add at end of `src/ui/status_bar.rs` (after line 357):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::FolderState;

    #[test]
    fn test_map_folder_state_idle() {
        let (state, label) = map_folder_state("idle", 0);
        assert_eq!(state, FolderState::Synced);
        assert_eq!(label, "Idle");
    }

    #[test]
    fn test_map_folder_state_scanning() {
        let (state, label) = map_folder_state("scanning", 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Scanning");
    }

    #[test]
    fn test_map_folder_state_syncing() {
        let (state, label) = map_folder_state("syncing", 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Syncing");
    }

    #[test]
    fn test_map_folder_state_preparing() {
        let (state, label) = map_folder_state("preparing", 0);
        assert_eq!(state, FolderState::Syncing);
        assert_eq!(label, "Preparing");
    }

    #[test]
    fn test_map_folder_state_waiting() {
        let (state, label) = map_folder_state("waiting-to-scan", 0);
        assert_eq!(state, FolderState::Scanning);
        assert_eq!(label, "Waiting to Scan");
    }

    #[test]
    fn test_map_folder_state_outofsync() {
        let (state, label) = map_folder_state("outofsync", 0);
        assert_eq!(state, FolderState::OutOfSync);
        assert_eq!(label, "Out of Sync");
    }

    #[test]
    fn test_map_folder_state_error() {
        let (state, label) = map_folder_state("error", 0);
        assert_eq!(state, FolderState::Error);
        assert_eq!(label, "Error");
    }

    #[test]
    fn test_map_folder_state_stopped() {
        let (state, label) = map_folder_state("stopped", 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Stopped");
    }

    #[test]
    fn test_map_folder_state_paused() {
        let (state, label) = map_folder_state("paused", 0);
        assert_eq!(state, FolderState::Paused);
        assert_eq!(label, "Paused");
    }

    #[test]
    fn test_map_folder_state_unshared() {
        let (state, label) = map_folder_state("unshared", 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "Unshared");
    }

    #[test]
    fn test_map_folder_state_local_additions() {
        // Local additions takes precedence over API state
        let (state, label) = map_folder_state("idle", 5);
        assert_eq!(state, FolderState::Loading); // Using Loading as LocalOnly
        assert_eq!(label, "Local Additions");
    }

    #[test]
    fn test_map_folder_state_unknown() {
        // Fallback for unknown API states
        let (state, label) = map_folder_state("unknown-state", 0);
        assert_eq!(state, FolderState::Unknown);
        assert_eq!(label, "unknown-state");
    }
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib status_bar::tests::test_map_folder_state --no-fail-fast
```

Expected: All tests FAIL with "cannot find function `map_folder_state`"

**Step 3: Implement the mapping function**

Add after line 11 in `src/ui/status_bar.rs` (after imports, before `build_status_paragraph`):

```rust
/// Map Syncthing API state to FolderState enum and user-friendly label
///
/// Special case: When receive_only_items > 0, returns "Local Additions"
/// state regardless of API state (matches Syncthing web UI behavior).
///
/// # Arguments
/// * `api_state` - Raw state string from Syncthing API
/// * `receive_only_items` - Number of local additions in receive-only folder
///
/// # Returns
/// Tuple of (FolderState enum, display label)
fn map_folder_state(api_state: &str, receive_only_items: u64) -> (FolderState, &'static str) {
    // Special case: Local Additions takes precedence
    if receive_only_items > 0 {
        // Use Loading variant to represent LocalOnly concept
        // (FolderState doesn't have LocalOnly, but Loading is unused elsewhere)
        return (FolderState::Loading, "Local Additions");
    }

    match api_state {
        "idle" => (FolderState::Synced, "Idle"),
        "scanning" => (FolderState::Scanning, "Scanning"),
        "syncing" => (FolderState::Syncing, "Syncing"),
        "preparing" => (FolderState::Syncing, "Preparing"),
        "waiting-to-scan" => (FolderState::Scanning, "Waiting to Scan"),
        "outofsync" => (FolderState::OutOfSync, "Out of Sync"),
        "error" => (FolderState::Error, "Error"),
        "stopped" => (FolderState::Paused, "Stopped"),
        "paused" => (FolderState::Paused, "Paused"),
        "unshared" => (FolderState::Unknown, "Unshared"),
        _ => (FolderState::Unknown, api_state), // Fallback: preserve unknown state
    }
}
```

**Step 4: Fix the test - Loading is not the right choice**

We need to update `IconRenderer` to support LocalOnly for folders. But wait - let's check if we should add a new FolderState variant instead.

Actually, looking at the icon renderer, `FolderState::Loading` maps to `StatusType::Scanning` which is wrong. We need to add `LocalOnly` to `FolderState` enum.

Add to `src/ui/icons.rs` at line 26 (after `Error` in FolderState enum):

```rust
    LocalOnly,
```

Update `src/ui/icons.rs` at line 103 (in `folder_with_status` match):

```rust
            FolderState::LocalOnly => self.status_icon(StatusType::LocalOnly),
```

Now update the test and implementation to use `FolderState::LocalOnly`:

In test (line ~385):
```rust
        assert_eq!(state, FolderState::LocalOnly);
```

In implementation (line ~19):
```rust
        return (FolderState::LocalOnly, "Local Additions");
```

**Step 5: Run tests to verify they pass**

```bash
cargo test --lib status_bar::tests::test_map_folder_state
```

Expected: All 12 tests PASS

**Step 6: Commit**

```bash
git add src/ui/status_bar.rs src/ui/icons.rs
git commit -m "feat: Add folder state mapping function

- Maps 10 Syncthing API states to FolderState enum
- Handles 'Local Additions' special case (receive-only folders)
- Add FolderState::LocalOnly variant for local additions
- Comprehensive test coverage (12 tests)

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Add IconRenderer Parameter to Status Bar Functions

**Goal:** Thread `IconRenderer` through status bar functions to enable icon rendering.

**Files:**
- Modify: `src/ui/status_bar.rs:13-98` (function signatures)
- Modify: `src/ui/render.rs:69,234` (caller sites)

**Step 1: Update function signatures in status_bar.rs**

Update `build_status_paragraph` signature (line 13):

```rust
pub fn build_status_paragraph(
    icon_renderer: &IconRenderer,
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> Paragraph<'static> {
```

Update call to `build_status_line` (line 27):

```rust
    let status_line = build_status_line(
        icon_renderer,
        focus_level,
        // ... rest of parameters unchanged
    );
```

Update `build_status_line` signature (line 85):

```rust
fn build_status_line(
    icon_renderer: &IconRenderer,
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> String {
```

Update `render_status_bar` signature (line 246):

```rust
pub fn render_status_bar(
    f: &mut Frame,
    area: Rect,
    icon_renderer: &IconRenderer,
    focus_level: usize,
    // ... rest unchanged
) {
```

Update call to `build_status_paragraph` inside `render_status_bar` (line 262):

```rust
    let status_bar = build_status_paragraph(
        icon_renderer,
        focus_level,
        // ... rest unchanged
    );
```

Update `calculate_status_height` signature (line 280):

```rust
pub fn calculate_status_height(
    terminal_width: u16,
    icon_renderer: &IconRenderer,
    focus_level: usize,
    // ... rest unchanged
) -> u16 {
```

Update call to `build_status_line` inside `calculate_status_height` (line 296):

```rust
    let status_line = build_status_line(
        icon_renderer,
        focus_level,
        // ... rest unchanged
    );
```

**Step 2: Add IconRenderer import**

Add to imports at top of `src/ui/status_bar.rs` (after line 1):

```rust
use crate::ui::icons::IconRenderer;
```

**Step 3: Update callers in render.rs**

Update call to `calculate_status_height` (line 69):

```rust
    let status_height = status_bar::calculate_status_height(
        size.width,
        &app.icon_renderer,
        app.model.navigation.focus_level,
        // ... rest unchanged
    );
```

Update call to `render_status_bar` (line 234):

```rust
    status_bar::render_status_bar(
        f,
        layout_info.status_area,
        &app.icon_renderer,
        app.model.navigation.focus_level,
        // ... rest unchanged
    );
```

**Step 4: Verify compilation**

```bash
cargo build
```

Expected: Builds successfully with warnings (unused parameter `icon_renderer`)

**Step 5: Commit**

```bash
git add src/ui/status_bar.rs src/ui/render.rs
git commit -m "refactor: Add IconRenderer parameter to status bar functions

Thread IconRenderer through all status bar functions to enable
icon rendering for folder and file/directory states.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Render Folder State with Icon

**Goal:** Update folder status display to show icon + label and reorder fields.

**Files:**
- Modify: `src/ui/status_bar.rs:99-177` (`build_status_line` focus_level == 0 branch)

**Step 1: Write integration test**

This is harder to unit test due to icon rendering returning Span objects. We'll verify with manual testing and rely on existing integration tests.

**Step 2: Update folder state rendering**

Replace lines 119-170 in `src/ui/status_bar.rs` with:

```rust
                } else if let Some(status) = folder_statuses.get(&folder.id) {
                    // Get API state (empty means paused)
                    let api_state = if status.state.is_empty() {
                        "paused"
                    } else {
                        &status.state
                    };

                    // Map to FolderState enum and get display label
                    let (folder_state, state_label) = map_folder_state(
                        api_state,
                        status.receive_only_total_items,
                    );

                    // Render state icon + label
                    let state_icon = icon_renderer.folder_with_status(folder_state);
                    let state_display = format!(
                        "{}{}",
                        state_icon.iter()
                            .map(|s| s.content.as_ref())
                            .collect::<Vec<_>>()
                            .join(""),
                        state_label
                    );

                    // Calculate sync metrics
                    let in_sync = status
                        .global_total_items
                        .saturating_sub(status.need_total_items);
                    let items_display = format!("{}/{} items", in_sync, status.global_total_items);

                    // Build status message considering both remote needs and local additions
                    let need_display = if status.receive_only_total_items > 0 {
                        // Has local additions
                        if status.need_total_items > 0 {
                            // Both local additions and remote needs
                            format!(
                                "↓{} ↑{} ({})",
                                status.need_total_items,
                                status.receive_only_total_items,
                                utils::format_bytes(
                                    status.need_bytes + status.receive_only_changed_bytes
                                )
                            )
                        } else {
                            // Only local additions
                            format!(
                                "{} items ({})",
                                status.receive_only_total_items,
                                utils::format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        // Only remote needs
                        format!(
                            "{} items ({})",
                            status.need_total_items,
                            utils::format_bytes(status.need_bytes)
                        )
                    } else {
                        "Up to date".to_string()
                    };

                    // NEW FIELD ORDER: name | state | need_display | type | bytes | items
                    format!(
                        "Folder: {} | {} | {} | {} | {} | {}",
                        folder_name,
                        state_display,
                        need_display,
                        type_display,
                        utils::format_bytes(status.global_bytes),
                        items_display,
                    )
```

**Step 3: Verify compilation**

```bash
cargo build
```

Expected: Builds successfully

**Step 4: Manual testing**

```bash
cargo run
```

Test checklist:
- [ ] Idle folder shows icon + "Idle"
- [ ] Syncing folder shows icon + "Syncing"
- [ ] Receive-only folder with local additions shows icon + "Local Additions"
- [ ] Field order: name → state → sync status → type → size → items
- [ ] Icons match icon_mode setting (emoji vs nerdfont)

**Step 5: Commit**

```bash
git add src/ui/status_bar.rs
git commit -m "feat: Render folder state with icon and reorder fields

- Show icon + label for folder state (e.g., '✅ Idle')
- Special handling for 'Local Additions' state
- Reorder fields: name → state → sync → type → size → items
- Matches Syncthing web UI terminology

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Add File/Directory State Display

**Goal:** Show sync state icons for files/directories in breadcrumb view.

**Files:**
- Modify: `src/ui/status_bar.rs:184-240` (`build_status_line` focus_level > 0 branch)

**Step 1: Add state field to metrics**

Find the section where metrics are built (lines 184-239). Insert state display after "Sort:" and before "Selected:".

Replace lines 184-239 with:

```rust
    } else {
        // Show current directory performance metrics
        let mut metrics = Vec::new();

        if let Some(folder_label) = breadcrumb_folder_label {
            metrics.push(format!("Folder: {}", folder_label));
        }

        if let Some(item_count) = breadcrumb_item_count {
            metrics.push(format!("{} items", item_count));
        }

        // Show sort mode
        let sort_display = format!(
            "Sort: {}{}",
            sort_mode,
            if sort_reverse { "↓" } else { "↑" }
        );
        metrics.push(sort_display);

        // Show sync state (NEW!) - appears before "Selected:"
        if let Some((item_name, item_type, sync_state, exists)) = &breadcrumb_selected_item {
            if let Some(state) = sync_state {
                // Determine if directory
                let is_dir = item_type == "FILE_INFO_TYPE_DIRECTORY";

                // Get state display
                let state_display = match state {
                    SyncState::Ignored => {
                        // Special handling for ignored items
                        let ignored_text = if exists.unwrap_or(false) {
                            "Ignored, not deleted!"
                        } else {
                            "Ignored"
                        };
                        // Render icon
                        let state_spans = icon_renderer.ignored_item(is_dir, exists.unwrap_or(false));
                        let icon_str = state_spans.iter()
                            .map(|s| s.content.as_ref())
                            .collect::<Vec<_>>()
                            .join("");
                        format!("{}{}", icon_str, ignored_text)
                    }
                    _ => {
                        // Regular state rendering
                        let state_spans = icon_renderer.item_with_sync_state(is_dir, *state);
                        let icon_str = state_spans.iter()
                            .map(|s| s.content.as_ref())
                            .collect::<Vec<_>>()
                            .join("");
                        let label = match state {
                            SyncState::Synced => "Synced",
                            SyncState::OutOfSync => "Out of Sync",
                            SyncState::LocalOnly => "Local Only",
                            SyncState::RemoteOnly => "Remote Only",
                            SyncState::Syncing => "Syncing",
                            SyncState::Unknown => "Unknown",
                            SyncState::Ignored => unreachable!(), // Handled above
                        };
                        format!("{}{}", icon_str, label)
                    }
                };

                metrics.push(state_display);
            }
        }

        // Show pending operations count if any
        if pending_operations_count > 0 {
            metrics.push(format!("⏳ {} deletions processing", pending_operations_count));
        }

        if let Some(load_time) = last_load_time_ms {
            metrics.push(format!("Load: {}ms", load_time));
        }

        if let Some(cache_hit) = cache_hit {
            metrics.push(format!("Cache: {}", if cache_hit { "HIT" } else { "MISS" }));
        }

        // Show selected item info if available (appears AFTER state)
        if let Some((item_name, item_type, _sync_state, _exists)) = breadcrumb_selected_item {
            // Format name based on type: "dirname/" for directories, "filename" for files
            let formatted_name = match item_type.as_str() {
                "FILE_INFO_TYPE_DIRECTORY" => format!("{}/", item_name),
                _ => item_name.to_string(),
            };
            metrics.push(format!("Selected: {}", formatted_name));
        }

        metrics.join(" | ")
    }
```

**Step 2: Verify compilation**

```bash
cargo build
```

Expected: Builds successfully

**Step 3: Manual testing**

```bash
cargo run
```

Test checklist:
- [ ] Synced file shows "✅ Synced"
- [ ] Out of sync file shows "⚠️ Out of Sync"
- [ ] Local only file shows "💻 Local Only"
- [ ] Remote only file shows "☁️ Remote Only"
- [ ] Ignored file (deleted) shows "🚫 Ignored" in red
- [ ] Ignored file (exists) shows "🚫 Ignored, not deleted!" in red
- [ ] Syncing file shows "🔄 Syncing"
- [ ] State appears AFTER "Sort:" and BEFORE "Selected:"
- [ ] Icons match icon_mode setting

**Step 4: Commit**

```bash
git add src/ui/status_bar.rs
git commit -m "feat: Add file/directory state display to status bar

- Show sync state icon + label for selected items
- Position: after 'Sort:' and before 'Selected:'
- Preserve red text for Ignored states
- Support both emoji and nerdfont icon modes

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Handle Color Rendering for Status Bar

**Goal:** The current approach extracts icon text from Spans but loses color. We need to update the status bar to support colored text.

**Problem:** `build_status_line()` returns a `String`, but we need colored `Span` objects to preserve icon colors.

**Solution:** Change `build_status_line()` to return `Vec<Span<'static>>` instead of `String`, and update callers.

**Files:**
- Modify: `src/ui/status_bar.rs:85-241` (change return type and implementation)
- Modify: `src/ui/status_bar.rs:42-76` (update `build_status_paragraph` to use Vec<Span>)
- Modify: `src/ui/status_bar.rs:312-342` (update `calculate_status_height` to use Vec<Span>)

**Step 1: Change build_status_line return type and refactor**

This is a significant refactor. Instead of building a string and then parsing "|" separators, we'll build `Vec<Span>` directly.

Replace `build_status_line` function (lines 85-241) with:

```rust
/// Build the status line spans (extracted for reuse)
fn build_status_line(
    icon_renderer: &IconRenderer,
    focus_level: usize,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    folders_state_selected: Option<usize>,
    breadcrumb_folder_label: Option<String>,
    breadcrumb_item_count: Option<usize>,
    breadcrumb_selected_item: Option<(String, String, Option<SyncState>, Option<bool>)>,
    sort_mode: &str,
    sort_reverse: bool,
    last_load_time_ms: Option<u64>,
    cache_hit: Option<bool>,
    pending_operations_count: usize,
) -> Vec<Span<'static>> {
    if focus_level == 0 {
        // Show selected folder status
        if let Some(selected) = folders_state_selected {
            if let Some(folder) = folders.get(selected) {
                let folder_name = folder.label.as_ref().unwrap_or(&folder.id);

                // Convert folder type to user-friendly display
                let type_display = match folder.folder_type.as_str() {
                    "sendonly" => "Send Only",
                    "sendreceive" => "Send & Receive",
                    "receiveonly" => "Receive Only",
                    _ => &folder.folder_type,
                };

                if folder.paused {
                    // Build spans: Folder: {name} | {type} | Paused
                    let mut spans = vec![
                        Span::styled("Folder:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {} ", folder_name)),
                        Span::raw("| "),
                        Span::styled("Type:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {} ", type_display)),
                        Span::raw("| "),
                        Span::styled("Paused", Style::default().fg(Color::Gray)),
                    ];
                    return spans;
                } else if let Some(status) = folder_statuses.get(&folder.id) {
                    // Get API state
                    let api_state = if status.state.is_empty() {
                        "paused"
                    } else {
                        &status.state
                    };

                    // Map to FolderState enum and get display label
                    let (folder_state, state_label) = map_folder_state(
                        api_state,
                        status.receive_only_total_items,
                    );

                    // Render state icon spans
                    let mut state_spans = icon_renderer.folder_with_status(folder_state);
                    state_spans.push(Span::raw(state_label));

                    // Calculate sync metrics
                    let in_sync = status
                        .global_total_items
                        .saturating_sub(status.need_total_items);
                    let items_display = format!("{}/{} items", in_sync, status.global_total_items);

                    // Build sync status message
                    let need_display = if status.receive_only_total_items > 0 {
                        if status.need_total_items > 0 {
                            format!(
                                "↓{} ↑{} ({})",
                                status.need_total_items,
                                status.receive_only_total_items,
                                utils::format_bytes(
                                    status.need_bytes + status.receive_only_changed_bytes
                                )
                            )
                        } else {
                            format!(
                                "{} items ({})",
                                status.receive_only_total_items,
                                utils::format_bytes(status.receive_only_changed_bytes)
                            )
                        }
                    } else if status.need_total_items > 0 {
                        format!(
                            "{} items ({})",
                            status.need_total_items,
                            utils::format_bytes(status.need_bytes)
                        )
                    } else {
                        "Up to date".to_string()
                    };

                    // Build spans: Folder: {name} | {state} | {sync} | {type} | {size} | {items}
                    let mut spans = vec![
                        Span::styled("Folder:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {} ", folder_name)),
                        Span::raw("| "),
                    ];
                    spans.extend(state_spans);
                    spans.push(Span::raw(" | "));
                    spans.push(Span::raw(format!("{} ", need_display)));
                    spans.push(Span::raw("| "));
                    spans.push(Span::styled(format!("{}: ", "Type"), Style::default().fg(Color::Yellow)));
                    spans.push(Span::raw(format!("{} ", type_display)));
                    spans.push(Span::raw("| "));
                    spans.push(Span::styled(format!("{}: ", "Size"), Style::default().fg(Color::Yellow)));
                    spans.push(Span::raw(format!("{} ", utils::format_bytes(status.global_bytes))));
                    spans.push(Span::raw("| "));
                    spans.push(Span::styled(format!("{}: ", "Items"), Style::default().fg(Color::Yellow)));
                    spans.push(Span::raw(items_display));

                    return spans;
                } else {
                    // Loading state
                    return vec![
                        Span::styled("Folder:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {} ", folder_name)),
                        Span::raw("| "),
                        Span::styled("Type:", Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {} ", type_display)),
                        Span::raw("| Loading..."),
                    ];
                }
            } else {
                return vec![Span::raw("No folder selected")];
            }
        } else {
            return vec![Span::raw("No folder selected")];
        }
    } else {
        // Breadcrumb view - build metrics
        let mut spans: Vec<Span<'static>> = vec![];
        let mut is_first = true;

        // Helper to add separator
        let mut add_separator = |spans: &mut Vec<Span<'static>>| {
            if !is_first {
                spans.push(Span::raw(" | "));
            }
            is_first = false;
        };

        if let Some(folder_label) = breadcrumb_folder_label {
            add_separator(&mut spans);
            spans.push(Span::styled("Folder:", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format!(" {}", folder_label)));
        }

        if let Some(item_count) = breadcrumb_item_count {
            add_separator(&mut spans);
            spans.push(Span::raw(format!("{} items", item_count)));
        }

        // Sort mode
        add_separator(&mut spans);
        spans.push(Span::styled("Sort:", Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(format!(
            " {}{}",
            sort_mode,
            if sort_reverse { "↓" } else { "↑" }
        )));

        // Sync state (NEW!)
        if let Some((item_name, item_type, sync_state, exists)) = &breadcrumb_selected_item {
            if let Some(state) = sync_state {
                add_separator(&mut spans);

                let is_dir = item_type == "FILE_INFO_TYPE_DIRECTORY";

                match state {
                    SyncState::Ignored => {
                        // Special red text for ignored
                        let ignored_text = if exists.unwrap_or(false) {
                            "Ignored, not deleted!"
                        } else {
                            "Ignored"
                        };
                        let mut state_spans = icon_renderer.ignored_item(is_dir, exists.unwrap_or(false));
                        spans.extend(state_spans);
                        spans.push(Span::styled(ignored_text, Style::default().fg(Color::Red)));
                    }
                    _ => {
                        let state_spans = icon_renderer.item_with_sync_state(is_dir, *state);
                        spans.extend(state_spans);
                        let label = match state {
                            SyncState::Synced => "Synced",
                            SyncState::OutOfSync => "Out of Sync",
                            SyncState::LocalOnly => "Local Only",
                            SyncState::RemoteOnly => "Remote Only",
                            SyncState::Syncing => "Syncing",
                            SyncState::Unknown => "Unknown",
                            SyncState::Ignored => unreachable!(),
                        };
                        spans.push(Span::raw(label));
                    }
                }
            }
        }

        // Pending operations
        if pending_operations_count > 0 {
            add_separator(&mut spans);
            spans.push(Span::raw(format!("⏳ {} deletions processing", pending_operations_count)));
        }

        // Performance metrics
        if let Some(load_time) = last_load_time_ms {
            add_separator(&mut spans);
            spans.push(Span::styled("Load:", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format!(" {}ms", load_time)));
        }

        if let Some(cache_hit) = cache_hit {
            add_separator(&mut spans);
            spans.push(Span::styled("Cache:", Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format!(" {}", if cache_hit { "HIT" } else { "MISS" })));
        }

        // Selected item (appears AFTER state)
        if let Some((item_name, item_type, _state, _exists)) = breadcrumb_selected_item {
            add_separator(&mut spans);
            spans.push(Span::styled("Selected:", Style::default().fg(Color::Yellow)));
            let formatted_name = match item_type.as_str() {
                "FILE_INFO_TYPE_DIRECTORY" => format!(" {}/", item_name),
                _ => format!(" {}", item_name),
            };
            spans.push(Span::raw(formatted_name));
        }

        spans
    }
}
```

**Step 2: Update build_status_paragraph**

Replace lines 42-76 with:

```rust
    let status_spans = build_status_line(
        icon_renderer,
        focus_level,
        folders,
        folder_statuses,
        folders_state_selected,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        sort_mode,
        sort_reverse,
        last_load_time_ms,
        cache_hit,
        pending_operations_count,
    );

    Paragraph::new(vec![Line::from(status_spans)])
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false })
```

**Step 3: Update calculate_status_height**

Replace lines 312-342 with:

```rust
    let status_spans = build_status_line(
        icon_renderer,
        focus_level,
        folders,
        folder_statuses,
        folders_state_selected,
        breadcrumb_folder_label,
        breadcrumb_item_count,
        breadcrumb_selected_item,
        sort_mode,
        sort_reverse,
        last_load_time_ms,
        cache_hit,
        pending_operations_count,
    );

    // Create paragraph WITHOUT block for accurate line counting
    let paragraph_for_counting = Paragraph::new(vec![Line::from(status_spans)])
        .wrap(Wrap { trim: false });

    // Calculate available width (subtract left + right borders)
    let available_width = terminal_width.saturating_sub(2);

    // Get exact line count for wrapped text
    let line_count = paragraph_for_counting.line_count(available_width);

    // Add top + bottom borders, ensure minimum of 3
    (line_count as u16).saturating_add(2).max(3)
```

**Step 4: Remove old parsing logic from build_status_paragraph**

The old parsing logic (lines 43-76 originally) is no longer needed since we're building spans directly.

**Step 5: Verify compilation**

```bash
cargo build
```

Expected: Builds successfully

**Step 6: Manual testing**

```bash
cargo run
```

Test checklist:
- [ ] Icons appear in color (not just text)
- [ ] "Folder:", "Type:", etc. labels appear in yellow
- [ ] Ignored items show in red
- [ ] Status bar wraps correctly on narrow terminals
- [ ] Both emoji and nerdfont modes work

**Step 7: Commit**

```bash
git add src/ui/status_bar.rs
git commit -m "refactor: Build status bar with colored Spans directly

Change build_status_line() to return Vec<Span> instead of String
to preserve icon colors and avoid parsing overhead.

- Icons render in color (green for synced, red for error, etc.)
- Labels render in yellow for better contrast
- Eliminates string parsing logic
- Maintains text wrapping behavior

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Update CHANGELOG

**Goal:** Document the new status bar state indicators feature.

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add changelog entry**

Add to the `## [Unreleased]` section in `CHANGELOG.md`:

```markdown
### ✨ New Features

**Status Bar State Indicators**
- Folder states: Visual indicators for all 10 Syncthing states (Idle, Scanning, Syncing, etc.)
- File/directory states: Shows all SyncState values (Synced, Out of Sync, Local Only, Remote Only, etc.)
- "Local Additions" state for receive-only folders (matches Syncthing web UI)
- Colored icons using emoji or nerdfont (respects icon_mode config)
- Reordered folder status: name → state → sync status → type → size → items
- File/directory state appears after "Sort:" and before "Selected:"
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: Add status bar state indicators to changelog

Document new visual state indicators feature for folders and
files/directories in status bar.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Testing Checklist

**Folder States (focus_level == 0):**
- [ ] Idle folder shows icon + "Idle" (green checkmark)
- [ ] Scanning folder shows icon + "Scanning" (magnifying glass)
- [ ] Syncing folder shows icon + "Syncing" (refresh arrows)
- [ ] Out of sync folder shows icon + "Out of Sync" (warning)
- [ ] Paused folder shows icon + "Paused" (pause symbol)
- [ ] Receive-only folder with local additions shows icon + "Local Additions" (computer)
- [ ] Field order: name → state → sync status → type → size → items
- [ ] Icons match icon_mode (emoji vs nerdfont)

**File/Directory States (focus_level > 0):**
- [ ] Synced file shows icon + "Synced"
- [ ] Out of sync file shows icon + "Out of Sync"
- [ ] Local only file shows icon + "Local Only"
- [ ] Remote only file shows icon + "Remote Only"
- [ ] Ignored file (deleted) shows icon + "Ignored" in red
- [ ] Ignored file (exists) shows icon + "Ignored, not deleted!" in red
- [ ] Syncing file shows icon + "Syncing"
- [ ] State appears after "Sort:" and before "Selected:"
- [ ] Icons match icon_mode

**Icon Modes:**
- [ ] Emoji mode renders correctly (📁, 📄, ✅, ⚠️, etc.)
- [ ] Nerdfont mode renders correctly (U+E5FF, U+F00C, etc.)
- [ ] Toggle icon_mode in config and verify both work

**Edge Cases:**
- [ ] Unknown states handled gracefully
- [ ] Empty folder status (loading state) doesn't crash
- [ ] Very long folder names don't break layout
- [ ] Status bar wraps correctly on narrow terminals (< 80 cols)
- [ ] Status bar wraps correctly on very narrow terminals (< 40 cols)

**Color Rendering:**
- [ ] Icons appear in color (green for synced, red for error, yellow for out of sync)
- [ ] Labels ("Folder:", "Type:", etc.) appear in yellow
- [ ] Ignored text appears in red
- [ ] Colors respect terminal theme (use terminal colors, not RGB)

**Regression Testing:**
- [ ] All existing status bar fields still appear correctly
- [ ] Sync progress (arrows, byte counts) unchanged
- [ ] Pending operations display unchanged
- [ ] Performance metrics (Load, Cache) unchanged

---

## Rollback Plan

If issues arise, revert commits in reverse order:

```bash
git revert HEAD      # docs: changelog
git revert HEAD~1    # refactor: colored spans
git revert HEAD~2    # feat: file/directory states
git revert HEAD~3    # feat: folder state rendering
git revert HEAD~4    # refactor: IconRenderer parameter
git revert HEAD~5    # feat: folder state mapping
```

---

## Future Enhancements

- Clickable states to show detailed sync information
- Tooltips with state explanations
- Additional color coding beyond icons (background colors, bold text, etc.)
- State history tracking (show when state last changed)
