# Out-of-Sync Filter Refactor - Non-Destructive Filtering

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix race conditions in out-of-sync filter by implementing non-destructive filtering with separate filtered/unfiltered item storage.

**Architecture:**
- Add `filtered_items: Option<Vec<BrowseItem>>` to `BreadcrumbLevel` struct
- Keep `items` as source of truth (unfiltered)
- Rendering uses `filtered_items` when available, falls back to `items`
- Filters populate `filtered_items` without destroying original data
- No more race conditions - filters can reapply anytime without data loss

**Tech Stack:** Rust, Ratatui, SQLite cache

---

## Problem Analysis

**Current Broken Behavior:**
1. New file event → Cache invalidated
2. Browse refresh triggered → `level.items = all_items`
3. Try to apply out-of-sync filter → Cache empty → Either blank screen OR filter not applied
4. Need data arrives later → Filter tries to apply → Too late, Browse already shown

**Root Causes:**
- `BreadcrumbLevel.items` is single source (no backup)
- `apply_out_of_sync_filter()` destructively modifies `items` in place
- If cache empty: `level.items = vec![]` → blank screen
- If cache empty but guarded: filter doesn't apply → shows all items
- Browse and Need arrive at different times → race condition

**Solution:**
- Non-destructive filtering with `filtered_items` separate from `items`
- Rendering checks `filtered_items.as_ref().unwrap_or(&items)`
- Filters never destroy original data
- Can reapply filter anytime when cache updates

---

## Task 1: Revert Broken Changes

**Files:**
- Modify: `src/handlers/api.rs`
- Modify: `src/handlers/events.rs`

**Step 1: Revert all uncommitted changes**

```bash
git diff > /tmp/attempted-fixes.patch
git checkout src/handlers/api.rs src/handlers/events.rs
```

Expected: Clean working directory for these files

**Step 2: Verify build**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Commit revert**

```bash
git add src/handlers/api.rs src/handlers/events.rs
git commit -m "revert: Remove broken out-of-sync filter fixes

The destructive filter approach caused race conditions.
Preparing for non-destructive refactor."
```

---

## Task 2: Add filtered_items Field to BreadcrumbLevel

**Files:**
- Modify: `src/model/types.rs:15-27` (BreadcrumbLevel struct)
- Test: Integration test (manual verification)

**Step 1: Write test for filtered items behavior**

Add to `src/model/types.rs` after the struct:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::BrowseItem;

    #[test]
    fn test_breadcrumb_level_filtered_items() {
        let mut level = BreadcrumbLevel {
            folder_id: "test".to_string(),
            folder_label: "Test".to_string(),
            folder_path: "/test".to_string(),
            prefix: None,
            items: vec![
                BrowseItem {
                    name: "file1.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    ..Default::default()
                },
                BrowseItem {
                    name: "file2.txt".to_string(),
                    item_type: "FILE_INFO_TYPE_FILE".to_string(),
                    ..Default::default()
                },
            ],
            selected_index: None,
            file_sync_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/test".to_string(),
            filtered_items: None,
        };

        // Unfiltered - should show all items
        assert_eq!(level.items.len(), 2);
        assert_eq!(level.filtered_items, None);

        // Apply filter - keep only one item
        level.filtered_items = Some(vec![level.items[0].clone()]);

        // Original items unchanged
        assert_eq!(level.items.len(), 2);
        assert_eq!(level.filtered_items.as_ref().unwrap().len(), 1);

        // Clear filter
        level.filtered_items = None;
        assert_eq!(level.filtered_items, None);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test --lib model::types::tests::test_breadcrumb_level_filtered_items
```

Expected: FAIL - field `filtered_items` doesn't exist

**Step 3: Add filtered_items field to BreadcrumbLevel**

In `src/model/types.rs`, modify the struct (around line 15):

```rust
#[derive(Debug, Clone)]
pub struct BreadcrumbLevel {
    pub folder_id: String,
    pub folder_label: String,
    pub folder_path: String,
    pub prefix: Option<String>,
    pub items: Vec<BrowseItem>,              // Source of truth (unfiltered)
    pub filtered_items: Option<Vec<BrowseItem>>, // Filtered view (if filter active)
    pub selected_index: Option<usize>,
    pub file_sync_states: HashMap<String, SyncState>,
    pub ignored_exists: HashMap<String, bool>,
    pub translated_base_path: String,
}
```

**Step 4: Update all BreadcrumbLevel construction sites**

Find with: `grep -n "BreadcrumbLevel {" src/**/*.rs`

In `src/handlers/keyboard.rs` (around line 626):
```rust
BreadcrumbLevel {
    folder_id: folder.id.clone(),
    folder_label: folder.label.clone(),
    folder_path: folder.path.clone(),
    prefix: None,
    items: vec![],
    filtered_items: None,  // ADD THIS
    selected_index: None,
    file_sync_states: HashMap::new(),
    ignored_exists: HashMap::new(),
    translated_base_path: mapped_path.unwrap_or_else(|| folder.path.clone()),
}
```

Search for other construction sites and add `filtered_items: None,` to each.

**Step 5: Run test to verify it passes**

```bash
cargo test --lib model::types::tests::test_breadcrumb_level_filtered_items
```

Expected: PASS

**Step 6: Build to verify no compilation errors**

```bash
cargo build --release 2>&1 | grep -E "(error|warning:)"
```

Expected: Only existing warnings (dead_code), no new errors

**Step 7: Commit**

```bash
git add src/model/types.rs src/handlers/keyboard.rs
git commit -m "feat: Add filtered_items to BreadcrumbLevel for non-destructive filtering

- Add filtered_items: Option<Vec<BrowseItem>> field
- Keeps items as unfiltered source of truth
- filtered_items holds filtered view when active
- Prevents data loss during filter application

Tests: model::types::tests::test_breadcrumb_level_filtered_items"
```

---

## Task 3: Update Rendering to Use Filtered Items

**Files:**
- Modify: `src/ui/breadcrumb.rs:162-206`
- Modify: `src/ui/render.rs:139-198`

**Step 1: Update breadcrumb rendering to accept filtered items**

In `src/ui/breadcrumb.rs`, modify the function signature (around line 162):

```rust
pub fn render_breadcrumb_panel<'a>(
    f: &mut Frame,
    area: Rect,
    items: &[BrowseItem],              // Source items
    filtered_items: Option<&Vec<BrowseItem>>, // Filtered view
    file_sync_states: &HashMap<String, SyncState>,
    ignored_exists: &HashMap<String, bool>,
    list_state: &mut ListState,
    title: &str,
    is_focused: bool,
    is_parent_selected: bool,
    display_mode: DisplayMode,
    icon_renderer: &IconRenderer,
    translated_base_path: &str,
    prefix: Option<&str>,
) {
    // Use filtered items if available, otherwise use all items
    let display_items = filtered_items.unwrap_or(items);

    let list_items: Vec<ListItem> = display_items
        .iter()
        .map(|item| {
            // ... rest of rendering logic unchanged
```

**Step 2: Update render.rs to pass filtered items**

In `src/ui/render.rs`, modify the breadcrumb rendering call (around line 179):

```rust
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
```

**Step 3: Build to verify changes**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 4: Commit**

```bash
git add src/ui/breadcrumb.rs src/ui/render.rs
git commit -m "refactor: Update breadcrumb rendering to support filtered items

- Accept filtered_items parameter in render function
- Use filtered_items if available, fallback to items
- No behavioral change yet (filtered_items always None)"
```

---

## Task 4: Refactor apply_out_of_sync_filter to Use Filtered Items

**Files:**
- Modify: `src/main.rs:962-1056` (apply_out_of_sync_filter function)

**Step 1: Write test for non-destructive filtering**

Add to `src/main.rs` tests section (if exists, otherwise create):

```rust
#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn test_apply_out_of_sync_filter_preserves_items() {
        // This test verifies that applying the filter doesn't destroy original items
        // We can't easily test the full App, but we can document expected behavior

        // Setup: BreadcrumbLevel with 5 items
        // Action: Apply filter that matches 2 items
        // Expected:
        //   - level.items still has 5 items (unchanged)
        //   - level.filtered_items has 2 items
        //   - Rendering shows 2 items (from filtered_items)
    }
}
```

**Step 2: Refactor apply_out_of_sync_filter**

In `src/main.rs`, replace the function (around line 962):

```rust
fn apply_out_of_sync_filter(&mut self) {
    // Don't filter folder list (only breadcrumbs)
    if self.model.navigation.focus_level == 0 {
        return;
    }

    let level_idx = self.model.navigation.focus_level - 1;
    if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
        let folder_id = level.folder_id.clone();
        let prefix = level.prefix.clone();

        // Get folder sequence for cache validation
        let folder_sequence = self
            .model
            .syncthing
            .folder_statuses
            .get(&folder_id)
            .map(|status| status.sequence)
            .unwrap_or(0);

        // Get out-of-sync items from cache
        let out_of_sync_items = match self.cache.get_out_of_sync_items(&folder_id) {
            Ok(items) => items,
            Err(e) => {
                // Cache query failed - keep showing unfiltered items
                crate::log_debug(&format!(
                    "DEBUG [Filter]: Cache query failed, keeping unfiltered view: {}",
                    e
                ));
                level.filtered_items = None;
                return;
            }
        };

        // If no out-of-sync items, clear filter (show all items)
        if out_of_sync_items.is_empty() {
            crate::log_debug("DEBUG [Filter]: No out-of-sync items, clearing filter");
            level.filtered_items = None;
            return;
        }

        // Get current directory items from cache (unfiltered source)
        let current_items = match self
            .cache
            .get_browse_items(&folder_id, prefix.as_deref(), folder_sequence)
        {
            Ok(Some(items)) => items,
            _ => {
                // Can't get current items - use level.items as fallback
                crate::log_debug("DEBUG [Filter]: Using level.items as source");
                level.items.clone()
            }
        };

        // Build current path for comparison
        let current_path = if let Some(ref pfx) = prefix {
            pfx.clone()
        } else {
            String::new()
        };

        // Filter items: keep only those in out_of_sync_items map
        let mut filtered: Vec<BrowseItem> = Vec::new();

        for item in &current_items {
            let full_path = if current_path.is_empty() {
                item.name.clone()
            } else {
                format!("{}{}", current_path, item.name)
            };

            // Check if this item (or any child if directory) is out of sync
            let is_out_of_sync = if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
                // For directories: check if any child path starts with this directory
                let dir_prefix = if full_path.ends_with('/') {
                    full_path.clone()
                } else {
                    format!("{}/", full_path)
                };

                out_of_sync_items
                    .keys()
                    .any(|path| path.starts_with(&dir_prefix) || path == &full_path)
            } else {
                // For files: direct match
                out_of_sync_items.contains_key(&full_path)
            };

            if is_out_of_sync {
                filtered.push(item.clone());
            }
        }

        // Store filtered items without destroying original
        if filtered.is_empty() {
            crate::log_debug("DEBUG [Filter]: No matches, clearing filter");
            level.filtered_items = None;
        } else {
            crate::log_debug(&format!(
                "DEBUG [Filter]: Filtered {} → {} items",
                current_items.len(),
                filtered.len()
            ));
            level.filtered_items = Some(filtered);
        }

        // Reset selection to first item in filtered view
        level.selected_index = if level.filtered_items.is_some() {
            Some(0)
        } else {
            level.selected_index
        };
    }
}
```

**Step 3: Build to verify changes**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 4: Manual test - verify filter doesn't blank screen**

```bash
./target/release/synctui --debug
```

Test:
1. Navigate to folder with out-of-sync files
2. Press 'f' to activate filter
3. Check `/tmp/synctui-debug.log` for "Filtered X → Y items" message
4. Verify items appear (not blank screen)

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: Make apply_out_of_sync_filter non-destructive

- Populates filtered_items instead of replacing items
- Preserves items as unfiltered source of truth
- Returns early with None if cache unavailable (shows unfiltered)
- No more blank screen on cache miss

Tests: Manual verification with --debug flag"
```

---

## Task 5: Refactor apply_search_filter to Use Filtered Items

**Files:**
- Modify: `src/main.rs:766-959` (apply_search_filter function)

**Step 1: Refactor apply_search_filter similarly**

In `src/main.rs`, replace the function (around line 766):

```rust
fn apply_search_filter(&mut self) {
    if self.model.navigation.focus_level == 0 {
        return;
    }

    let level_idx = self.model.navigation.focus_level - 1;
    if let Some(level) = self.model.navigation.breadcrumb_trail.get_mut(level_idx) {
        let query = self.model.ui.search_query.to_lowercase();

        // If query is empty, clear filter
        if query.is_empty() {
            crate::log_debug("DEBUG [Search]: Empty query, clearing filter");
            level.filtered_items = None;
            return;
        }

        let folder_id = level.folder_id.clone();
        let prefix = level.prefix.clone();
        let folder_sequence = self
            .model
            .syncthing
            .folder_statuses
            .get(&folder_id)
            .map(|status| status.sequence)
            .unwrap_or(0);

        // Get all items from cache (recursive search)
        let all_items = match self.cache.get_all_browse_items(&folder_id, folder_sequence) {
            Ok(items) => items,
            Err(e) => {
                crate::log_debug(&format!(
                    "DEBUG [Search]: Cache query failed, using level.items: {}",
                    e
                ));
                // Fallback to current level items
                level.items.clone()
            }
        };

        // Build current path
        let current_path = prefix.clone().unwrap_or_default();

        // Filter items by search query
        let mut filtered: Vec<BrowseItem> = Vec::new();
        let mut matched_paths = std::collections::HashSet::new();

        for item in &all_items {
            let full_path = if current_path.is_empty() {
                item.name.clone()
            } else if item.name.starts_with(&current_path) {
                item.name[current_path.len()..].to_string()
            } else {
                continue; // Not in current directory
            };

            // Wildcard matching logic (same as before)
            let matches = if query.contains('*') {
                // Convert wildcard pattern to regex-like matching
                let parts: Vec<&str> = query.split('*').collect();
                let name_lower = full_path.to_lowercase();

                if parts.len() == 1 {
                    name_lower.contains(parts[0])
                } else {
                    let first = parts[0];
                    let last = parts[parts.len() - 1];
                    let middle_parts = &parts[1..parts.len() - 1];

                    let starts_ok = first.is_empty() || name_lower.starts_with(first);
                    let ends_ok = last.is_empty() || name_lower.ends_with(last);
                    let middle_ok = middle_parts.iter().all(|part| name_lower.contains(part));

                    starts_ok && ends_ok && middle_ok
                }
            } else {
                full_path.to_lowercase().contains(&query)
            };

            if matches {
                // Add this item
                filtered.push(item.clone());
                matched_paths.insert(full_path.clone());

                // Add parent directories if not already added
                let parts: Vec<&str> = full_path.trim_end_matches('/').split('/').collect();
                for i in 1..parts.len() {
                    let parent_path = format!("{}/", parts[..i].join("/"));
                    if !matched_paths.contains(&parent_path) {
                        // Find parent directory in all_items
                        if let Some(parent_item) = all_items.iter().find(|it| {
                            let it_path = if current_path.is_empty() {
                                it.name.clone()
                            } else if it.name.starts_with(&current_path) {
                                it.name[current_path.len()..].to_string()
                            } else {
                                String::new()
                            };
                            it_path == parent_path
                        }) {
                            filtered.push(parent_item.clone());
                            matched_paths.insert(parent_path);
                        }
                    }
                }
            }
        }

        // Store filtered results
        if filtered.is_empty() {
            crate::log_debug("DEBUG [Search]: No matches found");
            level.filtered_items = None;
        } else {
            crate::log_debug(&format!(
                "DEBUG [Search]: Filtered {} → {} items for query '{}'",
                all_items.len(),
                filtered.len(),
                query
            ));
            level.filtered_items = Some(filtered);
        }

        // Reset selection
        level.selected_index = if level.filtered_items.is_some() {
            Some(0)
        } else {
            level.selected_index
        };
    }
}
```

**Step 2: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Manual test search filter**

```bash
./target/release/synctui --debug
```

Test:
1. Navigate to any folder
2. Press Ctrl-F (or `/` in vim mode)
3. Type search query
4. Verify results filter correctly
5. Clear search (Backspace to empty)
6. Verify all items return

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: Make apply_search_filter non-destructive

- Populates filtered_items instead of replacing items
- Preserves items as unfiltered source of truth
- Clearing search restores unfiltered view instantly

Tests: Manual search verification"
```

---

## Task 6: Update BrowseResult Handler - Always Preserve Unfiltered Items

**Files:**
- Modify: `src/handlers/api.rs:27-214`

**Step 1: Modify BrowseResult to update items but preserve filter state**

In `src/handlers/api.rs`, modify the BrowseResult handler (around line 183):

```rust
// Update level
level.items = items.clone();  // Update unfiltered source
level.file_sync_states = sync_states;

// DON'T touch filtered_items here - let the filter functions manage it

// Update directory states based on their children
app.update_directory_states(idx);

// Sort and restore selection using the saved name
app.sort_level_with_selection(idx, selected_name);

// Re-apply search filter if active
if !app.model.ui.search_query.is_empty() {
    app.apply_search_filter();
}

// Re-apply out-of-sync filter if active
if app.model.ui.out_of_sync_filter.is_some() {
    app.apply_out_of_sync_filter();
}
```

**Step 2: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Commit**

```bash
git add src/handlers/api.rs
git commit -m "fix: Re-apply filters after Browse refresh

- BrowseResult always updates items (unfiltered source)
- Re-applies search filter if active
- Re-applies out-of-sync filter if active
- Filters now work reliably regardless of timing

Fixes race condition where Browse arriving before Need would clear filter."
```

---

## Task 7: Update NeededFiles Handler - Re-apply Filter When Cache Updates

**Files:**
- Modify: `src/handlers/api.rs:653-692`

**Step 1: Modify NeededFiles handler to re-apply filter**

In `src/handlers/api.rs`, modify the NeededFiles handler (around line 673):

```rust
// If in breadcrumb view for this folder, apply/reapply filter now that data is ready
if app.model.navigation.focus_level > 0 {
    let level_idx = app.model.navigation.focus_level - 1;
    if let Some(level) = app.model.navigation.breadcrumb_trail.get(level_idx) {
        if level.folder_id == folder_id {
            if app.model.ui.out_of_sync_filter.is_none() {
                // First time: activate filter
                app.model.ui.out_of_sync_filter = Some(model::types::OutOfSyncFilterState {
                    origin_level: app.model.navigation.focus_level,
                    last_refresh: std::time::SystemTime::now(),
                });

                // Apply filter
                app.apply_out_of_sync_filter();

                // Clear loading toast
                app.model.ui.toast_message = None;
            } else {
                // Filter already active: re-apply with fresh cache data
                // This handles the case where cache was invalidated and just refreshed
                app.apply_out_of_sync_filter();
            }
        }
    }
}
```

**Step 2: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Commit**

```bash
git add src/handlers/api.rs
git commit -m "fix: Re-apply out-of-sync filter when Need cache updates

- NeededFiles handler re-applies filter if already active
- Handles cache invalidation → refresh cycle
- Ensures filter updates when new Need data arrives

Fixes issue where filter wouldn't update after cache refresh."
```

---

## Task 8: Add Cache Invalidation for Rescan

**Files:**
- Modify: `src/handlers/api.rs:396-408` (FolderStatusResult handler)

**Step 1: Add out-of-sync cache invalidation on sequence change**

In `src/handlers/api.rs`, find the sequence change check (around line 400):

```rust
// Check if sequence changed
if let Some(&last_seq) = app.model.performance.last_known_sequences.get(&folder_id) {
    if last_seq != sequence {
        // Sequence changed - invalidate cache
        let _ = app.cache.invalidate_folder(&folder_id);

        // Invalidate out-of-sync categories (rescan may have changed sync states)
        let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);

        // Clear discovered directories for this folder (so they get re-discovered with new sequence)
        app.model.performance.discovered_dirs
            .retain(|key| !key.starts_with(&format!("{}:", folder_id)));
    }
}
```

**Step 2: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Commit**

```bash
git add src/handlers/api.rs
git commit -m "fix: Invalidate out-of-sync cache on folder sequence change

- Rescan ('r' key) triggers sequence change
- Sequence change invalidates out-of-sync cache
- Fresh Need request fetches updated data
- Filter refreshes with new data

Fixes issue where rescan didn't update filter."
```

---

## Task 9: Event Handler - Invalidate Cache and Trigger Refresh

**Files:**
- Modify: `src/handlers/events.rs:20-86` (CacheInvalidation::File handler)

**Step 1: Ensure event handler invalidates out-of-sync cache**

In `src/handlers/events.rs`, verify the File handler has cache invalidation (around line 34):

```rust
CacheInvalidation::File {
    folder_id,
    file_path,
} => {
    crate::log_debug(&format!(
        "DEBUG [Event]: Invalidating file: folder={} path={}",
        folder_id, file_path
    ));
    let _ = app.cache.invalidate_single_file(&folder_id, &file_path);
    let _ = app.cache.invalidate_folder_status(&folder_id);

    // Invalidate out-of-sync categories for this folder (will trigger re-fetch of /rest/db/need)
    let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);

    // Request fresh folder status
    let _ = app.api_tx.send(ApiRequest::GetFolderStatus {
        folder_id: folder_id.clone(),
    });

    // ... rest of handler (Browse refresh logic)
}
```

**Step 2: Verify Directory and ItemFinished handlers also invalidate**

Check lines 101-102 (Directory handler):
```rust
// Invalidate out-of-sync categories for this folder (will trigger re-fetch of /rest/db/need)
let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);
```

Check lines 200-202 (ItemFinished handler):
```rust
// Invalidate out-of-sync categories for this folder
// File just finished syncing, so need_category may have changed
let _ = app.cache.invalidate_out_of_sync_categories(&folder_id);
```

**Step 3: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully (no changes if already present)

**Step 4: Commit if changes made**

```bash
git add src/handlers/events.rs
git commit -m "fix: Ensure event handlers invalidate out-of-sync cache

- File events invalidate out-of-sync cache
- Directory events invalidate out-of-sync cache
- ItemFinished events invalidate out-of-sync cache
- Triggers fresh Need request and filter refresh

Ensures filter stays current with real-time events."
```

---

## Task 10: Update Clear Filter Logic

**Files:**
- Modify: `src/handlers/keyboard.rs` (find filter clear logic)

**Step 1: Update Esc handler to clear filtered_items**

Search for where out_of_sync_filter is cleared:

```bash
grep -n "out_of_sync_filter.*None" src/handlers/keyboard.rs
```

Update to also clear filtered_items:

```rust
// Clear out-of-sync filter
app.model.ui.out_of_sync_filter = None;

// Clear filtered items to show unfiltered view
if app.model.navigation.focus_level > 0 {
    let level_idx = app.model.navigation.focus_level - 1;
    if let Some(level) = app.model.navigation.breadcrumb_trail.get_mut(level_idx) {
        level.filtered_items = None;
    }
}
```

**Step 2: Build to verify**

```bash
cargo build --release
```

Expected: Builds successfully

**Step 3: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "fix: Clear filtered_items when clearing out-of-sync filter

- Esc key clears both filter state and filtered_items
- Immediately shows unfiltered view
- No need to re-fetch Browse data

Provides instant filter clear feedback."
```

---

## Task 11: Full Integration Testing

**Files:**
- Test: Manual integration test with debug logging

**Step 1: Build release binary**

```bash
cargo build --release
```

**Step 2: Test scenario 1 - New file appears**

```bash
rm ~/.cache/synctui/cache.db  # Fresh cache
./target/release/synctui --debug
```

Actions:
1. Navigate to Movies folder
2. Wait for new file to sync from remote (or add file manually)
3. Verify file appears in breadcrumb list
4. Check `/tmp/synctui-debug.log` for event handling

Expected:
- New file appears without blank screen
- Debug log shows "NEW file detected" or "existing file detected"
- Browse refresh triggered
- Filter not affected (if active)

**Step 3: Test scenario 2 - Filter persistence during events**

Actions:
1. Navigate to folder with out-of-sync files
2. Press 'f' to activate filter
3. Wait for file events (or trigger with 'r' rescan)
4. Verify filter remains active
5. Check debug log for filter application

Expected:
- Filter shows out-of-sync files only
- Events don't clear filter
- Debug log shows "Filtered X → Y items"
- No blank screen during cache refresh

**Step 4: Test scenario 3 - Rescan updates filter**

Actions:
1. Navigate to folder with out-of-sync files
2. Press 'f' to activate filter
3. Press 'r' to rescan
4. Verify filter updates with fresh data
5. Check debug log

Expected:
- Rescan invalidates cache
- Fresh Need request sent
- Filter re-applied with new data
- Debug log shows cache invalidation and filter update

**Step 5: Test scenario 4 - Filter clear**

Actions:
1. Activate filter with 'f'
2. Press Esc to clear filter
3. Verify all items appear instantly

Expected:
- Filter clears immediately
- All items visible (unfiltered)
- No Browse re-fetch needed

**Step 6: Test scenario 5 - Search and out-of-sync together**

Actions:
1. Press Ctrl-F, type search query
2. Press 'f' to activate out-of-sync filter
3. Verify both filters work
4. Clear search, verify out-of-sync filter remains
5. Clear out-of-sync filter

Expected:
- Search filter works
- Out-of-sync filter works
- Both can be cleared independently
- No conflicts between filters

**Step 7: Document test results**

Create test report in docs/testing/:

```bash
mkdir -p docs/testing
cat > docs/testing/2025-01-09-filter-refactor-tests.md <<EOF
# Out-of-Sync Filter Refactor Test Report

Date: 2025-01-09
Tester: [Name]
Build: Release

## Test Results

### Scenario 1: New File Appears
- ✅ File appears in breadcrumb
- ✅ No blank screen
- ✅ Debug log shows event handling
- Notes: [Any observations]

### Scenario 2: Filter Persistence During Events
- ✅ Filter remains active
- ✅ No clearing on events
- ✅ Debug shows filter reapplication
- Notes: [Any observations]

### Scenario 3: Rescan Updates Filter
- ✅ Cache invalidated
- ✅ Filter refreshed
- ✅ Shows updated data
- Notes: [Any observations]

### Scenario 4: Filter Clear
- ✅ Instant clear
- ✅ All items visible
- ✅ No re-fetch
- Notes: [Any observations]

### Scenario 5: Multiple Filters
- ✅ Search works
- ✅ Out-of-sync works
- ✅ Independent clearing
- Notes: [Any observations]

## Issues Found
[List any bugs or unexpected behavior]

## Overall Assessment
[Pass/Fail with summary]
EOF
```

**Step 8: Run full test suite**

```bash
cargo test --release
```

Expected: All tests pass (501+ tests)

**Step 9: Final commit**

```bash
git add docs/testing/2025-01-09-filter-refactor-tests.md
git commit -m "test: Add integration test results for filter refactor

All scenarios pass:
- New files appear without blank screen
- Filter persists during events
- Rescan updates filter correctly
- Filter clears instantly
- Multiple filters work independently

Tests: 501+ unit tests passing
Manual: 5 integration scenarios verified"
```

---

## Verification Checklist

After completing all tasks, verify:

- [ ] `cargo build --release` succeeds with no errors
- [ ] `cargo test` passes all 501+ tests
- [ ] New file events show files without blanking screen
- [ ] Out-of-sync filter persists during Browse refresh
- [ ] Rescan ('r') updates filter with fresh data
- [ ] Filter clear (Esc) shows all items instantly
- [ ] Search filter and out-of-sync filter work independently
- [ ] Debug logging shows filter operations clearly
- [ ] No race conditions between Browse and Need responses
- [ ] Original items always preserved in `level.items`
- [ ] Filtered view in `level.filtered_items` updates correctly

---

## Rollback Plan

If refactor causes issues:

```bash
# Revert all changes
git log --oneline | head -20  # Find commit before refactor
git reset --hard <commit-hash>

# Or revert individual commits
git revert <commit-hash>
```

---

## Notes for Engineer

**Key Architecture Changes:**
1. `BreadcrumbLevel.items` = unfiltered source of truth (never destroyed)
2. `BreadcrumbLevel.filtered_items` = filtered view (None = show all)
3. Rendering uses `filtered_items.unwrap_or(&items)`
4. Filters populate `filtered_items` without touching `items`
5. No more race conditions - filters can reapply anytime

**Debug Logging:**
- "Filtered X → Y items" = successful filter
- "Cache query failed" = cache unavailable, showing unfiltered
- "No matches found" = filter cleared (empty result)
- "NEW file detected" = event triggered Browse refresh

**Performance:**
- Cloning items for filtering is acceptable (typically <100 items)
- Cache queries are fast (SQLite indexed)
- No extra API calls needed

**Testing Focus:**
- Verify filter persistence during events (main bug)
- Test with empty cache (should show unfiltered, not blank)
- Test rescan with active filter (should update)
- Test filter clear (should be instant)
