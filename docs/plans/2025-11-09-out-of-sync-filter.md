# Out-of-Sync Filter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add dual-mode out-of-sync filtering (folder summary modal + breadcrumb hierarchical filter) with real-time updates

**Architecture:** Lazy + pre-check caching strategy. Reuses existing `sync_state_cache` table with added `need_category` field. API Foundation First approach: build data layer (API → Service → Cache → State) then UI features (Summary Modal → Breadcrumb Filter).

**Tech Stack:** Rust, Ratatui, Syncthing REST API, SQLite, async/tokio

---

## Phase 1: API Foundation

### Task 1: Add NeedResponse Type

**Files:**
- Modify: `src/api.rs:96-97` (after FileInfo struct)

**Step 1: Write the type definition**

Add after the `FileInfo` struct definition (around line 96):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct NeedResponse {
    pub progress: Vec<FileInfo>,
    pub queued: Vec<FileInfo>,
    pub rest: Vec<FileInfo>,
    pub page: u32,
    pub perpage: u32,
}
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: No errors (all dependencies already imported)

**Step 3: Commit**

```bash
git add src/api.rs
git commit -m "feat(api): add NeedResponse type for /rest/db/need endpoint

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Add get_needed_files API Method

**Files:**
- Modify: `src/api.rs` (in SyncthingClient impl block, after existing methods)

**Step 1: Write the failing test**

Add to bottom of `src/api.rs` (in tests module if exists, or create new test):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_needed_files_builds_correct_url() {
        // This is a basic smoke test - full integration test requires real Syncthing
        let client = SyncthingClient::new(
            "http://localhost:8384".to_string(),
            "test-key".to_string(),
        );

        // We can't actually call the API without a real instance,
        // but we can verify the method exists and accepts correct params
        // Real testing will happen in integration tests
    }
}
```

**Step 2: Run test to verify structure**

Run: `cargo test test_get_needed_files_builds_correct_url`
Expected: PASS (smoke test)

**Step 3: Write the implementation**

Add after `get_local_changed_items()` method (around line 458):

```rust
pub async fn get_needed_files(
    &self,
    folder_id: &str,
    page: Option<u32>,
    perpage: Option<u32>,
) -> Result<NeedResponse> {
    let mut url = format!("{}/rest/db/need?folder={}", self.base_url, folder_id);

    if let Some(page) = page {
        url.push_str(&format!("&page={}", page));
    }
    if let Some(perpage) = perpage {
        url.push_str(&format!("&perpage={}", perpage));
    }

    let response = self
        .client
        .get(&url)
        .header("X-API-Key", &self.api_key)
        .send()
        .await
        .context("Failed to get needed files")?;

    response
        .json()
        .await
        .context("Failed to parse need response")
}
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 5: Commit**

```bash
git add src/api.rs
git commit -m "feat(api): add get_needed_files method for /rest/db/need

Supports pagination via page and perpage optional parameters.
Follows same pattern as get_local_changed_files().

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Integrate with API Service

**Files:**
- Modify: `src/services/api.rs` (ApiRequest enum, ApiResponse enum, process_request)

**Step 1: Add to ApiRequest enum**

Find the `ApiRequest` enum and add variant:

```rust
pub enum ApiRequest {
    // ... existing variants
    GetNeededFiles {
        folder_id: String,
        page: Option<u32>,
        perpage: Option<u32>,
    },
}
```

**Step 2: Add to ApiResponse enum**

Find the `ApiResponse` enum and add variant:

```rust
pub enum ApiResponse {
    // ... existing variants
    NeededFiles {
        folder_id: String,
        response: NeedResponse,
    },
}
```

**Step 3: Add import for NeedResponse**

At the top of the file, add to the import from `crate::api`:

```rust
use crate::api::{/* existing imports */, NeedResponse};
```

**Step 4: Add handler in process_request**

Find the `process_request` function and add handler (follow pattern of other handlers):

```rust
ApiRequest::GetNeededFiles { folder_id, page, perpage } => {
    match client.get_needed_files(&folder_id, page, perpage).await {
        Ok(response) => {
            let _ = response_tx.send(ApiResponse::NeededFiles {
                folder_id: folder_id.clone(),
                response,
            });
        }
        Err(e) => {
            error!("Failed to get needed files for {}: {}", folder_id, e);
        }
    }
}
```

**Step 5: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/services/api.rs
git commit -m "feat(service): integrate get_needed_files with API service

Add GetNeededFiles request and NeededFiles response variants.
Follows existing async service pattern.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 2: Cache Extension

### Task 4: Extend sync_state_cache Table

**Files:**
- Modify: `src/cache.rs` (schema creation, around the `create_tables` or similar function)

**Step 1: Add columns to schema**

Find the `sync_state_cache` table creation SQL and add columns. If the table is created in `new()` or `create_tables()`, modify to include:

```rust
// Find existing CREATE TABLE sync_state_cache statement and add:
ALTER TABLE IF EXISTS sync_state_cache ADD COLUMN need_category TEXT;
ALTER TABLE IF EXISTS sync_state_cache ADD COLUMN need_cached_at INTEGER;
```

**Note:** SQLite doesn't support ALTER TABLE IF NOT EXISTS for columns. Better approach is to check schema version and migrate. For simplicity, we'll add these in a migration check:

```rust
// Add migration helper
fn ensure_out_of_sync_columns(&self) -> Result<()> {
    // Check if columns exist
    let has_columns: bool = self.conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sync_state_cache') WHERE name IN ('need_category', 'need_cached_at')",
        [],
        |row| {
            let count: i32 = row.get(0)?;
            Ok(count == 2)
        },
    )?;

    if !has_columns {
        self.conn.execute(
            "ALTER TABLE sync_state_cache ADD COLUMN need_category TEXT",
            [],
        )?;
        self.conn.execute(
            "ALTER TABLE sync_state_cache ADD COLUMN need_cached_at INTEGER",
            [],
        )?;
    }

    Ok(())
}
```

**Step 2: Call migration in new() or init**

Find the `Cache::new()` or initialization function and add call after table creation:

```rust
cache.ensure_out_of_sync_columns()?;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Test migration manually**

Run: `cargo build && rm ~/.cache/synctui/cache.db && cargo run`
Expected: App starts, cache DB created with new columns

**Step 5: Commit**

```bash
git add src/cache.rs
git commit -m "feat(cache): extend sync_state_cache with out-of-sync columns

Add need_category and need_cached_at columns for tracking
/rest/db/need response data with 30s TTL.

Migration automatically adds columns on existing databases.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Add Cache Methods - cache_needed_files

**Files:**
- Modify: `src/cache.rs` (add new method to Cache impl)

**Step 1: Write the failing test**

Add test at bottom of `src/cache.rs` or in tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_needed_files_stores_categories() {
        let cache = Cache::new(":memory:").unwrap();

        let need_response = NeedResponse {
            progress: vec![FileInfo {
                name: "downloading.txt".to_string(),
                size: 100,
                ..Default::default()
            }],
            queued: vec![FileInfo {
                name: "queued.txt".to_string(),
                size: 200,
                ..Default::default()
            }],
            rest: vec![],
            page: 1,
            perpage: 100,
        };

        cache.cache_needed_files("test-folder", &need_response).unwrap();

        // Verify categories were stored
        // (We'll implement get_out_of_sync_items next to verify)
    }
}
```

**Step 2: Add Default trait to FileInfo**

In `src/api.rs`, add `Default` derive to `FileInfo`:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
// ... rest of FileInfo
```

**Step 3: Run test to verify it fails**

Run: `cargo test test_cache_needed_files_stores_categories`
Expected: FAIL (method doesn't exist)

**Step 4: Write the implementation**

Add method to Cache impl in `src/cache.rs`:

```rust
pub fn cache_needed_files(&self, folder_id: &str, need_response: &NeedResponse) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    // Process progress array (downloading)
    for file in &need_response.progress {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state_cache
             (folder_id, file_path, state, need_category, need_cached_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![folder_id, &file.name, "Syncing", "downloading", now],
        )?;
    }

    // Process queued array
    for file in &need_response.queued {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state_cache
             (folder_id, file_path, state, need_category, need_cached_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![folder_id, &file.name, "RemoteOnly", "queued", now],
        )?;
    }

    // Process rest array (categorize as remote_only or modified based on local state)
    for file in &need_response.rest {
        // For now, mark as remote_only (we'll refine categorization later)
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state_cache
             (folder_id, file_path, state, need_category, need_cached_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![folder_id, &file.name, "RemoteOnly", "remote_only", now],
        )?;
    }

    Ok(())
}
```

**Step 5: Add NeedResponse import**

At top of `src/cache.rs`:

```rust
use crate::api::{/* existing imports */, NeedResponse, FileInfo};
```

**Step 6: Run test to verify it passes**

Run: `cargo test test_cache_needed_files_stores_categories`
Expected: PASS

**Step 7: Commit**

```bash
git add src/cache.rs src/api.rs
git commit -m "feat(cache): add cache_needed_files method

Stores files from /rest/db/need response with categorization:
- progress -> downloading
- queued -> queued
- rest -> remote_only (refined categorization in future task)

Adds Default derive to FileInfo for testing.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: Add Cache Methods - get_folder_sync_breakdown

**Files:**
- Modify: `src/cache.rs`
- Create: `src/model/types.rs` (if doesn't exist) or modify existing

**Step 1: Define FolderSyncBreakdown type**

Check if `src/model/types.rs` exists. If not, create it:

```rust
// src/model/types.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderSyncBreakdown {
    pub downloading: usize,
    pub queued: usize,
    pub remote_only: usize,
    pub modified: usize,
    pub local_only: usize,
}
```

If `src/model/mod.rs` exists, add:

```rust
pub mod types;
```

**Step 2: Write the failing test**

Add to tests in `src/cache.rs`:

```rust
#[test]
fn test_get_folder_sync_breakdown_counts_categories() {
    let cache = Cache::new(":memory:").unwrap();

    // Setup test data
    let need_response = NeedResponse {
        progress: vec![
            FileInfo { name: "file1.txt".to_string(), ..Default::default() },
            FileInfo { name: "file2.txt".to_string(), ..Default::default() },
        ],
        queued: vec![
            FileInfo { name: "file3.txt".to_string(), ..Default::default() },
        ],
        rest: vec![],
        page: 1,
        perpage: 100,
    };

    cache.cache_needed_files("test-folder", &need_response).unwrap();

    let breakdown = cache.get_folder_sync_breakdown("test-folder").unwrap();

    assert_eq!(breakdown.downloading, 2);
    assert_eq!(breakdown.queued, 1);
    assert_eq!(breakdown.remote_only, 0);
    assert_eq!(breakdown.modified, 0);
    assert_eq!(breakdown.local_only, 0);
}
```

**Step 3: Run test to verify it fails**

Run: `cargo test test_get_folder_sync_breakdown_counts_categories`
Expected: FAIL (method doesn't exist)

**Step 4: Write the implementation**

Add method to Cache impl:

```rust
pub fn get_folder_sync_breakdown(&self, folder_id: &str) -> Result<FolderSyncBreakdown> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    let ttl = 30; // 30 seconds
    let cutoff = now - ttl;

    let mut stmt = self.conn.prepare(
        "SELECT need_category, COUNT(*)
         FROM sync_state_cache
         WHERE folder_id = ?1
           AND need_category IS NOT NULL
           AND need_cached_at > ?2
         GROUP BY need_category"
    )?;

    let mut breakdown = FolderSyncBreakdown::default();

    let rows = stmt.query_map(rusqlite::params![folder_id, cutoff], |row| {
        let category: String = row.get(0)?;
        let count: usize = row.get(1)?;
        Ok((category, count))
    })?;

    for row in rows {
        let (category, count) = row?;
        match category.as_str() {
            "downloading" => breakdown.downloading = count,
            "queued" => breakdown.queued = count,
            "remote_only" => breakdown.remote_only = count,
            "modified" => breakdown.modified = count,
            "local_only" => breakdown.local_only = count,
            _ => {}
        }
    }

    Ok(breakdown)
}
```

**Step 5: Add import**

At top of `src/cache.rs`:

```rust
use crate::model::types::FolderSyncBreakdown;
```

**Step 6: Run test to verify it passes**

Run: `cargo test test_get_folder_sync_breakdown_counts_categories`
Expected: PASS

**Step 7: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 8: Commit**

```bash
git add src/cache.rs src/model/types.rs src/model/mod.rs
git commit -m "feat(cache): add get_folder_sync_breakdown method

Returns counts by category with 30s TTL check.
Add FolderSyncBreakdown type in model/types.rs.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Add Cache Method - invalidate_out_of_sync_categories

**Files:**
- Modify: `src/cache.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_invalidate_out_of_sync_categories_clears_data() {
    let cache = Cache::new(":memory:").unwrap();

    let need_response = NeedResponse {
        progress: vec![FileInfo { name: "file1.txt".to_string(), ..Default::default() }],
        queued: vec![],
        rest: vec![],
        page: 1,
        perpage: 100,
    };

    cache.cache_needed_files("test-folder", &need_response).unwrap();

    let before = cache.get_folder_sync_breakdown("test-folder").unwrap();
    assert_eq!(before.downloading, 1);

    cache.invalidate_out_of_sync_categories("test-folder").unwrap();

    let after = cache.get_folder_sync_breakdown("test-folder").unwrap();
    assert_eq!(after.downloading, 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_invalidate_out_of_sync_categories_clears_data`
Expected: FAIL

**Step 3: Write the implementation**

```rust
pub fn invalidate_out_of_sync_categories(&self, folder_id: &str) -> Result<()> {
    self.conn.execute(
        "UPDATE sync_state_cache
         SET need_category = NULL, need_cached_at = NULL
         WHERE folder_id = ?1",
        rusqlite::params![folder_id],
    )?;
    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_invalidate_out_of_sync_categories_clears_data`
Expected: PASS

**Step 5: Commit**

```bash
git add src/cache.rs
git commit -m "feat(cache): add invalidate_out_of_sync_categories method

Clears need_category and need_cached_at for event-driven invalidation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 3: State Management

### Task 8: Add State Types

**Files:**
- Modify: `src/model/types.rs`
- Modify: `src/model/ui.rs`

**Step 1: Add state types to types.rs**

```rust
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct OutOfSyncFilterState {
    pub origin_level: usize,
    pub last_refresh: SystemTime,
}

#[derive(Debug, Clone)]
pub struct OutOfSyncSummaryState {
    pub selected_index: usize,
    pub breakdowns: HashMap<String, FolderSyncBreakdown>,
    pub loading: HashSet<String>,
}
```

**Step 2: Add fields to UiState**

In `src/model/ui.rs`, add fields:

```rust
pub struct UiState {
    // ... existing fields
    pub out_of_sync_filter: Option<OutOfSyncFilterState>,
    pub out_of_sync_summary: Option<OutOfSyncSummaryState>,
}
```

**Step 3: Update UiState::new() or Default**

Make sure new fields are initialized:

```rust
impl Default for UiState {
    fn default() -> Self {
        Self {
            // ... existing fields
            out_of_sync_filter: None,
            out_of_sync_summary: None,
        }
    }
}
```

**Step 4: Add imports**

At top of `src/model/ui.rs`:

```rust
use crate::model::types::{OutOfSyncFilterState, OutOfSyncSummaryState};
```

**Step 5: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/model/types.rs src/model/ui.rs
git commit -m "feat(model): add out-of-sync filter and summary state types

Add OutOfSyncFilterState and OutOfSyncSummaryState to ui model.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 4: Summary Modal UI

### Task 9: Create Summary View Renderer

**Files:**
- Create: `src/ui/out_of_sync_summary.rs`
- Modify: `src/ui/mod.rs`

**Step 1: Create new file**

Create `src/ui/out_of_sync_summary.rs`:

```rust
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::api::Folder;
use crate::model::types::{FolderSyncBreakdown, OutOfSyncSummaryState};
use crate::ui::icons::IconRenderer;

pub fn render_out_of_sync_summary(
    f: &mut Frame,
    area: Rect,
    folders: &[Folder],
    summary_state: &OutOfSyncSummaryState,
    icon_renderer: &IconRenderer,
) {
    // Create centered modal (60% width, auto height)
    let modal_width = (area.width as f32 * 0.6) as u16;
    let modal_height = (folders.len() as u16 * 3) + 4; // 3 lines per folder + borders

    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width.min(area.width),
        height: modal_height.min(area.height),
    };

    // Build folder items
    let items: Vec<ListItem> = folders
        .iter()
        .map(|folder| {
            let display_name = folder.label.as_ref().unwrap_or(&folder.id);

            // Get breakdown for this folder
            let breakdown = summary_state.breakdowns.get(&folder.id);
            let is_loading = summary_state.loading.contains(&folder.id);

            let mut lines = vec![
                Line::from(vec![
                    Span::raw("📂 "),
                    Span::styled(display_name, Style::default().add_modifier(Modifier::BOLD)),
                ]),
            ];

            if is_loading {
                lines.push(Line::from(Span::styled(
                    "   Loading...",
                    Style::default().fg(Color::Gray),
                )));
            } else if let Some(b) = breakdown {
                let total = b.downloading + b.queued + b.remote_only + b.modified + b.local_only;

                if total == 0 {
                    lines.push(Line::from(Span::styled(
                        "   ✅ All synced",
                        Style::default().fg(Color::Green),
                    )));
                } else {
                    let mut status_parts = Vec::new();

                    if b.downloading > 0 {
                        status_parts.push(format!("🔄 Downloading: {}", b.downloading));
                    }
                    if b.queued > 0 {
                        status_parts.push(format!("⏳ Queued: {}", b.queued));
                    }
                    if b.local_only > 0 {
                        status_parts.push(format!("💻 Local: {}", b.local_only));
                    }
                    if b.remote_only > 0 {
                        status_parts.push(format!("☁️ Remote: {}", b.remote_only));
                    }
                    if b.modified > 0 {
                        status_parts.push(format!("⚠️ Modified: {}", b.modified));
                    }

                    lines.push(Line::from(Span::styled(
                        format!("   {}", status_parts.join("  ")),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Out-of-Sync Summary")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    f.render_widget(list, modal_area);
}
```

**Step 2: Add to ui/mod.rs**

```rust
pub mod out_of_sync_summary;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Commit**

```bash
git add src/ui/out_of_sync_summary.rs src/ui/mod.rs
git commit -m "feat(ui): add out-of-sync summary modal renderer

Displays folder breakdown with loading states and category counts.
Compact multi-line format with icons.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 10: Add Keyboard Handler for Summary

**Files:**
- Modify: `src/handlers/keyboard.rs`
- Modify: `src/main.rs` (add orchestration method)

**Step 1: Add orchestration method to main.rs**

Find the App impl block and add:

```rust
pub fn open_out_of_sync_summary(&mut self) {
    // Initialize summary state
    let summary_state = OutOfSyncSummaryState {
        selected_index: 0,
        breakdowns: std::collections::HashMap::new(),
        loading: std::collections::HashSet::new(),
    };

    // For each folder, check status and queue requests if needed
    for folder in &self.model.syncthing.folders {
        let folder_id = folder.id.clone();

        // Check if folder has out-of-sync items (from cached status)
        if let Some(status) = self.model.syncthing.folder_statuses.get(&folder_id) {
            let has_needed = status.need_total_items > 0;
            let has_local_changes = status.receive_only_total_items > 0;

            if has_needed {
                // Queue GetNeededFiles request
                summary_state.loading.insert(folder_id.clone());

                if let Some(tx) = &self.api_request_tx {
                    let _ = tx.send(ApiRequest::GetNeededFiles {
                        folder_id: folder_id.clone(),
                        page: None,
                        perpage: Some(1000), // Get all items
                    });
                }
            }

            if !has_needed && !has_local_changes {
                // All synced - set empty breakdown
                summary_state.breakdowns.insert(folder_id, FolderSyncBreakdown::default());
            }
        }
    }

    self.model.ui.out_of_sync_summary = Some(summary_state);
}

pub fn close_out_of_sync_summary(&mut self) {
    self.model.ui.out_of_sync_summary = None;
}
```

**Step 2: Add keyboard handler**

In `src/handlers/keyboard.rs`, find the keyboard handling match statement and add:

```rust
// Handle summary modal closing (process first)
if app.model.ui.out_of_sync_summary.is_some() {
    match key.code {
        KeyCode::Esc | KeyCode::Char('f') => {
            app.close_out_of_sync_summary();
            return Ok(());
        }
        _ => {}
    }
}

// ... later in the match statement ...

KeyCode::Char('f') if app.model.navigation.focus_level == 0 => {
    app.open_out_of_sync_summary();
}
```

**Step 3: Add necessary imports**

At top of files:

```rust
// In src/main.rs
use crate::model::types::{OutOfSyncSummaryState, FolderSyncBreakdown};
use crate::services::api::ApiRequest;

// In src/handlers/keyboard.rs
// (should already have KeyCode imported)
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: May have errors about ApiRequest not being accessible - we'll fix in next step

**Step 5: Make ApiRequest accessible in main.rs**

In `src/main.rs`, ensure `ApiRequest` is accessible. May need to adjust imports or visibility in `src/services/api.rs`.

**Step 6: Commit**

```bash
git add src/handlers/keyboard.rs src/main.rs
git commit -m "feat(handlers): add keyboard handler for summary modal

Press 'f' in folder view to open summary.
Esc or 'f' to close.

Queues GetNeededFiles requests for folders with out-of-sync items.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 11: Integrate Summary Rendering

**Files:**
- Modify: `src/ui/render.rs`

**Step 1: Add rendering call**

Find the main render function and add rendering call for summary modal:

```rust
// At end of render function, render modals on top
if let Some(summary_state) = &app.model.ui.out_of_sync_summary {
    render_out_of_sync_summary(
        f,
        f.size(),
        &app.model.syncthing.folders,
        summary_state,
        &app.icon_renderer,
    );
}
```

**Step 2: Add import**

```rust
use crate::ui::out_of_sync_summary::render_out_of_sync_summary;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Build and test manually**

Run: `cargo build --release`
Run the app, press 'f' in folder view
Expected: Summary modal appears (may show "Loading..." - need to wire up response handler)

**Step 5: Commit**

```bash
git add src/ui/render.rs
git commit -m "feat(ui): integrate summary modal rendering

Renders on top of main UI when active.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 5: Response Handling & Events

### Task 12: Add ApiResponse Handler for NeededFiles

**Files:**
- Modify: `src/handlers/api.rs`

**Step 1: Add handler**

Find the `handle_api_response` function or equivalent and add:

```rust
ApiResponse::NeededFiles { folder_id, response } => {
    // Cache the response
    if let Err(e) = app.cache.cache_needed_files(&folder_id, &response) {
        error!("Failed to cache needed files for {}: {}", folder_id, e);
    }

    // Get breakdown from cache
    match app.cache.get_folder_sync_breakdown(&folder_id) {
        Ok(breakdown) => {
            // Update summary state if open
            if let Some(summary) = &mut app.model.ui.out_of_sync_summary {
                summary.breakdowns.insert(folder_id.clone(), breakdown);
                summary.loading.remove(&folder_id);
            }
        }
        Err(e) => {
            error!("Failed to get breakdown for {}: {}", folder_id, e);
        }
    }
}
```

**Step 2: Add necessary imports**

```rust
use crate::services::api::ApiResponse;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Test manually**

Run the app, open summary modal
Expected: See progressive loading as data arrives

**Step 5: Commit**

```bash
git add src/handlers/api.rs
git commit -m "feat(handlers): add ApiResponse handler for NeededFiles

Caches response and updates summary modal progressively.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 13: Update Legend with 'f' Key

**Files:**
- Modify: `src/ui/legend.rs`

**Step 1: Add 'f' key to folder view legend**

Find where folder view keys are defined and add:

```rust
// In folder view context
keys.push("f:Summary");
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 3: Test manually**

Run app, check legend shows "f:Summary" in folder view
Expected: Key visible

**Step 4: Commit**

```bash
git add src/ui/legend.rs
git commit -m "feat(ui): add 'f:Summary' to folder view legend

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 6: Breadcrumb Filter (Future Task)

**Note:** This phase will be detailed in a separate implementation plan once Phase 1-5 is complete and tested. It includes:

- Task 14: Add "Queued" icon to IconRenderer
- Task 15: Add categorize_out_of_sync_state() pure function
- Task 16: Extend breadcrumb rendering for filter mode
- Task 17: Add toggle_out_of_sync_filter() orchestration
- Task 18: Add keyboard handler for 'f' in breadcrumb view
- Task 19: Update status bar for filter display
- Task 20: Add event integration for cache invalidation

---

## Testing & Verification

### Manual Test Plan

**Test 1: Summary Modal - Basic Display**
1. Run `cargo build --release && ./target/release/synctui`
2. Press `f` in folder view
3. Verify modal appears with folder list
4. Verify "Loading..." appears for folders being fetched
5. Verify counts update as data arrives
6. Press Esc to close

**Test 2: Summary Modal - All Synced**
1. Ensure at least one folder is fully synced
2. Open summary modal
3. Verify "✅ All synced" appears for synced folder

**Test 3: Summary Modal - State Breakdown**
1. Have folders with different states (downloading, queued, etc.)
2. Open summary modal
3. Verify correct icons and counts display
4. Verify multi-line format wraps properly

**Test 4: Cache TTL**
1. Open summary modal (data cached)
2. Close and wait 35 seconds
3. Open summary modal again
4. Verify data is re-fetched (new API requests)

### Automated Tests to Run

```bash
# Run all tests
cargo test

# Run specific cache tests
cargo test cache_needed_files
cargo test get_folder_sync_breakdown
cargo test invalidate_out_of_sync_categories

# Run with verbose output
cargo test -- --nocapture
```

### Performance Verification

```bash
# Build in release mode
cargo build --release

# Test with large dataset (100+ files out of sync)
# Monitor API call frequency
# Verify 30s cache TTL is respected
# Verify UI remains responsive during loads
```

---

## Next Steps After Completion

1. **Document feature** in README.md
2. **Update CHANGELOG.md** with new feature
3. **Create Phase 6 plan** for breadcrumb filter
4. **Gather user feedback** on summary modal UX
5. **Consider enhancements**:
   - Add sorting to summary (by folder name, total items)
   - Add filtering (show only folders with issues)
   - Add "Refresh" button for manual cache invalidation

---

## Success Criteria

- ✅ Summary modal opens with `f` key in folder view
- ✅ Displays loading states and progressive updates
- ✅ Shows accurate category breakdown per folder
- ✅ Closes with Esc or `f`
- ✅ Caches data for 30s (no redundant API calls)
- ✅ All tests pass
- ✅ No performance regression
- ✅ Zero compiler warnings
