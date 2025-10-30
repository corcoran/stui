# Sync State Refactoring Plan

**Status**: Proposal (awaiting user evaluation)
**Date**: 2025-10-30
**Goal**: Simplify sync state management from 5 competing sources of truth to 1 authoritative source (Syncthing API)

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Current Architecture Problems](#current-architecture-problems)
3. [All State Change Scenarios](#all-state-change-scenarios)
4. [Proposed Architecture](#proposed-architecture)
5. [Migration Strategy](#migration-strategy)
6. [Testing Strategy](#testing-strategy)

---

## Executive Summary

### The Problem

The current sync state system has **5 competing sources of truth** fighting each other:

1. **In-memory breadcrumb states** (`level.file_sync_states`)
2. **SQLite cache** (validated by sequence numbers)
3. **Manually set states** (`manually_set_states` HashMap - 22 references)
4. **Syncing files tracker** (`syncing_files` HashSet - 17 references)
5. **Syncthing API** (FileInfo responses)

This creates **circular dependencies** where:
- API responses are rejected based on user actions
- Cache states override API responses
- In-memory states persist across API updates
- Manual tracking tries to "protect" states from being correct

**Result**: 650+ lines of defensive code added to paper over race conditions, and bugs still occur (subdirectories stuck as ignored after parent is unignored).

### The Solution

**Trust Syncthing's API as the authoritative source for sync states, with optimistic UI updates that can be reverted.**

**Two Sources of Truth**:
1. **Syncthing API**: Sync states (Synced, OutOfSync, LocalOnly, RemoteOnly, Ignored)
2. **Local Filesystem**: Existence check for ignored files (⚠️ exists vs 🚫 deleted) - **our unique feature**

**Key Changes**:
- **Remove**: manually_set_states (except for optimistic updates), syncing_files, state transition rejection, ancestor checking (660 lines)
- **Keep**: Cache for performance (validated by sequence numbers), filesystem existence checks
- **Add**: Optimistic updates with revert capability, smarter existence checking (skip children of non-existent ignored directories)
- **Simplify**: User Action → Optimistic UI → API Request → Accept Response (revert if conflicts) → Update UI

**Important Realities**:
- `.stignore` can be modified externally (text editor, git, other tools) - we cannot assume our writes are authoritative
- Syncthing does NOT emit events specifically for .stignore changes - we detect via `LocalIndexUpdated` events for affected files
- Filesystem existence for ignored files is NOT tracked by Syncthing - this is our feature, requires direct fs checks

---

## Current Architecture Problems

### Problem 1: Fighting Syncthing Instead of Trusting It

**Current Behavior** (lines 870-905 in main.rs):
```rust
// Check if this was recently manually set
if let Some(&manual_change) = self.manually_set_states.get(&state_key) {
    let is_valid_transition = match manual_change.action {
        ManualAction::SetIgnored => {
            // After ignoring: only accept Ignored state
            state == SyncState::Ignored
        }
        ManualAction::SetUnignored => {
            // After un-ignoring: reject Ignored state
            state != SyncState::Ignored
        }
    };

    if !is_valid_transition {
        log_debug("Rejecting invalid transition");
        continue; // REJECT Syncthing's own API response!
    }
}
```

**Why This Is Wrong:**
- Syncthing's API is the authoritative source of file states
- We MODIFIED Syncthing's .stignore file via `PUT /rest/db/ignores`
- Syncthing processed the change and responded with new states
- We then REJECTED those states because they didn't match our assumptions
- This creates stale data that persists until user leaves and re-enters directory

**Lines Involved**: 870-905 (state transition validation), entire manually_set_states system (22 references)

### Problem 2: Multiple Competing Update Mechanisms

**7 Different Ways States Get Updated:**

1. **FileInfo API Response Handler** (lines 840-950)
   - Updates `level.file_sync_states`
   - Checks manually_set_states for rejection
   - Checks syncing_files for protection
   - Checks cache for staleness

2. **Browse Result Handler - Root** (lines 1885-2008)
   - Merges cache states with in-memory states
   - Preserves syncing_files states
   - Preserves manually_set_states
   - Requests FileInfo for missing states

3. **Browse Result Handler - Non-Root** (lines 2009-2175)
   - 130+ lines of ancestor ignored checking (NEW)
   - Checks if parent was recently unignored
   - Clears stale Ignored states in current level
   - Clears stale Ignored states in parent breadcrumb level
   - Falls back to cache checking all ancestors
   - Same preservation logic as root handler

4. **ItemStarted Event** (lines 584-601)
   - Adds to syncing_files HashSet
   - Sets state to Syncing in all breadcrumb levels
   - Invalidates cache for that file

5. **ItemFinished Event** (lines 603-630)
   - Removes from syncing_files
   - Requests fresh FileInfo
   - Invalidates cache

6. **LocalIndexUpdated Event** (lines 632-678)
   - Invalidates cache for all mentioned files
   - Requests fresh FileInfo for visible files
   - Queues prefetch for nearby files

7. **Prefetch System** (lines 1710-1785)
   - Requests FileInfo for items above/below selection
   - Updates states for non-visible items
   - Triggers on idle (300ms after last input)

**Conflicts:**
- Browse handler preserves states from syncing_files, but ItemFinished removes from syncing_files → race condition
- manually_set_states blocks API updates, but API is the source of truth → stale data
- Cache states used as fallback, but might be stale if sequence changed → incorrect states
- Ancestor checking in Browse handler fights with FileInfo handler → 130 lines of band-aids

**Lines Involved**:
- 584-678 (event handlers)
- 840-950 (FileInfo handler)
- 1710-1785 (prefetch)
- 1885-2008 (browse root)
- 2009-2175 (browse non-root with 130+ lines of ancestor checking)

### Problem 3: State Preservation Creates Staleness

**Current Logic** (lines 1932-1945 for root, duplicated at 2124-2137 for non-root):
```rust
// Preserve Syncing states (actively syncing files)
let mut file_sync_states = HashMap::new();
for item in &items {
    let sync_key = format!("{}:{}", folder_id, &item.name);

    if self.syncing_files.contains(&sync_key) {
        // Keep Syncing state
        file_sync_states.insert(item.name.clone(), SyncState::Syncing);
        continue;
    }

    // Check if there's a manual state change in progress
    if let Some(_manual_change) = self.manually_set_states.get(&sync_key) {
        // Preserve existing state from memory
        if let Some(existing_level) = self.breadcrumb_trail.get(level_idx) {
            if let Some(&existing_state) = existing_level.file_sync_states.get(&item.name) {
                file_sync_states.insert(item.name.clone(), existing_state);
                continue;
            }
        }
    }

    // Try to get state from cache
    if let Some(&cached_state) = cached_states.get(&item.name) {
        file_sync_states.insert(item.name.clone(), cached_state);
    }
}
```

**Why This Creates Bugs:**
- **Scenario**: User unignores `XDA/4_punya` directory
  1. User action adds to manually_set_states with SetUnignored action
  2. API call sent: `PUT /rest/db/ignores` removes pattern from .stignore
  3. Syncthing processes change, folder sequence increments
  4. User immediately enters directory (before API responds)
  5. Browse API called for `prefix=XDA/4_punya/`
  6. Browse results arrive with list of subdirectories
  7. **Preservation logic checks manually_set_states** → finds parent was just unignored
  8. **Looks at cache for subdirectory states** → finds Ignored (stale, from before parent was unignored)
  9. **Sets all subdirectories to Ignored in UI** → BUG!
  10. FileInfo API eventually responds with correct states, but user already sees wrong states

- **Root Cause**: Preservation logic trusts in-memory/cache over fresh API data

**Lines Involved**: 1932-1945 (root), 2124-2137 (non-root), 2047-2122 (ancestor checking that tries to fix this but fails)

### Problem 4: Code Duplication

**Browse handler has two nearly-identical implementations:**

1. **Root directory** (lines 1885-2008): 123 lines
2. **Non-root directory** (lines 2009-2175): 166 lines

**Differences:**
- Non-root has 130+ lines of ancestor ignored checking (lines 2047-2122)
- Non-root has parent level state clearing (lines 2109-2122)
- Otherwise identical preservation logic

**Why This Is Bad:**
- Bugs must be fixed twice
- Changes must be synchronized
- Increases maintenance burden
- Shows that the architecture is fighting itself (needed special case for "inside ignored directory")

---

## All State Change Scenarios

This section documents **every single way** sync states change in the application, covering user actions, Syncthing events, API responses, and navigation.

### User-Initiated State Changes

#### 1. User Ignores a File/Directory

**Action**: Press `i` on selected item

**Code Path**:
1. **Line 3089-3190**: `toggle_ignore()` function
2. **Line 3111-3129**: Fetch current .stignore patterns via `GET /rest/db/ignores`
3. **Line 3131-3159**: Add new pattern with leading slash format (`/path/to/item`)
4. **Line 3161-3168**: Send updated patterns via `PUT /rest/db/ignores`
5. **Line 3178-3190**: On success:
   - Add to `manually_set_states` with `ManualAction::SetIgnored`
   - Set `file_sync_states` to `SyncState::Ignored` in current breadcrumb level
   - Add to `ignored_exists` HashMap (check filesystem for existence)
   - Request folder rescan via `POST /rest/db/scan`

**State Changes**:
- **Immediate**: UI shows Ignored state (⚠️ if exists, 🚫 if deleted)
- **Async**: Syncthing processes ignore, emits events, folder sequence increments
- **Eventually**: FileInfo API returns Ignored state (if not rejected by validation)

**Affected Data Structures**:
- `level.file_sync_states`: Updated immediately
- `level.ignored_exists`: Updated immediately (filesystem check)
- `manually_set_states`: Tracks user action for 10 seconds
- Cache: Will be invalidated by sequence change
- Syncthing .stignore: Modified on server

#### 2. User Unignores a File/Directory

**Action**: Press `i` on ignored item

**Code Path**:
1. **Line 3089-3190**: `toggle_ignore()` function
2. **Line 3111-3129**: Fetch current .stignore patterns
3. **Line 3131-3159**: Remove matching pattern from list
4. **Line 3161-3168**: Send updated patterns via `PUT /rest/db/ignores`
5. **Line 3178-3190**: On success:
   - Add to `manually_set_states` with `ManualAction::SetUnignored`
   - Remove from `file_sync_states` (clears Ignored state)
   - Remove from `ignored_exists`
   - Request folder rescan

**State Changes**:
- **Immediate**: UI clears state (shows Unknown or cached state)
- **Async**: Syncthing processes un-ignore, begins syncing file
- **Eventually**: FileInfo API returns actual state (Synced/OutOfSync/RemoteOnly/etc)

**Known Bug**: If user immediately enters unignored directory, subdirectories show as Ignored (stale cache states preserved by lines 2047-2122)

#### 3. User Ignores AND Deletes a File/Directory

**Action**: Press `I` (capital I) on selected item

**Code Path**:
1. **Line 3263-3345**: `ignore_and_delete()` function
2. **Line 3278-3287**: Fetch current .stignore patterns
3. **Line 3289-3314**: Add ignore pattern
4. **Line 3316-3323**: Send updated patterns
5. **Line 3325-3345**: On success:
   - Add to `manually_set_states` with `ManualAction::SetIgnored`
   - Set state to `SyncState::Ignored`
   - **Do NOT add to `ignored_exists`** (will be deleted, so exists=false)
   - Translate path to host filesystem
   - Delete file/directory via `fs::remove_file` or `fs::remove_dir_all`
   - Request folder rescan

**State Changes**:
- **Immediate**: UI shows 🚫 (ignored, deleted from disk)
- **Async**: Syncthing detects file deletion, updates states
- **Eventually**: FileInfo confirms Ignored state

#### 4. User Deletes (Without Ignoring)

**Action**: Press `d`, confirm deletion

**Code Path**:
1. **Line 2470-2561**: `delete_file()` function
2. **Line 2474-2495**: Show confirmation dialog
3. **Line 2497-2507**: Translate path to host filesystem
4. **Line 2509-2537**: Delete file/directory via `fs::remove_file` or `fs::remove_dir_all`
5. **Line 2539-2555**: On success:
   - Request folder rescan
   - Show toast notification
   - Cache invalidation happens via LocalIndexUpdated event

**State Changes**:
- **Immediate**: No state change (file still in UI until rescan)
- **Async**: Syncthing detects deletion via rescan, emits LocalIndexUpdated event
- **Eventually**: File disappears from Browse API results or shows as LocalOnly (if receive-only folder)

#### 5. User Restores Deleted Files (Receive-Only)

**Action**: Press `R` on folder with local changes

**Code Path**:
1. **Line 3347-3442**: `revert_folder()` function
2. **Line 3353-3357**: Show confirmation dialog with changed file count
3. **Line 3359-3394**: Send `POST /rest/db/revert?folder=<id>`
4. **Line 3396-3442**: On success:
   - Clear breadcrumb trail (return to folder list)
   - Invalidate entire folder cache
   - Request fresh folder status
   - Show toast notification

**State Changes**:
- **Immediate**: UI returns to folder list
- **Async**: Syncthing restores deleted files from remote devices
- **Eventually**: Files reappear in Browse API results with Synced state

### Navigation-Triggered State Changes

#### 6. User Enters a Directory

**Action**: Press `Enter` on directory item

**Code Path**:
1. **Line 1618-1755**: `enter_directory()` function
2. **Line 1625-1653**: Build new prefix path (e.g., `prefix=XDA/4_punya/`)
3. **Line 1655-1675**: Check cache for valid Browse result
4. **Line 1677-1691**: If cache valid, load immediately, then request fresh data in background
5. **Line 1693-1708**: If cache stale/missing, request Browse API with High priority
6. **Line 1710-1755**: Create new breadcrumb level, trigger prefetch for visible items

**State Changes During Cache Hit**:
1. **Immediate**: UI shows cached states (might be stale)
2. **Background**: Browse API requested with Medium priority
3. **Later**: FileInfo API requested for visible items (High priority)
4. **Eventually**: Fresh states replace cached states

**State Changes During Cache Miss**:
1. **Immediate**: UI shows empty (or loading state if implemented)
2. **Async**: Browse API returns item list
3. **Lines 2009-2175**: Browse handler processes results:
   - **Lines 2047-2122**: Check if inside ignored directory (ancestor checking)
   - **Lines 2124-2137**: Preserve syncing_files and manually_set_states
   - **Lines 2139-2165**: Fall back to cached states
   - **Lines 2167-2175**: Request FileInfo for items without states
4. **Eventually**: FileInfo responses populate fresh states

**Known Issue**: Lines 2047-2122 try to detect "just unignored parent" scenario but fail because they rely on stale cache states for subdirectories

#### 7. User Backs Out of Directory

**Action**: Press `←` or `Backspace`

**Code Path**:
1. **Line 1757-1787**: `go_back()` function
2. **Line 1765-1783**: Pop breadcrumb level, restore focus to parent level
3. **No API calls** (parent level already loaded)

**State Changes**:
- **None** - parent breadcrumb level retains existing states
- **UI Update**: Parent level becomes focused, shows existing states

#### 8. User Navigates Up/Down in File List

**Action**: Press `↑` / `↓` or `j` / `k` (vim mode)

**Code Path**:
1. **Line 494-569**: Main event loop, `KeyCode::Up` / `KeyCode::Down` handlers
2. **Line 509-527**: Update selection in current breadcrumb level's `ListState`
3. **Line 1789-1808**: Idle timer reset (prefetch delayed by 300ms)

**State Changes**:
- **Immediate**: Selection highlight moves to new item
- **After 300ms idle**: Prefetch triggered for items above/below selection (lines 1710-1755)

#### 9. User Hovers on Item (Idle Prefetch)

**Action**: Stop moving selection for 300ms

**Code Path**:
1. **Line 1789-1808**: Check if 300ms elapsed since last input
2. **Line 1710-1755**: Prefetch system activates
3. **Line 1715-1745**: Request FileInfo for selected item + 2 above + 2 below
4. **Line 1747-1755**: Queue additional prefetch requests with Low priority

**State Changes**:
- **Async**: FileInfo requests sent with Medium/Low priority
- **Eventually**: States updated for prefetched items (lines 840-950)

### Syncthing Event-Triggered State Changes

#### 10. ItemStarted Event (Sync Begins)

**Event Source**: `/rest/events` long-polling (line 401-483)

**Payload Example**:
```json
{
  "id": 1234,
  "type": "ItemStarted",
  "data": {
    "folder": "lok75-7d42r",
    "item": "XDA/4_punya/IMG_1234.jpg",
    "action": "update"
  }
}
```

**Code Path**:
1. **Line 584-601**: ItemStarted event handler
2. **Line 587**: Add to `syncing_files` HashSet (`"folder_id:file_path"`)
3. **Line 589-598**: Update state to `SyncState::Syncing` in ALL breadcrumb levels
4. **Line 600**: Invalidate cache for this file

**State Changes**:
- **Immediate**: All visible instances of file show 🔄 Syncing icon
- **Persistent**: Remains in Syncing state until ItemFinished event

**Lines Involved**: 584-601

#### 11. ItemFinished Event (Sync Completes)

**Event Source**: `/rest/events` long-polling

**Payload Example**:
```json
{
  "id": 1235,
  "type": "ItemFinished",
  "data": {
    "folder": "lok75-7d42r",
    "item": "XDA/4_punya/IMG_1234.jpg",
    "error": null,
    "action": "update"
  }
}
```

**Code Path**:
1. **Line 603-630**: ItemFinished event handler
2. **Line 606**: Remove from `syncing_files` HashSet
3. **Line 608**: Invalidate cache for this file
4. **Line 610-627**: Request fresh FileInfo to get final state

**State Changes**:
- **Immediate**: File removed from syncing_files (no longer protected from state updates)
- **Async**: FileInfo API request sent
- **Eventually**: Fresh state replaces Syncing state (Synced/OutOfSync/etc)

**Race Condition**: If Browse result arrives between ItemFinished and FileInfo response, preservation logic (lines 1932-1945) will see file NOT in syncing_files, check cache (stale), and use stale state

**Lines Involved**: 603-630

#### 12. LocalIndexUpdated Event (Local Changes)

**Event Source**: `/rest/events` long-polling

**Payload Example**:
```json
{
  "id": 1236,
  "type": "LocalIndexUpdated",
  "data": {
    "folder": "lok75-7d42r",
    "items": 5,
    "filenames": [
      "XDA/4_punya/IMG_1234.jpg",
      "XDA/4_punya/IMG_1235.jpg",
      "XDA/4_punya/notes.txt"
    ],
    "sequence": 15234
  }
}
```

**Code Path**:
1. **Line 632-678**: LocalIndexUpdated event handler
2. **Line 638-643**: Update folder sequence in cache (invalidates all cached Browse results)
3. **Line 645-668**: For each filename:
   - Invalidate file's cached state
   - If file is visible in current breadcrumb, request fresh FileInfo (High priority)
   - Otherwise queue prefetch for later (Low priority)
4. **Line 670-676**: If current directory affected, request fresh Browse

**State Changes**:
- **Immediate**: Cache invalidated for changed files
- **Async**: FileInfo requests sent for visible files
- **Eventually**: Fresh states updated in UI

**Lines Involved**: 632-678

### API Response-Triggered State Changes

#### 13. Browse API Response (Root Directory)

**API Call**: `GET /rest/db/browse?folder=<id>`

**Response Example**:
```json
[
  {"name": "XDA", "type": "FILE_INFO_TYPE_DIRECTORY", "size": 0, "modTime": "2025-10-26T20:58:21Z"},
  {"name": "Movies", "type": "FILE_INFO_TYPE_DIRECTORY", "size": 0, "modTime": "2025-10-25T14:32:10Z"},
  {"name": "notes.txt", "type": "FILE_INFO_TYPE_FILE", "size": 1234, "modTime": "2025-10-26T22:15:43Z"}
]
```

**Code Path**:
1. **Line 1885-2008**: Browse result handler for root directory
2. **Line 1888-1901**: Load cached states (validated by folder sequence)
3. **Line 1903-1913**: Sort items by selected mode (Icon/A-Z/DateTime/Size)
4. **Line 1915-1930**: Update cache with new Browse result
5. **Line 1932-1945**: Preserve syncing/manual states, fall back to cache (STATE PRESERVATION LOGIC)
6. **Line 1947-1955**: Check filesystem existence for ignored items
7. **Line 1957-1995**: Create/update breadcrumb level with merged states
8. **Line 1997-2006**: Request FileInfo for items without states

**State Priority (in preservation logic)**:
1. **Syncing files** (in syncing_files HashSet) → keep Syncing state
2. **Manually set files** (in manually_set_states) → preserve existing in-memory state
3. **Cached states** (from SQLite) → use if available
4. **No state** → request FileInfo

**Problem**: Preservation logic trusts stale cache over fresh API responses that will arrive shortly

**Lines Involved**: 1885-2008

#### 14. Browse API Response (Non-Root Directory)

**API Call**: `GET /rest/db/browse?folder=<id>&prefix=XDA/4_punya/`

**Code Path**:
1. **Line 2009-2175**: Browse result handler for non-root directory
2. **Line 2017-2045**: Same loading/sorting/caching as root handler
3. **Line 2047-2122**: **NEW ANCESTOR CHECKING LOGIC** (130 lines):
   - **Lines 2050-2063**: Check if current directory or any ancestor was recently unignored
   - **Lines 2065-2089**: Walk up path checking manually_set_states for SetUnignored action
   - **Lines 2091-2107**: If found, clear all Ignored states from cache/memory
   - **Lines 2109-2122**: Also clear Ignored state in parent breadcrumb level
   - **Lines 2124-2165**: Otherwise, check if inside ignored parent directory (cache check all ancestors)
4. **Line 2124-2137**: Same preservation logic as root (STATE PRESERVATION)
5. **Line 2139-2165**: Mark all items as Ignored if inside ignored parent
6. **Line 2167-2175**: Request FileInfo for items without states

**Why Lines 2047-2122 Don't Fix the Bug**:
1. User unignores `XDA/4_punya` → adds to manually_set_states
2. User enters directory → Browse API called with `prefix=XDA/4_punya/`
3. Browse results arrive → handler runs ancestor checking
4. **Finds parent was unignored** → clears Ignored states from current level
5. **BUT** - preservation logic (lines 2124-2137) runs AFTER ancestor checking
6. Preservation logic sees cached states are Ignored → puts them back!
7. **Bug persists** because two pieces of code fight each other

**Lines Involved**: 2009-2175 (entire handler), 2047-2122 (ancestor checking band-aid)

#### 15. FileInfo API Response

**API Call**: `GET /rest/db/file?folder=<id>&file=<path>`

**Response Example**:
```json
{
  "availability": [
    {"id": "52FR4F4-...", "fromTemporary": false}
  ],
  "global": {
    "name": "XDA/4_punya/IMG_1234.jpg",
    "type": "FILE_INFO_TYPE_FILE",
    "size": 2456789,
    "modTime": "2025-10-26T20:58:21.580021398Z",
    "sequence": 15234,
    "deleted": false,
    "invalid": false
  },
  "local": {
    "name": "XDA/4_punya/IMG_1234.jpg",
    "type": "FILE_INFO_TYPE_FILE",
    "size": 2456789,
    "modTime": "2025-10-26T20:58:21.580021398Z",
    "sequence": 15234
  }
}
```

**Code Path**:
1. **Line 840-950**: FileInfo response handler
2. **Line 845-865**: Parse global/local/availability from response
3. **Line 867-914**: **DETERMINE SYNC STATE** from API data:
   - Global deleted + Local exists → OutOfSync
   - Global exists + Local missing → RemoteOnly
   - Local exists + Global missing → LocalOnly
   - Global invalid or sequences differ → OutOfSync
   - Both exist, sequences match → Synced
4. **Line 870-905**: **STATE TRANSITION VALIDATION** (REJECTION LOGIC):
   - Check if file in manually_set_states
   - If SetIgnored action, only accept Ignored state
   - If SetUnignored action, reject Ignored state
   - **Otherwise SKIP UPDATE** (keep stale in-memory state)
5. **Line 907-914**: If validation passes, update state in cache
6. **Line 916-946**: Update state in ALL visible breadcrumb levels
7. **Line 948**: Check filesystem existence if state is Ignored

**Problem**: Lines 870-905 reject Syncthing's authoritative response based on user action from up to 10 seconds ago

**Lines Involved**: 840-950

### Cache-Triggered State Changes

#### 16. Cache Hit on Directory Entry

**Trigger**: User enters directory, cache has valid Browse result (sequence matches)

**Code Path**:
1. **Line 1655-1675**: Check cache in `enter_directory()`
2. **Line 1660-1670**: Load cached Browse result + cached states
3. **Line 1677-1691**: Display cached data immediately, request fresh data in background

**State Changes**:
- **Immediate**: UI shows cached states (might be seconds/minutes old)
- **Background**: Fresh Browse + FileInfo requests queued (Medium priority)
- **Eventually**: Fresh states replace cached states (if not rejected by validation)

**Advantage**: Instant navigation, no loading delay
**Disadvantage**: Might show stale states briefly

#### 17. Cache Miss on Directory Entry

**Trigger**: User enters directory, no cache or cache stale (sequence changed)

**Code Path**:
1. **Line 1655-1675**: Check cache in `enter_directory()`
2. **Line 1693-1708**: Request fresh Browse API (High priority)
3. **Line 1710-1755**: Wait for Browse response → handler creates breadcrumb level → requests FileInfo

**State Changes**:
- **Immediate**: UI shows empty/loading (current: just empty)
- **Async**: Browse API responds with item list
- **Later**: FileInfo API responds with states
- **Eventually**: UI populated with fresh data

**Advantage**: Always fresh data
**Disadvantage**: Visible delay during navigation

### System-Triggered State Changes

#### 18. Folder Sequence Change (Global Invalidation)

**Trigger**: LocalIndexUpdated event with new sequence number

**Code Path**:
1. **Line 638-643**: Update folder sequence in cache
2. **Effect**: All cached Browse results for this folder now invalid (sequence mismatch)
3. **Next navigation**: Cache misses, fresh Browse API requested

**State Changes**:
- **Immediate**: None (cache updated, UI unchanged)
- **Next browse**: Cache miss → fresh data loaded

**Purpose**: Ensures cache stays synchronized with Syncthing's actual folder state

#### 19. File Sequence Change (Targeted Invalidation)

**Trigger**: LocalIndexUpdated event with filenames array

**Code Path**:
1. **Line 645-668**: For each filename, invalidate cached state
2. **Line 650-661**: If file visible, request fresh FileInfo (High priority)
3. **Line 663-666**: Otherwise queue prefetch (Low priority)

**State Changes**:
- **Immediate**: Cache invalidated (specific files only)
- **Async**: FileInfo requests sent
- **Eventually**: Fresh states updated in visible breadcrumbs

**Purpose**: Granular invalidation - only refresh changed files, not entire directory

#### 20. Manual State Expiration (10 Second Timeout)

**Trigger**: 10 seconds pass since user action

**Code Path**:
1. **Line 870-883**: Check timestamp in manually_set_states
2. **Line 884**: If older than 10 seconds, accept any state from API
3. **Effect**: State transition validation disabled after timeout

**State Changes**:
- **After timeout**: API responses no longer rejected
- **Next FileInfo**: Fresh state accepted regardless of transition

**Purpose**: Safety valve to prevent permanent state blocking
**Problem**: Arbitrary timeout, not based on actual event completion

### Summary: State Change Flow Diagram

```
User Action (ignore/unignore/delete)
  ├─> Update manually_set_states (expires in 10s)
  ├─> Update file_sync_states (immediate UI change)
  ├─> Update ignored_exists (filesystem check)
  ├─> PUT /rest/db/ignores (modify .stignore)
  ├─> POST /rest/db/scan (trigger rescan)
  └─> Syncthing processes change
       ├─> Folder sequence increments
       ├─> LocalIndexUpdated event emitted
       │    ├─> Cache invalidated
       │    └─> FileInfo requests sent
       ├─> ItemStarted event (if syncing)
       │    ├─> Add to syncing_files
       │    └─> Set state to Syncing
       ├─> ItemFinished event (when complete)
       │    ├─> Remove from syncing_files
       │    └─> Request fresh FileInfo
       └─> FileInfo API returns final state
            ├─> State transition validation runs
            ├─> Accept or REJECT based on manually_set_states
            └─> Update cache + visible breadcrumbs

User Navigation (enter directory)
  ├─> Check cache for Browse result
  ├─> If cache valid (sequence matches)
  │    ├─> Display cached states immediately
  │    └─> Request fresh data in background
  └─> If cache stale/missing
       ├─> Request Browse API (High priority)
       ├─> Browse response arrives
       │    ├─> Check if inside ignored directory (ancestor checking)
       │    ├─> Preserve syncing/manual states (STATE PRESERVATION)
       │    ├─> Fall back to cached states
       │    └─> Create breadcrumb level
       └─> Request FileInfo for items without states
            └─> FileInfo responses update states (if not rejected)

Syncthing Events (background monitoring)
  ├─> ItemStarted: Add to syncing_files, set state to Syncing
  ├─> ItemFinished: Remove from syncing_files, request FileInfo
  └─> LocalIndexUpdated: Invalidate cache, request FileInfo for visible items

Idle Prefetch (300ms after last input)
  └─> Request FileInfo for selected + nearby items (Low priority)
```

---

## Proposed Architecture

### Core Principle: Trust the API

**Syncthing's REST API is the single source of truth for all file states.**

We modified Syncthing's configuration → Syncthing processes changes → Syncthing tells us the result → **We trust it.**

### New Data Flow

```
User Action
  ├─> Set UI to "Loading" state
  ├─> Send API request (PUT/POST)
  └─> Wait for response
       ├─> Success: Request fresh data (GET FileInfo/Browse)
       └─> Failure: Show error, revert UI

API Response
  ├─> Parse response
  ├─> Update cache (for performance)
  └─> Update UI (no rejection, no validation)

Cache
  ├─> Used for instant navigation (show immediately)
  ├─> Validated by sequence numbers (Syncthing's mechanism)
  └─> Always replaced by fresh API data when available
```

### What Gets Removed (660 lines)

1. **manually_set_states HashMap** (22 references):
   - Lines 242 (declaration)
   - Lines 870-905 (state transition validation)
   - Lines 1932-1945, 2124-2137 (preservation logic references)
   - Lines 2050-2089 (ancestor unignore checking)
   - Lines 3178-3190 (toggle_ignore updates)
   - Lines 3325-3345 (ignore_and_delete updates)
   - **Why**: Fighting API instead of trusting it

2. **syncing_files HashSet** (17 references):
   - Lines 244 (declaration)
   - Lines 587 (ItemStarted adds)
   - Lines 606 (ItemFinished removes)
   - Lines 1932-1945, 2124-2137 (preservation checks)
   - Lines 89 (render.rs override check)
   - **Why**: API already tells us sync state via ItemStarted/ItemFinished events

3. **State transition validation** (lines 870-905):
   - Entire validation block that rejects API responses
   - **Why**: Syncthing's API is authoritative

4. **Ancestor ignored checking** (lines 2047-2122):
   - 130+ lines checking if parent was recently unignored
   - Clearing stale states from current + parent levels
   - **Why**: If we trust API, no need to fight stale cache

5. **State preservation logic** (lines 1932-1945, 2124-2137):
   - Code that preserves syncing/manual states from memory
   - Falls back to cache instead of waiting for API
   - **Why**: Fresh API data is better than stale cache

6. **Duplicate Browse handlers** (consolidate into one):
   - Root handler (lines 1885-2008) and non-root handler (lines 2009-2175)
   - Combine into single handler, remove special cases
   - **Why**: No need for ancestor checking, same logic for all directories

### What Gets Added

1. **Explicit Loading States**:
   - Add `SyncState::Loading` enum variant
   - Show loading indicator in UI (spinner or dimmed icon)
   - Set during API requests, cleared when response arrives

2. **Simplified Browse Handler** (single version):
   ```rust
   ApiResponse::BrowseResult { folder_id, prefix, items } => {
       let level_idx = self.get_level_index(&folder_id, prefix.as_deref());

       // Sort items
       let mut items = items.clone();
       items.sort_by(...);

       // Update cache with fresh Browse result
       cache_manager.cache_browse_result(...);

       // Load cached states (might be stale, will be replaced)
       let cached_states = cache_manager.load_cached_file_states(...);

       // Build initial state map from cache
       let mut file_sync_states = HashMap::new();
       for item in &items {
           if let Some(&cached_state) = cached_states.get(&item.name) {
               file_sync_states.insert(item.name.clone(), cached_state);
           }
       }

       // Check filesystem existence for ignored items (from cache)
       let ignored_exists = check_ignored_existence(&items, &file_sync_states, ...);

       // Create/update breadcrumb level
       let level = BreadcrumbLevel {
           folder_id: folder_id.clone(),
           prefix: prefix.clone(),
           items,
           file_sync_states,
           ignored_exists,
           state: ListState::default().with_selected(Some(0)),
           ...
       };

       // Update or push breadcrumb level
       if let Some(existing_level) = self.breadcrumb_trail.get_mut(level_idx) {
           *existing_level = level;
       } else {
           self.breadcrumb_trail.push(level);
       }

       // Request fresh FileInfo for ALL items (let API service deduplicate)
       for item in &level.items {
           let item_path = if let Some(ref prefix) = prefix {
               format!("{}{}", prefix, item.name)
           } else {
               item.name.clone()
           };

           self.api_request_tx.send(ApiRequest::GetFileInfo {
               folder_id: folder_id.clone(),
               file_path: item_path,
               priority: Priority::Medium,
           }).ok();
       }
   }
   ```

3. **Simplified FileInfo Handler** (no validation):
   ```rust
   ApiResponse::FileInfoResult { folder_id, file_path, details } => {
       let Ok(details) = details else {
           log_debug(&format!("FileInfo error: {:?}", details));
           return;
       };

       // Determine state from API response (no validation!)
       let state = determine_sync_state(&details.global, &details.local);

       // Update cache
       cache_manager.cache_file_state(&folder_id, &file_path, state);

       // Update ALL visible breadcrumb levels containing this file
       for level in &mut self.breadcrumb_trail {
           if level.folder_id != folder_id {
               continue;
           }

           let item_name = extract_item_name(&file_path, level.prefix.as_deref());

           if level.file_sync_states.contains_key(&item_name) {
               level.file_sync_states.insert(item_name.clone(), state);

               // Update ignored_exists if needed
               if state == SyncState::Ignored {
                   let exists = check_single_file_exists(&item_name, ...);
                   level.ignored_exists.insert(item_name, exists);
               }
           }
       }
   }
   ```

4. **Event Handlers Use Loading State**:
   ```rust
   // ItemStarted event
   if event_type == "ItemStarted" {
       let state_key = format!("{}:{}", folder_id, item_path);

       // Set to Loading (not Syncing)
       for level in &mut self.breadcrumb_trail {
           if level.folder_id == folder_id {
               let item_name = extract_item_name(&item_path, level.prefix.as_deref());
               if level.file_sync_states.contains_key(&item_name) {
                   level.file_sync_states.insert(item_name, SyncState::Loading);
               }
           }
       }

       // Invalidate cache
       cache_manager.invalidate_file_state(&folder_id, &item_path);

       // NO tracking in HashSet - just request fresh data when ItemFinished arrives
   }

   // ItemFinished event
   if event_type == "ItemFinished" {
       // Simply request fresh FileInfo - no tracking needed
       self.api_request_tx.send(ApiRequest::GetFileInfo {
           folder_id: folder_id.clone(),
           file_path: item_path.clone(),
           priority: Priority::High,
       }).ok();

       // API response will update state naturally (no race conditions)
   }
   ```

5. **User Actions Return to Loading State**:
   ```rust
   // In toggle_ignore() after successful API call
   // Before: Set to Ignored immediately, add to manually_set_states
   // After: Set to Loading, wait for API

   // Send ignore/unignore request
   let response = client.set_ignore_patterns(&folder_id, &updated_patterns).await;

   if response.is_ok() {
       // Set to Loading state (show spinner)
       if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
           level.file_sync_states.insert(item_name.clone(), SyncState::Loading);
       }

       // Trigger rescan
       self.api_request_tx.send(ApiRequest::RescanFolder {
           folder_id: folder_id.clone(),
       }).ok();

       // NO manually_set_states tracking - just wait for API response
       // LocalIndexUpdated event will trigger FileInfo request
       // FileInfo response will update state naturally
   }
   ```

### What Stays the Same

1. **Cache system** (SQLite database):
   - Browse results cached with folder sequence validation
   - File states cached with file sequence validation
   - **Purpose**: Performance (instant navigation), not correctness
   - **Rule**: Cache is always REPLACED by fresh API data, never trusted over it

2. **Event monitoring** (long-polling /rest/events):
   - ItemStarted, ItemFinished, LocalIndexUpdated events
   - Triggers cache invalidation and fresh API requests
   - **Change**: No tracking HashSets, just request fresh data

3. **Prefetch system** (idle detection):
   - Request FileInfo for items near selection
   - 300ms idle threshold
   - **Change**: Prefetch doesn't check manually_set_states, just requests data

4. **Priority queue** (api_service.rs):
   - High priority: user actions
   - Medium priority: visible items
   - Low priority: prefetch
   - **No changes needed**

### State Lifecycle in New Architecture

```
Initial State: Unknown (or cached if available)
  ├─> User Action: → Loading → API responds → Final State
  ├─> Navigation: → Cached State → FileInfo responds → Fresh State
  ├─> Event: → Cache invalidated → FileInfo responds → Fresh State
  └─> Prefetch: → Cached State → FileInfo responds → Fresh State

Loading State
  ├─> Shows spinner or dimmed icon in UI
  ├─> Prevents user actions (can't ignore/delete while loading)
  └─> Cleared when API responds (success or error)

Final States (from API only)
  ├─> Synced ✅
  ├─> OutOfSync ⚠️
  ├─> LocalOnly 💻
  ├─> RemoteOnly ☁️
  ├─> Ignored (⚠️ exists or 🚫 deleted)
  └─> Unknown (API error or not requested yet)
```

### Advantages of New Architecture

1. **Correctness**: Always shows Syncthing's actual state (no stale data from 10s ago)
2. **Simplicity**: 660 fewer lines, single Browse handler, no special cases
3. **Maintainability**: One truth source (API), easier to debug and reason about
4. **No Race Conditions**: No competing updates, no validation conflicts
5. **Clear Feedback**: Loading states show when operations are in progress
6. **Faster Development**: No need to track timing windows or state transitions

### Potential Concerns & Mitigations

**Concern 1**: "What if user ignores a file and immediately enters directory? Will it flash between states?"

**Mitigation**: Loading state prevents confusion
- User presses `i` → item shows 🔄 Loading
- User enters directory → items show 🔄 Loading (or cached states)
- LocalIndexUpdated event arrives → FileInfo requests sent
- FileInfo responses arrive → items show ⚠️ Ignored
- No confusing flashes because Loading state is explicit

**Concern 2**: "What if network is slow? Will UI be unresponsive?"

**Mitigation**: Cache provides instant navigation
- Cache hits still show data immediately (might be slightly stale)
- Fresh data replaces cache in background (no blocking)
- Only truly slow: first load of never-seen directory (unavoidable)

**Concern 3**: "What if Syncthing is behind? Will we show wrong states?"

**Answer**: **We already do** - current code shows manually set states that might not match Syncthing's reality. New architecture at least shows Syncthing's actual state, even if Syncthing is behind. If Syncthing is wrong, that's a Syncthing bug, not our bug.

**Concern 4**: "What about the ItemStarted → ItemFinished window? How do we show Syncing state?"

**Solution**: Use Loading state temporarily
- ItemStarted event: Set to Loading (🔄)
- ItemFinished event: Request FileInfo
- FileInfo response: Update to final state (Synced, etc)
- Alternative: Add back Syncing state but populate it from events only, not manual tracking

---

## Migration Strategy

### Phase 1: Preparation & Testing Harness (1-2 hours)

**Goal**: Set up infrastructure for safe refactoring

**Tasks**:

1. **Create test script** (`test_sync_states.sh`):
   ```bash
   #!/bin/bash
   # Test scenarios for sync state refactoring

   FOLDER_ID="lok75-7d42r"
   TEST_DIR="XDA/test_refactor"

   echo "Test 1: Ignore file, enter directory"
   # - Navigate to file
   # - Press 'i' to ignore
   # - Immediately enter parent directory
   # - Expected: File shows Loading → Ignored
   # - Current bug: File shows Synced (stale)

   echo "Test 2: Unignore directory, enter immediately"
   # - Navigate to ignored directory
   # - Press 'i' to unignore
   # - Immediately press Enter
   # - Expected: Subdirectories show Loading → RemoteOnly/Synced
   # - Current bug: Subdirectories show Ignored (stale)

   echo "Test 3: Delete file, rescan"
   # - Navigate to file
   # - Press 'd' to delete
   # - Wait for rescan
   # - Expected: File disappears or shows LocalOnly (if receive-only)
   # - Current: Works correctly

   echo "Test 4: Rapid navigation during sync"
   # - Trigger sync (unignore large directory)
   # - Rapidly navigate in/out of directory
   # - Expected: States consistent, no crashes
   # - Current: Sometimes shows wrong states
   ```

2. **Add comprehensive debug logging**:
   - Log every state change with timestamp and source
   - Log every API request/response
   - Log cache hits/misses
   - Create `/tmp/synctui-refactor.log` during migration

3. **Document current behavior**:
   - Run test script, capture logs and screenshots
   - Create `BEFORE_REFACTOR.md` with observed bugs
   - Reference for comparison after refactor

4. **Create git branch**:
   ```bash
   git checkout -b refactor-sync-states
   git commit -m "Checkpoint before sync state refactoring"
   ```

### Phase 2: Add Loading State (2-3 hours)

**Goal**: Introduce explicit loading state without removing old code

**Tasks**:

1. **Add SyncState::Loading variant** (src/api.rs):
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum SyncState {
       Synced,
       OutOfSync,
       LocalOnly,
       RemoteOnly,
       Ignored,
       Syncing,      // Keep for now, will merge with Loading later
       Loading,      // NEW
       Unknown,
   }
   ```

2. **Update icon renderer** (src/ui/icons.rs):
   ```rust
   SyncState::Loading => {
       // Show spinner or dimmed icon
       vec![
           Span::styled("🔄", Style::default().fg(Color::Yellow)),
           Span::raw(" "),
       ]
   }
   ```

3. **Test Loading state display**:
   - Manually set a file to Loading
   - Verify icon renders correctly
   - Verify status bar shows "Loading"

4. **Commit**: `git commit -m "Add SyncState::Loading variant and icon"`

### Phase 3: Simplify Browse Handler (3-4 hours)

**Goal**: Remove state preservation and ancestor checking, keep functionality

**Tasks**:

1. **Consolidate Browse handlers** (src/main.rs):
   - Copy root handler (lines 1885-2008) to new function `handle_browse_result()`
   - Remove lines 2047-2122 (ancestor checking)
   - Remove lines 1932-1945 (state preservation)
   - Replace with simple cached state loading
   - Update both root and non-root cases to call new function

2. **New simplified handler**:
   ```rust
   fn handle_browse_result(
       &mut self,
       folder_id: String,
       prefix: Option<String>,
       items: Vec<BrowseItem>,
   ) {
       // Sort items
       let mut items = items;
       self.sort_items(&mut items);

       // Update cache
       self.cache_manager.cache_browse_result(&folder_id, prefix.as_deref(), &items);

       // Load cached states (for instant display)
       let cached_states = self.cache_manager.load_cached_file_states(&folder_id, prefix.as_deref());

       // Build state map (cached only, will be replaced by FileInfo)
       let mut file_sync_states = HashMap::new();
       for item in &items {
           if let Some(&state) = cached_states.get(&item.name) {
               file_sync_states.insert(item.name.clone(), state);
           }
       }

       // Check ignored existence (from cached states)
       let ignored_exists = self.check_ignored_existence(&items, &file_sync_states, &prefix);

       // Create breadcrumb level
       let level = BreadcrumbLevel {
           folder_id: folder_id.clone(),
           folder_label: self.get_folder_label(&folder_id),
           prefix,
           items: items.clone(),
           file_sync_states,
           ignored_exists,
           state: ListState::default().with_selected(Some(0)),
           translated_base_path: self.translate_path(&folder_id, prefix.as_deref()),
       };

       // Update breadcrumb trail
       let level_idx = self.get_or_create_level_index(&folder_id, level.prefix.as_deref());
       if level_idx < self.breadcrumb_trail.len() {
           self.breadcrumb_trail[level_idx] = level;
       } else {
           self.breadcrumb_trail.push(level);
       }

       // Request FileInfo for ALL items (no filtering)
       for item in &items {
           let file_path = if let Some(ref prefix) = level.prefix {
               format!("{}{}", prefix, item.name)
           } else {
               item.name.clone()
           };

           let _ = self.api_request_tx.send(ApiRequest::GetFileInfo {
               folder_id: folder_id.clone(),
               file_path,
               priority: Priority::Medium,
           });
       }
   }
   ```

3. **Update Browse response handler**:
   ```rust
   ApiResponse::BrowseResult { folder_id, prefix, items } => {
       match items {
           Ok(items) => {
               self.handle_browse_result(folder_id, prefix, items);
           }
           Err(e) => {
               log_debug(&format!("Browse error: {}", e));
               // Show error in status bar
           }
       }
   }
   ```

4. **Test**:
   - Navigate through directories
   - Verify states load from cache initially
   - Verify FileInfo updates replace cached states
   - Check debug logs for removed preservation logic

5. **Commit**: `git commit -m "Simplify Browse handler, remove state preservation"`

### Phase 4: Remove State Transition Validation (1-2 hours)

**Goal**: Accept all FileInfo responses without rejection

**Tasks**:

1. **Remove validation block** (lines 870-905 in src/main.rs):
   ```rust
   // BEFORE (delete this):
   if let Some(&manual_change) = self.manually_set_states.get(&state_key) {
       let is_valid_transition = match manual_change.action {
           ManualAction::SetIgnored => state == SyncState::Ignored,
           ManualAction::SetUnignored => state != SyncState::Ignored,
       };

       if !is_valid_transition {
           log_debug("Rejecting invalid transition");
           continue; // DELETE THIS
       }
   }

   // AFTER (just accept the state):
   // (no code - just process the state)
   ```

2. **Simplify FileInfo handler**:
   ```rust
   ApiResponse::FileInfoResult { folder_id, file_path, details } => {
       let Ok(details) = details else {
           log_debug(&format!("FileInfo error for {}:{}", folder_id, file_path));
           return;
       };

       // Determine state (unchanged)
       let state = determine_sync_state(&details.global, &details.local);

       // Update cache (unchanged)
       self.cache_manager.cache_file_state(&folder_id, &file_path, state);

       // Update ALL visible breadcrumb levels (unchanged)
       for level in &mut self.breadcrumb_trail {
           if level.folder_id != folder_id {
               continue;
           }

           let item_name = Self::extract_item_name(&file_path, level.prefix.as_deref());

           if level.file_sync_states.contains_key(&item_name) {
               level.file_sync_states.insert(item_name.clone(), state);

               if state == SyncState::Ignored {
                   let exists = self.check_single_file_exists(&item_name, &level.translated_base_path);
                   level.ignored_exists.insert(item_name, exists);
               }
           }
       }
   }
   ```

3. **Test**:
   - Ignore file → immediately check state (should be Ignored after API)
   - Unignore file → immediately check state (should be RemoteOnly/Synced after API)
   - Check logs: no "Rejecting invalid transition" messages

4. **Commit**: `git commit -m "Remove state transition validation, trust API"`

### Phase 5: Remove Tracking HashMaps (2-3 hours)

**Goal**: Delete manually_set_states and syncing_files, remove all references

**Tasks**:

1. **Remove declarations** (lines 242, 244):
   ```rust
   // DELETE:
   manually_set_states: HashMap<String, ManualStateChange>,
   syncing_files: HashSet<String>,
   ```

2. **Remove struct definitions**:
   ```rust
   // DELETE (around line 234):
   struct ManualStateChange {
       action: ManualAction,
       timestamp: std::time::Instant,
   }

   enum ManualAction {
       SetIgnored,
       SetUnignored,
   }
   ```

3. **Find all references** (use IDE or grep):
   ```bash
   grep -n "manually_set_states" src/main.rs
   grep -n "syncing_files" src/main.rs
   ```

4. **Remove from App initialization** (around line 3500):
   ```rust
   // DELETE:
   manually_set_states: HashMap::new(),
   syncing_files: HashSet::new(),
   ```

5. **Remove from toggle_ignore()** (lines 3178-3190):
   ```rust
   // DELETE:
   self.manually_set_states.insert(
       sync_key,
       ManualStateChange {
           action: if is_ignored { ManualAction::SetUnignored } else { ManualAction::SetIgnored },
           timestamp: std::time::Instant::now(),
       },
   );
   ```

6. **Remove from ignore_and_delete()** (lines 3325-3345):
   ```rust
   // DELETE:
   self.manually_set_states.insert(
       sync_key,
       ManualStateChange {
           action: ManualAction::SetIgnored,
           timestamp: std::time::Instant::now(),
       },
   );
   ```

7. **Remove from ItemStarted event** (lines 587):
   ```rust
   // DELETE:
   self.syncing_files.insert(sync_key.clone());
   ```

8. **Remove from ItemFinished event** (lines 606):
   ```rust
   // DELETE:
   self.syncing_files.remove(&sync_key);
   ```

9. **Remove from render.rs** (lines 88-92):
   ```rust
   // DELETE:
   if app.syncing_files.contains(&sync_key) {
       level.file_sync_states.insert(item.name.clone(), crate::api::SyncState::Syncing);
   }
   ```

10. **Update ItemStarted event to use Loading state**:
    ```rust
    if event_type == "ItemStarted" {
        // Set to Loading (instead of adding to syncing_files)
        for level in &mut self.breadcrumb_trail {
            if level.folder_id == folder_id {
                let item_name = Self::extract_item_name(&item_path, level.prefix.as_deref());
                if level.file_sync_states.contains_key(&item_name) {
                    level.file_sync_states.insert(item_name, SyncState::Loading);
                }
            }
        }

        // Invalidate cache
        self.cache_manager.invalidate_file_state(&folder_id, &item_path);
    }
    ```

11. **Test**:
    - Run `cargo build` → should compile without errors
    - Ignore/unignore files → check states update correctly
    - Trigger sync → check Loading state appears during ItemStarted → ItemFinished
    - Navigate during sync → verify no crashes

12. **Commit**: `git commit -m "Remove manually_set_states and syncing_files tracking"`

### Phase 6: Update User Action Handlers (2-3 hours)

**Goal**: Make user actions show Loading state, wait for API

**Tasks**:

1. **Update toggle_ignore()** (lines 3089-3190):
   ```rust
   // Add after successful PUT /rest/db/ignores:

   // Set to Loading state
   if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
       level.file_sync_states.insert(item_name.clone(), SyncState::Loading);

       // Remove from ignored_exists (will be updated when FileInfo arrives)
       level.ignored_exists.remove(&item_name);
   }

   // Trigger rescan (unchanged)
   let _ = self.api_request_tx.send(ApiRequest::RescanFolder {
       folder_id: folder_id.clone(),
   });

   // Show toast (unchanged)
   self.toast_message = Some((
       format!("{} {}", if was_ignored { "Unignored" } else { "Ignored" }, item_name),
       std::time::Instant::now(),
   ));
   ```

2. **Update ignore_and_delete()** (lines 3263-3345):
   ```rust
   // Add after successful PUT /rest/db/ignores:

   // Set to Loading state (will become Ignored after API responds)
   if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
       level.file_sync_states.insert(item_name.clone(), SyncState::Loading);
   }

   // Delete file (unchanged)
   if is_dir {
       fs::remove_dir_all(&host_path)?;
   } else {
       fs::remove_file(&host_path)?;
   }

   // Note: ignored_exists will be set to false when FileInfo confirms Ignored state

   // Trigger rescan (unchanged)
   // Show toast (unchanged)
   ```

3. **Update delete_file()** (lines 2470-2561):
   ```rust
   // Add after successful deletion:

   // Set to Loading state (file will disappear from Browse or show LocalOnly)
   if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
       level.file_sync_states.insert(item_name.clone(), SyncState::Loading);
   }

   // Trigger rescan (unchanged)
   // Show toast (unchanged)
   ```

4. **Prevent actions during Loading state** (in event handlers):
   ```rust
   // In handle_key_event(), before any action:
   if let Some(level) = self.breadcrumb_trail.get(level_idx) {
       if let Some(selected_idx) = level.state.selected() {
           if let Some(item) = level.items.get(selected_idx) {
               if let Some(&state) = level.file_sync_states.get(&item.name) {
                   if state == SyncState::Loading {
                       // Show message: "Please wait for operation to complete"
                       self.toast_message = Some((
                           "Operation in progress, please wait...".to_string(),
                           std::time::Instant::now(),
                       ));
                       return Ok(()); // Don't allow action
                   }
               }
           }
       }
   }
   ```

5. **Test**:
   - Ignore file → should show 🔄 Loading → then ⚠️ Ignored
   - Unignore file → should show 🔄 Loading → then ✅ Synced or ☁️ RemoteOnly
   - Try to ignore file while Loading → should show toast and prevent action
   - Delete file → should show 🔄 Loading → then disappear

6. **Commit**: `git commit -m "Update user actions to show Loading state"`

### Phase 7: Testing & Bug Fixes (3-4 hours)

**Goal**: Comprehensive testing, fix any issues

**Test Scenarios**:

1. **Basic ignore/unignore**:
   - Ignore file → verify shows Loading → Ignored
   - Unignore file → verify shows Loading → Synced/RemoteOnly
   - Ignore directory → enter immediately → verify children show correct states

2. **Navigation during sync**:
   - Unignore large directory (many files)
   - Rapidly navigate in/out while syncing
   - Verify no crashes, states eventually settle to correct values

3. **Edge cases**:
   - Ignore file, delete manually via filesystem, rescan → verify shows 🚫
   - Ignore directory, unignore immediately, enter before API responds → verify children not stuck as Ignored
   - Network slow (simulate via firewall rules) → verify Loading states persist, then update

4. **Performance**:
   - Large directories (1000+ files) → verify no lag
   - Rapid navigation → verify cache still provides instant display
   - Check debug logs for API request deduplication

5. **Comparison with old behavior**:
   - Compare logs and screenshots with `BEFORE_REFACTOR.md`
   - Verify all bugs are fixed
   - Document any new issues

**Bug Fix Process**:
1. Reproduce bug with test script
2. Add debug logging to narrow down cause
3. Fix code
4. Re-test
5. Commit fix with descriptive message

**Commit**: `git commit -m "Fix [specific bug description]"` (one per fix)

### Phase 8: Cleanup & Documentation (1-2 hours)

**Goal**: Remove old code, update documentation

**Tasks**:

1. **Remove dead code**:
   - Search for any remaining references to deleted structs
   - Remove unused imports
   - Remove unused functions (e.g., old ancestor checking helpers)

2. **Update SYNC.md**:
   - Document new simplified architecture
   - Update state change descriptions
   - Remove references to manually_set_states and syncing_files
   - Add section on Loading states

3. **Update CLAUDE.md**:
   - Simplify architecture section (remove complexity notes)
   - Update "Known Limitations" (remove "state flicker" issues)
   - Add note about Loading states in user actions

4. **Update code comments**:
   - Remove comments about "state preservation" and "fighting race conditions"
   - Add comments explaining Loading state usage
   - Document why we trust API over cache

5. **Final cleanup**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```

6. **Commit**: `git commit -m "Cleanup: remove dead code, update documentation"`

### Phase 9: Merge & Monitor (ongoing)

**Goal**: Merge to main, monitor for issues

**Tasks**:

1. **Create PR** (if using GitHub workflow):
   ```bash
   git push origin refactor-sync-states
   # Create PR with detailed description
   ```

2. **Self-review**:
   - Read through all changes in `git diff master`
   - Verify no unintended changes
   - Check for leftover debug code

3. **Merge to main**:
   ```bash
   git checkout master
   git merge refactor-sync-states
   git push origin master
   ```

4. **Monitor usage**:
   - Use app normally for several days
   - Watch debug logs for unexpected behavior
   - Note any performance changes (should be faster)

5. **Create issues for follow-ups** (if any):
   - Document any new bugs discovered
   - Note potential optimizations (e.g., batch FileInfo requests)

---

## Testing Strategy

### Automated Tests (future work, not in this refactor)

**Unit Tests**:
- `determine_sync_state()` with various API response combinations
- `extract_item_name()` path parsing
- `translate_path()` path mapping

**Integration Tests**:
- Mock Syncthing API responses
- Verify state updates propagate to UI
- Test event handling (ItemStarted/ItemFinished/LocalIndexUpdated)

**Not in scope for this refactor** (focus on manual testing first, add automated tests later)

### Manual Testing Checklist

**Pre-Refactor** (capture baseline):
- [ ] Document current bugs (subdirectories stuck as Ignored)
- [ ] Capture debug logs for ignore → enter directory → back out → re-enter
- [ ] Screenshot states at each step

**During Refactor** (after each phase):
- [ ] Verify code compiles
- [ ] Run basic navigation test (folder list → enter directory → back out)
- [ ] Check debug logs for errors

**Post-Refactor** (comprehensive):

**1. Ignore/Unignore File**:
- [ ] Ignore file → verify Loading → Ignored (⚠️ or 🚫)
- [ ] Unignore file → verify Loading → Synced/RemoteOnly
- [ ] Ignore file, delete manually, rescan → verify 🚫 (deleted)
- [ ] Unignore file, check .stignore → pattern removed

**2. Ignore/Unignore Directory**:
- [ ] Ignore directory → verify Loading → Ignored
- [ ] Enter ignored directory → verify children show Ignored
- [ ] Unignore directory → verify Loading → Synced/RemoteOnly
- [ ] **CRITICAL**: Unignore directory → immediately enter → verify children NOT stuck as Ignored
- [ ] Unignore directory → wait for sync → enter → verify children show correct states

**3. Navigation During Sync**:
- [ ] Unignore large directory (100+ files)
- [ ] Immediately enter directory → verify shows Loading states
- [ ] Back out and re-enter → verify states update as sync progresses
- [ ] Navigate to different folders and back → verify states consistent

**4. Rapid Navigation**:
- [ ] Rapidly press Enter/Backspace through nested directories
- [ ] Verify no crashes or panics
- [ ] Verify states eventually settle correctly

**5. Loading State Prevention**:
- [ ] Ignore file → immediately press `i` again → verify prevented with toast
- [ ] Delete file → immediately press `d` again → verify prevented with toast
- [ ] Ignore file → immediately press `o` (open) → verify prevented with toast

**6. Cache Behavior**:
- [ ] Enter directory → back out → re-enter → verify instant display (cache hit)
- [ ] Modify file in another app → rescan → verify state updates
- [ ] Check logs: cache validated by sequence numbers

**7. Events**:
- [ ] Modify file via Syncthing web UI → verify ItemStarted → Loading → ItemFinished → Synced
- [ ] Delete file via filesystem → rescan → verify LocalIndexUpdated → FileInfo → state updates

**8. Edge Cases**:
- [ ] Network error during API call → verify error shown in status bar
- [ ] Ignore pattern with special characters (spaces, unicode) → verify works
- [ ] Very large directory (1000+ files) → verify no lag
- [ ] Ignore directory, delete directory, unignore → verify cleans up correctly

**9. Comparison with Old Behavior**:
- [ ] Run same tests as pre-refactor
- [ ] Compare debug logs (should be simpler, no rejection messages)
- [ ] Verify all original bugs fixed
- [ ] Verify no new bugs introduced

### Performance Testing

**Metrics to Track**:
- Time to enter directory (cache hit vs cache miss)
- Time for FileInfo API to respond (should be similar)
- Number of API requests per navigation (should be same or fewer)
- CPU usage during idle (should be ~1-2%)
- Memory usage (should be similar, maybe lower without tracking HashMaps)

**Performance Test Scenarios**:
1. **Large directory** (1000 files):
   - Enter directory → measure time to show cached states
   - Measure time until all states updated
   - Check logs for API request count

2. **Nested navigation** (5+ levels deep):
   - Navigate down → measure cumulative time
   - Check memory usage
   - Back out → verify breadcrumbs cleaned up

3. **Rapid navigation** (100 Enter/Backspace in 10 seconds):
   - Check for lag or UI freezing
   - Verify API request deduplication working
   - Check CPU usage

**Success Criteria**:
- All bugs from BEFORE_REFACTOR.md fixed
- No new crashes or panics
- Performance similar or better than before
- Code ~660 lines shorter
- Debug logs simpler and easier to read

---

## Summary: Before & After

### Before Refactoring

**Architecture**:
- 5 competing sources of truth
- 7 different update mechanisms
- State transition validation (rejecting API)
- State preservation (trusting cache over API)
- Ancestor checking (130 lines of band-aids)
- Tracking HashMaps (manually_set_states, syncing_files)

**Code Size**: ~4,200 lines in main.rs

**Bugs**:
- Subdirectories stuck as Ignored after parent unignored
- State flicker during navigation
- Race conditions between Browse and FileInfo handlers
- Stale states shown for up to 10 seconds

**Maintenance**: Difficult - changes require updating multiple systems

### After Refactoring

**Architecture**:
- 1 source of truth (Syncthing API)
- 2 update mechanisms (API response, cache invalidation)
- No validation (trust API)
- No preservation (cache is hint, not truth)
- No tracking HashMaps
- Explicit Loading states

**Code Size**: ~3,540 lines in main.rs (660 lines removed)

**Bugs**: Fixed
- Subdirectories immediately show correct state after parent unignored
- No state flicker (Loading state is explicit)
- No race conditions (API is authoritative)
- States update as soon as API responds

**Maintenance**: Easy - single update pathway, clear data flow

---

## Open Questions for User

1. **Loading State Icon**: Should we use 🔄 spinner, or dimmed version of regular icon, or something else?

2. **ItemStarted Handling**: Should we:
   - Option A: Use Loading state (🔄)
   - Option B: Add back Syncing state but populate from events only (no HashSet tracking)
   - Option C: Keep showing cached state until ItemFinished + FileInfo responds

3. **Error Handling**: If API call fails, should we:
   - Option A: Revert to previous state (undo optimistic update)
   - Option B: Show error icon/state
   - Option C: Keep Loading state with error indicator

4. **Migration Timeline**: Prefer to do this:
   - Option A: All at once (1-2 days of focused work)
   - Option B: Gradually over a week (phase by phase with testing in between)
   - Option C: Create parallel implementation first, then swap

5. **Rollback Plan**: If serious bugs discovered after merge:
   - Option A: Revert entire refactor
   - Option B: Fix forward (prefer this if possible)
   - Option C: Feature flag (keep old code path as fallback)

---

## Conclusion

This refactoring will **simplify the codebase by 660 lines** while **fixing persistent bugs** and **improving maintainability**. The core principle is simple: **Trust Syncthing's API as the single source of truth.**

By removing defensive programming (state preservation, validation, tracking HashMaps), we eliminate race conditions and circular dependencies. The new architecture is easier to understand, debug, and extend.

**Ready for user evaluation and feedback.**
