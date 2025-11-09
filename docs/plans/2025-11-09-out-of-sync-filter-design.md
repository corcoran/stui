# Out-of-Sync Filter Feature Design

**Date:** 2025-11-09
**Status:** Approved, Ready for Implementation

## Overview

Add `f` key for out-of-sync filtering with two independent context-aware modes:
1. **Folder view** (`focus_level == 0`): Summary modal showing state breakdown per folder (read-only)
2. **Breadcrumb view** (`focus_level > 0`): Hierarchical filter showing out-of-sync files (like search)

Both modes update in real-time via event-driven cache invalidation.

## User Experience

### Mode 1: Folder Summary View

**Trigger:** Press `f` in folder list

**Display:**
```
┌─ Out-of-Sync Summary ─────────────────────────────────────┐
│                                                            │
│ 📂 Movies                                                  │
│    🔄 Downloading: 3  ⏳ Queued: 12  ☁️ Remote: 5  ⚠️ Modified: 2│
│                                                            │
│ 📂 Pictures                                                │
│    💻 Local Changes: 8                                     │
│                                                            │
│ 📂 Music                                                   │
│    🔄 Downloading: 5  ⏳ Queued: 89  💻 Local: 12          │
│    ☁️ Remote: 45  ⚠️ Modified: 8                           │
│                                                            │
│ 📂 Documents                                               │
│    ✅ All synced                                           │
│                                                            │
│ Esc to close                                               │
└────────────────────────────────────────────────────────────┘
```

**Actions:**
- `↑↓` - Navigate folders (visual selection)
- `Esc` or `f` - Close modal (no effect on breadcrumb navigation)

**Behavior:**
- Read-only modal overlay
- Updates in real-time as sync events occur
- Progressive loading: shows "Loading..." for folders still fetching data

### Mode 2: Hierarchical Filter

**Trigger:** Press `f` in breadcrumb view

**Display:**
```
Current: Movies/               Filter: Out-of-Sync (18 items)

📁 Action/                     ← Parent dir (shown because children match)
├─ 📄🔄 BigMovie.mkv           ← Downloading
├─ 📄⏳ NewRelease.mp4         ← Queued
├─ 📄☁️ OldMovie.mkv           ← Remote only

📁 Documentary/
└─ Subdir/
   └─ 📄☁️ nature.mp4          ← Remote only

📁 TV Shows/
├─ 📄💻 edited.mkv             ← Local change
└─ 📄⚠️  modified.mp4          ← Out of sync
```

**Actions:**
- `↑↓` - Navigate filtered results
- `Enter` on directory - Drill into subdirectory (filter persists)
- `Enter` on file - Clear filter, jump to exact file location
- Standard keys work: `?`, `d`, `i`, `c`
- `Esc` or `f` - Clear filter

**Behavior:**
- Works like current search feature (hierarchical display)
- Shows parent directories if children match
- Filter persists when navigating down
- Context-aware clearing when backing out past origin
- Status bar shows: "Filter: Out-of-Sync (N items)"

## Architecture

### Implementation Strategy

**Approach:** API Foundation First + Lazy Pre-check

1. Build API layer (`/rest/db/need` endpoint and types)
2. Integrate with async service
3. Extend existing cache (`sync_state_cache` table)
4. Add state management
5. Build UI features (summary modal, then breadcrumb filter)

**Caching Strategy:** Lazy + Pre-check
- Only call `/rest/db/need` when user requests it (presses `f`)
- Check `/rest/db/status` first - skip folders with `needTotalItems == 0`
- Cache results for 30 seconds
- Event-driven invalidation on ItemStarted/ItemFinished

### API Layer

**New Type in `src/api.rs`:**
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct NeedResponse {
    pub progress: Vec<FileInfo>,  // Currently downloading
    pub queued: Vec<FileInfo>,     // Queued for download
    pub rest: Vec<FileInfo>,       // Other files needed
    pub page: u32,
    pub perpage: u32,
}
```

**New Method in `SyncthingClient`:**
```rust
pub async fn get_needed_files(
    &self,
    folder_id: &str,
    page: Option<u32>,
    perpage: Option<u32>
) -> Result<NeedResponse>
```

Follows pattern of existing methods like `get_local_changed_files()`.

### Service Integration

**Add to `src/services/api.rs`:**

```rust
// ApiRequest enum
GetNeededFiles {
    folder_id: String,
    page: Option<u32>,
    perpage: Option<u32>,
}

// ApiResponse enum
NeededFiles {
    folder_id: String,
    response: NeedResponse,
}
```

Handler in service processes request via existing async infrastructure.

### Cache Layer

**Extend existing `sync_state_cache` table:**

```sql
ALTER TABLE sync_state_cache ADD COLUMN need_category TEXT;
ALTER TABLE sync_state_cache ADD COLUMN need_cached_at INTEGER;
```

**Categories:** `"downloading"`, `"queued"`, `"remote_only"`, `"modified"`, `"local_only"`

**New Methods in `src/cache.rs`:**
- `cache_needed_files()` - Enrich sync_state_cache with need_category
- `cache_local_changed()` - Mark files as "local_only"
- `get_out_of_sync_items()` - Query by category with TTL check
- `get_folder_sync_breakdown()` - Get counts by category
- `invalidate_out_of_sync_categories()` - Clear on events

**Invalidation Strategy:**
- Existing sequence-based invalidation for sync states (unchanged)
- Need categories have 30s TTL (check `need_cached_at`)
- Event-driven: clear categories on ItemStarted/ItemFinished

### State Management

**New Types in `src/model/types.rs`:**

```rust
pub struct OutOfSyncFilterState {
    pub origin_level: usize,      // For context-aware clearing
    pub last_refresh: SystemTime,
}

pub struct OutOfSyncSummaryState {
    pub selected_index: usize,
    pub breakdowns: HashMap<String, FolderSyncBreakdown>,
    pub loading: HashSet<String>,
}

pub struct FolderSyncBreakdown {
    pub downloading: usize,
    pub queued: usize,
    pub remote_only: usize,
    pub modified: usize,
    pub local_only: usize,
}
```

**Add to `src/model/ui.rs`:**
```rust
pub out_of_sync_filter: Option<OutOfSyncFilterState>,
pub out_of_sync_summary: Option<OutOfSyncSummaryState>,
```

### Orchestration

**In `src/main.rs`:**

```rust
fn open_out_of_sync_summary(&mut self) {
    // 1. Check /rest/db/status for each folder (already cached)
    // 2. Queue GetNeededFiles for folders with needTotalItems > 0
    // 3. Use existing local changed data for receive-only folders
    // 4. Set model.ui.out_of_sync_summary
    // 5. Update breakdowns as ApiResponse::NeededFiles arrives
}

fn toggle_out_of_sync_filter(&mut self) {
    // 1. Check /rest/db/status for current folder
    // 2. If nothing out-of-sync, show toast "All files synced!"
    // 3. Queue GetNeededFiles request (if not cached)
    // 4. Set model.ui.out_of_sync_filter
    // 5. Breadcrumb queries cache for filtered items
}
```

## State Categorization Logic

**From `/rest/db/need`:**
- `progress` array → **Downloading** (🔄)
- `queued` array → **Queued** (⏳)
- `rest` array:
  - Local doesn't exist → **Remote Only** (☁️)
  - Local exists, different version → **Modified** (⚠️)

**From `/rest/db/localchanged`:**
- All files → **Local Changes** (💻) (receive-only folders only)

## Icons

**Reuse existing + new "Queued":**
- 🔄 **Downloading**: `SyncState::Syncing`
- ⏳ **Queued**: New icon (emoji: ⏳, nerdfont: \u{F253})
- ☁️ **Remote Only**: `SyncState::RemoteOnly`
- ⚠️ **Modified**: `SyncState::OutOfSync`
- 💻 **Local Changes**: `SyncState::LocalOnly`

## Real-Time Updates

**Event-driven invalidation:**
1. `ItemStarted` → Move file from queued → downloading, update UI
2. `ItemFinished` → Remove from cache (now synced), update counts
3. `LocalIndexUpdated` → Refresh local changes
4. Folder status polling → Update summary counts (existing)

Both summary and filter update automatically when events occur.

## Performance Considerations

**Summary Modal:**
- First render: `/rest/db/status` (instant, already cached)
- Background: `/rest/db/need` for folders with items (parallel, non-blocking)
- Progressive updates as responses arrive
- 30s cache TTL

**Breadcrumb Filter:**
- Pre-check: `/rest/db/status` (fast fail if synced)
- Single `/rest/db/need` call per folder (expensive but cached)
- Event-driven invalidation (no polling)
- Reuse hierarchical query from search

**API Call Optimization:**
- `/rest/db/need` is expensive (high CPU/RAM)
- Only called when user requests (lazy)
- Pre-check with `/rest/db/status` skips synced folders
- 30s cache prevents redundant calls
- Event invalidation for real-time accuracy

## Implementation Checklist

### Phase 1: Foundation
- [ ] Add `NeedResponse` type to `src/api.rs`
- [ ] Add `get_needed_files()` method to `SyncthingClient`
- [ ] Add `GetNeededFiles` / `NeededFiles` to service enums
- [ ] Extend `sync_state_cache` table with need_category fields
- [ ] Add cache methods for out-of-sync data
- [ ] Write tests for API, service, cache

### Phase 2: State & Icons
- [ ] Add state types (`OutOfSyncFilterState`, `OutOfSyncSummaryState`, `FolderSyncBreakdown`)
- [ ] Add fields to `model.ui`
- [ ] Add "Queued" icon to `IconRenderer`
- [ ] Write tests for state management

### Phase 3: Summary Modal
- [ ] Create `src/ui/out_of_sync_summary.rs`
- [ ] Implement `render_out_of_sync_summary()`
- [ ] Add `open_out_of_sync_summary()` orchestration
- [ ] Add keyboard handler (`f` in folder view)
- [ ] Update legend
- [ ] Write UI tests

### Phase 4: Breadcrumb Filter
- [ ] Add `categorize_out_of_sync_state()` pure function
- [ ] Extend breadcrumb rendering for filter mode
- [ ] Add `toggle_out_of_sync_filter()` orchestration
- [ ] Add keyboard handler (`f` in breadcrumb view)
- [ ] Update status bar for filter display
- [ ] Write integration tests

### Phase 5: Events & Polish
- [ ] Add event handlers (ItemStarted/ItemFinished)
- [ ] Implement cache invalidation
- [ ] Add ApiResponse::NeededFiles handler
- [ ] Manual testing with real Syncthing
- [ ] Performance testing (100+ out-of-sync files)
- [ ] Documentation updates

## Open Questions

- **Mutual exclusivity with search**: Should out-of-sync filter and search be mutually exclusive? **Recommendation: Yes** (simpler UX)
- **Summary refresh**: Event-driven only, or also poll while open? **Recommendation: Event-driven only** (reuse existing folder status polling)

## Success Criteria

- ✅ Summary modal opens instantly using cached `/rest/db/status`
- ✅ Detailed breakdowns load progressively without blocking UI
- ✅ Breadcrumb filter shows hierarchical results (parent dirs when children match)
- ✅ Real-time updates when files start/finish syncing
- ✅ No redundant expensive API calls (30s cache + pre-check)
- ✅ All tests pass (unit + integration)
- ✅ Zero performance regression on existing features
