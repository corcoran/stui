# Changelog

All notable changes to stui will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 2025-11-10

### 🎨 Code Quality & Architecture

**Pure Logic Extraction (Test-Driven Development)**
- Extracted 4 critical algorithms into testable pure functions using strict TDD methodology
- Added 22 new tests covering edge cases and business rules (RED → GREEN → REFACTOR cycle)
- `aggregate_directory_state`: Directory sync state calculation now fully testable with 5 tests
- `find_item_index_by_name`: Selection preservation logic extracted with 4 tests
- `compare_browse_items`: Sort comparison extracted from 60-line closure into reusable function (7 tests)
- `should_cleanup_stale_pending` & `should_verify_pending`: Time-based cleanup rules now testable (6 tests)
- **Impact**: Better test coverage (216 tests, up from 194), cleaner separation of concerns, algorithms can be understood and verified in isolation

**Major Refactoring: Simplification Cascades (-733 lines)**
- Cleaned up codebase by removing 733 lines of unused and duplicate code
- Removed speculative architecture that was never integrated (Elm-style state system, message passing)
- Unified confirmation dialog handling - all confirmation prompts now use a single, extensible pattern
- Consolidated duplicate type definitions into canonical locations
- **Impact**: 5% smaller codebase, cleaner architecture, easier to maintain and extend

**Module Organization**
- Extracted business logic into focused modules for better testability
- Organized app methods by domain (navigation, file operations, preview, sorting)
- Main.rs reduced from 3,582 lines to 1,397 lines (61% reduction)
- Created dedicated logic modules with comprehensive test coverage (188+ tests)

### ✨ New Features

**Status Bar State Indicators**
- Visual sync state indicators for folders and files/directories in status bar
- **Folder states**: Idle, Scanning, Syncing, Out of Sync, Local Additions, Error, Paused
- **File/directory states**: Synced, Out of Sync, Local Only, Remote Only, Syncing, Ignored (exists/deleted), Unknown
- Icons match Syncthing web UI terminology (e.g., "Local Additions" for receive-only folders with extra files)
- Consistent icon display between folder list and status bar
- Supports both emoji and nerdfont icon modes
- Smart "Out of Sync" detection: shows warning when idle folder has items needing sync

**ANSI/ASCII Art Viewer**
- View classic ANSI art files directly in the terminal
- Auto-detection of ANSI codes in any file (not just .ans/.asc extensions)
- Supports CP437 encoding (original IBM PC character set with box-drawing characters)
- 80-column automatic wrapping matches traditional ANSI art viewers
- Full color support (foreground colors 30-37, 90-97; background colors 40-47, 100-107)

**Quick Open Web UI**
- Press `o` in folder view to open Syncthing's web UI instantly
- Context-aware: same key opens files/folders when browsing (if configured)
- Automatically opens to the correct folder page

**Wildcard Search**
- Search with wildcards: `*jeff*`, `*.txt`, `photo*`
- Real-time filtering as you type - no lag on large directories
- Persistent search when navigating into subdirectories
- Smart context-aware clearing when backing out

**Out-of-Sync Filter with Summary Modal**
- Press `f` in folder view to see detailed breakdown of out-of-sync items
- **Summary modal** shows categorized counts: Downloading, Queued, Remote Only, Modified, Local Only
- **Breadcrumb filter** - press `f` again to filter current directory to show only out-of-sync items
- Status bar shows active filter indicator
- Real-time updates as events come in
- Cached for performance - instant display on subsequent views
- Includes local-only files in receive-only folders

**Search and Out-of-Sync Filter Mutual Exclusivity**
- Search and out-of-sync filter cannot be active simultaneously
- Activating one automatically clears the other
- Prevents UI confusion and ensures predictable behavior
- Clear visual feedback via status bar indicators

**Rescan Confirmation Dialog with Force Refresh**
- Press `r` to show rescan options instead of immediate rescan
- **Normal rescan** (`y`): Ask Syncthing to scan for changes (waits for sequence change to invalidate cache)
- **Force refresh** (`f`): Immediately clear cache + trigger rescan (useful for stale cache bugs)
- **Cancel** (`n` or `Esc`): Close dialog without action
- Cyan border dialog with clear option descriptions
- Works in both folder list and breadcrumb views

### 🔧 Improvements

**Filtering & Search**
- Non-destructive filtering with `filtered_items` field - original data preserved
- Search debouncing improves performance on large directories
- Filters persist when navigating into subdirectories
- Sorting order retained when toggling filters on/off
- Search works correctly while cache is building
- Out-of-sync filter updates in real-time as events arrive

**UI & Display**
- Local-only files now display properly in breadcrumbs
- Distinct icon for ignored files that still exist on disk (⚠️ vs 🚫)
- Out-of-sync filter indicator in status bar
- Queued status icon added to IconRenderer

**Reliability & Error Handling**
- Fixed folder refresh after reconnection - folders now populate correctly
- Transient state polling: UI updates when folders finish scanning/syncing
- Better offline mode: gracefully handles network errors without crashing
- Cached device names eliminate "Unknown" flash on startup
- Clearer error messages (removed unhelpful technical context)
- Out-of-sync cache invalidation on folder sequence change
- Ignore pattern matching works correctly with path components

**Performance**
- Optimized folder status polling (medium priority vs high)
- Non-blocking operations keep UI responsive during network issues
- Efficient cache validation for offline browsing
- Out-of-sync data cached for instant subsequent views
- Event parsing optimization: reverse processing with early termination (500-event fetch, stops after finding all folders)
- Startup API call pre-populates folder updates for instant display

### 🐛 Bug Fixes

**Critical: File Actions Target Wrong Items During Search/Filter** ⚠️
- Fixed serious bug where file operations targeted wrong items when search or out-of-sync filter was active
- **Impact**: Selecting file #2 in filtered view could delete/open/copy a completely different file from the unfiltered list
- **Fixed operations**: Delete (`d`), Open (`o`), Copy (`c`), Toggle ignore (`i`), Ignore+Delete (`I`)
- Delete confirmation now removes items by name (not index) from both filtered and unfiltered lists
- All 6 affected code locations corrected with comprehensive test coverage

**Search & Filtering**
- Fixed empty search query handling and state management
- Fixed search term clearing behavior
- Fixed cache filter state management
- Search results properly update when clearing terms
- Filters clear correctly from all breadcrumb levels
- Filtered results update correctly in subdirectories
- Extracted search/filter logic into dedicated `filters.rs` module (498 lines refactored)

**Out-of-Sync Filter**
- Fixed filter persistence when new files/events arrive
- Local files now trigger proper cache invalidation
- Out-of-sync summary updates correctly as events come in
- Filter re-applies correctly after Browse cache refresh
- Filter re-applies correctly after Need cache updates

**Display & State**
- Fixed sync state for files with sequence=0
- Preserved need_category when updating sync states
- Out-of-sync filter activates only when data is cached (prevents empty results)

**Last Update Timestamps**
- Fixed "Last Update" showing incorrect timestamps - now uses actual event time instead of processing time
- Fixed 1-minute startup delay - folder updates now appear instantly using `/rest/stats/folder` endpoint (matches Syncthing web GUI exactly)
- Fixed missing folders on startup - now shows "Last Update" for all folders, including those where last operation was a deletion (matches web GUI behavior)
- Removed "(remote changes)" placeholder - event stream now only updates when specific file paths are available (RemoteIndexUpdated events don't have file paths)
- Real-time updates continue for all file events: LocalIndexUpdated, ItemFinished, LocalChangeDetected, RemoteChangeDetected

### 📚 Documentation

**Test-Driven Development**
- Added comprehensive TDD guidelines to project documentation
- Real success story: 10 tests written first exposed exact bug, guided perfect solution
- Clear RED → GREEN → REFACTOR workflow
- Examples of what to do vs what not to do

**Feature Documentation**
- Added detailed implementation plan for out-of-sync filter feature
- Documented search/filter mutual exclusion design
- Simplified README for end users
- Added git commit pattern to avoid STDIN prefix
- Documented limitation with receive-only local changes in summary modal

### 🧪 Testing

- **569 total tests passing** (507 binary + 38 integration + 24 doc tests)
- Added 8 comprehensive tests for filter index bug (demonstrates bug + validates fix)
- Added 5 tests for search race conditions and empty query handling
- Added tests for out-of-sync filter and summary modal functionality
- Added tests for search and filter mutual exclusivity
- Added tests for non-destructive filtering (filtered_items field)
- Added 23 new tests for status bar state indicators (14 folder state + 9 file/directory state)
- Added 22 new tests for pure logic functions (TDD methodology)
- Added 10 comprehensive reconnection flow tests
- Added 6 tests for unified confirmation dialogs
- Fixed flaky timestamp test - now uses dynamic timestamps instead of hardcoded dates
- Fixed status_bar tests for out-of-sync filter parameter
- Fixed FolderStatus doctest after field additions
- All refactoring covered by existing test suite
- Zero compiler warnings

---

## How to Read This Changelog

- **Features** (✨): New capabilities you can use
- **Improvements** (🔧): Enhancements to existing features
- **Fixes** (🐛): Bug fixes
- **Code Quality** (🎨): Behind-the-scenes improvements that make development faster
- **Documentation** (📚): Improvements to guides and instructions
- **Testing** (🧪): Test coverage improvements

---

## Previous Development

For earlier changes, see the git commit history:
```bash
git log --oneline --since="2025-10-01"
```

Key milestones:
- Folder pause/resume functionality
- Folder type changing (Send Only/Send & Receive/Receive Only)
- Dynamic UI with text wrapping
- Sort mode cycling with visual indicators
- Image preview with Kitty/iTerm2/Sixel/Halfblocks protocols
- Event-driven cache invalidation
- SQLite caching for instant navigation
