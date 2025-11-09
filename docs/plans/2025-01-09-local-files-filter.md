# Local Files Filter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add local changed files filtering to out-of-sync filter, enabling users to filter breadcrumb view by receive-only folder local changes in addition to remote needed files.

**Architecture:** Extend existing out-of-sync filter infrastructure with parallel local changes tracking. Reuse `sync_states` table with added `local_changed` and `local_cached_at` columns. Mirror remote filter pattern: API → Service → Cache → State → UI. Integrate with `LocalChangeDetected` event for cache invalidation.

**Tech Stack:** Rust, Ratatui, Syncthing REST API (`/rest/db/localchanged`), SQLite, async/tokio

---

## Phase 1: Cache Extension for Local Files

### Task 1: Extend sync_states Table Schema

**Files:**
- Modify: `src/cache.rs` (schema migration helper)

**Step 1: Write the failing test**

Add to bottom of `src/cache.rs` tests module:

```rust
#[test]
fn test_local_changed_columns_exist() {
    let cache = CacheDb::new_in_memory().unwrap();

    // Verify columns exist
    let has_local_changed: bool = cache.conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sync_states') WHERE name = 'local_changed'",
        [],
        |row| {
            let count: i32 = row.get(0)?;
            Ok(count == 1)
        },
    ).unwrap();

    let has_local_cached_at: bool = cache.conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sync_states') WHERE name = 'local_cached_at'",
        [],
        |row| {
            let count: i32 = row.get(0)?;
            Ok(count == 1)
        },
    ).unwrap();

    assert!(has_local_changed, "local_changed column should exist");
    assert!(has_local_cached_at, "local_cached_at column should exist");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_local_changed_columns_exist`
Expected: FAIL (columns don't exist)

**Step 3: Add migration helper method**

Add to `CacheDb` impl in `src/cache.rs` (after `ensure_need_columns` method):

```rust
fn ensure_local_changed_columns(&self) -> Result<()> {
    // Check if columns exist
    let has_columns: bool = self.conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('sync_states')
         WHERE name IN ('local_changed', 'local_cached_at')",
        [],
        |row| {
            let count: i32 = row.get(0)?;
            Ok(count == 2)
        },
    )?;

    if !has_columns {
        self.conn.execute(
            "ALTER TABLE sync_states ADD COLUMN local_changed INTEGER DEFAULT 0",
            [],
        )?;
        self.conn.execute(
            "ALTER TABLE sync_states ADD COLUMN local_cached_at INTEGER",
            [],
        )?;
    }

    Ok(())
}
```

**Step 4: Call migration in new_with_conn**

Find the `new_with_conn` method and add call after `ensure_need_columns()`:

```rust
cache.ensure_local_changed_columns()?;
```

**Step 5: Run test to verify it passes**

Run: `cargo test test_local_changed_columns_exist`
Expected: PASS

**Step 6: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 7: Commit**

```bash
git add src/cache.rs
git commit -m "$(cat <<'EOF'
feat(cache): extend sync_states with local changed columns

Add local_changed and local_cached_at columns for tracking
/rest/db/localchanged response data with 30s TTL.

Migration automatically adds columns on existing databases.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Add cache_local_changed_files Method

**Files:**
- Modify: `src/cache.rs`

**Step 1: Write the failing test**

Add test to `src/cache.rs` tests:

```rust
#[test]
fn test_cache_local_changed_files_stores_flag() {
    let cache = CacheDb::new_in_memory().unwrap();

    let local_files = vec![
        "dir1/file1.txt".to_string(),
        "file2.txt".to_string(),
    ];

    cache.cache_local_changed_files("test-folder", &local_files).unwrap();

    // Verify files were marked as local_changed
    let items = cache.get_local_changed_items("test-folder").unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.contains(&"dir1/file1.txt".to_string()));
    assert!(items.contains(&"file2.txt".to_string()));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_cache_local_changed_files_stores_flag`
Expected: FAIL (method doesn't exist)

**Step 3: Write the implementation**

Add method to `CacheDb` impl in `src/cache.rs` (after `cache_needed_files` method):

```rust
pub fn cache_local_changed_files(&self, folder_id: &str, file_paths: &[String]) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    // Clear all local_changed flags for this folder first
    self.conn.execute(
        "UPDATE sync_states
         SET local_changed = 0, local_cached_at = NULL
         WHERE folder_id = ?1",
        params![folder_id],
    )?;

    // Set local_changed flag for provided files
    for file_path in file_paths {
        self.conn.execute(
            "INSERT INTO sync_states (folder_id, file_path, local_changed, local_cached_at)
             VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(folder_id, file_path) DO UPDATE SET
                 local_changed = 1,
                 local_cached_at = ?3",
            params![folder_id, file_path, now],
        )?;
    }

    Ok(())
}
```

**Step 4: Run test to verify it still fails**

Run: `cargo test test_cache_local_changed_files_stores_flag`
Expected: FAIL (get_local_changed_items doesn't exist)

**Step 5: Add get_local_changed_items method**

Add method to `CacheDb` impl:

```rust
pub fn get_local_changed_items(&self, folder_id: &str) -> Result<Vec<String>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    let ttl = 30; // 30 seconds
    let cutoff = now - ttl;

    let mut stmt = self.conn.prepare(
        "SELECT file_path
         FROM sync_states
         WHERE folder_id = ?1
           AND local_changed = 1
           AND local_cached_at > ?2"
    )?;

    let rows = stmt.query_map(params![folder_id, cutoff], |row| {
        let file_path: String = row.get(0)?;
        Ok(file_path)
    })?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }

    Ok(items)
}
```

**Step 6: Run test to verify it passes**

Run: `cargo test test_cache_local_changed_files_stores_flag`
Expected: PASS

**Step 7: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 8: Commit**

```bash
git add src/cache.rs
git commit -m "$(cat <<'EOF'
feat(cache): add cache_local_changed_files and get_local_changed_items

Stores files from /rest/db/localchanged response with 30s TTL.
get_local_changed_items returns list of local-changed file paths.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Add invalidate_local_changed Method

**Files:**
- Modify: `src/cache.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_invalidate_local_changed_clears_data() {
    let cache = CacheDb::new_in_memory().unwrap();

    let local_files = vec!["file1.txt".to_string()];
    cache.cache_local_changed_files("test-folder", &local_files).unwrap();

    let before = cache.get_local_changed_items("test-folder").unwrap();
    assert_eq!(before.len(), 1);

    cache.invalidate_local_changed("test-folder").unwrap();

    let after = cache.get_local_changed_items("test-folder").unwrap();
    assert_eq!(after.len(), 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_invalidate_local_changed_clears_data`
Expected: FAIL

**Step 3: Write the implementation**

```rust
pub fn invalidate_local_changed(&self, folder_id: &str) -> Result<()> {
    self.conn.execute(
        "UPDATE sync_states
         SET local_changed = 0, local_cached_at = NULL
         WHERE folder_id = ?1",
        params![folder_id],
    )?;
    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_invalidate_local_changed_clears_data`
Expected: PASS

**Step 5: Commit**

```bash
git add src/cache.rs
git commit -m "$(cat <<'EOF'
feat(cache): add invalidate_local_changed method

Clears local_changed and local_cached_at for event-driven invalidation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Phase 2: API Service Integration

### Task 4: Add GetLocalChanged to API Service

**Files:**
- Modify: `src/services/api.rs` (ApiRequest, ApiResponse enums, process_request)

**Step 1: Add to ApiRequest enum**

Find `ApiRequest` enum in `src/services/api.rs` and add variant:

```rust
pub enum ApiRequest {
    // ... existing variants
    GetLocalChanged {
        folder_id: String,
    },
}
```

**Step 2: Add to ApiResponse enum**

Find `ApiResponse` enum and add variant:

```rust
pub enum ApiResponse {
    // ... existing variants
    LocalChanged {
        folder_id: String,
        file_paths: Vec<String>,
    },
}
```

**Step 3: Add handler in process_request**

Find the `process_request` function and add handler (follow pattern of GetNeededFiles):

```rust
ApiRequest::GetLocalChanged { folder_id } => {
    match client.get_local_changed_files(&folder_id).await {
        Ok(file_paths) => {
            let _ = response_tx.send(ApiResponse::LocalChanged {
                folder_id: folder_id.clone(),
                file_paths,
            });
        }
        Err(e) => {
            error!("Failed to get local changed files for {}: {}", folder_id, e);
        }
    }
}
```

**Step 4: Add get_local_changed_files helper**

Before `process_request`, add helper function:

```rust
async fn get_local_changed_files(client: &SyncthingClient, folder_id: &str) -> Result<Vec<String>> {
    let items = client.get_local_changed_items(folder_id, None).await?;
    Ok(items.into_iter().map(|item| item.name).collect())
}
```

**Step 5: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/services/api.rs
git commit -m "$(cat <<'EOF'
feat(service): integrate get_local_changed with API service

Add GetLocalChanged request and LocalChanged response variants.
Follows existing async service pattern.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Phase 3: Event Integration

### Task 5: Add LocalChanged Cache Invalidation to Event Handler

**Files:**
- Modify: `src/handlers/events.rs`

**Step 1: Find LocalChangeDetected handler**

Read `src/handlers/events.rs` and locate the event type handling for `LocalChangeDetected`.

**Step 2: Add cache invalidation call**

In the `LocalChangeDetected` handler (or wherever local change events are processed), add:

```rust
// Invalidate local changed cache for this folder
let _ = app.cache.invalidate_local_changed(&folder_id);
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Test manually**

Run: `cargo build`
Make a local change in a receive-only folder
Verify debug logs show cache invalidation

**Step 5: Commit**

```bash
git add src/handlers/events.rs
git commit -m "$(cat <<'EOF'
feat(events): invalidate local changed cache on LocalChangeDetected

Ensures cache refreshes when local files change.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Phase 4: Response Handler

### Task 6: Add ApiResponse Handler for LocalChanged

**Files:**
- Modify: `src/handlers/api.rs`

**Step 1: Add handler**

Find `handle_api_response` or match statement for `ApiResponse` and add:

```rust
ApiResponse::LocalChanged { folder_id, file_paths } => {
    // Cache the response
    if let Err(e) = app.cache.cache_local_changed_files(&folder_id, &file_paths) {
        crate::log_debug(&format!("Failed to cache local changed files for {}: {}", folder_id, e));
    }

    // If filter is active for this folder, re-apply it with updated data
    if let Some(filter_state) = &app.model.ui.out_of_sync_filter {
        let current_folder_id = app.model.navigation.breadcrumb_trail
            .get(0)
            .map(|level| &level.folder_id);

        if current_folder_id == Some(&folder_id) {
            // Re-sort and re-filter to pick up new local changed files
            app.sort_all_levels();
            app.apply_out_of_sync_filter();
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 3: Commit**

```bash
git add src/handlers/api.rs
git commit -m "$(cat <<'EOF'
feat(handlers): add ApiResponse handler for LocalChanged

Caches response and re-applies filter if active.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Phase 5: Filter Integration

### Task 7: Extend apply_out_of_sync_filter to Include Local Files

**Files:**
- Modify: `src/main.rs` (apply_out_of_sync_filter method)

**Step 1: Read current implementation**

Read `src/main.rs` lines 959-1093 to see `apply_out_of_sync_filter` implementation.

**Step 2: Extend to fetch local changed items**

After getting `out_of_sync_items`, add:

```rust
// Get local changed items from cache (once for entire folder)
let local_changed_items = match self.cache.get_local_changed_items(&folder_id) {
    Ok(items) => items.into_iter().collect::<std::collections::HashSet<_>>(),
    Err(e) => {
        crate::log_debug(&format!(
            "DEBUG [Filter]: Failed to get local changed items: {}",
            e
        ));
        std::collections::HashSet::new()
    }
};
```

**Step 3: Update filtering logic**

In the loop over `current_items`, extend the `is_out_of_sync` check:

```rust
let is_out_of_sync = if item.item_type == "FILE_INFO_TYPE_DIRECTORY" {
    // For directories: check if any child is out of sync OR local changed
    let dir_prefix = if full_path.ends_with('/') {
        full_path.clone()
    } else {
        format!("{}/", full_path)
    };

    out_of_sync_items
        .keys()
        .any(|path| path.starts_with(&dir_prefix) || path == &full_path)
    || local_changed_items
        .iter()
        .any(|path| path.starts_with(&dir_prefix) || path == &full_path)
} else {
    // For files: direct match in either remote or local
    out_of_sync_items.contains_key(&full_path) || local_changed_items.contains(&full_path)
};
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 5: Build and test manually**

Run: `cargo build`
Test with receive-only folder containing local changes
Toggle filter and verify local changed files appear

**Step 6: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
feat(filter): extend apply_out_of_sync_filter to include local files

Filter now shows both remote needed files AND local changed files.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Update toggle_out_of_sync_filter to Queue Local Changed Request

**Files:**
- Modify: `src/main.rs` (toggle_out_of_sync_filter method)

**Step 1: Read current implementation**

Read `src/main.rs` lines 1183-1250 to see `toggle_out_of_sync_filter`.

**Step 2: Add local changed cache check**

After checking `has_cached_data` for out-of-sync items, add:

```rust
let has_local_cached = self
    .cache
    .get_local_changed_items(&folder_id)
    .map(|items| !items.is_empty())
    .unwrap_or(false);
```

**Step 3: Queue GetLocalChanged request if needed**

Where GetNeededFiles is queued, also queue local changed:

```rust
if !has_cached_data {
    // Queue GetNeededFiles request
    let _ = self.api_tx.send(services::api::ApiRequest::GetNeededFiles {
        folder_id: folder_id.clone(),
        page: None,
        perpage: Some(1000),
    });
}

// Also check for local changes if folder is receive-only
if let Some(folder) = self.model.syncthing.folders.iter().find(|f| f.id == folder_id) {
    if folder.folder_type == "receiveonly" && !has_local_cached {
        let _ = self.api_tx.send(services::api::ApiRequest::GetLocalChanged {
            folder_id: folder_id.clone(),
        });
    }
}

if !has_cached_data || (!has_local_cached && /* is receive-only */) {
    // Show loading toast and return
    self.model
        .ui
        .show_toast("Loading out-of-sync files...".to_string());
    return;
}
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 5: Build and test**

Run: `cargo build`
Test filter toggle queues both requests

**Step 6: Commit**

```bash
git add src/main.rs
git commit -m "$(cat <<'EOF'
feat(filter): queue GetLocalChanged when toggling filter

Ensures local changed files are fetched for receive-only folders.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Phase 6: UI Enhancements

### Task 9: Update Status Bar to Show Filter Type

**Files:**
- Modify: `src/ui/status_bar.rs`

**Step 1: Find filter display logic**

Read `src/ui/status_bar.rs` to locate where filter state is displayed.

**Step 2: Enhance filter display**

Where filter is shown, add detail about what's being filtered:

```rust
if let Some(_filter_state) = &app.model.ui.out_of_sync_filter {
    // Determine filter type based on folder
    let folder_id = app.model.navigation.breadcrumb_trail
        .get(0)
        .map(|level| &level.folder_id);

    let filter_desc = if let Some(fid) = folder_id {
        if let Some(folder) = app.model.syncthing.folders.iter().find(|f| &f.id == fid) {
            if folder.folder_type == "receiveonly" {
                "Filter: Remote + Local"
            } else {
                "Filter: Remote"
            }
        } else {
            "Filter: Active"
        }
    } else {
        "Filter: Active"
    };

    // Display filter_desc in status bar
}
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors

**Step 4: Test manually**

Run: `cargo build`
Test with send/receive folder (should show "Filter: Remote")
Test with receive-only folder (should show "Filter: Remote + Local")

**Step 5: Commit**

```bash
git add src/ui/status_bar.rs
git commit -m "$(cat <<'EOF'
feat(ui): enhance status bar to show filter type

Shows 'Filter: Remote' or 'Filter: Remote + Local' depending on folder type.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

---

## Testing & Verification

### Manual Test Plan

**Test 1: Receive-Only Folder with Local Changes**
1. Create receive-only folder with local changes
2. Navigate to folder breadcrumbs
3. Press `f` to toggle filter
4. Verify loading toast appears
5. Verify local changed files appear in filtered view
6. Verify status bar shows "Filter: Remote + Local"

**Test 2: Send/Receive Folder**
1. Navigate to send/receive folder
2. Toggle filter
3. Verify status bar shows "Filter: Remote"
4. Verify only remote needed files appear

**Test 3: Cache TTL**
1. Toggle filter (cache populated)
2. Toggle off and wait 35 seconds
3. Toggle on again
4. Verify new API requests are made

**Test 4: Event Invalidation**
1. Toggle filter on
2. Make local change in receive-only folder
3. Verify LocalChangeDetected event triggers cache invalidation
4. Verify filter refreshes with new data

**Test 5: Combined Remote + Local**
1. Set up folder with both remote needed files AND local changes
2. Toggle filter
3. Verify both types appear in filtered view
4. Verify directories containing either type are shown

### Automated Tests to Run

```bash
# Run all tests
cargo test

# Run specific cache tests
cargo test test_cache_local_changed_files_stores_flag
cargo test test_invalidate_local_changed_clears_data
cargo test test_local_changed_columns_exist

# Run with verbose output
cargo test -- --nocapture
```

### Performance Verification

```bash
# Build in release mode
cargo build --release

# Test with folder containing 100+ local changes
# Monitor API call frequency
# Verify 30s cache TTL is respected
# Verify UI remains responsive during loads
```

---

## Success Criteria

- ✅ Filter includes local changed files for receive-only folders
- ✅ Cache stores local changed files with 30s TTL
- ✅ Events invalidate local changed cache appropriately
- ✅ Status bar shows filter type (Remote vs Remote + Local)
- ✅ No redundant API calls within TTL window
- ✅ All tests pass
- ✅ No performance regression
- ✅ Zero compiler warnings

---

## Architecture Notes

**Why separate local_changed from need_category:**
- Remote needed files come from `/rest/db/need` with categorization (downloading, queued, etc.)
- Local changed files come from `/rest/db/localchanged` with simple boolean flag
- Separating columns allows independent TTLs and invalidation
- Union of both sets creates comprehensive "out of sync" filter

**Event Integration:**
- `LocalChangeDetected` → invalidate local_changed cache
- `ItemFinished` → invalidate need_category cache (existing)
- Both trigger filter refresh if active

**Filter Behavior:**
- Send/Receive folders: Filter by remote needed only (existing behavior)
- Receive-Only folders: Filter by remote needed + local changed (new behavior)
- Directories shown if ANY descendant matches either condition
