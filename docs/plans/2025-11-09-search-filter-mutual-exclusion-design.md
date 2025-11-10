# Search and Filter Mutual Exclusion Design

**Date:** 2025-11-09
**Status:** Approved

## Problem Statement

Search and out-of-sync filter can currently be active simultaneously, which is confusing and not the intended behavior. Users may think they can search and filter at the same time, leading to unexpected results.

## Solution

Make search and out-of-sync filter mutually exclusive:
- Activating search clears any active filter
- Activating filter clears any active search
- Show toast notifications to make the mutual exclusion clear to users

## Core Principles

### Mutual Exclusion Pattern

**When activating out-of-sync filter** (pressing `f` in breadcrumbs):
- Check if search is active (`!search_query.is_empty()` or `search_mode`)
- If yes: Clear search completely, show toast "Search cleared - filter active"
- Proceed to activate filter

**When activating search** (pressing Ctrl-F or `/`):
- Check if filter is active (`out_of_sync_filter.is_some()`)
- If yes: Clear filter completely, show toast "Filter cleared - search active"
- Proceed to enter search mode

### Selection Preservation

When clearing filters, preserve the user's cursor position by name:
- **Preserve selection (true)**: Mutual exclusion cases, toggling off filter - user stays in same context
- **Don't preserve (false)**: Backing out to folder view, clearing stale filters - user leaving context

## Implementation

### New Helper Methods in `src/main.rs`

#### 1. Clear Search

```rust
/// Clear search state and filtered items
///
/// # Arguments
/// * `show_toast` - Optional toast message to display
fn clear_search(&mut self, show_toast: Option<&str>) {
    self.model.ui.search_query.clear();
    self.model.ui.search_mode = false;
    self.model.ui.search_origin_level = None;
    self.model.performance.discovered_dirs.clear();

    // Clear filtered items from all breadcrumb levels
    for level in &mut self.model.navigation.breadcrumb_trail {
        level.filtered_items = None;
    }

    if let Some(msg) = show_toast {
        self.model.ui.show_toast(msg);
    }
}
```

#### 2. Clear Filter

```rust
/// Clear out-of-sync filter state and filtered items
///
/// # Arguments
/// * `preserve_selection` - Whether to keep cursor on same item by name
/// * `show_toast` - Optional toast message to display
fn clear_out_of_sync_filter(&mut self, preserve_selection: bool, show_toast: Option<&str>) {
    self.model.ui.out_of_sync_filter = None;

    if preserve_selection {
        // Clear filtered items while preserving selection by name
        for level in &mut self.model.navigation.breadcrumb_trail {
            let selected_name = level.selected_index
                .and_then(|idx| level.display_items().get(idx))
                .map(|item| item.name.clone());

            level.filtered_items = None;

            if let Some(name) = selected_name {
                level.selected_index = logic::navigation::find_item_index_by_name(&level.items, &name);
            }
        }
    } else {
        // Simple clear without preserving selection
        for level in &mut self.model.navigation.breadcrumb_trail {
            level.filtered_items = None;
        }
    }

    if let Some(msg) = show_toast {
        self.model.ui.show_toast(msg);
    }
}
```

#### 3. Enter Search Mode

```rust
/// Enter search mode (handles mutual exclusion with filter)
fn enter_search_mode(&mut self) {
    // Only works in breadcrumb view
    if self.model.navigation.focus_level == 0 {
        return;
    }

    // Clear filter if active (mutual exclusion)
    if self.model.ui.out_of_sync_filter.is_some() {
        self.clear_out_of_sync_filter(true, Some("Filter cleared - search active"));
    }

    // Activate search mode
    self.model.ui.search_mode = true;
    self.model.ui.search_query.clear();
    self.model.ui.search_origin_level = Some(self.model.navigation.focus_level);
}
```

#### 4. Activate Filter

```rust
/// Activate out-of-sync filter (handles mutual exclusion with search)
fn activate_out_of_sync_filter(&mut self) {
    // Only works in breadcrumb view
    if self.model.navigation.focus_level == 0 {
        return;
    }

    // Clear search if active (mutual exclusion)
    if !self.model.ui.search_query.is_empty() || self.model.ui.search_mode {
        self.clear_search(Some("Search cleared - filter active"));
    }

    // Toggle the filter
    self.toggle_out_of_sync_filter();
}
```

### Refactoring Existing Code

#### File: `src/handlers/keyboard.rs`

**1. Esc in search mode (lines 483-495):**
```rust
KeyCode::Esc => {
    app.clear_search(None);  // No toast, user explicitly pressed Esc
    return Ok(());
}
```

**2. Esc in breadcrumbs with search active (lines 579-592):**
```rust
if app.model.navigation.focus_level > 0 && !app.model.ui.search_query.is_empty() {
    app.clear_search(None);  // No toast, user explicitly pressed Esc
    // ... refresh logic stays ...
}
```

**3. Ctrl-F enter search (lines 617-626):**
```rust
KeyCode::Char('f') if !app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
    app.enter_search_mode();
}
```

**4. `/` enter search - vim mode (lines 727-733):**
```rust
KeyCode::Char('/') if app.model.ui.vim_mode => {
    app.enter_search_mode();
}
```

**5. `f` toggle filter in breadcrumbs (line 690):**
```rust
KeyCode::Char('f') if app.model.navigation.focus_level > 0 => {
    app.activate_out_of_sync_filter();
}
```

#### File: `src/app/navigation.rs`

**1. Context-aware search clearing (lines 343-353):**
```rust
if should_clear_search {
    self.clear_search(None);  // No toast, contextual clearing

    // Refresh breadcrumbs logic stays...
    self.refresh_all_breadcrumbs().await?;
}
```

**2. Clear filter when backing out to folder view (lines 389-395):**
```rust
if self.model.ui.out_of_sync_filter.is_some() {
    self.clear_out_of_sync_filter(false, None);  // Don't preserve (leaving breadcrumbs), no toast
}
```

#### File: `src/main.rs`

**1. Toggling filter off (lines 1237-1256):**
```rust
if self.model.ui.out_of_sync_filter.is_some() {
    self.clear_out_of_sync_filter(true, None);  // Preserve selection, no toast (user toggling off)
    return;
}
```

**2. Clear stale filter (line 1260):**
```rust
// Clear any stale filter from a different folder/level
self.clear_out_of_sync_filter(false, None);  // Don't preserve (stale context), no toast
```

#### File: `src/model/ui.rs`

**Line 142 (`close_all_modals()`):**
```rust
// Keep as-is - just clear fields directly
// This is modal cleanup in Model layer, not user-initiated action
self.search_mode = false;
self.search_query.clear();
self.search_origin_level = None;
```

## Benefits

### Code Quality
- **~50 lines of duplicate code eliminated**
- **Single source of truth** for clearing logic
- **Consistent behavior** across all clearing operations
- **Easier maintenance** - changes in one place

### User Experience
- **Clear feedback** via toast notifications
- **Prevents confusion** about what's active
- **Preserves cursor position** when appropriate
- **Predictable behavior** - activating one always clears the other

## Testing Considerations

### Manual Testing Scenarios

1. **Search → Filter:**
   - Enter search, type query
   - Press `f` to activate filter
   - ✓ Search cleared, toast shown, filter active

2. **Filter → Search:**
   - Activate filter with `f`
   - Press Ctrl-F or `/`
   - ✓ Filter cleared, toast shown, search mode active

3. **Selection preservation:**
   - Filter active, cursor on specific file
   - Toggle filter off with `f`
   - ✓ Cursor stays on same file by name

4. **Esc behavior:**
   - Search active with query
   - Press Esc
   - ✓ Search cleared, no toast (explicit user action)

5. **Context changes:**
   - Search active in subdirectory
   - Back out to folder view
   - ✓ Search cleared automatically, no toast

### Edge Cases

- Empty search query but search mode active
- Filter active but no out-of-sync items
- Rapid toggling between search and filter
- Backing out multiple levels with active filters

## Notes

### Why Toast Notifications?

Users might try to use both features simultaneously. Without feedback, they'd be confused why their previous filter/search disappeared. Toasts make the mutual exclusion explicit and educate users about the behavior.

### Why Preserve Selection?

When a user is actively working with filtered items and clears the filter, they want to stay on the same item. Jumping to a different item because the index changed would be disorienting.

### When NOT to Preserve Selection?

When the user is leaving the context entirely (backing out to folder view), preserving breadcrumb selection is meaningless since they're no longer in breadcrumbs.
