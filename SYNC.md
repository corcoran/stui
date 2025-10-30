# Sync State System Documentation

## Overview

The sync state system tracks the synchronization status of files and directories across Syncthing devices. It combines data from Syncthing's REST API, SQLite cache, event streams, and computed aggregate states to provide real-time visual feedback in the TUI.

---

## Sync State Enum

**Location:** `src/api.rs:40-48` *(verified)*

```rust
pub enum SyncState {
    Synced,       // ✅ Local matches global
    OutOfSync,    // ⚠️ Local differs from global
    LocalOnly,    // 💻 Only on this device
    RemoteOnly,   // ☁️ Only on remote devices
    Ignored,      // 🚫 In .stignore
    Syncing,      // 🔄 Currently syncing
    Unknown,      // ❓ Not yet determined
}
```

### State Meanings

| State | Meaning | Source |
|-------|---------|--------|
| `Synced` | File exists locally and remotely with identical content | FileInfo comparison |
| `OutOfSync` | File exists both places but content differs | FileInfo version/blocks_hash mismatch |
| `LocalOnly` | File exists only on this device | FileInfo: local exists, no global |
| `RemoteOnly` | File exists on remote devices but not locally | FileInfo: no local, global exists |
| `Ignored` | File/directory is in `.stignore` | FileInfo: `ignored` flag |
| `Syncing` | File is actively downloading/uploading | ItemStarted/ItemFinished events |
| `Unknown` | State not yet determined | Initial state or cache miss |

---

## Data Sources

### 1. Syncthing REST API

#### `/rest/db/file?folder=<id>&file=<path>` (FileInfo)
**Purpose:** Get detailed sync state for a single file/directory
**Returns:** `FileDetails` struct with `local`, `global`, and `availability` fields
**Used by:**
- `batch_fetch_visible_sync_states()` - src/main.rs:1417
- `fetch_directory_states()` - src/main.rs:1545
- `fetch_selected_item_sync_state()` - src/main.rs:1664

**State Determination:** `determine_sync_state()` - src/api.rs:533-566 *(verified)*
- Compares `local` vs `global` FileInfo
- Checks `deleted`, `ignored`, `version`, `blocks_hash` flags
- Returns appropriate SyncState based on comparison

#### `/rest/db/browse?folder=<id>&prefix=<path>` (Browse)
**Purpose:** List directory contents
**Returns:** Array of `BrowseItem` (name, type, size, mod_time)
**Used by:** Browse result handler - src/main.rs:585-804
**Note:** Does NOT return sync states, only file metadata

#### `/rest/events?since=<id>` (Event Stream)
**Purpose:** Real-time notifications of sync activity
**Handled by:** `event_listener.rs:65-260`
**Key Events:**
- `ItemStarted` - File begins syncing
- `ItemFinished` - File finishes syncing
- `LocalIndexUpdated` - Local changes detected (with `filenames` array)
- `LocalChangeDetected` / `RemoteChangeDetected` - Individual file changes

### 2. SQLite Cache

**Location:** `~/.cache/synctui/cache.db` (or `/tmp/synctui-cache`)
**Schema:**
```sql
CREATE TABLE sync_states (
    folder_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_sequence INTEGER NOT NULL,
    sync_state INTEGER NOT NULL,
    PRIMARY KEY (folder_id, file_path)
);

CREATE TABLE browse_cache (
    folder_id TEXT NOT NULL,
    prefix TEXT,
    folder_sequence INTEGER NOT NULL,
    items TEXT NOT NULL,
    cached_at INTEGER NOT NULL,
    PRIMARY KEY (folder_id, prefix)
);
```

**Key Functions:**
- `save_sync_state()` - src/cache.rs:304 - Save state with sequence number
- `get_sync_state_unvalidated()` - src/cache.rs:285 - Load cached state without validation
- `invalidate_file()` - src/cache.rs:318 - Remove stale file state
- `invalidate_directory()` - src/cache.rs:328 - Remove stale directory + children states

**Sequence Validation:**
- Folder has a `sequence` number that increments on every change
- Cached states are invalidated when folder sequence changes
- Prevents displaying stale data after remote updates

---

## State Storage Locations

### In-Memory Storage
**Location:** `BreadcrumbLevel.file_sync_states: HashMap<String, SyncState>`
**Struct:** src/main.rs:190-200

Each breadcrumb level (directory view) maintains a HashMap mapping file/directory names to their sync states. This is the source of truth for UI rendering.

### Tracking Collections

#### `syncing_files: HashSet<String>`
**Location:** `App` struct - src/main.rs:244
**Format:** `"folder_id:file_path"`
**Purpose:** Track files actively syncing (between ItemStarted and ItemFinished)
**Protection:** Files in this set are protected from state invalidation

#### `manually_set_states: HashMap<String, ManualStateChange>`
**Location:** `App` struct - src/main.rs:242
**Format:** `"folder_id:file_path" -> ManualStateChange`
**Purpose:** Track user actions (ignore/unignore) to validate state transitions
**Timeout:** 10 seconds (prevents permanent blocking)

```rust
struct ManualStateChange {
    action: ManualAction,  // SetIgnored or SetUnignored
    timestamp: Instant,
}
```

---

## State Update Mechanisms

### 1. File State Updates (Direct)

**Via FileInfo API Response**
Handler: `handle_api_response()` -> `ApiResponse::FileInfoResult` - src/main.rs:807-923

**Flow:**
1. API response arrives with FileDetails
2. `determine_sync_state()` computes state from local/global comparison
3. State transition validation checks (see below)
4. Update `file_sync_states` HashMap
5. Save to cache with sequence number
6. If file was syncing, remove from `syncing_files`

**State Transition Validation** (src/main.rs:852-894):
```rust
// Check manually set states
if let Some(manual_change) = manually_set_states.get(&state_key) {
    match manual_change.action {
        SetIgnored => state must be Ignored,
        SetUnignored => state must NOT be Ignored,
    }
}

// Reject illegal transitions
if state == Unknown {
    match current_state {
        Syncing | RemoteOnly | LocalOnly | OutOfSync | Synced => REJECT
    }
}

// Remove from syncing_files when final state confirmed
if was_syncing && state != Syncing {
    syncing_files.remove(&key);
}
```

### 2. Directory State Updates (Computed)

**Via Children Aggregation**
Function: `update_directory_states()` - src/main.rs:1350-1437 *(line numbers updated)*

**Logic:**
1. For each directory in current level:
   - Check directory's own FileInfo state (for Ignored/RemoteOnly)
   - Query cache for children's states
   - Compute aggregate state based on priority:
     - **Syncing** > RemoteOnly > OutOfSync > LocalOnly > Synced
   - Apply computed state to directory

**When Called:**
- After browse result processing (src/main.rs:724, 795)
- Periodically in idle loop (src/main.rs:4024-4027)

**Throttling:**
- Runs at most once every 2 seconds (src/main.rs:1362-1364)
- Prevents continuous cache queries when hovering over directories
- Uses `last_directory_update: Instant` timestamp to track last execution

**Special Cases:**
- If directory itself is `Ignored` or `RemoteOnly` (doesn't exist locally), use that directly
- If no cached children, fall back to directory's direct state
- Uses cache only (non-blocking)

### 3. Browse Result Updates

**Handler:** `handle_api_response()` -> `ApiResponse::BrowseResult` - src/main.rs:585-804

**Challenge:** Browse results don't include sync states, only file metadata. Must merge with cached/in-memory states without losing data.

**State Preservation Logic** (src/main.rs:679-721):
```rust
for (name, state) in existing_states {
    let is_directory = check item type;
    let cache_has_state = check cached_states;

    // Decide whether to preserve
    let should_preserve = if is_directory {
        // Only preserve intrinsic states (Ignored)
        matches!(state, SyncState::Ignored)
    } else {
        // Preserve all non-Unknown states
        state != SyncState::Unknown
    };

    // Always preserve actively Syncing files/dirs
    if state == Syncing && syncing_files.contains(&key) {
        preserve = true;
    }

    if !cache_has_state && should_preserve {
        preserved_states.insert(name, state);
    }
}

// Merge: cached_states + local_only_items + preserved_states
```

**Why Directories Only Preserve Ignored:**
- Directory states like Syncing, RemoteOnly are **aggregate states** computed from children
- Preserving these prevents `update_directory_states()` from updating them
- `Ignored` is an **intrinsic state** about the directory itself, not its children

### 4. Event-Driven Updates

**Handler:** `handle_cache_invalidation()` - src/main.rs:996-1255

#### ItemStarted Event
**Source:** Event stream - src/event_listener.rs:165-177
**Flow:**
1. Event arrives: `{ folder: "...", item: "..." }`
2. Add to `syncing_files` HashSet
3. Set state to `Syncing` in UI
4. Remove from `manually_set_states` (allow final state to come through)

**Code:** src/main.rs:1157-1229

#### ItemFinished Event
**Source:** Event stream - src/event_listener.rs:178-188
**Flow:**
1. Event arrives: `{ folder: "...", item: "..." }`
2. **Keep** in `syncing_files` (changed from old behavior)
3. Request high-priority FileInfo to get final state
4. FileInfo handler will remove from `syncing_files` after confirming state

**Code:** src/main.rs:1231-1251

**Why This Change:**
- Old behavior: Remove from `syncing_files` immediately
- Problem: File invalidation event clears state (no longer protected)
- Solution: Keep protected until FileInfo confirms final state

#### File/Directory Invalidation
**Source:** Event stream after ItemFinished, or from LocalIndexUpdated
**Flow:**
1. Event arrives with file/directory path
2. Check if file in `syncing_files` or `manually_set_states`
3. If protected, skip clearing
4. If not protected, remove from in-memory state
5. State will be re-fetched on next render

**Code:** src/main.rs:1029-1124

---

## State Transition Rules

### Valid Transitions

```
Unknown → Any state (initial determination)
Any → Syncing (ItemStarted event)
Syncing → Synced/OutOfSync/etc (ItemFinished + FileInfo)

RemoteOnly → Syncing (download starts)
LocalOnly → Syncing (upload starts)
OutOfSync → Syncing (reconciliation starts)

Synced/OutOfSync/etc → Ignored (user ignores)
Ignored → Unknown (user unignores, then re-determines)
```

### Rejected Transitions

```
Known state → Unknown (REJECTED - prevents flickering)
  - Known = Syncing, RemoteOnly, LocalOnly, OutOfSync, Synced

After SetIgnored:
  - Any state except Ignored (REJECTED - enforces consistency)

After SetUnignored:
  - Ignored state (REJECTED - prevents stale Ignored state)
```

**Implementation:** src/main.rs:852-894

---

## Critical Patterns

### 1. Syncing State Protection

**Problem:** File is syncing, browse results come in without cached state, file shows as Unknown

**Solution:**
- Add file to `syncing_files` on ItemStarted
- Keep in `syncing_files` until FileInfo confirms final state
- Browse results preserve Syncing state if file in `syncing_files`
- File invalidation skips clearing if file in `syncing_files`

**Code Locations:**
- Add to syncing_files: src/main.rs:1195
- Check in browse preservation: src/main.rs:706-711, 781-787
- Check in invalidation: src/main.rs:1089
- Remove from syncing_files: src/main.rs:885-892

### 2. Manual State Change Validation

**Problem:** User ignores file, but stale FileInfo response says it's Synced

**Solution:**
- Track user action in `manually_set_states` with timestamp
- Validate API responses against expected state
- Reject transitions that contradict user action
- Timeout after 10 seconds to prevent permanent blocking

**Code:** src/main.rs:858-880

### 3. Directory Aggregate States

**Problem:** Directory shows Synced while children are still RemoteOnly/Syncing

**Solution:**
- Compute directory state from children's cached states
- Priority: Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced
- Preserve only intrinsic states (Ignored), not aggregate states
- Re-compute periodically and after browse results

**Code:** src/main.rs:1328-1421

### 4. Cache Invalidation Strategy

**Granular Invalidation:**
- **File-level:** Clear single file state
- **Directory-level:** Clear directory + all children states
- **Folder-level:** Clear all states for folder

**Event-Driven:**
- `LocalIndexUpdated` with `filenames` array → Invalidate specific files
- `ItemFinished` → File invalidation (but protected if still in syncing_files)
- Folder sequence change → Invalidate all cached browse results

**Code:**
- File invalidation: src/main.rs:1029-1124
- Directory invalidation: src/main.rs:1125-1148
- Cache DB functions: src/cache.rs:318-349

---

## Performance Optimizations

### 1. Cache-First Rendering
- Load cached states immediately on directory open
- Display cached data while background fetches run
- Update UI incrementally as FileInfo responses arrive

### 2. Batch Fetching
**Function:** `batch_fetch_visible_sync_states()` - src/main.rs:1417
- Only fetch states for visible items on screen
- Limit concurrent requests (default: 5)
- Skip already cached/loading items

### 3. Prefetching
**Hovered Directory:** `prefetch_hovered_subdirectories()` - src/main.rs:1464
- Recursively discover and prefetch subdirectory states
- Non-blocking, uses cache only
- Improves navigation responsiveness

**Directory Metadata:** `fetch_directory_states()` - src/main.rs:1545
- Fetch directory own sync states (not children)
- Only for directories without cached states
- Limit: 10 concurrent requests

### 4. Idle Detection & Throttling
**Main Loop:** src/main.rs:4007-4028
- Only run prefetch when user idle for 300ms
- Prevents blocking keyboard input
- Reduces CPU usage from ~18% to <2%

**Directory State Update Throttling:**
- `update_directory_states()` throttled to 2-second intervals
- Prevents continuous cache queries for all visible directories
- Reduces log spam and unnecessary computation
- Timestamp: `App.last_directory_update: Instant`

### 5. Request Deduplication
**API Service:** src/api_service.rs
- Track in-flight requests by key: `"folder_id:file_path"`
- Skip duplicate requests already pending
- Priority queue: High > Medium > Low

---

## Common Scenarios

### Scenario 1: Un-ignoring a Deleted File During Active Sync

**Initial State:** File ignored and deleted locally, other folders syncing

**User Action:** Toggle ignore off

**State Transitions:**
```
1. Ignored → Unknown (immediate UI feedback)
   - manually_set_states["folder:path"] = SetUnignored

2. Unknown → RemoteOnly (FileInfo response)
   - Browse results come in (other folders syncing)
   - RemoteOnly state preserved (not in cache yet, is file, not Unknown)

3. RemoteOnly → Syncing (ItemStarted event)
   - syncing_files.add("folder:path")
   - Browse results preserve Syncing state

4. Syncing → Syncing (ItemFinished event)
   - Stay in syncing_files (don't remove yet)
   - File invalidation event ignored (still protected)
   - High-priority FileInfo requested

5. Syncing → Synced (FileInfo confirms)
   - syncing_files.remove("folder:path")
   - manually_set_states timeout expired
```

**No flickering** - state preserved through all browse results

### Scenario 2: Directory with Syncing Children

**Initial State:** Directory exists, children syncing

**State Computation:**
```
1. Browse result arrives for parent directory
2. Directory's FileInfo returns Synced (directory metadata only)
3. update_directory_states() runs:
   - Queries cache for children states
   - Finds: 3 files Syncing, 5 files Synced
   - Applies priority: Syncing > Synced
   - Sets directory to Syncing
4. User sees: 📁🔄 directory_name
```

**As children finish:**
- Children transition Syncing → Synced
- update_directory_states() re-computes
- Eventually all children Synced → Directory shows Synced

### Scenario 3: Ignoring a Directory

**User Action:** Press `i` on directory

**Flow:**
```
1. Add pattern to .stignore
2. Set state to Ignored in memory
3. manually_set_states["folder:path"] = SetIgnored
4. Trigger rescan
5. Browse results arrive (triggered by other activity)
   - Directory is directory: check if should preserve
   - State is Ignored: YES, preserve (intrinsic state)
   - State preserved through browse result
6. FileInfo confirms Ignored state
7. Cache updated with Ignored state
8. manually_set_states timeout expires
```

---

## Debugging Tips

### Enable Debug Logging
```bash
synctui --debug
tail -f /tmp/synctui-debug.log
```

**Key Log Patterns:**
- `DEBUG [FileInfo]: Updating <name> <old> -> <new>` - State changes
- `DEBUG [Browse]: Preserving <state> state for <name>` - Preservation logic
- `DEBUG [Event]: ItemStarted/ItemFinished` - Sync activity
- `DEBUG [update_directory_states]: Setting <name> to <state>` - Directory aggregation

### Common Issues

**States flickering Unknown:**
- Check browse result preservation logic (lines 679-721, 746-788)
- Verify cache has states: `sqlite3 ~/.cache/synctui/cache.db "SELECT * FROM sync_states WHERE folder_id='...';"`
- Check if states being cleared by invalidation

**Directory stuck in wrong state:**
- Check `update_directory_states()` logs
- Verify children states in cache
- Check if directory's direct state taking precedence

**States not updating after sync:**
- Check ItemFinished handling (src/main.rs:1231)
- Verify file in syncing_files during transition
- Check FileInfo response arriving

---

## Future Improvements

### Potential Optimizations
1. **Batch Directory State Computation:** Instead of re-computing every directory on every idle loop, track which directories have changed children
2. **Smart Cache Warming:** Pre-fetch states for likely navigation paths (e.g., subdirectories of hovered items)
3. **State Transition History:** Log state changes for debugging and undo functionality

### Refactoring Opportunities (from PLAN.md)
1. **Extract State Manager:** `src/state/sync_manager.rs`
   - Manage file_sync_states, manually_set_states, syncing_files
   - Pure state transitions without UI coupling

2. **Extract State Logic:** `src/logic/sync_states.rs`
   - State transition validation
   - Directory aggregation logic
   - Pure functions, fully testable

3. **Add Comprehensive Tests:**
   - Unit tests for state transitions
   - Property-based tests for state machines
   - Integration tests with mocked API responses

---

## Code Reference Summary

### Key Files
- `src/api.rs` - SyncState enum, FileDetails, determine_sync_state()
- `src/cache.rs` - SQLite operations for sync states
- `src/event_listener.rs` - Event stream handling
- `src/main.rs` - Main state management logic
- `src/api_service.rs` - Request queue and prioritization

### Key Functions
- `determine_sync_state()` - src/api.rs:533 - Compute state from FileInfo
- `handle_api_response()` - src/main.rs:581 - Process all API responses
- `handle_cache_invalidation()` - src/main.rs:996 - Process events
- `update_directory_states()` - src/main.rs:1328 - Compute directory aggregates
- `batch_fetch_visible_sync_states()` - src/main.rs:1417 - Fetch file states
- `load_sync_states_from_cache()` - src/main.rs:1285 - Load cached states

### Key Data Structures
- `SyncState` enum - src/api.rs:40
- `FileDetails` struct - src/api.rs:115
- `BreadcrumbLevel` struct - src/main.rs:190
- `ManualStateChange` struct - src/main.rs:49
- `CacheInvalidation` enum - src/event_listener.rs:53

---

*Last Updated: 2025-01-30*
*Reflects fixes for state transition issues and directory aggregate states*
