# Changelog

All notable changes to stui will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 🐛 Bug Fixes

- The out-of-sync filter (`f`) no longer overwrites what stui remembers about each file. Afterwards a file could show the wrong icon when opened again: ❓ for one that was downloading, or ⛅ for one deleted on another device. In a Receive Only folder, a file with local changes could also drop out of the filter
- Pausing, resuming or editing a folder in Syncthing's web GUI or another app shows in stui right away. A folder you are browsing returns you to the folder list when it is paused

---

## [0.14.0] - 2026-11-22

### ✨ New Features

- **Local deletions**: In a Receive Only folder, a file deleted on this device shows ❌ (a trash can in Nerd Font mode) and "Deleted locally". It no longer looks like a pending download. Its preview says how to restore it
- **Undelete one file**: In a Receive Only folder, press `u` on a file deleted on this device to download it again from your other devices. `R` still reverts the whole folder. Undeletes run in the background, so you can press `u` on several files in a row
- **Terminal title**: The terminal or tab title follows where you are (`stui · Projects/website`, or the file while previewing) and goes back to the previous title on exit. Set `terminal_title: false` to leave the title alone
- **Copy over SSH**: With no `clipboard_command`, `c` copies through your terminal (OSC 52), so it works locally and over SSH with no setup
- **Open files over SSH**: Over SSH or without a desktop session, `o` opens text files and directories in your editor inside stui's window. The editor is `editor`, else `$VISUAL`, else `$EDITOR`. Quit the editor to return to stui. Binary files only open on a desktop. To edit text files inside stui on the desktop too, set `open_text_in_editor: true`
- **Open from the preview**: `o` in the file preview opens the previewed file, and the preview reloads when you return from the editor
- **NFO and DIZ art**: `.nfo` and `.diz` files, and other CP437 art with no escape codes, preview as ANSI art (UTF-8 or XML `.nfo` files preview as text)
- **DOS symbols in art**: ANSI art shows the DOS symbols for control bytes (☺ ☻ ♥ ↑ ▲ and the rest), which artists use for shading
- **Amiga ASCII**: Amiga art (ISO-8859-1, with `·` `¯` `¥` and the like) previews with its own characters instead of DOS box pieces, in `.asc`, `.txt` and other files
- **Blinking art**: ANSI art that uses blink blinks, in terminals that support it
- **UTF-16 text**: UTF-16 text files preview as text
- **JSON preview**: `.json` files preview pretty-printed with colors. Press `r` for the raw file
- **CSV/TSV preview**: `.csv` and `.tsv` files preview as a table fitted to the pane, with long cells ending in `…`. Press `r` for the raw file. JSON, CSV and TSV files over 2 MB show raw
- **Archive preview**: zip (including jar, apk, epub, cbz and Office files), tar and tar.gz files preview as a listing with each entry's date, size and path, whatever their extension. Zips with more than 10,000 entries list their first 10,000 rows (the totals cover the whole archive). Tarballs list their first 10,000 entries or first 64 MB (tar.gz: or first 512 MB unpacked)
- **gzip preview**: a gzip file that isn't a tarball previews as the file inside it, so `syslog.1.gz` shows the log and `config.json.gz` shows formatted JSON. Previews show the first 20 MB unpacked. A damaged or partly written file shows what decompressed and says it is incomplete

### 🔧 Improvements

- The preview popup keeps one column of space inside the borders of its info and text panes
- ANSI art previews in the classic DOS colors on black, the same in every terminal. Uncolored text is light grey instead of your terminal's text color, and bold text is bright
- ANSI art with a SAUCE width is drawn at that width
- ANSI art rows are cut off at the pane edge instead of wrapping
- Large text previews scroll smoothly. Only opening the file takes time; a 20 MB file used to stall on every scroll step

### 🐛 Bug Fixes

- The breadcrumb legend shows `f:Filter` for the out-of-sync filter; it was only listed in the README
- Photos with an EXIF orientation (most phone and camera shots) preview upright instead of sideways, and their resolution reads the right way round
- Selecting a paused folder, or pausing the selected one, no longer leaves a folder's files on screen; the pane closes, and resuming the folder shows its files again
- Deleting a file in a Receive Only folder keeps it in the list as deleted locally, instead of removing the row and briefly showing ✅
- A file shows ✅ as soon as its download finishes, instead of after the next directory refresh
- Preview error messages wrap at word boundaries instead of mid-word
- Wide characters (Chinese, Japanese, Korean) at the end of a wrapped preview row move to the next row instead of hanging off the pane edge
- Icons no longer flip back to an older state after you delete, ignore, unignore, undelete or revert
- `R` updates the whole folder at once instead of file by file
- A failed delete or revert shows an error
- Previewing an image that is not on disk says why, instead of staying on "Loading image..."
- Sorting by sync state (`s`) follows the icons: a file being restored by a revert sorts with syncing files, and a file deleted locally sorts with local changes
- A config without `path_map` loads instead of failing with "missing field `path_map`"
- ANSI art previews show color 37 as light grey and 97 as white; they were swapped, as were backgrounds 47 and 107. Codes 39 and 49 switch back to the default colors
- Text and ANSI art previews keep blank lines, so art lines up as drawn. Blank lines at the end are dropped
- ANSI art that moves the cursor up, down or back draws in the right place, instead of adding extra lines between rows
- ANSI art with codes like `ESC[?7h` no longer prints `7h`; cursor positioning, erase in line and tabs draw in the right place
- ANSI art keeps a color across line breaks until the file changes it, instead of resetting at each new line
- `clipboard_command` and `open_command` accept arguments, for example `xclip -selection clipboard`
- The shipped `config.yaml.example` lists `terminal_title`, says where the file goes, and no longer suggests `vim` as an `open_command`

### 🧪 Testing

- `demo/run.sh` builds the working tree and runs it with `--debug` against the demo cluster. Its cache and debug log stay under `demo/.run/`, away from a real stui

---

## [0.13.0] - 2026-11-20

### ✨ New Features

- **Markdown preview**: `.md` files render in the preview with headings, emphasis, lists, quotes, code blocks, links and tables. Press `r` to switch to the raw source
- **Step through files in the preview**: `Shift+↑`/`Shift+↓` (or `K`/`J` in vim mode) preview the previous or next file in the directory without closing the popup. Directories are skipped, and the list selection follows
- **Folder groups** (Syncthing 2.1+): folders are listed under their group, with ungrouped folders last. Press `e` in the folder list to pick a group, remove one, or create a new one. On older Syncthing the list looks as before, and `e` says which version groups need
- **Folder cards rely on icons**: folder type shows as ⇅ ⇡ ⇣, status words appear only for progress (`Syncing 62%`), scanning and errors, and cards drop detail instead of being cut off when the folder pane narrows while browsing. The selected folder gets a full-width highlight
- **Syncthing version** in the system bar, e.g. `laptop · Syncthing v2.1.7`

### 🐛 Bug Fixes

- Scrolling to the bottom of a text preview reaches the last line when long lines wrap
- A last-activity time that can't be parsed no longer shows as "just now"; the folder shows no last activity instead
- Toasts show what happened: failures are red ✗ and refusals or missing settings are yellow `!`, where many used to show a green ✓. Progress messages like "Rescanning…" and "Sort: A-Z" are cyan
- "Modified By" in the file info popup shows the device name instead of a short device ID like `3QFSJG3`
- Folder cards show scanning, waiting-to-scan and cleaning folders with 🔍 instead of as up to date, and paused folders no longer show "0 B │ 0 files"

---

## [0.12.0] - 2026-11-20

### ✨ New Features

- **Syncing icons for files in transfer**: Files Syncthing is downloading show 🔄 right away, even if stui started mid-transfer. Sorting by sync state groups them too
- **Directory states without visiting**: Directories show remote-only files or local changes inside them as soon as the folder opens, derived from Syncthing's need and local-changes lists
- **Full out-of-sync list**: The `f` filter loads every needed item in the background instead of stopping at the first 1000, with `loading X/Y` in the status bar. Your cursor stays on its item as more arrive
- **Dimmed background behind popups**: File preview, update history, menus and confirmations fade the UI behind them
- **Unignore toast**: Unignoring a file confirms right away while Syncthing rescans

### 🐛 Bug Fixes

- Folder cards no longer stick on "Out of sync" while Syncthing is still syncing
- Icons no longer flash to `?` after an ignore toggle, a remote change, or when entering a folder; items still loading show a blank slot
- Timestamps display in local time instead of UTC
- Folders with no activity no longer show "Last activity: 739939 days ago"
- Folder cards follow `icon_mode` (they always used emoji)
- Image previews work again in Konsole and in kitty started from Konsole with `image_protocol: auto` (Konsole uses iTerm2, kitty uses Kitty graphics)
- Borders no longer break in Konsole next to the "Binary file" note and other warning symbols
- Emoji mode uses ❗ for out-of-sync and ⛅ for remote-only; the old ⚠️ and ☁️ broke borders in Konsole
- API requests are served in priority order; background prefetches used to run ahead of navigation

### ⚡ Performance

- Ignore, unignore and delete rescan only the affected path instead of the whole folder (Syncthing still rescans the folder itself after an ignore change)
- Need lists are fetched only for folders that are open or syncing, and the folder-list preview waits until the selection rests

### 🔧 Changes

- Dependencies upgraded: ratatui 0.30, ratatui-image 11, reqwest 0.13, rusqlite 0.43, dirs 7
- Chafa image rendering is an opt-in `chafa` cargo feature: it needs libchafa at build and run time, so release builds leave it out
- The out-of-sync summary modal is removed; it had no key binding since 0.11.0
- README screenshots and demo GIF, generated by `demo/record.sh`

---

## [0.11.1] - 2025-11-16

### 🐛 Bug Fixes

- Fix the Windows release packaging step that copies README and CHANGELOG

---

## [0.11.0] - 2025-11-16

### ✨ New Features

- **Folder cards**: The folder list shows each folder as a card with its type, size, file count and status, plus a breakdown of out-of-sync items
- **Activity feed**: In folder view the status bar shows the latest sync activity and the number of connected devices

---

## [0.10.0] - 2025-11-15

### ✨ New Features

**Folder Update History Enhancements**
- **Jump to File from History**: Press `Enter` on any file in the folder history modal to navigate breadcrumbs directly to that file's location
- Automatically enters the folder and traverses the directory tree to highlight the selected file
- Graceful error handling: navigates as deep as possible if intermediate directories are missing
- Clear toast messages guide user through navigation errors (missing directories, missing files)
- **Implementation**: Pure path parsing logic with comprehensive test coverage (5 tests), reusable folder entry method

### 🔧 Improvements

**File Preview Datetime Formatting**
- Standardized datetime display in file preview metadata to match folder history format
- Before: `Modified: 2024-01-15T14:30:45.123456789Z` (raw RFC 3339)
- After: `Modified: 2024-01-15 14:30:45` (clean, human-readable)
- Added `format_datetime()` utility function with doctest coverage

### 🧪 Testing

- **604 total tests passing** (up from 569)
- Added 5 new tests for path parsing logic (deep paths, root-level files, spaces in names)
- Added integration test for jump-to-file path parsing
- Added doctest for datetime formatting function
- Zero compiler warnings, zero clippy warnings

---

## [0.9.1] - 2025-11-10

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
