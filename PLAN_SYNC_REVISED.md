# Sync State Refactoring Plan - REVISED

**Status**: Proposal (awaiting user approval)
**Date**: 2025-10-30
**Goal**: Simplify sync state management by trusting API responses and using optimistic updates with revert capability

---

## Executive Summary

### The Core Problem

**Current system has 650+ lines of defensive code that fights Syncthing's API instead of trusting it.**

The code rejects API responses based on assumptions about what the state "should be" (from user actions 10 seconds ago), leading to:
- Subdirectories stuck showing Ignored after parent is unignored
- Stale states persisting until user navigates away and back
- Complex preservation logic with race conditions
- Duplicate code (root vs non-root Browse handlers)

### The Solution

**Trust Syncthing's API responses, use optimistic UI updates that can be reverted when conflicts occur.**

**Two Sources of Truth**:
1. **Syncthing API**: Authoritative for sync states (Synced, OutOfSync, LocalOnly, RemoteOnly, Ignored)
2. **Local Filesystem**: Authoritative for ignored file existence (⚠️ exists vs 🚫 deleted) - **our unique feature**

**Key Principle**: API response always wins. If external .stignore modification conflicts with user action, accept API result and notify user.

---

## Critical Realities (Corrected Understanding)

### Reality 1: .stignore Can Be Modified Externally

**NOT SAFE TO ASSUME**: Our `PUT /rest/db/ignores` request is authoritative

**REALITY**:
- User can edit `.stignore` in text editor while app is running
- Git can modify it during checkout/merge/pull
- Other Syncthing tools can modify it
- Syncthing itself may reorder/normalize patterns

**IMPLICATION**: We must use optimistic updates that can be reverted when API reports different state

### Reality 2: Syncthing Doesn't Track Ignored File Existence

**NOT AVAILABLE FROM API**: Whether ignored file exists on disk or was deleted

**REALITY**:
- Syncthing API returns `Ignored` state for pattern-matched files
- Syncthing does NOT distinguish between ignored+exists vs ignored+deleted
- This distinction (⚠️ vs 🚫) is **our unique feature**
- We must check filesystem directly via `fs::metadata()`

**IMPLICATION**: The `ignored_exists` HashMap must stay, but can be optimized (don't check children of non-existent ignored directories)

### Reality 3: No .stignore-Specific Events

**NOT AVAILABLE**: `ConfigSaved`, `FolderIgnoresUpdated`, or similar events

**REALITY** (confirmed via code research):
- Syncthing does NOT emit events when .stignore is modified
- When .stignore changes (via API or external edit), Syncthing rescans automatically
- We detect changes via `LocalIndexUpdated` events for affected files
- Current code manually triggers rescan after `PUT /rest/db/ignores`

**IMPLICATION**: Keep manual rescan trigger, rely on events for detection (no polling needed)

---

## Current Architecture Problems (Unchanged from Original Plan)

### Problem 1: Rejecting API Responses (Lines 870-905)

```rust
// WRONG: Reject API response based on user action from up to 10 seconds ago
if let Some(&manual_change) = self.manually_set_states.get(&state_key) {
    let is_valid_transition = match manual_change.action {
        ManualAction::SetIgnored => state == SyncState::Ignored,
        ManualAction::SetUnignored => state != SyncState::Ignored,
    };
    if !is_valid_transition {
        continue; // REJECT Syncthing's authoritative response!
    }
}
```

**Why This Causes Bugs**:
- User unignores parent directory
- User immediately enters directory
- Browse results arrive, preservation logic uses stale cached Ignored states for children
- FileInfo responses arrive with correct Not Ignored states
- **Validation rejects them** because parent was just unignored
- Children stuck showing Ignored until user navigates away and back

### Problem 2: State Preservation (Lines 1932-1945, 2124-2137)

```rust
// WRONG: Trust in-memory/cache states over fresh API data
if self.syncing_files.contains(&sync_key) {
    file_sync_states.insert(item.name.clone(), SyncState::Syncing);
    continue; // Don't request fresh data
}

if let Some(_manual_change) = self.manually_set_states.get(&sync_key) {
    // Preserve existing in-memory state
    if let Some(&existing_state) = existing_level.file_sync_states.get(&item.name) {
        file_sync_states.insert(item.name.clone(), existing_state);
        continue; // Don't request fresh data
    }
}

// Fall back to cached state (might be stale!)
if let Some(&cached_state) = cached_states.get(&item.name) {
    file_sync_states.insert(item.name.clone(), cached_state);
}
```

**Why This Causes Bugs**:
- Preservation logic runs when Browse results arrive
- Trusts stale cache/memory over FileInfo responses that will arrive shortly
- Creates race condition: if FileInfo arrives first, preservation overwrites it

### Problem 3: Ancestor Checking Band-Aids (Lines 2047-2122)

130+ lines of code trying to detect "was parent just unignored?" and clear stale states.

**Why This Doesn't Work**:
- Runs before preservation logic
- Preservation logic puts stale states back!
- Two pieces of code fighting each other

### Problem 4: Code Duplication

- Root Browse handler: 123 lines (1885-2008)
- Non-root Browse handler: 166 lines (2009-2175)
- Nearly identical except for ancestor checking in non-root

---

## Proposed Architecture (Updated)

### New Data Flow

```
USER IGNORES FILE
├─> Update UI optimistically (show Ignored ⚠️/🚫 immediately)
├─> Store optimistic action with 5-second TTL (for conflict detection)
├─> PUT /rest/db/ignores (add pattern)
├─> POST /rest/db/scan (trigger rescan)
└─> Wait for events...
     ├─> LocalIndexUpdated event fires (with affected filenames)
     ├─> FileInfo requests sent for affected files
     └─> FileInfo responses arrive
          ├─> If matches optimistic state: done (remove from TTL tracker)
          ├─> If differs: revert UI, show toast "External change detected"
          └─> Update ignored_exists (check filesystem for ⚠️ vs 🚫)

USER NAVIGATES INTO DIRECTORY
├─> Check cache for Browse result
├─> If cache valid (sequence matches):
│    ├─> Display cached items + states immediately
│    └─> Request fresh data in background (Medium priority)
└─> If cache stale/missing:
     ├─> Request Browse API (High priority)
     └─> Wait for response...
          ├─> Browse response: list of items (no states)
          ├─> Load cached states (might be stale, just for instant display)
          ├─> Request FileInfo for ALL items (Medium priority)
          └─> FileInfo responses: update states as they arrive

SYNCTHING EVENTS (Background Monitoring)
├─> ItemStarted: Set item to Syncing state
├─> ItemFinished: Request fresh FileInfo
└─> LocalIndexUpdated:
     ├─> Update folder sequence (invalidates all cached Browse results)
     ├─> Invalidate cached states for mentioned files
     └─> Request fresh FileInfo for visible files
```

### What Gets Removed (~600-650 lines)

1. **State Transition Validation** (lines 870-905):
   - Delete entire validation block
   - Accept all FileInfo responses without rejection

2. **manually_set_states HashMap** (22 references):
   - Delete HashMap and ManualStateChange struct
   - Replace with simple 5-second optimistic tracker (for conflict detection only)

3. **syncing_files HashSet** (17 references):
   - Delete HashSet
   - Use Syncing state from ItemStarted/ItemFinished events directly

4. **Ancestor Checking** (lines 2047-2122):
   - Delete all 130+ lines
   - Not needed when we trust API

5. **State Preservation Logic** (lines 1932-1945, 2124-2137):
   - Delete preservation checks
   - Just use cached states for instant display, replace with FileInfo responses

6. **Duplicate Browse Handlers**:
   - Consolidate root and non-root into single function
   - Remove special-case logic

### What Gets Kept (Corrected)

1. **Cache System** (SQLite):
   - Browse results cached with folder sequence validation
   - File states cached with file sequence validation
   - **Purpose**: Performance (instant display), not correctness

2. **ignored_exists HashMap** (our unique feature):
   - Stores whether ignored files exist on disk (⚠️) or were deleted (🚫)
   - Checked via `fs::metadata()` when state is Ignored
   - **Optimization**: If ignored directory doesn't exist, skip checking children

3. **Event Monitoring** (`/rest/events` long-polling):
   - ItemStarted, ItemFinished, LocalIndexUpdated events
   - Triggers cache invalidation and fresh API requests
   - **Detects all changes** including external .stignore edits

4. **Manual Rescan Trigger**:
   - After `PUT /rest/db/ignores`, call `POST /rest/db/scan`
   - Ensures Syncthing detects changes immediately
   - Generates LocalIndexUpdated events

5. **Prefetch System** (idle detection):
   - Request FileInfo for items above/below selection
   - 300ms idle threshold
   - **Simplified**: No checking manually_set_states, just request data

### What Gets Added

1. **Optimistic Update Tracker** (replaces manually_set_states):
   ```rust
   struct OptimisticUpdate {
       expected_state: SyncState,
       timestamp: Instant,
   }

   // TTL: 5 seconds (not 10)
   // Purpose: Conflict detection, not state protection
   optimistic_updates: HashMap<String, OptimisticUpdate>
   ```

2. **Conflict Detection** (in FileInfo handler):
   ```rust
   // Check if this differs from optimistic expectation
   if let Some(optimistic) = self.optimistic_updates.get(&state_key) {
       if optimistic.timestamp.elapsed() < Duration::from_secs(5) {
           if state != optimistic.expected_state {
               // Conflict detected! External change won.
               self.toast_message = Some((
                   format!("External change detected: {} state differs from action", item_name),
                   Instant::now(),
               ));
           }
       }
       // Always remove (accept API result regardless)
       self.optimistic_updates.remove(&state_key);
   }

   // Accept state from API (no rejection!)
   level.file_sync_states.insert(item_name.clone(), state);
   ```

3. **Smart Existence Checking**:
   ```rust
   fn check_ignored_existence(
       items: &[BrowseItem],
       sync_states: &HashMap<String, SyncState>,
       translated_base_path: &str,
       parent_exists: Option<bool>, // NEW parameter
   ) -> HashMap<String, bool> {
       let mut result = HashMap::new();

       for item in items {
           if sync_states.get(&item.name) == Some(&SyncState::Ignored) {
               // Optimization: If parent doesn't exist, children can't either
               if parent_exists == Some(false) {
                   result.insert(item.name.clone(), false);
                   continue;
               }

               // Check filesystem directly
               let path = format!("{}/{}", translated_base_path, item.name);
               let exists = std::fs::metadata(&path).is_ok();
               result.insert(item.name.clone(), exists);
           }
       }

       result
   }
   ```

4. **Simplified Browse Handler** (single version):
   ```rust
   fn handle_browse_result(&mut self, folder_id: String, prefix: Option<String>, items: Vec<BrowseItem>) {
       // Sort items
       let mut items = items;
       self.sort_items(&mut items);

       // Update cache
       self.cache_manager.cache_browse_result(&folder_id, prefix.as_deref(), &items);

       // Load cached states (for instant display, will be replaced)
       let cached_states = self.cache_manager.load_cached_file_states(&folder_id, prefix.as_deref());

       // Build initial state map from cache
       let mut file_sync_states = HashMap::new();
       for item in &items {
           if let Some(&cached_state) = cached_states.get(&item.name) {
               file_sync_states.insert(item.name.clone(), cached_state);
           }
       }

       // Check filesystem existence for ignored items
       let ignored_exists = self.check_ignored_existence(&items, &file_sync_states, prefix.as_deref(), None);

       // Create/update breadcrumb level
       let level = BreadcrumbLevel {
           folder_id: folder_id.clone(),
           folder_label: self.get_folder_label(&folder_id),
           prefix: prefix.clone(),
           items: items.clone(),
           file_sync_states,
           ignored_exists,
           state: ListState::default().with_selected(Some(0)),
           translated_base_path: self.translate_path(&folder_id, prefix.as_deref()),
       };

       // Update breadcrumb trail
       self.update_breadcrumb_level(level);

       // Request FileInfo for ALL items (no filtering, let API service deduplicate)
       for item in &items {
           let file_path = if let Some(ref pfx) = prefix {
               format!("{}{}", pfx, item.name)
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

5. **Simplified FileInfo Handler** (no validation):
   ```rust
   ApiResponse::FileInfoResult { folder_id, file_path, details } => {
       let Ok(details) = details else {
           log_debug(&format!("FileInfo error for {}:{}", folder_id, file_path));
           return;
       };

       // Determine state from API (standard logic, unchanged)
       let state = determine_sync_state(&details.global, &details.local);

       // Check for conflict with optimistic update
       let state_key = format!("{}:{}", folder_id, file_path);
       if let Some(optimistic) = self.optimistic_updates.get(&state_key) {
           if optimistic.timestamp.elapsed() < Duration::from_secs(5) {
               if state != optimistic.expected_state {
                   // External change detected
                   let item_name = file_path.split('/').last().unwrap_or(&file_path);
                   self.toast_message = Some((
                       format!("External change: {} is {:?}", item_name, state),
                       Instant::now(),
                   ));
               }
           }
           self.optimistic_updates.remove(&state_key);
       }

       // Update cache
       self.cache_manager.cache_file_state(&folder_id, &file_path, state);

       // Update ALL visible breadcrumb levels (unchanged)
       for level in &mut self.breadcrumb_trail {
           if level.folder_id != folder_id {
               continue;
           }

           let item_name = Self::extract_item_name(&file_path, level.prefix.as_deref());

           if level.file_sync_states.contains_key(&item_name) {
               level.file_sync_states.insert(item_name.clone(), state);

               // Update existence check if ignored
               if state == SyncState::Ignored {
                   let path = format!("{}/{}", level.translated_base_path, item_name);
                   let exists = std::fs::metadata(&path).is_ok();
                   level.ignored_exists.insert(item_name, exists);
               } else {
                   // Not ignored, remove from existence map
                   level.ignored_exists.remove(&item_name);
               }
           }
       }
   }
   ```

---

## Migration Strategy (Updated)

### Phase 1: Add Optimistic Update Tracker (1 hour)

**Goal**: Replace manually_set_states with simpler conflict detection

**Tasks**:

1. **Add new struct** (src/main.rs, near line 234):
   ```rust
   struct OptimisticUpdate {
       expected_state: SyncState,
       timestamp: std::time::Instant,
   }
   ```

2. **Replace manually_set_states declaration** (line 242):
   ```rust
   // OLD: manually_set_states: HashMap<String, ManualStateChange>,
   // NEW:
   optimistic_updates: HashMap<String, OptimisticUpdate>,
   ```

3. **Update App initialization** (around line 3500):
   ```rust
   // OLD: manually_set_states: HashMap::new(),
   // NEW:
   optimistic_updates: HashMap::new(),
   ```

4. **Test**: Verify app compiles (will have errors in code that references manually_set_states, that's expected)

**Commit**: `git commit -m "Add OptimisticUpdate tracker, prepare to remove manually_set_states"`

### Phase 2: Remove State Transition Validation (1 hour)

**Goal**: Accept all FileInfo responses without rejection

**Tasks**:

1. **Delete validation block** (lines 870-905):
   ```rust
   // DELETE ENTIRE BLOCK (35 lines)
   ```

2. **Add conflict detection** (insert where validation was):
   ```rust
   // Check for conflict with optimistic update
   let state_key = format!("{}:{}", folder_id, file_path);
   if let Some(optimistic) = self.optimistic_updates.get(&state_key) {
       if optimistic.timestamp.elapsed() < Duration::from_secs(5) {
           if state != optimistic.expected_state {
               let item_name = file_path.split('/').last().unwrap_or(&file_path);
               self.toast_message = Some((
                   format!("External change: {} is {:?}", item_name, state),
                   std::time::Instant::now(),
               ));
           }
       }
       self.optimistic_updates.remove(&state_key);
   }

   // Continue with normal state update (no rejection!)
   ```

3. **Test**:
   - Ignore file → immediately check state
   - Unignore file → immediately check state
   - States should update based on API responses

**Commit**: `git commit -m "Remove state transition validation, add conflict detection"`

### Phase 3: Simplify Browse Handler (2-3 hours)

**Goal**: Single handler, no preservation logic, no ancestor checking

**Tasks**:

1. **Create new function** `handle_browse_result()` (insert around line 1880):
   - Copy simplified version from "What Gets Added" section above
   - No preservation checks
   - No ancestor checking
   - Just: cache load → request FileInfo for all items

2. **Update root Browse response handler** (replace lines 1885-2008):
   ```rust
   ApiResponse::BrowseResult { folder_id, prefix, items } if prefix.is_none() => {
       match items {
           Ok(items) => self.handle_browse_result(folder_id, prefix, items),
           Err(e) => log_debug(&format!("Browse error: {}", e)),
       }
   }
   ```

3. **Update non-root Browse response handler** (replace lines 2009-2175):
   ```rust
   ApiResponse::BrowseResult { folder_id, prefix, items } => {
       match items {
           Ok(items) => self.handle_browse_result(folder_id, prefix, items),
           Err(e) => log_debug(&format!("Browse error: {}", e)),
       }
   }
   ```

4. **Delete duplicate code** (lines 2009-2175 can be removed entirely)

5. **Test**:
   - Navigate through directories
   - Check cached states appear instantly
   - Check FileInfo updates replace cached states
   - Verify no "preserving state" logs

**Commit**: `git commit -m "Simplify Browse handler, remove preservation and ancestor checking"`

### Phase 4: Remove syncing_files HashSet (1-2 hours)

**Goal**: Use Syncing state from events directly, no tracking

**Tasks**:

1. **Remove declaration** (line 244):
   ```rust
   // DELETE: syncing_files: HashSet<String>,
   ```

2. **Remove from initialization** (around line 3500):
   ```rust
   // DELETE: syncing_files: HashSet::new(),
   ```

3. **Update ItemStarted event** (lines 584-601):
   ```rust
   // OLD: self.syncing_files.insert(sync_key.clone());
   // NEW: Just set state to Syncing directly

   for level in &mut self.breadcrumb_trail {
       if level.folder_id == folder_id {
           let item_name = Self::extract_item_name(&item_path, level.prefix.as_deref());
           if level.file_sync_states.contains_key(&item_name) {
               level.file_sync_states.insert(item_name, SyncState::Syncing);
           }
       }
   }

   // Invalidate cache (unchanged)
   self.cache_manager.invalidate_file_state(&folder_id, &item_path);
   ```

4. **Update ItemFinished event** (lines 603-630):
   ```rust
   // OLD: self.syncing_files.remove(&sync_key);
   // NEW: Just request fresh FileInfo (already does this)

   // Delete the HashSet removal line, keep the rest
   ```

5. **Remove from render.rs** (lines 88-92):
   ```rust
   // DELETE: Override check for syncing_files
   // FileInfo responses will naturally set Syncing state
   ```

6. **Test**:
   - Trigger sync (unignore large directory)
   - Verify Syncing icon appears during ItemStarted → ItemFinished
   - Verify state updates to Synced/etc after ItemFinished

**Commit**: `git commit -m "Remove syncing_files tracking, use event-driven Syncing state"`

### Phase 5: Update User Action Handlers (1-2 hours)

**Goal**: Use optimistic updates, accept API results

**Tasks**:

1. **Update toggle_ignore()** (lines 3089-3190):
   ```rust
   // After successful PUT /rest/db/ignores:

   // Add optimistic update
   let state_key = format!("{}:{}", folder_id, item_path);
   self.optimistic_updates.insert(state_key, OptimisticUpdate {
       expected_state: if was_ignored {
           SyncState::Synced  // Expect un-ignored → synced
       } else {
           SyncState::Ignored  // Expect ignored
       },
       timestamp: std::time::Instant::now(),
   });

   // Update UI optimistically
   if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
       let new_state = if was_ignored {
           // Don't set state (will come from API)
           level.file_sync_states.remove(&item_name);
       } else {
           // Set to Ignored immediately, check filesystem
           let path = format!("{}/{}", level.translated_base_path, item_name);
           let exists = std::fs::metadata(&path).is_ok();
           level.ignored_exists.insert(item_name.clone(), exists);
           level.file_sync_states.insert(item_name.clone(), SyncState::Ignored);
       };
   }

   // Trigger rescan (unchanged)
   // Show toast (unchanged)
   ```

2. **Update ignore_and_delete()** (lines 3263-3345):
   ```rust
   // Similar optimistic update, but set exists=false
   self.optimistic_updates.insert(state_key, OptimisticUpdate {
       expected_state: SyncState::Ignored,
       timestamp: std::time::Instant::now(),
   });

   // Update UI: Ignored with exists=false (🚫)
   if let Some(level) = self.breadcrumb_trail.get_mut(level_idx) {
       level.file_sync_states.insert(item_name.clone(), SyncState::Ignored);
       level.ignored_exists.insert(item_name.clone(), false);
   }

   // Delete file (unchanged)
   // Trigger rescan (unchanged)
   ```

3. **Update delete_file()** (lines 2470-2561):
   ```rust
   // No optimistic update needed (file will disappear from Browse API)
   // Just trigger rescan (already does this)
   ```

4. **Test**:
   - Ignore file → should show Ignored immediately (⚠️ or 🚫)
   - Unignore file → should clear state, then update when FileInfo arrives
   - Check no rejection logs
   - Check states update within ~1 second

**Commit**: `git commit -m "Update user actions to use optimistic updates"`

### Phase 6: Smart Existence Checking (1-2 hours)

**Goal**: Optimize filesystem checks (skip children of non-existent ignored directories)

**Tasks**:

1. **Update check_ignored_existence()** (around line 1814):
   ```rust
   fn check_ignored_existence(
       &self,
       items: &[BrowseItem],
       sync_states: &HashMap<String, SyncState>,
       prefix: Option<&str>,
       parent_exists: Option<bool>,  // NEW parameter
   ) -> HashMap<String, bool> {
       let mut result = HashMap::new();

       let base_path = self.translate_path(&folder_id, prefix);

       for item in items {
           if let Some(&SyncState::Ignored) = sync_states.get(&item.name) {
               // Optimization: If parent doesn't exist, children can't either
               if parent_exists == Some(false) {
                   result.insert(item.name.clone(), false);
                   continue;
               }

               // Check filesystem
               let path = format!("{}/{}", base_path, item.name);
               let exists = std::fs::metadata(&path).is_ok();
               result.insert(item.name.clone(), exists);
           }
       }

       result
   }
   ```

2. **Update handle_browse_result()** to pass parent_exists:
   ```rust
   // Determine if parent exists (if we're in a subdirectory)
   let parent_exists = if prefix.is_some() {
       // Check if current directory exists
       let dir_path = self.translate_path(&folder_id, prefix.as_deref());
       Some(std::fs::metadata(&dir_path).is_ok())
   } else {
       None  // Root directory, no parent to check
   };

   let ignored_exists = self.check_ignored_existence(
       &items,
       &file_sync_states,
       prefix.as_deref(),
       parent_exists,
   );
   ```

3. **Test**:
   - Ignore directory that doesn't exist
   - Enter that directory
   - Verify children all show 🚫 (not individually checked)
   - Check debug logs for reduced filesystem calls

**Commit**: `git commit -m "Add smart existence checking (skip children of non-existent dirs)"`

### Phase 7: Cleanup & Testing (2-3 hours)

**Goal**: Remove dead code, comprehensive testing

**Tasks**:

1. **Remove unused code**:
   - Search for remaining `manually_set_states` references (should be none)
   - Search for remaining `syncing_files` references (should be none)
   - Delete `ManualStateChange` struct and `ManualAction` enum
   - Remove unused imports

2. **Run formatter and linter**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```

3. **Manual testing** (comprehensive checklist in original plan, key tests):
   - Ignore file → verify shows Ignored immediately → verify persists after API
   - Unignore file → verify clears → verify updates to Synced/RemoteOnly
   - **Critical**: Unignore directory → immediately enter → verify children NOT stuck as Ignored
   - External edit .stignore → verify detected via LocalIndexUpdated event
   - Rapid navigation during sync → verify no crashes

4. **Compare with before**:
   - Check line count: `wc -l src/main.rs` (should be ~600 lines shorter)
   - Check logs: simpler, no rejection messages
   - Verify all original bugs fixed

**Commit**: `git commit -m "Cleanup: remove dead code, final testing complete"`

### Phase 8: Update Documentation (1 hour)

**Goal**: Document new simplified architecture

**Tasks**:

1. **Update SYNC.md**:
   - Remove references to manually_set_states, syncing_files
   - Document optimistic updates with 5-second TTL
   - Document conflict detection and toast notifications
   - Document smart existence checking

2. **Update CLAUDE.md**:
   - Simplify architecture section
   - Update "Known Limitations" (remove state flicker issues)
   - Add note about optimistic UI updates

3. **Create REFACTOR_NOTES.md**:
   - Document what was removed and why
   - Document new behaviors (optimistic updates, conflict detection)
   - Note breaking changes (if any)

**Commit**: `git commit -m "Update documentation for simplified architecture"`

---

## Testing Strategy (Updated)

### Critical Test Cases

**Test 1: Subdirectories Stuck as Ignored (Primary Bug)**
```
1. Navigate to directory with ignored subdirectory
2. Unignore subdirectory
3. Immediately press Enter to enter directory
4. Expected: Subdirectories NOT shown as Ignored
5. Current Bug: Subdirectories stuck as Ignored until back out and re-enter
6. New Behavior: Subdirectories show cached Ignored briefly, then update to correct state within ~1s
```

**Test 2: External .stignore Modification**
```
1. Open .stignore in text editor
2. In app, ignore file "test.txt"
3. In text editor, remove "test.txt" pattern and save
4. Expected: LocalIndexUpdated event fires, FileInfo returns Not Ignored, conflict toast shown
5. Verify: File shows correct state (Not Ignored) within ~1s
```

**Test 3: Optimistic Update Conflict**
```
1. Ignore file
2. Simultaneously (within 5 seconds), external tool unignores it
3. Expected: File shows Ignored immediately, then reverts to Not Ignored with toast
4. Verify: Toast says "External change: [filename] is Synced" (or similar)
```

**Test 4: Smart Existence Checking**
```
1. Ignore directory that doesn't exist on disk
2. Enter that directory
3. Expected: All children show 🚫 (deleted), no filesystem checks per child
4. Verify: Debug logs show single existence check (parent), not N checks (children)
```

**Test 5: Rapid Navigation During Sync**
```
1. Unignore large directory (100+ files)
2. Rapidly press Enter/Backspace through nested directories
3. Expected: No crashes, states eventually settle correctly
4. Verify: Syncing states appear briefly, then update to final states
```

### Performance Benchmarks

**Before Refactor**:
- Lines in main.rs: ~4,200
- Time to enter directory (cache hit): <10ms
- Time for states to update: 500ms-2s (with rejections causing delays)
- Bug: Subdirectories stuck until re-navigate

**After Refactor** (targets):
- Lines in main.rs: ~3,600 (600 fewer)
- Time to enter directory (cache hit): <10ms (unchanged)
- Time for states to update: 500ms-1s (no rejections)
- Bug: Fixed (subdirectories update immediately)

---

## Summary: Key Behavioral Changes

### Before Refactoring

**User ignores file**:
1. UI shows Ignored immediately
2. Adds to manually_set_states (10s timeout)
3. FileInfo responses **rejected if they differ** from expected state
4. If external .stignore edit conflicts, **stays wrong for 10 seconds**

**User enters directory**:
1. Browse results arrive with item list
2. **Preservation logic** checks syncing_files, manually_set_states, cache
3. Uses stale states for items "protected" by tracking HashMaps
4. FileInfo responses **rejected if conflicts** with preserved states
5. Bug: Subdirectories stuck as Ignored after parent unignored

### After Refactoring

**User ignores file**:
1. UI shows Ignored immediately (optimistic)
2. Adds to optimistic_updates (5s timeout)
3. FileInfo responses **always accepted**
4. If external .stignore edit conflicts, **reverts within 1 second + shows toast**

**User enters directory**:
1. Browse results arrive with item list
2. Loads cached states (no preservation, just for display)
3. Requests FileInfo for ALL items
4. FileInfo responses **always accepted**, update UI immediately
5. Fix: Subdirectories update to correct state within ~1 second

### Conflict Handling Example

**Scenario**: User ignores "test.txt", simultaneously someone edits .stignore externally to remove pattern

**Timeline**:
```
T+0ms:    User presses 'i' on test.txt
T+10ms:   UI shows Ignored ⚠️ (optimistic)
T+20ms:   PUT /rest/db/ignores sent (adds pattern)
T+50ms:   External edit removes pattern
T+100ms:  POST /rest/db/scan sent
T+500ms:  LocalIndexUpdated event arrives
T+600ms:  FileInfo requested
T+800ms:  FileInfo returns: NotIgnored (external edit won)
T+810ms:  optimistic_updates checked: conflict detected!
T+820ms:  UI reverts to NotIgnored, toast shown: "External change: test.txt is Synced"
T+821ms:  User sees correct state + understands why action didn't stick
```

**Old Behavior** (for comparison):
```
T+800ms:  FileInfo returns: NotIgnored
T+810ms:  Validation checks manually_set_states: SetIgnored action found
T+820ms:  Response REJECTED because doesn't match SetIgnored
T+821ms:  UI stays showing Ignored (WRONG!)
...
T+10000ms: Timeout expires, next FileInfo accepted
T+10100ms: UI updates to NotIgnored (10 seconds late!)
```

---

## Open Questions (For User Approval)

### 1. Optimistic Update TTL

**Question**: Should optimistic updates expire after 5 seconds or longer?

**Consideration**:
- 5 seconds: Faster conflict detection, less time showing wrong state
- 10 seconds: More forgiving if network is slow
- Current code uses 10 seconds for manually_set_states

**Recommendation**: Start with 5 seconds, adjust if network issues occur

### 2. Conflict Toast Duration

**Question**: How long should conflict notification toast display?

**Options**:
- 3 seconds (current toast duration)
- 5 seconds (longer for important notifications)
- Until dismissed (requires new UI interaction)

**Recommendation**: 5 seconds (user needs time to read and understand)

### 3. Existence Check Caching

**Question**: Current plan says "always check filesystem" but should we cache for same render frame?

**Scenario**: If rendering same breadcrumb level 60 times per second (scrolling), do we check filesystem 60 times?

**Recommendation**: Cache within single render frame, invalidate on next event loop iteration

### 4. Migration Timeline

**Question**: Prefer to do this all at once or phase by phase?

**Options**:
- All at once: 1-2 days focused work, merge when complete
- Phase by phase: 1 week, merge after each phase, test in production
- Parallel implementation: Build new system alongside old, swap atomically

**Recommendation**: Phase by phase (safer, easier to bisect if issues found)

### 5. Rollback Plan

**Question**: If serious bugs discovered after merge, should we:

**Options**:
- Revert entire refactor (safest)
- Fix forward (preferred if bugs are minor)
- Feature flag (keep both code paths, toggle via config)

**Recommendation**: Fix forward for minor issues, revert if major architectural problems

---

## Conclusion

This refactoring will:
- **Remove ~600 lines** of defensive code
- **Fix persistent bugs** (subdirectories stuck as Ignored)
- **Improve maintainability** (single Browse handler, clear data flow)
- **Add useful features** (conflict detection with user notifications)
- **Preserve unique features** (⚠️ vs 🚫 ignored file existence)

**Core principle**: Trust Syncthing's API, use optimistic updates that can be reverted when external changes conflict.

**Ready for user approval and execution.**
