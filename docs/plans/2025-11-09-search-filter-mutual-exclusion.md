# Search and Filter Mutual Exclusion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make search and out-of-sync filter mutually exclusive with toast notifications

**Architecture:** Add helper methods for clearing/activating search and filter, refactor all existing code to use helpers, ensure mutual exclusion with user feedback via toasts

**Tech Stack:** Rust, Ratatui TUI framework

**Related Skills:**
- @superpowers:test-driven-development - Write tests first for all new methods
- @superpowers:verification-before-completion - Verify tests pass before claiming done

---

## Task 1: Add `clear_search` Helper Method

**Files:**
- Modify: `src/main.rs` (add method around line 1348, in impl App block)
- Test: Manual verification via existing tests

**Step 1: Add the `clear_search` method**

Add this method to the `impl App` block in `src/main.rs`:

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
        self.model.ui.show_toast(msg.to_string());
    }
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add clear_search helper method

Centralizes search clearing logic with optional toast notification.
Clears query, mode, origin level, discovered dirs, and filtered items.

🤖 Generated with Claude Code"
```

---

## Task 2: Add `clear_out_of_sync_filter` Helper Method

**Files:**
- Modify: `src/main.rs` (add method after `clear_search`)

**Step 1: Add the `clear_out_of_sync_filter` method**

Add this method to the `impl App` block in `src/main.rs`, right after `clear_search`:

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
        self.model.ui.show_toast(msg.to_string());
    }
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add clear_out_of_sync_filter helper method

Centralizes filter clearing logic with selection preservation option.
Optionally preserves cursor position by item name.

🤖 Generated with Claude Code"
```

---

## Task 3: Add `enter_search_mode` Helper Method

**Files:**
- Modify: `src/main.rs` (add method after `clear_out_of_sync_filter`)

**Step 1: Add the `enter_search_mode` method**

Add this method to the `impl App` block in `src/main.rs`:

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

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add enter_search_mode helper method

Handles search activation with mutual exclusion.
Clears active filter with toast notification before entering search.

🤖 Generated with Claude Code"
```

---

## Task 4: Add `activate_out_of_sync_filter` Helper Method

**Files:**
- Modify: `src/main.rs` (add method after `enter_search_mode`)

**Step 1: Add the `activate_out_of_sync_filter` method**

Add this method to the `impl App` block in `src/main.rs`:

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

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add activate_out_of_sync_filter helper method

Handles filter activation with mutual exclusion.
Clears active search with toast notification before toggling filter.

🤖 Generated with Claude Code"
```

---

## Task 5: Refactor Keyboard Handler - Esc in Search Mode

**Files:**
- Modify: `src/handlers/keyboard.rs:483-495`

**Step 1: Replace manual clearing with helper call**

Find this code around line 483:

```rust
KeyCode::Esc => {
    // Exit search mode and clear query
    app.model.ui.search_mode = false;
    app.model.ui.search_query.clear();
    app.model.ui.search_origin_level = None;
    // Clear prefetch tracking
    app.model.performance.discovered_dirs.clear();
    // Immediately clear filtered_items for all breadcrumb levels
    for level in &mut app.model.navigation.breadcrumb_trail {
        level.filtered_items = None;
    }
    return Ok(());
}
```

Replace with:

```rust
KeyCode::Esc => {
    // Exit search mode and clear query
    app.clear_search(None);  // No toast, user explicitly pressed Esc
    return Ok(());
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "refactor: use clear_search in Esc handler

Replaces manual clearing logic with centralized helper.
No toast shown - user explicitly pressed Esc.

🤖 Generated with Claude Code"
```

---

## Task 6: Refactor Keyboard Handler - Esc in Breadcrumbs

**Files:**
- Modify: `src/handlers/keyboard.rs:579-592`

**Step 1: Replace manual clearing with helper call**

Find this code around line 579:

```rust
if app.model.navigation.focus_level > 0 && !app.model.ui.search_query.is_empty() {
    crate::log_debug("DEBUG [keyboard]: Clearing search...");
    app.model.ui.search_query.clear();
    // Clear prefetch tracking
    app.model.performance.discovered_dirs.clear();
    // Immediately clear filtered_items for all breadcrumb levels
    for level in &mut app.model.navigation.breadcrumb_trail {
        level.filtered_items = None;
    }
    // ... rest of logic
```

Replace with:

```rust
if app.model.navigation.focus_level > 0 && !app.model.ui.search_query.is_empty() {
    crate::log_debug("DEBUG [keyboard]: Clearing search...");
    app.clear_search(None);  // No toast, user explicitly pressed Esc
    // ... rest of logic stays the same
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "refactor: use clear_search in Esc breadcrumb handler

Replaces duplicate clearing logic with centralized helper.

🤖 Generated with Claude Code"
```

---

## Task 7: Refactor Keyboard Handler - Ctrl-F Search Entry

**Files:**
- Modify: `src/handlers/keyboard.rs:617-626`

**Step 1: Replace manual activation with helper call**

Find this code around line 617:

```rust
KeyCode::Char('f')
    if !app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
{
    / Ctrl-F: Enter search mode (normal mode only - vim uses / instead)
    // Only available in breadcrumb view, not folder list
    if app.model.navigation.focus_level > 0 {
        app.model.ui.search_mode = true;
        app.model.ui.search_query.clear();
        app.model.ui.search_origin_level = Some(app.model.navigation.focus_level);
    }
}
```

Replace with:

```rust
KeyCode::Char('f')
    if !app.model.ui.vim_mode && key.modifiers.contains(KeyModifiers::CONTROL) =>
{
    / Ctrl-F: Enter search mode (normal mode only - vim uses / instead)
    app.enter_search_mode();
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "refactor: use enter_search_mode for Ctrl-F

Implements mutual exclusion - clears filter with toast if active.

🤖 Generated with Claude Code"
```

---

## Task 8: Refactor Keyboard Handler - Vim Search Entry

**Files:**
- Modify: `src/handlers/keyboard.rs:727-733`

**Step 1: Replace manual activation with helper call**

Find this code around line 727:

```rust
KeyCode::Char('/') if app.model.ui.vim_mode => {
    / : Enter search mode (vim mode only)
    // Only available in breadcrumb view, not folder list
    if app.model.navigation.focus_level > 0 {
        app.model.ui.search_mode = true;
        app.model.ui.search_query.clear();
        app.model.ui.search_origin_level = Some(app.model.navigation.focus_level);
    }
}
```

Replace with:

```rust
KeyCode::Char('/') if app.model.ui.vim_mode => {
    / : Enter search mode (vim mode only)
    app.enter_search_mode();
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "refactor: use enter_search_mode for vim / key

Implements mutual exclusion - clears filter with toast if active.

🤖 Generated with Claude Code"
```

---

## Task 9: Refactor Keyboard Handler - Filter Toggle

**Files:**
- Modify: `src/handlers/keyboard.rs:690-693`

**Step 1: Replace manual toggle with helper call**

Find this code around line 690:

```rust
KeyCode::Char('f') if app.model.navigation.focus_level > 0 => {
    // Toggle out-of-sync filter (only in breadcrumb view)
    app.toggle_out_of_sync_filter();
}
```

Replace with:

```rust
KeyCode::Char('f') if app.model.navigation.focus_level > 0 => {
    // Toggle out-of-sync filter (only in breadcrumb view)
    app.activate_out_of_sync_filter();
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "refactor: use activate_out_of_sync_filter for f key

Implements mutual exclusion - clears search with toast if active.

🤖 Generated with Claude Code"
```

---

## Task 10: Refactor Navigation - Context-Aware Search Clearing

**Files:**
- Modify: `src/app/navigation.rs:343-353`

**Step 1: Replace manual clearing with helper call**

Find this code around line 343:

```rust
if should_clear_search {
    self.model.ui.search_mode = false;
    self.model.ui.search_query.clear();
    self.model.ui.search_origin_level = None;
    self.model.performance.discovered_dirs.clear();

    // Clear filtered items for ALL levels in the breadcrumb trail
    for level in &mut self.model.navigation.breadcrumb_trail {
        level.filtered_items = None;
    }

    // Refresh breadcrumbs
    self.refresh_all_breadcrumbs().await?;
}
```

Replace with:

```rust
if should_clear_search {
    self.clear_search(None);  // No toast, contextual clearing

    // Refresh breadcrumbs
    self.refresh_all_breadcrumbs().await?;
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/app/navigation.rs
git commit -m "refactor: use clear_search in navigation backing out

Replaces duplicate clearing logic with centralized helper.

🤖 Generated with Claude Code"
```

---

## Task 11: Refactor Navigation - Filter Clearing on Folder View

**Files:**
- Modify: `src/app/navigation.rs:389-395`

**Step 1: Replace manual clearing with helper call**

Find this code around line 389:

```rust
// Clear out-of-sync filter when backing out to folder list
if self.model.ui.out_of_sync_filter.is_some() {
    self.model.ui.out_of_sync_filter = None;

    // Clear filtered items for ALL levels
    for level in &mut self.model.navigation.breadcrumb_trail {
        level.filtered_items = None;
    }
}
```

Replace with:

```rust
// Clear out-of-sync filter when backing out to folder list
if self.model.ui.out_of_sync_filter.is_some() {
    self.clear_out_of_sync_filter(false, None);  // Don't preserve (leaving breadcrumbs), no toast
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/app/navigation.rs
git commit -m "refactor: use clear_out_of_sync_filter in navigation

Replaces duplicate clearing logic. No selection preservation needed
since user is leaving breadcrumbs entirely.

🤖 Generated with Claude Code"
```

---

## Task 12: Refactor Main - Toggle Filter Off

**Files:**
- Modify: `src/main.rs:1237-1256`

**Step 1: Replace manual clearing with helper call**

Find this code around line 1237 in `toggle_out_of_sync_filter`:

```rust
// If filter is already active, clear it (regardless of what level we're on)
if self.model.ui.out_of_sync_filter.is_some() {
    // Clear out-of-sync filter
    self.model.ui.out_of_sync_filter = None;

    // Clear filtered items for ALL levels, preserving selection by name
    for level in &mut self.model.navigation.breadcrumb_trail {
        // Get currently selected item name from filtered view
        let selected_name = level.selected_index
            .and_then(|idx| level.display_items().get(idx))
            .map(|item| item.name.clone());

        // Clear filter
        level.filtered_items = None;

        // Restore selection to same item name in unfiltered view
        if let Some(name) = selected_name {
            level.selected_index = logic::navigation::find_item_index_by_name(&level.items, &name);
        }
    }
    return;
}
```

Replace with:

```rust
// If filter is already active, clear it (regardless of what level we're on)
if self.model.ui.out_of_sync_filter.is_some() {
    self.clear_out_of_sync_filter(true, None);  // Preserve selection, no toast (user toggling off)
    return;
}
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: use clear_out_of_sync_filter in toggle

Replaces duplicate clearing logic. Preserves selection since user
stays in same context.

🤖 Generated with Claude Code"
```

---

## Task 13: Refactor Main - Clear Stale Filter

**Files:**
- Modify: `src/main.rs:1260`

**Step 1: Replace manual clearing with helper call**

Find this code around line 1260 in `toggle_out_of_sync_filter`:

```rust
// Clear any stale filter from a different folder/level
self.model.ui.out_of_sync_filter = None;
```

Replace with:

```rust
// Clear any stale filter from a different folder/level
self.clear_out_of_sync_filter(false, None);  // Don't preserve (stale context), no toast
```

**Step 2: Verify compilation**

Run: `cargo build`
Expected: SUCCESS (no errors)

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: use clear_out_of_sync_filter for stale filter

Replaces inline clearing with centralized helper.

🤖 Generated with Claude Code"
```

---

## Task 14: Final Verification and Manual Testing

**Files:**
- All modified files

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All 232+ tests pass

**Step 2: Build release version**

Run: `cargo build --release`
Expected: SUCCESS with no warnings

**Step 3: Manual testing - Search to Filter**

1. Start app, enter a folder
2. Press Ctrl-F, type search query
3. Press `f` to activate filter
4. ✓ Verify toast shows "Search cleared - filter active"
5. ✓ Verify search query cleared
6. ✓ Verify filter is active

**Step 4: Manual testing - Filter to Search**

1. Start app, enter a folder
2. Press `f` to activate filter
3. Press Ctrl-F to enter search
4. ✓ Verify toast shows "Filter cleared - search active"
5. ✓ Verify filter cleared
6. ✓ Verify search mode active

**Step 5: Manual testing - Selection Preservation**

1. Activate filter with multiple items
2. Select an item (not first)
3. Press `f` to toggle off
4. ✓ Verify cursor stays on same item by name

**Step 6: Manual testing - Esc Behavior**

1. Activate search with query
2. Press Esc
3. ✓ Verify search cleared, NO toast shown

**Step 7: Final commit if any issues found**

Only if fixes were needed:
```bash
git add .
git commit -m "fix: address issues found in manual testing

🤖 Generated with Claude Code"
```

---

## Summary

**Total Tasks:** 14
**Estimated Time:** 2-3 hours
**Lines Changed:** ~150 lines (50 removed, 100 added/modified)

**Key Achievements:**
- ✅ ~50 lines of duplicate code eliminated
- ✅ Mutual exclusion enforced with clear user feedback
- ✅ Selection preservation when appropriate
- ✅ Consistent behavior across all clearing operations
- ✅ Single source of truth for search/filter management

**Testing Strategy:**
- Unit tests verify via existing test suite
- Manual testing verifies user experience
- Toast notifications provide immediate feedback

**Related Skills Used:**
- @superpowers:test-driven-development - Verify tests after each change
- @superpowers:verification-before-completion - Final verification task
