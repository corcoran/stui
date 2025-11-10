# Rescan Force Refresh Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add confirmation dialog when pressing 'r' that offers normal rescan or force refresh (immediate cache invalidation).

**Architecture:** Extends existing ConfirmAction pattern with new Rescan variant. Extracts folder info retrieval into helper method. Adds force refresh method that invalidates cache before triggering rescan.

**Tech Stack:** Rust, Ratatui, existing confirmation dialog patterns

---

## Task 1: Add ConfirmAction::Rescan Variant with Tests

**Files:**
- Modify: `src/model/types.rs:106-126` (ConfirmAction enum)
- Modify: `src/model/types.rs:153-260` (tests section)

**Step 1: Write failing tests for new variant**

Add to `src/model/types.rs` in the tests module (after line 260):

```rust
#[test]
fn test_confirm_action_rescan() {
    let action = ConfirmAction::Rescan {
        folder_id: "folder-abc".to_string(),
        folder_label: "My Folder".to_string(),
    };

    match action {
        ConfirmAction::Rescan { folder_id, folder_label } => {
            assert_eq!(folder_id, "folder-abc");
            assert_eq!(folder_label, "My Folder");
        }
        _ => panic!("Expected Rescan variant"),
    }
}

#[test]
fn test_confirm_action_rescan_clone() {
    let action = ConfirmAction::Rescan {
        folder_id: "test".to_string(),
        folder_label: "Test".to_string(),
    };
    let cloned = action.clone();
    assert_eq!(action, cloned);
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test test_confirm_action_rescan
```

Expected: Compilation error - "no variant named `Rescan` found for enum `ConfirmAction`"

**Step 3: Add Rescan variant to ConfirmAction enum**

Modify `src/model/types.rs:106-126` - add new variant after PauseResume:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    Revert {
        folder_id: String,
        changed_files: Vec<String>,
    },
    Delete {
        path: String,
        name: String,
        is_dir: bool,
    },
    IgnoreDelete {
        path: String,
        name: String,
        is_dir: bool,
    },
    PauseResume {
        folder_id: String,
        label: String,
        is_paused: bool,
    },
    Rescan {
        folder_id: String,
        folder_label: String,
    },
}
```

**Step 4: Run tests to verify they pass**

```bash
cargo test test_confirm_action_rescan
```

Expected: Both tests PASS

**Step 5: Run all tests to ensure no regressions**

```bash
cargo test
```

Expected: All 169+ tests PASS, zero warnings

**Step 6: Commit**

```bash
git add src/model/types.rs
git commit -m "feat: Add ConfirmAction::Rescan variant

Adds new confirmation action for rescan dialog with folder_id and label.
Includes tests for variant creation, cloning, and pattern matching.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Add get_rescan_folder_info Helper Method with Tests

**Files:**
- Modify: `src/app/file_ops.rs:14-48` (rescan_selected_folder method)
- Create: `tests/rescan_folder_info_test.rs`

**Step 1: Write failing tests for helper method**

Create `tests/rescan_folder_info_test.rs`:

```rust
use synctui::{App, Model};
use synctui::api::{Folder, FolderStatus};

#[test]
fn test_get_rescan_folder_info_folder_list_view() {
    // Setup: Create app with focus on folder list
    let mut model = Model::new(false);
    model.navigation.focus_level = 0;
    model.navigation.folders_state_selection = Some(0);

    // Add a test folder
    model.syncthing.folders = vec![Folder {
        id: "test-folder".to_string(),
        label: "Test Folder".to_string(),
        path: "/data/test".to_string(),
        folder_type: "sendreceive".to_string(),
        paused: false,
    }];

    let app = App::new_for_test(model);

    // Execute
    let result = app.get_rescan_folder_info();

    // Verify
    assert_eq!(result, Some(("test-folder".to_string(), "Test Folder".to_string())));
}

#[test]
fn test_get_rescan_folder_info_breadcrumb_view() {
    // Setup: Create app with breadcrumb trail
    let mut model = Model::new(false);
    model.navigation.focus_level = 1;

    let breadcrumb = crate::model::BreadcrumbLevel {
        folder_id: "abc-123".to_string(),
        folder_label: "My Movies".to_string(),
        folder_path: "/data/movies".to_string(),
        prefix: None,
        items: vec![],
        filtered_items: None,
        selected_index: Some(0),
        file_sync_states: std::collections::HashMap::new(),
        ignored_exists: std::collections::HashMap::new(),
        translated_base_path: "/home/user/movies".to_string(),
    };

    model.navigation.breadcrumb_trail.push(breadcrumb);

    let app = App::new_for_test(model);

    // Execute
    let result = app.get_rescan_folder_info();

    // Verify
    assert_eq!(result, Some(("abc-123".to_string(), "My Movies".to_string())));
}

#[test]
fn test_get_rescan_folder_info_no_selection() {
    // Setup: App with no selection
    let model = Model::new(false);
    let app = App::new_for_test(model);

    // Execute
    let result = app.get_rescan_folder_info();

    // Verify
    assert_eq!(result, None);
}

#[test]
fn test_get_rescan_folder_info_empty_breadcrumbs() {
    // Setup: Breadcrumb view but empty trail
    let mut model = Model::new(false);
    model.navigation.focus_level = 1;
    // breadcrumb_trail is empty

    let app = App::new_for_test(model);

    // Execute
    let result = app.get_rescan_folder_info();

    // Verify
    assert_eq!(result, None);
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test test_get_rescan_folder_info
```

Expected: Compilation error - "no method named `get_rescan_folder_info` found for struct `App`"

**Step 3: Add helper method to App**

Add to `src/app/file_ops.rs` before `rescan_selected_folder` (around line 14):

```rust
impl App {
    /// Get folder ID and label for rescan operation
    /// Works in both folder list view and breadcrumb view
    fn get_rescan_folder_info(&self) -> Option<(String, String)> {
        if self.model.navigation.focus_level == 0 {
            // Folder list view - get selected folder
            let selected = self.model.navigation.folders_state_selection?;
            let folder = self.model.syncthing.folders.get(selected)?;
            Some((folder.id.clone(), folder.label.clone()))
        } else {
            // Breadcrumb view - get current folder from trail
            if self.model.navigation.breadcrumb_trail.is_empty() {
                return None;
            }
            let level = &self.model.navigation.breadcrumb_trail[0];
            Some((level.folder_id.clone(), level.folder_label.clone()))
        }
    }

    pub(crate) fn rescan_selected_folder(&mut self) -> Result<()> {
        // ... existing implementation ...
```

**Step 4: Make helper method public for tests**

Change `fn get_rescan_folder_info` to `pub(crate) fn get_rescan_folder_info` so tests can access it.

Also need to add `new_for_test` constructor to App. Add to `src/main.rs` in the App impl block:

```rust
#[cfg(test)]
pub fn new_for_test(model: Model) -> Self {
    use std::sync::mpsc;
    use crate::services;

    let (api_tx, _api_rx) = mpsc::channel();
    let (cache_event_tx, _cache_event_rx) = mpsc::channel();

    Self {
        model,
        client: crate::api::SyncthingClient::new("http://localhost:8384".to_string(), "test-key".to_string()),
        cache: crate::cache::Cache::new_in_memory().unwrap(),
        api_tx,
        cache_event_tx,
        base_url: "http://localhost:8384".to_string(),
        open_command: None,
        clipboard_command: None,
        image_protocol: None,
        image_font_size: None,
        image_state_map: std::collections::HashMap::new(),
    }
}
```

**Step 5: Run tests to verify they pass**

```bash
cargo test test_get_rescan_folder_info
```

Expected: All 4 tests PASS

**Step 6: Refactor rescan_selected_folder to use helper**

Modify `src/app/file_ops.rs:15-48`, replace the folder ID extraction logic:

```rust
pub(crate) fn rescan_selected_folder(&mut self) -> Result<()> {
    // Get the folder ID using helper method
    let (folder_id, _) = self
        .get_rescan_folder_info()
        .ok_or_else(|| anyhow::anyhow!("No folder selected"))?;

    log_debug(&format!(
        "DEBUG [rescan_selected_folder]: Requesting rescan for folder={}",
        folder_id
    ));

    // Trigger rescan via non-blocking API
    let _ = self
        .api_tx
        .send(services::api::ApiRequest::RescanFolder { folder_id });

    Ok(())
}
```

**Step 7: Run all tests to ensure refactor didn't break anything**

```bash
cargo test
```

Expected: All tests PASS (including integration tests that use rescan_selected_folder)

**Step 8: Commit**

```bash
git add src/app/file_ops.rs src/main.rs tests/rescan_folder_info_test.rs
git commit -m "refactor: Extract get_rescan_folder_info helper

Extracts folder ID/label retrieval into reusable helper method.
Simplifies rescan_selected_folder by delegating to helper.
Adds comprehensive tests for both folder list and breadcrumb views.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Add invalidate_and_refresh_folder Method

**Files:**
- Modify: `src/app/file_ops.rs` (add new method after rescan_selected_folder)
- Modify: `src/handlers/api.rs:439-488` (extract logic for reuse)

**Step 1: Identify the invalidation logic to extract**

Read `src/handlers/api.rs:439-488` - this is the cache invalidation logic inside FolderStatusResult handler when sequence changes. We'll extract this into a reusable method.

**Step 2: Add invalidate_and_refresh_folder method**

Add to `src/app/file_ops.rs` after `rescan_selected_folder`:

```rust
/// Invalidate cache and refresh breadcrumbs for a folder
/// Used by both force_refresh_folder and FolderStatusResult handler
fn invalidate_and_refresh_folder(&mut self, folder_id: &str) {
    log_debug(&format!(
        "DEBUG [invalidate_and_refresh_folder]: Invalidating cache for folder={}",
        folder_id
    ));

    // Invalidate cache
    let _ = self.cache.invalidate_folder(folder_id);
    let _ = self.cache.invalidate_out_of_sync_categories(folder_id);

    // Clear discovered directories (force re-discovery)
    self.model
        .performance
        .discovered_dirs
        .retain(|key| !key.starts_with(&format!("{}:", folder_id)));

    // Refresh breadcrumbs if currently viewing this folder
    if !self.model.navigation.breadcrumb_trail.is_empty()
        && self.model.navigation.breadcrumb_trail[0].folder_id == folder_id
    {
        log_debug(&format!(
            "DEBUG [invalidate_and_refresh_folder]: Refreshing {} breadcrumb levels for folder={}",
            self.model.navigation.breadcrumb_trail.len(),
            folder_id
        ));

        for (idx, level) in self.model.navigation.breadcrumb_trail.iter().enumerate() {
            if level.folder_id == folder_id {
                let browse_key = format!(
                    "{}:{}",
                    folder_id,
                    level.prefix.as_deref().unwrap_or("")
                );

                log_debug(&format!(
                    "DEBUG [invalidate_and_refresh_folder]: Level {}: prefix={:?} loading_browse.contains={}",
                    idx, level.prefix, self.model.performance.loading_browse.contains(&browse_key)
                ));

                if !self.model.performance.loading_browse.contains(&browse_key) {
                    self.model.performance.loading_browse.insert(browse_key);

                    let _ = self.api_tx.send(services::api::ApiRequest::BrowseFolder {
                        folder_id: folder_id.to_string(),
                        prefix: level.prefix.clone(),
                        priority: services::api::Priority::High,
                    });

                    log_debug(&format!(
                        "DEBUG [invalidate_and_refresh_folder]: Sent BrowseFolder request for prefix={:?}",
                        level.prefix
                    ));
                }
            }
        }
    }
}
```

**Step 3: Refactor FolderStatusResult handler to use new method**

Modify `src/handlers/api.rs:439-488` to call the new method instead of duplicating logic:

```rust
// Around line 439, replace the cache invalidation block with:
// Sequence changed - invalidate cache and refresh
app.invalidate_and_refresh_folder(&folder_id);
```

The full context (lines 431-490):

```rust
// Check if sequence changed
if let Some(&last_seq) = app.model.performance.last_known_sequences.get(&folder_id) {
    if last_seq != sequence {
        crate::log_debug(&format!(
            "DEBUG [FolderStatusResult]: Sequence changed from {} to {} for folder={}",
            last_seq, sequence, folder_id
        ));

        // Use extracted method instead of inline logic
        app.invalidate_and_refresh_folder(&folder_id);
    }
}
```

**Step 4: Make method public so handlers can access it**

Change `fn invalidate_and_refresh_folder` to `pub(crate) fn invalidate_and_refresh_folder` in `src/app/file_ops.rs`.

**Step 5: Build and verify no compilation errors**

```bash
cargo build
```

Expected: Build succeeds with zero warnings

**Step 6: Run all tests**

```bash
cargo test
```

Expected: All tests PASS (no behavior changes, just refactoring)

**Step 7: Commit**

```bash
git add src/app/file_ops.rs src/handlers/api.rs
git commit -m "refactor: Extract invalidate_and_refresh_folder method

Moves cache invalidation logic from FolderStatusResult handler
into reusable method. Prepares for force_refresh_folder feature.
No behavior changes - pure refactoring.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Add force_refresh_folder Method with Tests

**Files:**
- Modify: `src/app/file_ops.rs` (add method after invalidate_and_refresh_folder)
- Modify: `tests/rescan_folder_info_test.rs` (add force refresh tests)

**Step 1: Write failing tests for force_refresh_folder**

Add to `tests/rescan_folder_info_test.rs`:

```rust
#[test]
fn test_force_refresh_folder_invalidates_cache() {
    use synctui::api::BrowseItem;

    // Setup: App with cached browse data
    let mut model = Model::new(false);
    let mut app = App::new_for_test(model);

    let folder_id = "test-folder";

    // Pre-populate cache with browse data
    let items = vec![
        BrowseItem {
            name: "file1.txt".to_string(),
            item_type: "file".to_string(),
            size: 1024,
            mod_time: "2024-01-01T00:00:00Z".to_string(),
        },
    ];

    app.cache.save_browse(folder_id, None, &items, 100).unwrap();

    // Verify cache has data before force refresh
    let cached = app.cache.get_browse(folder_id, None, 100).unwrap();
    assert!(cached.is_some());

    // Execute
    let result = app.force_refresh_folder(folder_id);

    // Verify
    assert!(result.is_ok());

    // Cache should be invalidated (return None even with same sequence)
    let cached_after = app.cache.get_browse(folder_id, None, 100).unwrap();
    assert!(cached_after.is_none(), "Cache should be invalidated");
}

#[test]
fn test_force_refresh_folder_sends_rescan_request() {
    use std::sync::mpsc;

    // Setup: App with custom channel to capture API requests
    let model = Model::new(false);
    let (api_tx, api_rx) = mpsc::channel();
    let (cache_event_tx, _cache_event_rx) = mpsc::channel();

    let mut app = App {
        model,
        client: synctui::api::SyncthingClient::new(
            "http://localhost:8384".to_string(),
            "test-key".to_string(),
        ),
        cache: synctui::cache::Cache::new_in_memory().unwrap(),
        api_tx,
        cache_event_tx,
        base_url: "http://localhost:8384".to_string(),
        open_command: None,
        clipboard_command: None,
        image_protocol: None,
        image_font_size: None,
        image_state_map: std::collections::HashMap::new(),
    };

    // Execute
    let result = app.force_refresh_folder("test-folder");

    // Verify
    assert!(result.is_ok());

    // Should send RescanFolder request
    let request = api_rx.try_recv().ok();
    assert!(matches!(
        request,
        Some(synctui::services::api::ApiRequest::RescanFolder { folder_id })
        if folder_id == "test-folder"
    ));
}

#[test]
fn test_force_refresh_folder_clears_discovered_dirs() {
    // Setup: App with discovered directories
    let mut model = Model::new(false);
    model.performance.discovered_dirs.insert("test-folder:Movies/".to_string());
    model.performance.discovered_dirs.insert("test-folder:Photos/".to_string());
    model.performance.discovered_dirs.insert("other-folder:Data/".to_string());

    let mut app = App::new_for_test(model);

    // Execute
    let result = app.force_refresh_folder("test-folder");

    // Verify
    assert!(result.is_ok());

    // Should clear only discovered_dirs for this folder
    assert!(!app.model.performance.discovered_dirs.contains("test-folder:Movies/"));
    assert!(!app.model.performance.discovered_dirs.contains("test-folder:Photos/"));
    // Other folders should remain
    assert!(app.model.performance.discovered_dirs.contains("other-folder:Data/"));
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test test_force_refresh_folder
```

Expected: Compilation error - "no method named `force_refresh_folder` found for struct `App`"

**Step 3: Implement force_refresh_folder**

Add to `src/app/file_ops.rs` after `invalidate_and_refresh_folder`:

```rust
/// Force refresh a folder - invalidate cache immediately and trigger rescan
/// Unlike normal rescan, this doesn't wait for sequence change to invalidate cache
pub(crate) fn force_refresh_folder(&mut self, folder_id: &str) -> Result<()> {
    log_debug(&format!(
        "DEBUG [force_refresh_folder]: Force refreshing folder={}",
        folder_id
    ));

    // Step 1: Invalidate cache and refresh breadcrumbs immediately
    self.invalidate_and_refresh_folder(folder_id);

    // Step 2: Trigger Syncthing rescan
    let _ = self
        .api_tx
        .send(services::api::ApiRequest::RescanFolder {
            folder_id: folder_id.to_string(),
        });

    Ok(())
}
```

**Step 4: Run tests to verify they pass**

```bash
cargo test test_force_refresh_folder
```

Expected: All 3 tests PASS

**Step 5: Run all tests**

```bash
cargo test
```

Expected: All 172+ tests PASS

**Step 6: Commit**

```bash
git add src/app/file_ops.rs tests/rescan_folder_info_test.rs
git commit -m "feat: Add force_refresh_folder method

Implements force refresh that invalidates cache immediately
before triggering rescan. Unlike normal rescan, doesn't wait
for sequence change. Includes tests for cache invalidation,
API request sending, and discovered_dirs clearing.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Update Keyboard Handler for Rescan Dialog

**Files:**
- Modify: `src/handlers/keyboard.rs:80-200` (confirmation handlers)
- Modify: `src/handlers/keyboard.rs:580-585` ('r' key handler)

**Step 1: Add confirmation handler for Rescan action**

Insert at top of keyboard handler after other ConfirmAction handlers (around line 170, after PauseResume handler):

```rust
ConfirmAction::Rescan { folder_id, folder_label } => {
    match key.code {
        KeyCode::Char('y') => {
            // Normal rescan
            app.model.ui.confirm_action = None;
            let _ = app.rescan_selected_folder();
            app.model.ui.show_toast(format!("Rescanning {}...", folder_label));
        }
        KeyCode::Char('f') => {
            // Force refresh
            app.model.ui.confirm_action = None;
            let _ = app.force_refresh_folder(&folder_id);
            app.model.ui.show_toast(format!("Force refreshing {}...", folder_label));
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            // Cancel
            app.model.ui.confirm_action = None;
        }
        _ => {} // Ignore other keys while dialog is open
    }
    return Ok(());
}
```

**Step 2: Replace 'r' key handler to show dialog**

Modify `src/handlers/keyboard.rs:580-585`:

Replace:
```rust
KeyCode::Char('r') => {
    // Rescan the selected/current folder
    let _ = app.rescan_selected_folder();
}
```

With:
```rust
KeyCode::Char('r') => {
    // Show rescan confirmation dialog
    if let Some((folder_id, folder_label)) = app.get_rescan_folder_info() {
        app.model.ui.confirm_action = Some(ConfirmAction::Rescan {
            folder_id,
            folder_label,
        });
    }
}
```

**Step 3: Build to verify compilation**

```bash
cargo build
```

Expected: Build succeeds with zero warnings

**Step 4: Manual test - run the app**

```bash
cargo run
```

Test sequence:
1. Press 'r' on a folder
2. Should see compilation error about missing render function
3. This is expected - we'll add rendering in next task

Press `q` to quit.

**Step 5: Commit**

```bash
git add src/handlers/keyboard.rs
git commit -m "feat: Add keyboard handling for rescan dialog

Updates 'r' key to show confirmation dialog instead of
immediate rescan. Adds handler for y/f/n responses with
appropriate toast messages. Rendering not yet implemented.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Add Dialog Rendering

**Files:**
- Modify: `src/ui/dialogs.rs` (add render_rescan_confirmation function)
- Modify: `src/ui/render.rs:265-280` (add render call)

**Step 1: Add render_rescan_confirmation function**

Add to `src/ui/dialogs.rs` at the end of the file (after other dialog functions):

```rust
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
```

**Step 2: Add render call in render.rs**

Modify `src/ui/render.rs:265-280` - add new match arm after PauseResume (around line 278):

```rust
crate::model::ConfirmAction::PauseResume { label, is_paused, .. } => {
    dialogs::render_pause_resume_confirmation(f, label, *is_paused);
}
crate::model::ConfirmAction::Rescan { folder_label, .. } => {
    dialogs::render_rescan_confirmation(f, folder_label);
}
```

**Step 3: Build to verify compilation**

```bash
cargo build
```

Expected: Build succeeds with zero warnings

**Step 4: Manual test - full flow**

```bash
cargo run
```

Test sequence:
1. Navigate to a folder (or stay in folder list)
2. Press 'r'
3. Dialog should appear with folder name and options
4. Press 'y' → toast "Rescanning [folder]..." appears, dialog closes
5. Press 'r' again, press 'f' → toast "Force refreshing [folder]..." appears
6. Press 'r' again, press 'n' → dialog closes, no action
7. Press 'r' again, press 'Esc' → dialog closes, no action

Expected: All interactions work correctly, dialog displays properly centered

**Step 5: Test in breadcrumb view**

1. Navigate into a folder (press Enter on a folder)
2. Press 'r'
3. Dialog should show with folder name
4. Test y/f/n/Esc responses

Expected: Works identically in both views

**Step 6: Commit**

```bash
git add src/ui/dialogs.rs src/ui/render.rs
git commit -m "feat: Add rescan confirmation dialog rendering

Implements render_rescan_confirmation with cyan border
and centered layout. Shows folder name, y/f/n options,
and clear descriptions. Matches existing dialog styling.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: Integration Testing and Documentation

**Files:**
- Create: `tests/rescan_force_refresh_integration_test.rs`
- Modify: `CLAUDE.md` (update hotkey documentation)

**Step 1: Write integration tests**

Create `tests/rescan_force_refresh_integration_test.rs`:

```rust
//! Integration tests for rescan force refresh feature

use synctui::{App, Model};
use synctui::api::{Folder, BrowseItem};
use synctui::model::ConfirmAction;

#[test]
fn test_rescan_dialog_appears_in_folder_list() {
    // Setup: App in folder list view with folder selected
    let mut model = Model::new(false);
    model.navigation.focus_level = 0;
    model.navigation.folders_state_selection = Some(0);

    model.syncthing.folders = vec![Folder {
        id: "test".to_string(),
        label: "Test Folder".to_string(),
        path: "/data/test".to_string(),
        folder_type: "sendreceive".to_string(),
        paused: false,
    }];

    let mut app = App::new_for_test(model);

    // Simulate pressing 'r' key
    if let Some((folder_id, folder_label)) = app.get_rescan_folder_info() {
        app.model.ui.confirm_action = Some(ConfirmAction::Rescan {
            folder_id,
            folder_label,
        });
    }

    // Verify dialog state
    assert!(app.model.ui.confirm_action.is_some());
    match &app.model.ui.confirm_action {
        Some(ConfirmAction::Rescan { folder_id, folder_label }) => {
            assert_eq!(folder_id, "test");
            assert_eq!(folder_label, "Test Folder");
        }
        _ => panic!("Expected Rescan confirmation"),
    }
}

#[test]
fn test_rescan_dialog_appears_in_breadcrumb_view() {
    // Setup: App with breadcrumb trail
    let mut model = Model::new(false);
    model.navigation.focus_level = 1;

    let breadcrumb = crate::model::BreadcrumbLevel {
        folder_id: "movies".to_string(),
        folder_label: "Movies".to_string(),
        folder_path: "/data/movies".to_string(),
        prefix: None,
        items: vec![],
        filtered_items: None,
        selected_index: Some(0),
        file_sync_states: std::collections::HashMap::new(),
        ignored_exists: std::collections::HashMap::new(),
        translated_base_path: "/home/user/movies".to_string(),
    };

    model.navigation.breadcrumb_trail.push(breadcrumb);

    let mut app = App::new_for_test(model);

    // Simulate pressing 'r' key
    if let Some((folder_id, folder_label)) = app.get_rescan_folder_info() {
        app.model.ui.confirm_action = Some(ConfirmAction::Rescan {
            folder_id,
            folder_label,
        });
    }

    // Verify dialog state
    match &app.model.ui.confirm_action {
        Some(ConfirmAction::Rescan { folder_id, folder_label }) => {
            assert_eq!(folder_id, "movies");
            assert_eq!(folder_label, "Movies");
        }
        _ => panic!("Expected Rescan confirmation"),
    }
}

#[test]
fn test_normal_rescan_closes_dialog() {
    // Setup: App with rescan dialog open
    let mut model = Model::new(false);
    model.ui.confirm_action = Some(ConfirmAction::Rescan {
        folder_id: "test".to_string(),
        folder_label: "Test".to_string(),
    });

    let mut app = App::new_for_test(model);

    // Simulate 'y' response
    app.model.ui.confirm_action = None;
    let _ = app.rescan_selected_folder();

    // Verify dialog closed
    assert!(app.model.ui.confirm_action.is_none());
}

#[test]
fn test_force_refresh_closes_dialog() {
    // Setup: App with rescan dialog open
    let mut model = Model::new(false);
    model.ui.confirm_action = Some(ConfirmAction::Rescan {
        folder_id: "test".to_string(),
        folder_label: "Test".to_string(),
    });

    let mut app = App::new_for_test(model);

    // Simulate 'f' response
    app.model.ui.confirm_action = None;
    let _ = app.force_refresh_folder("test");

    // Verify dialog closed
    assert!(app.model.ui.confirm_action.is_none());
}

#[test]
fn test_cancel_closes_dialog() {
    // Setup: App with rescan dialog open
    let mut model = Model::new(false);
    model.ui.confirm_action = Some(ConfirmAction::Rescan {
        folder_id: "test".to_string(),
        folder_label: "Test".to_string(),
    });

    let mut app = App::new_for_test(model);

    // Simulate 'n' or Esc
    app.model.ui.confirm_action = None;

    // Verify dialog closed and no action taken
    assert!(app.model.ui.confirm_action.is_none());
}

#[test]
fn test_force_refresh_invalidates_before_rescan() {
    // This test verifies the order of operations:
    // 1. Cache invalidation happens first
    // 2. Then rescan is triggered

    use std::sync::mpsc;

    let mut model = Model::new(false);
    let (api_tx, api_rx) = mpsc::channel();
    let (cache_event_tx, _) = mpsc::channel();

    // Add breadcrumb so invalidation logic runs
    let breadcrumb = crate::model::BreadcrumbLevel {
        folder_id: "test".to_string(),
        folder_label: "Test".to_string(),
        folder_path: "/data/test".to_string(),
        prefix: None,
        items: vec![],
        filtered_items: None,
        selected_index: Some(0),
        file_sync_states: std::collections::HashMap::new(),
        ignored_exists: std::collections::HashMap::new(),
        translated_base_path: "/home/user/test".to_string(),
    };
    model.navigation.breadcrumb_trail.push(breadcrumb);

    let mut app = App {
        model,
        client: synctui::api::SyncthingClient::new(
            "http://localhost:8384".to_string(),
            "test-key".to_string(),
        ),
        cache: synctui::cache::Cache::new_in_memory().unwrap(),
        api_tx,
        cache_event_tx,
        base_url: "http://localhost:8384".to_string(),
        open_command: None,
        clipboard_command: None,
        image_protocol: None,
        image_font_size: None,
        image_state_map: std::collections::HashMap::new(),
    };

    // Pre-populate cache
    let items = vec![BrowseItem {
        name: "old.txt".to_string(),
        item_type: "file".to_string(),
        size: 100,
        mod_time: "2024-01-01T00:00:00Z".to_string(),
    }];
    app.cache.save_browse("test", None, &items, 50).unwrap();

    // Execute force refresh
    let _ = app.force_refresh_folder("test");

    // Verify cache was invalidated
    let cached = app.cache.get_browse("test", None, 50).unwrap();
    assert!(cached.is_none(), "Cache should be invalidated immediately");

    // Verify BrowseFolder request was sent (for breadcrumb refresh)
    let request1 = api_rx.try_recv().ok();
    assert!(matches!(
        request1,
        Some(synctui::services::api::ApiRequest::BrowseFolder { folder_id, .. })
        if folder_id == "test"
    ), "Should send BrowseFolder for breadcrumb refresh");

    // Verify RescanFolder request was sent
    let request2 = api_rx.try_recv().ok();
    assert!(matches!(
        request2,
        Some(synctui::services::api::ApiRequest::RescanFolder { folder_id })
        if folder_id == "test"
    ), "Should send RescanFolder after invalidation");
}
```

**Step 2: Run integration tests**

```bash
cargo test rescan_force_refresh_integration
```

Expected: All 7 integration tests PASS

**Step 3: Run all tests**

```bash
cargo test
```

Expected: All 179+ tests PASS (169 existing + 4 helper tests + 3 force refresh tests + 3 from Task 4 + 7 integration tests)

**Step 4: Update CLAUDE.md documentation**

Modify the "User Actions" section around line 52 to update 'r' key description:

Find:
```markdown
- `r`: Rescan folder via `POST /rest/db/scan`
```

Replace with:
```markdown
- `r`: **Rescan folder** - Shows confirmation dialog with options:
  - `y`: Normal rescan - Trigger Syncthing scan, wait for sequence change to invalidate cache
  - `f`: Force refresh - Immediately invalidate cache and trigger rescan (useful for stale cache bugs)
  - `n` or `Esc`: Cancel
```

**Step 5: Commit**

```bash
git add tests/rescan_force_refresh_integration_test.rs CLAUDE.md
git commit -m "test: Add integration tests for rescan force refresh

Adds 7 integration tests covering:
- Dialog appearance in both folder list and breadcrumb views
- Normal rescan (y) closes dialog correctly
- Force refresh (f) closes dialog correctly
- Cancel (n/Esc) closes without action
- Force refresh invalidates cache before rescan
- Request ordering verification

Updates CLAUDE.md with new 'r' key behavior documentation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8: Final Verification and Cleanup

**Step 1: Run complete test suite**

```bash
cargo test
```

Expected: All 179+ tests PASS, zero failures

**Step 2: Check for warnings**

```bash
cargo build
cargo clippy
```

Expected: Zero warnings, zero clippy suggestions

**Step 3: Manual end-to-end testing**

Run the app:
```bash
cargo run --release
```

Test checklist:
- [ ] Press 'r' in folder list → dialog appears with correct folder name
- [ ] Press 'y' → toast shows "Rescanning [folder]...", normal rescan works
- [ ] Press 'r', then 'f' → toast shows "Force refreshing [folder]...", breadcrumbs refresh immediately
- [ ] Press 'r', then 'n' → dialog closes, no action
- [ ] Press 'r', then 'Esc' → dialog closes, no action
- [ ] Navigate into folder, press 'r' → dialog shows breadcrumb folder name
- [ ] Test force refresh actually clears stale cache (if you have a test case)
- [ ] Test normal rescan still works with sequence validation

All tests should pass.

**Step 4: Review git log**

```bash
git log --oneline -8
```

Expected: 8 commits in logical order:
1. Add ConfirmAction::Rescan variant
2. Extract get_rescan_folder_info helper
3. Extract invalidate_and_refresh_folder method
4. Add force_refresh_folder method
5. Add keyboard handling for rescan dialog
6. Add rescan confirmation dialog rendering
7. Add integration tests + docs
8. (this task - if needed)

**Step 5: Final commit if needed**

If any fixes were needed:
```bash
git add [files]
git commit -m "fix: Final cleanup for rescan force refresh

[describe any final fixes]

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

Otherwise, feature is complete!

---

## Completion Checklist

- [ ] All 179+ tests passing
- [ ] Zero compiler warnings
- [ ] Zero clippy warnings
- [ ] Manual testing completed successfully
- [ ] Documentation updated in CLAUDE.md
- [ ] 7-8 clean commits in git history
- [ ] Feature works in both folder list and breadcrumb views
- [ ] Normal rescan (y) still works with sequence validation
- [ ] Force refresh (f) invalidates cache immediately
- [ ] Cancel (n/Esc) closes dialog without action
- [ ] Dialog renders correctly with cyan border and centered layout
- [ ] Toast messages show correct feedback for each action

## Success Criteria

✅ Pressing 'r' shows confirmation dialog instead of immediate rescan
✅ 'y' performs normal rescan (existing behavior)
✅ 'f' performs force refresh (new behavior - immediate cache invalidation)
✅ 'n' and 'Esc' cancel without action
✅ Works in both folder list view and breadcrumb view
✅ All tests pass (179+ total)
✅ Zero warnings
✅ Documentation updated
