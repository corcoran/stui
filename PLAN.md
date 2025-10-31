# Syncthing CLI TUI Manager — Development Plan

This document outlines the step-by-step plan for building a **Rust Ratatui CLI tool** that interfaces with **Syncthing's REST API** to manage folders, view sync states, and control ignored/deleted files. The steps are structured to allow **progressive, testable milestones**, ideal for both human and LLM collaboration.

---
## 📊 Current State Summary (2025-10-31)

**What's Working:**
- ✅ All planned features are implemented and functional
- ✅ UI is well-organized into 11 focused modules
- ✅ Services (API, cache, events) are properly separated
- ✅ Advanced features: image preview, real-time sync monitoring, vim keybindings
- ✅ Performance optimized (idle CPU <1-2%, responsive keyboard input)

**What Needs Work:**
- ⚠️ **Main blocker:** `main.rs` is 4,570 lines (monolithic business logic)
- ⚠️ **No tests:** Zero unit or integration tests
- ⚠️ **Refactoring not started:** Phase 1 and Phase 2 remain unimplemented
- 📋 **Priority:** Begin Phase 1 refactoring (extract handlers and business logic)

**Bottom Line:** The app is feature-complete and works well, but the codebase architecture needs refactoring for long-term maintainability and testability.

---
## Next Steps
 -  **PRIORITY: Begin Phase 1 Refactoring** _(main.rs has grown to 4,570 lines and needs breaking up)_
    - Write characterization tests before any refactoring (Phase 1.0)
    - Extract message types and handlers (Phase 1.1-1.3)
    - Extract business logic into logic/ modules (Phase 1.4)
 -  Pause / Resume folder toggle hotkey + status (with confirmation)
 -  Change Folder Type toggle hotkey + status (with confirmation)
 -  Remote Devices (Name, Download / Upload rate, Folders)
 -  Add event history viewer with persistent logging
 -  Add filtering functionality (show only ignored files, by type, etc.)
 -  Build comprehensive test suite (alongside refactoring)
 -  Improve error handling, display, and timeouts
 -  Performance testing with large-scale datasets (validate idle CPU usage and responsiveness)

---

## Refactoring Plan: Elm Architecture Migration

### Overview
Transform the codebase from a monolithic 4,570-line `main.rs` into a testable, maintainable architecture using the Elm/Redux pattern. This will be done in two phases: **Moderate** (foundational restructuring) followed by **Comprehensive** (full architectural overhaul).

**⚠️ REFACTORING STATUS: NOT STARTED** — Phase 1 and Phase 2 have not been initiated yet. Recent development has focused on feature completeness (image preview, file info popup, optimization) rather than architectural refactoring. The monolithic `main.rs` has actually grown from 3,016 lines to 4,570 lines (+51%) since this plan was written.

---

### Current Code Structure (Before Refactoring)

Understanding what exists helps plan the refactoring. Here's a snapshot of the current architecture:

#### File Sizes (Updated 2025-10-31)
```
src/main.rs                 4570 lines  (CRITICAL - NEEDS BREAKING UP!)
src/ui/dialogs.rs            637 lines  (Confirmation dialogs + file info popup)
src/api.rs                   579 lines  (Syncthing API client)
src/cache.rs                 554 lines  (SQLite cache operations)
src/api_service.rs           399 lines  (API request queue service)
src/event_listener.rs        337 lines  (Event stream listener)
src/ui/breadcrumb.rs         291 lines  (Breadcrumb panel rendering)
src/ui/icons.rs              241 lines  (Icon rendering)
src/ui/render.rs             216 lines  (Main render orchestration)
src/ui/status_bar.rs         215 lines  (Bottom status bar rendering)
src/ui/system_bar.rs         133 lines  (Top system info bar)
src/ui/folder_list.rs        118 lines  (Folder list panel)
src/ui/legend.rs             109 lines  (Hotkey legend rendering)
src/ui/layout.rs             109 lines  (Screen layout calculations)
src/ui/toast.rs               58 lines  (Toast notifications)
src/config.rs                 34 lines  (Configuration parsing)
src/utils.rs                  21 lines  (Utility functions)
```

**Total:** 8,648 lines across 18 files
**Status:** UI module is well-organized (11 files, good separation). Core blocker is monolithic `main.rs` which has grown 51% since plan was written.

#### Key Structs in `main.rs`
```rust
struct App {
    // API & Services (will move to services/)
    client: SyncthingClient,
    cache: CacheDb,
    api_tx/api_rx: API service channels,
    invalidation_rx/event_id_rx: Event listener channels,

    // State (will move to state/)
    folders: Vec<Folder>,
    folder_statuses: HashMap<String, FolderStatus>,
    breadcrumb_trail: Vec<BreadcrumbLevel>,
    focus_level: usize,

    // Sync State Management (will move to state/sync_manager.rs)
    file_sync_states: scattered in BreadcrumbLevel,
    manually_set_states: HashMap<String, ManualStateChange>,
    syncing_files: HashSet<String>,

    // UI State (will move to state/)
    display_mode: DisplayMode,
    sort_mode: SortMode,
    sort_reverse: bool,

    // Confirmation dialogs (will move to state/)
    confirm_revert: Option<...>,
    confirm_delete: Option<...>,
    pattern_selection: Option<...>,
}

struct BreadcrumbLevel {
    folder_id: String,
    items: Vec<BrowseItem>,
    state: ListState,
    file_sync_states: HashMap<String, SyncState>,  // Mixed responsibility!
    ignored_exists: HashMap<String, bool>,
}
```

#### Key Methods in `App` (40+ methods, 3000+ lines)

**API Response Handling (→ handlers/api.rs):**
- `handle_api_response()` - 400 lines, handles BrowseResult, FileInfoResult, FolderStatusResult
- `load_sync_states_from_cache()` - Load cached states
- `merge_local_only_files()` - Merge receive-only folder changes

**Event Handling (→ handlers/events.rs):**
- `handle_cache_invalidation()` - 180 lines, handles file/directory/folder invalidation
- Handles ItemStarted/ItemFinished for syncing state

**Navigation (→ logic/navigation.rs):**
- `load_root_level()` - 100 lines, enter folder
- `enter_directory()` - 130 lines, drill down breadcrumb
- `go_back()` - Exit breadcrumb level
- `sort_level()`, `sort_level_with_selection()` - Sorting logic
- `next_item()`, `previous_item()`, `jump_to_first()`, `jump_to_last()`, `page_up()`, `page_down()` - Navigation

**Sync State Logic (→ logic/sync_states.rs):**
- State transition validation (lines 663-701)
- `update_ignored_exists_for_file()` - Check if ignored files exist
- `check_ignored_existence()` - Batch check on directory load
- Syncing state preservation logic (scattered in multiple places)

**Ignore Operations (→ logic/ignore.rs):**
- `toggle_ignore()` - 100 lines, add/remove from .stignore
- `ignore_and_delete()` - Ignore + delete from disk
- `pattern_matches()` - Pattern matching logic
- `find_matching_patterns()` - Find which patterns match a file

**File Operations (→ handlers/keyboard.rs or domain/file.rs):**
- `delete_file()` - Delete with confirmation
- `restore_selected_file()` - Revert receive-only folder
- `rescan_selected_folder()` - Trigger Syncthing rescan

**Keyboard Handling (→ handlers/keyboard.rs):**
- `handle_key()` - 700 lines, giant match on KeyEvent
- Vim mode state tracking (`gg` command)

**Background Operations (needs better home):**
- `batch_fetch_visible_sync_states()` - Prefetch sync states for visible items
- `prefetch_hovered_subdirectories()` - Prefetch child directories
- `fetch_directory_states()` - Fetch directory metadata
- `discover_subdirectories_sync()` - Recursive directory discovery

#### Key Enums & Types
```rust
enum SyncState { Synced, OutOfSync, LocalOnly, RemoteOnly, Ignored, Syncing, Unknown }
enum DisplayMode { Off, TimestampOnly, TimestampAndSize }
enum SortMode { VisualIndicator, Alphabetical, DateTime, Size }
enum ManualAction { SetIgnored, SetUnignored }  // For state transition validation

struct ManualStateChange {
    action: ManualAction,
    timestamp: Instant,
}
```

#### Critical Patterns to Preserve During Refactoring

1. **State Transition Validation** (lines 663-701):
   - Manual action tracking (SetIgnored/SetUnignored)
   - Illegal transition blocking (Syncing → Unknown)
   - Must be preserved in any new architecture

2. **Syncing State Preservation** (lines 549-582, 604-624):
   - Browse results must preserve actively syncing files
   - Check `syncing_files` HashSet before overwriting
   - Two code paths: root level + non-root levels

3. **Ignored File Existence Tracking**:
   - `ignored_exists` HashMap updated on every state change
   - Inline filesystem checks to avoid borrow checker issues
   - Critical for status bar display

4. **Event-Driven Cache Invalidation**:
   - Granular invalidation (file/directory/folder level)
   - Persistent event ID across restarts
   - Auto-recovery from stale event IDs

---

### Testing Strategy

Tests will be added **throughout the refactoring process**, not just at the end:

#### Before Phase 1: Characterization Tests
- **Goal:** Lock down existing behavior before refactoring
- Write integration tests for critical user flows:
  - Navigate folders → enter directory → toggle ignore → verify state
  - Un-ignore file → verify ItemStarted → verify Syncing → verify Synced
  - Browse results don't overwrite Syncing states
- These tests document current behavior and catch regressions during refactoring
- **Tool:** Run actual app against mocked Syncthing API responses

#### During Phase 1: Unit Tests for Extracted Logic
- As we extract pure functions to `src/logic/`, add unit tests immediately:
  - `sync_states.rs` → Test state transition validation rules
  - `navigation.rs` → Test sorting, selection preservation
  - `ignore.rs` → Test pattern matching edge cases
- Each extraction should come with tests (no naked code)
- **Benefit:** Proves the extracted logic works correctly in isolation

#### During Phase 2: Comprehensive Test Suite
- Unit tests for domain models (pure business logic)
- Property-based tests for state machines (use `proptest` crate)
- Integration tests with mocked services
- **Target:** 80%+ coverage on business logic, lower on UI/glue code

#### Continuous: Regression Tests
- Any bug found during refactoring gets a test first, then fixed
- Prevents same bug from reappearing after further changes

---

### Phase 1: Moderate Refactoring
**Goal:** Break apart the 3000-line `main.rs` into manageable pieces while establishing Elm Architecture foundations.

#### 1.0: Write Characterization Tests (FIRST!)
- Add `tests/` directory with integration tests
- Mock Syncthing API responses for critical flows
- Document current behavior before making changes
- Run tests after each refactoring step to verify no regressions

#### 1.1: Extract Message Types
- **New file:** `src/messages.rs`
- Create `AppMessage` enum containing all events:
  - `KeyPress(KeyEvent)`
  - `ApiResponse(ApiResponse)`
  - `CacheInvalidation(CacheInvalidation)`
  - `Tick` (for periodic updates)
- Move from scattered handling to centralized message dispatch

#### 1.2: Extract State Management
- **New file:** `src/state.rs`
- Extract pure state structs from `App`:
  - `AppState` (folders, statuses, breadcrumbs)
  - `NavigationState` (focus_level, selection)
  - `SyncStateManager` (file_sync_states, manually_set_states, syncing_files)
- Keep `App` as coordinator but move data to state structs

#### 1.3: Extract Handlers
- **New directory:** `src/handlers/`
  - `mod.rs` - Module exports
  - `keyboard.rs` - All keyboard handling (`handle_key` method)
  - `api.rs` - API response handling (`handle_api_response`)
  - `events.rs` - Event invalidation (`handle_cache_invalidation`)
- Each handler becomes: `fn handle_xxx(state: &mut AppState, msg: Xxx) -> Result<Option<Command>>`

#### 1.4: Extract Business Logic
- **New directory:** `src/logic/`
  - `mod.rs` - Module exports
  - `sync_states.rs` - Sync state transitions, validation rules
  - `navigation.rs` - Breadcrumb logic, sorting, selection
  - `ignore.rs` - Pattern matching, toggle logic
- Pure functions that can be unit tested

#### 1.5: Consolidate Services
- **New directory:** `src/services/`
  - Move `api_service.rs` → `services/api.rs`
  - Move `event_listener.rs` → `services/events.rs`
  - Keep `cache.rs` and `api.rs` (client) as-is for now

**Expected Outcome:** `main.rs` reduces from ~3000 to ~500 lines, core logic becomes testable

---

### Phase 2: Comprehensive Refactoring
**Goal:** Full Elm Architecture with complete separation of concerns

#### 2.1: Pure Elm Update Function
- Single `update(state: AppState, msg: AppMessage) -> (AppState, Command)` function
- All handlers become pure functions
- Side effects isolated into `Command` enum for execution outside update loop

#### 2.2: Domain Models
- **New directory:** `src/domain/`
  - `folder.rs` - Folder domain logic and state machine
  - `sync_state.rs` - Sync state machine with clear transitions
  - `file.rs` - File metadata and operations
- Pure business logic with zero dependencies on UI or services

#### 2.3: Full Directory Restructure
```
src/
├── main.rs              # 50 lines - just setup and event loop
├── app.rs               # App coordinator (manages update loop)
├── messages.rs          # All message types (AppMessage enum)
│
├── state/               # Pure state (no I/O, no side effects)
│   ├── mod.rs
│   ├── app_state.rs
│   ├── navigation.rs
│   └── sync_manager.rs
│
├── logic/               # Business logic (pure functions)
│   ├── mod.rs
│   ├── sync_states.rs   # State transition validation
│   ├── navigation.rs    # Breadcrumb/sorting logic
│   └── ignore.rs        # Pattern matching
│
├── handlers/            # Message handlers (thin layer)
│   ├── mod.rs
│   ├── keyboard.rs
│   ├── api.rs
│   └── events.rs
│
├── services/            # External I/O (HTTP, cache, events)
│   ├── mod.rs
│   ├── api.rs           # API request service
│   ├── events.rs        # Event listener
│   └── cache.rs         # SQLite cache
│
├── domain/              # Domain models (pure business logic)
│   ├── mod.rs
│   ├── folder.rs
│   ├── sync_state.rs
│   └── file.rs
│
├── ui/                  # Already well-organized
│   ├── mod.rs
│   ├── render.rs
│   ├── breadcrumb.rs
│   ├── folder_list.rs
│   ├── status_bar.rs
│   ├── system_bar.rs
│   ├── legend.rs
│   ├── dialogs.rs
│   ├── layout.rs
│   └── icons.rs
│
├── api.rs               # Syncthing API client
├── config.rs            # Configuration
│
└── tests/               # Unit and integration tests
    ├── logic_tests.rs
    ├── state_tests.rs
    └── integration_tests.rs
```

#### 2.4: Add Comprehensive Tests
- Unit tests for state transitions
- Unit tests for business logic (ignore patterns, sync validation)
- Integration tests with mocked services
- Property-based tests for state machines

---

### Benefits of This Approach

✅ **Testability:** Pure functions with no side effects can be unit tested in isolation
✅ **Maintainability:** Clear separation of concerns, small focused files (<300 lines each)
✅ **Predictability:** Unidirectional data flow makes state changes easy to reason about
✅ **Gradual Migration:** Phase 1 can be done incrementally, Phase 2 builds on solid foundation
✅ **Elm Pattern:** Proven architecture for TUI apps, handles async gracefully via Commands
✅ **Debugging:** All state changes flow through single update function with full visibility

---

## 📍 Current Status (Updated 2025-10-31)

**Architecture Status:**
- ⚠️ **Refactoring not yet started** — Phase 1 and Phase 2 from the Refactoring Plan above remain unimplemented
- ✅ **UI module well-organized** — 11 focused files with clear separation of concerns
- ⚠️ **main.rs is monolithic** — 4,570 lines containing all business logic, state, and event handling
- ✅ **Services properly separated** — API client, cache, event listener, and API queue are isolated
- ⚠️ **No tests** — Zero unit or integration tests exist to validate business logic
- 📊 **Feature completeness** — Application is functionally complete with advanced features

**Recent Accomplishments (Session 2025-10-31):**
- ✅ Optimizations and performance improvements
- ✅ Block against pending operations (unignore while delete is in progress)
- ✅ Sync state management refinements

**Recent Accomplishments (Session 2025-10-28):**

**File Info Popup Feature:**
- ✅ Comprehensive file information popup triggered by `?` key
- ✅ Two-column layout: metadata (35 chars fixed) + preview (remaining width with min 50 chars)
- ✅ Metadata shows: name, type, size, modified time, local state, permissions, modified by device, sync status, device availability, disk existence
- ✅ Text preview with full vim keybindings (j/k, ^d/^u, ^f/^b, gg/G, PgUp/PgDn)
- ✅ Visual scrollbar for preview pane with accurate position tracking
- ✅ Smart scroll clamping prevents offset drift beyond valid bounds
- ✅ Binary file detection with text extraction (strings-like algorithm)
- ✅ Sync status uses icon renderer (respects emoji vs nerdfont preference)
- ✅ Device names resolved from IDs with fallback to short ID
- ✅ User-friendly sync status display (✅ In Sync, ⚠️ Behind, ⚠️ Ahead)
- ✅ Shows only connected/online devices in availability list
- ✅ Terminal theme compatible (uses default background)
- ✅ Max file size: 20MB (supports future image preview)

**UI Layout Reorganization:**
- ✅ System bar moved to top (full width) showing device info, uptime, transfer rates
- ✅ Main content area (folders + breadcrumbs) with smart horizontal sizing
- ✅ Hotkeys legend moved to full-width bar above status
- ✅ Status bar at bottom (full width) for folder/directory metrics
- ✅ Current folder gets 50-60% screen width for better visibility
- ✅ Parent folders share remaining 40-50% equally
- ✅ All ancestor breadcrumbs stay highlighted (blue border) when drilling deeper
- ✅ Current breadcrumb has cyan border + arrow, parents have blue border only

**Icon Pattern Consistency:**
- ✅ Changed ignored file icons to follow `<file|dir><status>` pattern
- ✅ Ignored + exists: `📄⚠️` or `📁⚠️` (file/dir + warning)
- ✅ Ignored + deleted: `📄🚫` or `📁🚫` (file/dir + ban)
- ✅ Consistent with all other sync state icons (Synced, RemoteOnly, LocalOnly, etc.)

**Smart Hotkey Legend:**
- ✅ Context-aware key display based on focus level
- ✅ Folder view: Hides Sort, Info, Ignore, Delete (not applicable to folders)
- ✅ Breadcrumb view: Shows all file operation keys
- ✅ Restore only shown when folder has local changes (receive_only_total_items > 0)
- ✅ Rescan always visible (works in both folder list and breadcrumbs)

**Previous Accomplishments (Session 2025-01-28):**

**State Transition Validation System:**
- ✅ Replaced arbitrary time-based heuristics (3s/5s timeouts) with logical state transition validation
- ✅ Action tracking: `ManualStateChange` struct tracks SetIgnored/SetUnignored actions with timestamps
- ✅ Transition validation: After SetIgnored only accepts Ignored state, after SetUnignored rejects Ignored state
- ✅ No more race conditions - works regardless of network latency or event timing
- ✅ Safety valve: 10-second timeout prevents permanent blocking in edge cases
- ✅ Much more robust and predictable than time-based approach

**Syncing State Implementation:**
- ✅ Added `Syncing` variant to `SyncState` enum
- ✅ Real-time syncing indicator using ItemStarted/ItemFinished events
- ✅ Files show spinning icon (🔄) during active downloads/uploads
- ✅ Protection against premature state clearing during sync operations
- ✅ Smooth transitions: Unknown → RemoteOnly → Syncing → Synced

**Ignored File State Handling:**
- ✅ Fixed file invalidation clearing states inappropriately
- ✅ Fixed browse results clearing Syncing states
- ✅ Fixed Unknown state flashing on un-ignore operations
- ✅ Fixed Ignored state flashing on ignore/delete operations
- ✅ Fixed stuck states for already-synced files after un-ignore

**Event Listener Improvements:**
- ✅ Auto-recovery from stale event IDs (resets to 0 if high ID returns nothing)
- ✅ Comprehensive ItemStarted/ItemFinished event handling
- ✅ Clean, concise debug logging without overwhelming log files

**Previous Accomplishments (Session 2025-01-27):**

**System Status Bar:**
- ✅ Device status bar at bottom of Folders panel
- ✅ Shows device name, uptime (formatted as "3d 16h" or "15h 44m")
- ✅ Local state summary: total files, directories, and storage across all folders
- ✅ Live download/upload transfer rates (updated every 2.5 seconds)
- ✅ Pastel yellow labels matching Hotkeys bar style
- ✅ Consistent gray text color across all status bars

**Icon System Refactor:**
- ✅ Centralized icon rendering in `src/icons.rs` module
- ✅ Support for both Emoji and Nerd Fonts modes (configurable via `icon_mode` in config.yaml)
- ✅ Pastel color scheme: blue folders/files, colored status icons
- ✅ Eliminated ~75 lines of duplicated icon rendering code
- ✅ Proper alignment for all icon types including ignored items
- ✅ Option B coloring: folder/file icons stay blue, status icons get their own colors

**Configuration:**
- ✅ Config file now properly located at `~/.config/synctui/config.yaml`
- ✅ Added `icon_mode` setting ("emoji" or "nerdfont")
- ✅ Fallback to `./config.yaml` for development

**Sorting System:**
- ✅ Multi-mode sorting with `s` key: Icon (sync state) → A-Z → DateTime → Size
- ✅ Reverse sort with `S` key
- ✅ Sort mode displayed in status bar (e.g., "Sort: DateTime↑")
- ✅ Directories always prioritized above files regardless of sort mode
- ✅ Selection preserved when re-sorting
- ✅ Proper handling of emoji icon widths using unicode-width

**File Info Display (Three-State Toggle):**
- ✅ Three display modes with `t` key: Off → TimestampOnly → TimestampAndSize → Off
- ✅ File sizes shown in human-readable format (e.g., `1.2K`, `5.3M`, `2.1G`)
- ✅ Bytes < 1KB shown as plain digits (e.g., `123`, `999`)
- ✅ Size omitted for directories (semantically correct)
- ✅ Smart truncation handles all three modes gracefully
- ✅ Info displayed in dark gray for subtle appearance
- ✅ Unicode-aware alignment (handles emoji widths correctly)

**Vim Keybindings:**
- ✅ Optional vim navigation mode with `--vim` CLI flag or `vim_mode: true` in config
- ✅ Full vim navigation: `hjkl`, `gg`, `G`, `Ctrl-d/u`, `Ctrl-f/b`
- ✅ Standard keys also available (PageUp/Down, Home/End) but not advertised
- ✅ Dynamic hotkey legend shows vim keys when enabled
- ✅ State tracking for `gg` double-key command

**Database Schema Updates:**
- ✅ Added `mod_time` and `size` fields to `browse_cache` table
- ✅ Proper cache invalidation when schema changes (requires manual cache clear)

**UI Improvements:**
- ✅ Hotkey legend now wraps automatically to multiple lines
- ✅ Updated legend with all keys: `s`, `S`, `t`, vim keys (when enabled)
- ✅ Cache clearing fix for schema migrations

**Performance Optimizations (Completed 2025-10-31):**
- ✅ Idle detection (300ms threshold) prevents background operations from blocking keyboard input
- ✅ Non-blocking prefetch operations converted from async to sync (cache-only, no `.await`)
- ✅ Event poll timeout increased from 100ms to 250ms (60% reduction in wakeups)
- ✅ CPU usage reduced from ~18% idle to <1-2% measured
- ✅ Instant keyboard responsiveness even during background caching
- ✅ Request deduplication prevents redundant API calls
- ✅ Smart caching with sequence-based validation ensures data freshness without excessive polling


## Polishing and Extensions

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

**Final Deliverable:**  
A cross-platform, keyboard-driven TUI manager for Syncthing that provides complete visibility and control over folders and files using only the REST API.
