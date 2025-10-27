# Syncthing CLI TUI Manager — Development Plan

This document outlines the step-by-step plan for building a **Rust Ratatui CLI tool** that interfaces with **Syncthing's REST API** to manage folders, view sync states, and control ignored/deleted files. The steps are structured to allow **progressive, testable milestones**, ideal for both human and LLM collaboration.

---

## 📍 Current Status (Updated 2025-01-26)

**Where we are:**
- ✅ **Phase 1 complete** - Basic prototype with folder/directory listing, recursive browsing, caching, and directory prioritization
- ✅ **Phase 1.5 complete** - Major async refactor eliminating all blocking API calls in background operations
- ✅ **Phase 2 mostly complete** - Rescan, ignore toggling, ignore+delete, file deletion, and event-based cache invalidation working
- ✅ **Phase 3 partially complete** - Breadcrumb navigation, status bar, and hotkey legend implemented
- ✅ **Phase 4 complete** - Event listener with live updates fully functional
- 🚧 **Currently ready for Phase 1.6** - Feature additions (filtering, error handling) and testing

**Recent Accomplishments (Session 2025-01-26):**

**Event-Based Cache Invalidation:**
- ✅ Event listener tracks `LocalIndexUpdated`, `ItemFinished`, `LocalChangeDetected`, `RemoteChangeDetected`
- ✅ Granular cache invalidation: file-level, directory-level, and folder-level
- ✅ Immediate UI refresh when viewing affected directories
- ✅ Last update timestamp displayed per folder with file path
- ✅ Event ID persistence across app restarts

**UI Improvements:**
- ✅ Folder statistics fixed (4th column showing in_sync/total items correctly)
- ✅ Hotkey legend spanning across breadcrumb panels (excludes Folders panel)
- ✅ Legend positioned at bottom, left-aligned against Folders panel edge
- ✅ Comprehensive hotkey display: `↑/↓`, `Enter`, `←`, `i`, `I`, `d`, `r`, `R`, `q`

**New Features:**
- ✅ File deletion with `d` key (with confirmation dialog) - works for any file, not just ignored
- ✅ `I` key retained for ignore + delete workflow
- ✅ Last update line color fixed (RGB 150,150,150) - visible on both highlighted and normal backgrounds

**Next steps:**
1. Build comprehensive test suite to prevent regressions
2. Add filtering functionality (Phase 1.6)
3. Improve error handling and display
4. Performance testing with large-scale datasets

---

## Phase 1: Basic Prototype — Folder and Directory Listing

### Objective
Create a minimal working prototype that queries Syncthing's REST API and lists folders and their contents in a simple Ratatui UI.

### Steps

1. **✅ COMPLETED: Setup Project**
   - Project initialized with all required dependencies
   - Working Cargo.toml with ratatui, crossterm, reqwest, serde, tokio

2. **✅ COMPLETED: Implement Config Loader**
   - config.yaml with API key, base_url, and path_map support
   - YAML deserialization working

3. **✅ COMPLETED: Query Folders via Syncthing API**
   - Successfully fetching `/rest/system/config`
   - Folder IDs and labels parsed

4. **✅ COMPLETED: Render Folder List (TUI)**
   - Scrollable folder list with state icons (✅, ⚠️, ⏸)
   - Keyboard navigation (↑ ↓, `q` to quit, `Enter` to drill in)

5. **✅ COMPLETED: Query Folder Contents (with Recursion)**
   - `/rest/db/browse` integration working
   - **Full recursive directory traversal implemented**
   - **Directories prioritized in display order**
   - Icons rendering correctly (📁, 💻, ☁️, ⚠️)

6. **✅ COMPLETED: Persistent Cache**
   - SQLite-based caching implemented
   - Current folder + 1 level deeper prefetching
   - Timestamp-based invalidation logic
   - Cache survives app restarts

---

## Phase 1.5: Non-Blocking Architecture Refactor ⚡ ✅ COMPLETED

### Objective
**Critical Performance Fix:** Refactor all API calls to be truly non-blocking, eliminating UI freezes when navigating directories with many files.

### Problem Statement (Original)

**Issues Fixed:**
- ✅ Navigating directories no longer blocks the UI thread
- ✅ Folders with thousands of files don't freeze the interface
- ✅ Cache-first rendering eliminates multi-second delays
- ✅ Rapid navigation (holding DOWN arrow) is smooth even during heavy cache building

**Root Causes (Addressed):**
1. ✅ Synchronous API call pattern replaced with channel-based async architecture
2. ✅ Cache-first rendering: cached data shown immediately, updates streamed
3. ✅ Sequence-based cache invalidation: only invalidates when data actually changes
4. ✅ Request prioritization: High (selected) > Medium (visible) > Low (prefetch)

### Implementation Completed

**Architecture Pattern:**

```rust
// Before (blocking):
match self.client.get_file_info(&folder_id, &path).await {
    Ok(details) => { /* process */ }
}

// After (non-blocking):
let _ = self.api_tx.send(ApiRequest::GetFileInfo {
    folder_id: folder_id.clone(),
    file_path: path.clone(),
    priority: Priority::Medium,
});
// Response handled asynchronously in handle_api_response()
```

**Key Components:**

1. **✅ Async API Service Layer** (`src/api_service.rs:192-456`)
   - Background worker task processing requests via channel
   - Priority queue: High → Medium → Low
   - Concurrent request limiting (max 10 in-flight)
   - Request deduplication to prevent duplicate API calls
   - Completion tracking to clean up in-flight state

2. **✅ Smart Cache Invalidation** (`src/cache.rs:160-168`, `src/main.rs:2245-2260`)
   - Sequence-based validation: `is_folder_status_valid(folder_id, current_sequence)`
   - Only invalidates when Syncthing reports actual changes
   - Browse cache validated per-directory with folder sequence
   - Sync state cache validated per-file with file sequence

3. **✅ Non-Blocking Background Operations** (all in `src/main.rs`)
   - `fetch_directory_states` (lines 721-787): Prefetch states for visible directories - **Made non-blocking**
   - `fetch_selected_item_sync_state` (lines 789-831): High-priority fetch for selected item - **Made non-blocking**
   - `discover_subdirectories_recursive` (lines 647-716): Recursive cache building - **Made non-blocking**
   - `prefetch_hovered_subdirectories` (lines 600-644): Speculative prefetching - **Made non-blocking**
   - `batch_fetch_visible_sync_states` (lines 833-880): Already async, batch file info fetching
   - Periodic folder status polling (lines 2141-2177): Already async background loop

4. **✅ Response Handling** (`src/main.rs:1982-2129`)
   - `handle_api_response()`: Central async response handler
   - Updates cache as responses arrive
   - Removes from `loading_sync_states` tracking
   - Progressive UI updates without blocking

5. **✅ Priority System** (`src/api_service.rs:26-32`)
   - **High**: User-initiated actions (navigation, toggle ignore, selected item)
   - **Medium**: Visible items (current directory contents)
   - **Low**: Prefetching, background updates, speculative loading

### Blocking vs Non-Blocking Operations

**Intentionally Blocking Operations** (for clear user feedback):
- Initial app load (loading folder list and statuses)
- Navigation actions (`load_root_level`, `enter_directory`)
- User actions (toggle ignore, delete, revert, rescan)

**Non-Blocking Background Operations** (all completed):
- ✅ Periodic folder status polling
- ✅ Visible file sync state fetching (batch operations)
- ✅ Directory state prefetching (cache building)
- ✅ Selected item sync state fetching (high-priority)
- ✅ Recursive subdirectory discovery
- ✅ Hovered subdirectory prefetching (speculative)

### Testing Status

**Manual Testing Completed:**
- ✅ Smooth scrolling verified on large directories (no stutter while holding DOWN)
- ✅ Cache building happens in background without UI freeze
- ✅ Navigation actions complete instantly with cached data
- ✅ All existing features (ignore, delete, rescan) work correctly
- ✅ Icons render correctly with progressive state updates

**Remaining Testing (Phase 1.6):**
- [ ] **Unit tests** for cache invalidation logic
- [ ] **Integration tests** with mock Syncthing API
- [ ] **Performance tests** with 10k+ file directories
- [ ] **Regression tests** to ensure no behavior changes
- [ ] Benchmark directory navigation speed (target: <50ms)
- [ ] Test with real Syncthing instance (100k+ files)
- [ ] Measure memory usage under heavy caching
- [ ] Profile with `cargo flamegraph` to find bottlenecks

### Performance Impact

**Before Phase 1.5:**
- Holding DOWN arrow caused stuttering during cache building
- UI froze waiting for API responses
- Large directories felt sluggish

**After Phase 1.5:**
- Smooth scrolling even during heavy cache building
- UI always responsive, shows cached data immediately
- Background operations don't impact user interactions

### Files Modified

- `src/main.rs`: Converted 4 functions from blocking to non-blocking
  - Lines 721-787: `fetch_directory_states`
  - Lines 789-831: `fetch_selected_item_sync_state`
  - Lines 647-716: `discover_subdirectories_recursive`
  - Lines 600-644: `prefetch_hovered_subdirectories` state fetch loop
- `src/api_service.rs`: Core async architecture (already existed, improved)
- `src/cache.rs`: Sequence-based validation (already existed)

---

## Phase 1.6: Feature Additions (Post-Refactor)

### Steps

7. **Filtering**
   - Add the ability to filter through each type of file by pressing "f". If a
     file matches one of the filters and is nested, show the directory in order
     for the user to be able to traverse this.
   - Filtering must respect the new async architecture

8. **Basic Error Handling**
   - Graceful error display if API unavailable.
   - Handle timeouts and authentication errors.
   - Show errors in status bar without blocking UI

---

## Phase 2: Folder State and Actions

### Objective
Add interactivity — rescan, pause/resume, and ignore actions.

### Steps

1. **Add Folder Status Queries**
   - Endpoint: `/rest/db/status?folder=<id>`.
   - Display "progress" or "needs rescan" state.
   - **Status:** Partially implemented, needs refactor integration

2. **✅ COMPLETED: Add Folder Controls**
   - `r` → POST `/rest/db/scan?folder=<id>` (rescan) **✅ Working**
   - `p` → pause/resume folder (update via `/rest/system/config` PUT) **⏳ Pending**
   - Confirmation dialogs implemented

3. **✅ COMPLETED: Add Ignoring Support**
   - `i` → Toggle directory in `.stignore` via `/rest/db/ignores?folder=<id>` PUT **✅ Working**
   - `I` → Add to `.stignore` AND delete locally (with confirmation) **✅ Working**
   - Wildcard support with custom selection for ignore removal **✅ Working**
   - Both file and folder ignore operations functional

   **Notes:**
   - Ignore toggling works for both directories and files
   - Wildcard patterns handled correctly
   - Delete operation includes safety confirmations
   - Path mapping for Docker container paths working

4. **✅ COMPLETED: File Deletion**
   - `d` → Delete any file/directory from disk with confirmation **✅ Working**
   - Confirmation dialog shows file/directory name
   - Displays warning: "This action cannot be undone!"
   - Works for both files and directories (recursive delete)
   - Triggers rescan after successful deletion
   - Independent from ignore status (unlike old behavior)

---

## Phase 3: UX Improvements

### Objective
Make navigation smoother and display richer data.

### Steps

1. **✅ COMPLETED: Breadcrumb Navigation**
   - Traversing directories with `Enter` / `Backspace` working
   - Navigation stack per folder implemented
   - Multi-pane horizontal layout showing navigation trail

2. **⏳ Pending: Async Loading Indicators**
   - Show spinners during REST requests

3. **✅ COMPLETED: Status Bar**
   - Shows folder sync state (idle, syncing, paused)
   - Displays data sizes (global bytes, local bytes, need bytes)
   - Shows in_sync/total items count
   - Displays sync status messages ("Up to date", "Out of sync", etc.)
   - Shows last update timestamp and file path per folder

4. **✅ COMPLETED: Keyboard Shortcuts Help**
   - Hotkey legend displayed at bottom of breadcrumb panels
   - Spans horizontally across all directory panels
   - Shows all available hotkeys: `↑/↓`, `Enter`, `←`, `i`, `I`, `d`, `r`, `R`, `q`
   - Keys highlighted in yellow for easy scanning

---

## Phase 4: Event Listening and Live Updates ✅ COMPLETED

### Objective
Subscribe to `/rest/events` for live status updates.

### Implementation

1. **✅ COMPLETED: Event Listener (async task)** (`src/event_listener.rs`)
   - Long-polling `/rest/events` endpoint
   - Processes event types:
     - `LocalIndexUpdated` - local file changes (has `filenames` array)
     - `ItemFinished` - sync completion
     - `LocalChangeDetected` - local changes detected
     - `RemoteChangeDetected` - remote changes detected
   - Event ID persistence across app restarts
   - Automatic reconnection on connection drops

2. **✅ COMPLETED: Granular Cache Invalidation** (`src/cache.rs`)
   - File-level invalidation: `invalidate_single_file()`
   - Directory-level invalidation: `invalidate_directory()`
   - Folder-level invalidation: `invalidate_folder()`
   - Browse cache and sync state cache properly cleared

3. **✅ COMPLETED: Realtime UI Updates**
   - Icons automatically update based on events (✅, ⚠️, ⏸, 💻, ☁️)
   - Immediate refresh when viewing affected directories
   - Last update timestamp displayed per folder
   - Progressive state updates without blocking UI

4. **✅ COMPLETED: Event Processing**
   - Channel-based architecture: `invalidation_tx` → `invalidation_rx`
   - Separate channel for event ID persistence
   - Debug logging for troubleshooting (controlled by DEBUG_MODE flag)

---

## Phase 5: Polishing and Extensions

### Objective
Add quality-of-life improvements and new modes.

### Steps

1. **Filesystem Diff Mode**
   - Compare local vs remote contents using `/rest/db/browse` and `/rest/db/file`.

2. **Batch Operations**
   - Multi-select directories for ignore/delete/rescan.

3. **Configurable Keybindings**
   - Optional TOML or YAML keymap file.

4. **Cross-Platform Packaging**
   - Build for Linux, macOS, and Windows with cross-compilation via `cross`.

---

## Future Considerations

- Live disk usage stats (`du`-like)
- Integration with Docker volumes
- CLI flags for headless operations
- Log viewer for Syncthing system logs
- Offline cache for quick folder browsing

---

### Summary of Phased Goals

| Phase | Status | Goal | Core Feature |
|-------|--------|------|---------------|
| 1 | ✅ Done | Initial prototype | Display folders & directories (with recursion & caching) |
| 1.5 | ✅ Done | Async refactor | Non-blocking API calls, smooth scrolling, performance optimization |
| 1.6 | 🚧 Next | Feature additions | Filtering, advanced error handling, comprehensive testing |
| 2 | ✅ Done | Control actions | Ignore ✅, delete ✅, rescan ✅, file deletion ✅, pause ⏳ |
| 3 | ✅ Mostly Done | UX polish | Breadcrumb nav ✅, status bar ✅, hotkey legend ✅, spinners ⏳ |
| 4 | ✅ **DONE** | **Live updates** | **Event streaming ✅, cache invalidation ✅, realtime icons ✅** |
| 5 | ⏳ Planned | Advanced features | Diff view, batch actions, packaging |

---

**Final Deliverable:**  
A cross-platform, keyboard-driven TUI manager for Syncthing that provides complete visibility and control over folders and files using only the REST API.
