# Syncthing CLI TUI Manager — Development Plan

This document outlines the step-by-step plan for building a **Rust Ratatui CLI tool** that interfaces with **Syncthing's REST API** to manage folders, view sync states, and control ignored/deleted files. The steps are structured to allow **progressive, testable milestones**, ideal for both human and LLM collaboration.

---

## 📍 Current Status (Updated 2025-10-26)

**Where we are:**
- ✅ **Phase 1 complete** - Basic prototype with folder/directory listing, recursive browsing, caching, and directory prioritization
- ✅ **Phase 2 partially complete** - Rescan, ignore toggling, and ignore+delete operations working
- 🚧 **Currently in Phase 1.5** - Major async refactor to eliminate blocking API calls

**Why the refactor?**
The current implementation blocks the UI thread during API calls, causing the interface to freeze when navigating folders with many files. This is unacceptable for folders with 10k+ items. The refactor will:
- Make all navigation instant (show cached data immediately)
- Fetch updates in the background without blocking
- Prioritize directories over files
- Implement intelligent cache invalidation (only when data actually changes)

**Next steps:**
1. Complete async architecture refactor (Phase 1.5)
2. Build comprehensive test suite to prevent regressions
3. Validate performance with large-scale testing
4. Add filtering and remaining features (Phase 1.6+)

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

## Phase 1.5: Non-Blocking Architecture Refactor ⚡

### Objective
**Critical Performance Fix:** Refactor all API calls to be truly non-blocking, eliminating UI freezes when navigating directories with many files.

### Problem Statement

**Current Issue:**
- Navigating directories blocks the UI thread while waiting for API responses
- Folders with thousands of files cause the entire interface to freeze
- Cache misses result in multi-second delays before any UI response
- Rapid navigation (↑↓↑↓) queues blocking calls, making the app feel unresponsive

**Root Causes:**
1. Synchronous API call pattern: `select_directory() → block → fetch → parse → render`
2. All-or-nothing rendering: UI waits for complete response before showing anything
3. Cache invalidation too aggressive: destroys existing data unnecessarily
4. Files and directories fetched together, slowing down the critical path

### Refactor Strategy

**Core Principles:**
1. **Cache-first rendering** - Always render cached data immediately, fetch updates in background
2. **Intelligent cache invalidation** - Only invalidate when Syncthing reports actual changes (timestamps/events)
3. **Directory-priority loading** - Fetch/render directories first, stream files after
4. **Progressive updates** - Update UI as data arrives, not all-at-once
5. **Rate limiting** - Batch file queries to prevent API/UI overwhelm

**Architecture Changes:**

```
OLD (blocking):
User presses ↓ → API call blocks → Response → Parse → Render → UI updates

NEW (non-blocking):
User presses ↓ → UI updates immediately (cached) → Background fetch → Stream updates
```

**Implementation Plan:**

1. **Separate UI and API Threads**
   - Move all API calls to background tokio tasks
   - Use channels (`tokio::sync::mpsc`) for UI ↔ API communication
   - UI thread only renders, never blocks on I/O

2. **Smart Cache Invalidation**
   - Track per-folder `last_modified` timestamps from API
   - Compare with cached timestamps before invalidating
   - Only invalidate specific paths, not entire subtrees
   - Use `/rest/db/status` to detect if rescan needed

3. **Two-Phase Loading**
   - **Phase 1 (Priority):** Fetch only directories for current path
   - **Phase 2 (Background):** Stream files in batches
   - Render Phase 1 results immediately, Phase 2 progressively

4. **Rate Limiting & Batching**
   - Limit concurrent API requests to 5-10
   - Batch file queries by chunks (100-500 items)
   - Debounce rapid navigation (wait 100ms before fetching)

5. **State Management**
   - Maintain "loading", "cached", "fresh" states per path
   - Show loading indicators only when cache empty
   - Display cached data with subtle "refreshing..." indicator

### Steps

1. **Create Async API Service Layer**
   - New module: `api_service.rs`
   - Spawn background worker task on app startup
   - Implement channel-based request/response pattern
   - Add request prioritization queue (directories > files)

2. **Implement Smart Cache Invalidation**
   - Add `cache_metadata` table tracking timestamps per folder/path
   - Query `/rest/db/status` for `modifiedBy` and `sequence` fields
   - Compare with cached metadata to decide on refresh
   - Never destroy cache unless confirmed stale

3. **Refactor Directory Loading**
   - Split `fetch_browse()` into `fetch_directories()` + `fetch_files()`
   - Directories fetch returns immediately to UI
   - Files fetch streams results via channel

4. **Add Loading States to UI**
   - Display cached content with "⟳" icon when refreshing
   - Show spinner only when no cache exists
   - Progress indicator for large file lists

5. **Testing Suite (Critical)**
   - **Unit tests** for cache invalidation logic
   - **Integration tests** with mock Syncthing API
   - **Performance tests** with 10k+ file directories
   - **Regression tests** to ensure no behavior changes:
     - Navigation still works
     - Ignore/rescan actions unchanged
     - Cache persistence intact
     - Icons render correctly

6. **Performance Validation**
   - Benchmark directory navigation speed (target: <50ms)
   - Test with real Syncthing instance (100k+ files)
   - Measure memory usage under heavy caching
   - Profile with `cargo flamegraph` to find bottlenecks

### Testing Requirements

Before merging refactor:
- [ ] All existing features work identically (no regressions)
- [ ] Navigation in 10k+ file folders feels instant
- [ ] Cache invalidation only triggers on actual changes
- [ ] API rate limiting prevents overwhelming Syncthing
- [ ] Memory usage remains reasonable (<500MB for 100k files)

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

---

## Phase 3: UX Improvements

### Objective
Make navigation smoother and display richer data.

### Steps

1. **Breadcrumb Navigation**
   - Allow traversing directories with `Enter` / `Backspace`.
   - Maintain a navigation stack per folder.

2. **Async Loading Indicators**
   - Show spinners during REST requests.

3. **Status Bar**
   - Show connection status, folder count, last API poll time.

4. **Keyboard Shortcuts Help**
   - Display modal on `?` showing all hotkeys.

---

## Phase 4: Event Listening and Live Updates

### Objective
Subscribe to `/rest/events` for live status updates.

### Steps

1. **Implement Event Listener (async task)**
   - Stream events and update UI reactively.
   - Detect folder rescans, sync completion, etc.

2. **Display Realtime Icons**
   - Automatically update states (✅, ⚠️, ⏸).

3. **Handle Connection Drops**
   - Reconnect and retry event stream automatically.

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
| 1.5 | 🚧 In Progress | **Async refactor** | **Non-blocking API calls, performance optimization** |
| 1.6 | ⏳ Planned | Feature additions | Filtering, advanced error handling |
| 2 | 🚧 Partial | Control actions | Ignore ✅, delete ✅, rescan ✅, pause ⏳ |
| 3 | ⏳ Planned | UX polish | Navigation, help modal, status bar |
| 4 | ⏳ Planned | Live updates | Event streaming and reactive icons |
| 5 | ⏳ Planned | Advanced features | Diff view, batch actions, packaging |

---

**Final Deliverable:**  
A cross-platform, keyboard-driven TUI manager for Syncthing that provides complete visibility and control over folders and files using only the REST API.
