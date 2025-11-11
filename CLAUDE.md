# stui - Syncthing TUI Manager

## Project Overview

Building a Rust Ratatui CLI that manages Syncthing via its REST API — listing folders, showing contents, adding .stignore rules, and deleting directories safely with Docker path-mapping support.

**What makes this app unique:**
- **Advanced file management**: Breadcrumb navigation, selective ignore, and visibility of ignored files that still exist on disk
- **Terminal image preview**: High-performance image rendering (40-200ms) with adaptive quality using Kitty/iTerm2/Sixel/Halfblocks protocols
- **ANSI art rendering**: Full-featured ANSI/ASCII art viewer with CP437 encoding, 80-column wrapping, and proper color support
- **Real-time sync monitoring**: Event-driven cache invalidation with granular file-level updates

The file management, image preview, and ANSI art rendering features are core differentiators and should be modified with care.

## claude instructions

### CRITICAL: Test-Driven Development is MANDATORY

**YOU MUST WRITE TESTS FIRST. NO EXCEPTIONS.**

Every code change requires this exact workflow:

1. **STOP and think**: "What tests do I need?"
2. **Write tests FIRST** that expose the bug or define the feature
3. **Verify tests fail** (proving they test the right thing)
4. **Implement** minimal code to pass tests
5. **Verify tests pass**
6. **Only then** commit

**Why this matters:**
- Without tests, you waste user's time and money debugging blind
- Tests document expected behavior and catch regressions
- TDD prevents over-engineering and scope creep
- Example: Commit fcf4362 (reconnection fix) - 10 tests written first, exposed exact bug, guided perfect solution

**Red flags that you're doing it wrong:**
- ❌ "Let me try this change and see if it works"
- ❌ "I'll add some debug logging to investigate"
- ❌ Making multiple attempts without tests
- ❌ Saying "I think this should work"

**What you should do instead:**
- ✅ "Let me write a test that reproduces this bug"
- ✅ "I'll write tests for these 3 scenarios first"
- ✅ "Here's a failing test - now I'll implement the fix"
- ✅ "All 10 tests pass, ready to commit"

**When to write tests:**
- Adding new features → Write feature tests first
- Fixing bugs → Write test that reproduces bug first
- Refactoring → Ensure existing tests pass, add coverage if missing
- Changing state logic → Write state transition tests first
- User reports "X doesn't work" → Write test showing X failing

**If you catch yourself coding before testing:**
1. STOP immediately
2. Delete/revert the code
3. Write tests first
4. Start over with proper TDD

This is not optional. This is not a suggestion. **This is how professional software is built.**

### Git Commit Commands

**CRITICAL: Avoid "STDIN" prefix in commit messages**

The user has `cat` aliased to `bat`, which adds "STDIN" label when reading from heredocs. **Always use `/bin/cat` instead of `cat` in git commit commands.**

**Bad pattern (adds "STDIN" prefix):**
```bash
git commit -m "$(cat <<'EOF'
commit message
EOF
)"
```

**Good pattern (no STDIN prefix):**
```bash
git commit -m "$(/bin/cat <<'EOF'
commit message
EOF
)"
```

**Why:** `cat` is aliased to `bat --style header --style snip --style changes --style header`, and bat labels stdin input as "STDIN".

### Syncthing API Testing with curl

**CRITICAL: Always read API credentials from user config file**

When testing Syncthing API endpoints with curl commands:

1. **Read the config file first**: `~/.config/stui/config.yaml`
2. **Extract `api_key` and `base_url` from the config**
3. **Use those values in curl commands**

**Example workflow:**
```bash
# Read config to get API key and base URL
cat ~/.config/stui/config.yaml

# Then use extracted values in curl
curl -s -H "X-API-Key: <key-from-config>" "<base_url-from-config>/rest/db/need?folder=lok75-7d42r"
```

**NEVER:**
- ❌ Use hardcoded API keys
- ❌ Guess at API credentials
- ❌ Use old/stale credentials from previous sessions

**ALWAYS:**
- ✅ Read config file first
- ✅ Use current credentials from config
- ✅ Verify base_url matches user's setup

### Preferred Tools for File Operations

**Use `ag` (The Silver Surfer) for content search:**
```bash
# Search for text pattern in code
ag "search_term"

# Search in specific file type
ag --rust "pattern"

# Search with context
ag -C 3 "pattern"

# Case-insensitive search
ag -i "pattern"
```

**Use `fd` for finding files:**
```bash
# Find files by name pattern
fd "pattern"

# Find in specific directory
fd "pattern" src/

# Find by file type
fd -e rs      # Rust files
fd -e toml    # TOML files

# Combine with other commands
fd "test" | head -10
```

**Why these tools:**
- `ag`: Faster than grep, respects .gitignore, better syntax highlighting
- `fd`: Faster than find, simpler syntax, respects .gitignore

**AVOID using:**
- ❌ `grep -r` - use `ag` instead
- ❌ `find` - use `fd` instead

### Other Instructions

- If you make a change that doesn't work, do not just keep adding more things on. If a change didn't fix things, consider that and revert it before attempting a new solution.
- Use debug logs for general development and troubleshooting
- Make logging comprehensive but concise - debug logs should be informative without overwhelming
- When adding new features or fixes, git commit once user has confirmed working and tests are written

## CRITICAL: Data Type Constants

**BrowseItem.item_type values** (from Syncthing API):
- Directory: `"FILE_INFO_TYPE_DIRECTORY"` (the ONLY directory type)
- File: Anything else (various file types, but NOT equal to `"FILE_INFO_TYPE_DIRECTORY"`)

**NEVER use**:
- ❌ `"directory"`
- ❌ `"file"`
- ❌ `"dir"`

**ALWAYS use**:
- ✅ `item.item_type == "FILE_INFO_TYPE_DIRECTORY"` for checking directories
- ✅ `item.item_type != "FILE_INFO_TYPE_DIRECTORY"` for checking files

See `src/api.rs:31` for `BrowseItem` struct definition.


## Architecture Context

- **Syncthing Environment**: Runs in Docker container
- **CLI Environment**: Runs on host machine
- **Path Translation**: Container paths differ from host paths; use configurable `path_map` to translate container paths to host paths for file operations
- **Data Source**: Syncthing REST API (not direct filesystem scanning)

## Tech Stack

- **Language**: Rust
- **TUI Framework**: Ratatui
- **Dependencies**:
  - `reqwest` (HTTP client)
  - `serde` / `serde_json` / `serde_yaml` (serialization)
  - `crossterm` (terminal handling)
  - `tokio` (async runtime)
  - `rusqlite` (SQLite cache)
  - `ratatui-image` (terminal image rendering)
  - `image` (image processing and resizing)
  - `anyhow` (error handling)
  - `urlencoding` (URL encoding)
  - `dirs` (directory paths)
  - `glob` (pattern matching)
  - `clap` (CLI argument parsing)
  - `unicode-width` (text width calculations)
  - `codepage-437` (CP437 encoding for ANSI art)

## Core Features

### Display & Navigation

- List folders from `/rest/system/config` with real-time status icons
- Browse folder contents via `/rest/db/browse` with recursive traversal
- Multi-pane breadcrumb navigation showing directory hierarchy
- Keyboard navigation:
  - `↑` / `↓`: Navigate items
  - `Enter`: Preview file (if file) or drill into folder (if directory)
  - `←` / `Backspace`: Go back
  - `q`: Quit

### Sync State Icons

Display visual indicators for file/folder states following `<file|dir><status>` pattern:
- `📄✅` / `📁✅` Synced
- `📄☁️` / `📁☁️` Remote-only
- `📄💻` / `📁💻` Local-only
- `📄⚠️` / `📁⚠️` Out-of-sync OR Ignored (exists on disk)
- `📄⏸` / `📁⏸` Paused
- `📄🚫` / `📁🚫` Ignored (deleted from disk)
- `📄🔄` / `📁🔄` Syncing (actively downloading/uploading)
- `📄❓` / `📁❓` Unknown

### User Actions

- `?` or `Enter` (on files): Show detailed file info popup with metadata (sync state, permissions, device availability) and preview:
  - **Text files**: Scrollable preview with vim keybindings (j/k, gg/G, Ctrl-d/u/f/b, PgUp/PgDn)
  - **ANSI art files**: Full-featured ANSI/ASCII art rendering with auto-detection
    - Auto-detects ANSI codes in any file (ESC[ sequences), not just .ans/.asc extensions
    - CP437 encoding support (original IBM PC character set with box-drawing characters)
    - 80-column automatic wrapping (matching PabloDraw and other ANSI art viewers)
    - Line buffer rendering with proper cursor positioning (CSI `[nC` codes)
    - Full SGR color support (foreground colors 30-37, 90-97; background colors 40-47, 100-107)
    - SAUCE metadata stripping (Ctrl-Z delimiter)
    - Multiple line ending formats (\r\n, \n, \r)
    - Background color stripping from spaces to prevent bleeding
    - Fixed 80-character width container with text wrapping enabled
  - **Image files**: Terminal image rendering with Kitty/iTerm2/Sixel/Halfblocks protocols
    - Auto-detects terminal capabilities and font size
    - Non-blocking background loading (popup appears immediately)
    - Adaptive quality/performance (40-200ms load times)
    - Smart centering and aspect ratio preservation
    - Shows resolution in metadata column
  - **Binary files**: Shows extracted text or metadata
- `c`: **Context-aware action**:
  - **Folder view** (focus_level == 0): Change folder type (Send Only, Send & Receive, Receive Only) with selection menu
  - **Breadcrumb view** (focus_level > 0): Copy file/directory path to clipboard (uses mapped host paths)
- `i`: Toggle ignore state (add/remove from `.stignore`) via `PUT /rest/db/ignores`
- `I`: Ignore AND delete locally (immediate action, no confirmation)
- `o`: Open file/directory with configured command (e.g., `xdg-open`, `code`, `vim`)
- `d`: Delete file/directory from disk (with confirmation prompt)
- `r`: **Rescan folder** - Shows confirmation dialog with options:
  - `y`: Normal rescan - Trigger Syncthing scan, wait for sequence change to invalidate cache
  - `f`: Force refresh - Immediately invalidate cache and trigger rescan (useful for stale cache bugs)
  - `n` or `Esc`: Cancel
- `R`: Restore deleted files (revert receive-only folder)
- `s`: Cycle sort mode (Sync State → A-Z → Timestamp → Size)
- `S`: Toggle reverse sort order
- `t`: Toggle info display (Off → TimestampOnly → TimestampAndSize → Off)
- `p`: Pause/resume folder (folder view only, with confirmation)
- Vim keybindings (optional): `hjkl`, `gg`, `G`, `Ctrl-d/u`, `Ctrl-f/b`

### Search Feature

**Real-time recursive file search** with wildcard support and intelligent filtering:

- **Trigger**: `Ctrl-F` (normal mode) or `/` (vim mode) in breadcrumb view
- **Search Scope**: Recursively searches all cached files/directories in current folder, including deeply nested subdirectories
- **Search Input**:
  - Appears above hotkey legend with `Match: ` prompt and blinking cursor
  - Cyan border when active, gray when inactive
  - Shows match count in title: `Search (3 matches) - Esc to cancel`
- **Real-Time Filtering**: Items filter as you type (no debouncing needed - fast!)
- **Pattern Matching**:
  - Case-insensitive substring matching
  - Wildcard support: `*` matches any sequence of characters
  - Examples: `*jeff*`, `*.txt`, `photo*`
  - Matches file names and path components
- **Intelligent Display**:
  - Shows parent directories if they contain matching descendants
  - Example: Searching "jeff" in root shows "Movies/" if "Movies/Photos/jeff-1.txt" exists
  - Drilling into "Movies/" shows "Photos/" breadcrumb
  - Drilling into "Photos/" shows actual "jeff-1.txt" file
- **Search Persistence**:
  - Query persists when navigating into subdirectories
  - Search automatically applies to new directories
  - **Context-aware clearing**: Only clears when backing out past the directory where search was initiated
    - Start search in `Foo/` → navigate to `Foo/Bar/` → search persists
    - Back out to `Foo/` → search still active
    - Back out to folder list → search clears automatically
- **Exit Options**:
  - `Esc`: Clear search and restore all items
  - `Enter`: Accept search (keep filtering, deactivate input)
  - `Backspace` to empty: Auto-exit and restore all items
- **Performance**: Optimized for speed - handles 1000+ files instantly with recursive search
- **UI Integration**:
  - Search box appears between main content and legend
  - Legend shows mode-specific hotkey: `^F:Search` or `/:Search`
  - During search: `Esc:Exit Search` or `Esc:Clear Search`

**Implementation Details**:
- Uses SQLite cache for fast recursive queries across all subdirectories
- Prefetch system ensures all subdirectories are cached for instant search
- State tracking prevents duplicate API requests with `discovered_dirs` HashSet
- Search origin level tracking (`search_origin_level`) enables context-aware clearing

### Status Bar & UI Elements

**UI Layout (top to bottom):**
- **System Bar** (full width): Device name, uptime, local state summary, transfer rates
- **Main Content**: Folders pane + Breadcrumb panels (horizontal split with smart sizing)
- **Hotkey Legend** (full width): Context-aware key display with text wrapping
- **Status Bar** (full width): Folder state, folder type, data sizes, sync progress, sort mode

**Breadcrumb Layout:**
- Current folder gets 50-60% of screen width for better visibility
- Parent folders share remaining 40-50% equally
- All ancestor breadcrumbs remain highlighted (blue border) when drilling deeper
- Current breadcrumb has cyan border + `> ` arrow

**Other UI Features:**
- **Last Update Display**: Shows timestamp and filename of most recent change per folder
- **File Info Display**: Three-state toggle showing timestamp and/or size (human-readable: `1.2K`, `5.3M`, etc.)
  - Off: No info displayed
  - TimestampOnly: Shows modification time (e.g., `2025-10-26 20:58`)
  - TimestampAndSize: Shows size + timestamp for files (e.g., `1.2M 2025-10-26 20:58`), timestamp only for directories
- **Smart Hotkey Legend**: Context-aware display
  - Folder view: Shows navigation, Change Type, Pause/Resume, Rescan, Quit
  - Breadcrumb view: Shows all keys including file operations (Copy, Sort, Info, Ignore, Delete)
  - Restore only appears when folder has local changes (receive_only_total_items > 0)
  - Text wrapping enabled (wraps within fixed 3-line height on narrow terminals)
  - Scrollbar indicators: Automatically appear on breadcrumb panels when content exceeds viewport height
- **Folder Type Display**: Status bar shows folder type (Send Only, Send & Receive, Receive Only) before state field
- **Confirmation Dialogs**: For destructive operations (delete, revert, ignore+delete)
- **Sorting**: Multi-mode sorting (Sync State/A-Z/Timestamp/Size) with visual indicators in status bar and toast notifications, directories always sorted first

### Configuration

YAML config file at platform-specific location (Linux: `~/.config/stui/config.yaml`, macOS: `~/Library/Application Support/stui/config.yaml`, Windows: `%APPDATA%\stui\config.yaml`) containing:
- API key
- Base URL
- `path_map` (container-to-host path translations)
- `vim_mode` (optional, boolean to enable vim keybindings)
- `icon_mode` (optional, string): Icon rendering mode - `"nerdfont"` or `"emoji"` (default: `"nerdfont"`)
- `open_command` (optional, string): Command to execute for opening files/directories (e.g., `xdg-open`, `code`, `vim`)
- `clipboard_command` (optional, string): Command to copy text to clipboard via stdin (e.g., `wl-copy`, `xclip`, `pbcopy`)
- **Image Preview Settings**:
  - `image_preview_enabled` (boolean, default: `true`): Enable/disable image preview
  - `image_protocol` (string, default: `"auto"`): Terminal graphics protocol - `"auto"`, `"kitty"`, `"iterm2"`, `"sixel"`, or `"halfblocks"`

CLI flags:
- `--debug`: Enable debug logging to `/tmp/stui-debug.log` (includes image loading performance metrics)
- `--vim`: Enable vim keybindings (overrides config file setting)
- `--config <path>`: Specify custom config file path

### Safety Features

- Confirmation prompts for destructive actions
- Optional folder pause before deletions
- Path mapping validation

## Syncthing REST API Endpoints

```
/rest/system/config                           # Get folders and devices
/rest/config/folders/<id>                     # PATCH to modify folder config (e.g., pause/resume)
/rest/db/status?folder=<id>                   # Folder sync status (with sequence numbers)
/rest/db/browse?folder=<id>[&prefix=subdir/]  # Browse contents
/rest/db/file?folder=<id>&file=<path>         # Get file sync details
/rest/db/ignores?folder=<id>                  # Get/set .stignore rules
/rest/db/scan?folder=<id>                     # Trigger folder rescan
/rest/db/revert?folder=<id>                   # Revert receive-only folder
/rest/events?since=<id>&timeout=60            # Event stream (long-polling, IMPLEMENTED)
/rest/system/status                           # System status (device info, uptime)
/rest/system/connections                      # Connection/transfer statistics
```

## Architecture Highlights

### Code Organization

**Main Application Structure:**
- `src/main.rs` (~3,300 lines) - App struct, main event loop, orchestration methods
- `src/handlers/` - Event handlers (keyboard, API responses, cache events)
  - `keyboard.rs` - All keyboard input handling with confirmation dialogs
  - `api.rs` - API response processing and state updates
  - `events.rs` - Event stream processing and cache invalidation
- `src/services/` - Background services (API queue, event listener)
  - `api.rs` - Async API service with priority queue and request deduplication
  - `events.rs` - Long-polling event listener for real-time updates
- `src/model/` - Pure application state (Elm Architecture)
  - `mod.rs` - Main Model struct with helper methods
  - `syncthing.rs` - Syncthing data (folders, devices, statuses)
  - `navigation.rs` - Breadcrumb trail and focus state
  - `ui.rs` - UI preferences, dialogs, popups
  - `performance.rs` - Loading tracking, metrics, pending operations
  - `types.rs` - Shared types (BreadcrumbLevel, FileInfoPopupState, etc.)
- `src/logic/` - Pure business logic functions (18 functions, 110+ tests)
  - `file.rs` - File type detection, binary detection, ANSI art parsing with CP437 encoding
  - `folder.rs` - Folder validation and state checking
  - `formatting.rs` - Data formatting (uptime, file sizes)
  - `ignore.rs` - Pattern matching for .stignore
  - `layout.rs` - UI layout calculations
  - `navigation.rs` - Selection navigation logic
  - `path.rs` - Path translation (container ↔ host)
  - `performance.rs` - Batching and throttling logic
  - `search.rs` - Search pattern matching (wildcards, case-insensitive)
  - `sync_states.rs` - Sync state priority and validation
  - `ui.rs` - UI state transitions (display modes, sort, vim commands)
- `src/ui/` - Rendering components (12 modules)
  - `render.rs` - Main render coordinator
  - `folder_list.rs` - Folder list panel with status icons
  - `breadcrumb.rs` - Breadcrumb navigation panels with scrollbars
  - `dialogs.rs` - Confirmation dialogs and popups
  - `icons.rs` - Icon rendering (emoji/nerdfont) with color themes
  - `legend.rs` - Context-aware hotkey legend
  - `search.rs` - Search input box with real-time filtering
  - `status_bar.rs` - Bottom status bar with sync info
  - `system_bar.rs` - Top system bar with device info
  - `layout.rs` - Panel layout calculations
- `src/api.rs` - Syncthing API client (all REST endpoints)
- `src/cache.rs` - SQLite cache for browse results and sync states
- `src/state.rs` - Legacy state types (being phased out)

**Additional Documentation:**
- See [RATATUI_NOTES.md](RATATUI_NOTES.md) for comprehensive Ratatui UI implementation patterns, constraint system behavior, dynamic height calculations, and best practices

**Key Application Patterns:**
- **App initialization** (`main.rs:361-480`): `App::new()` loads folders via `client.get_folders()`, spawns API service and event listener
- **Main event loop** (`main.rs:2973-3150`): Always-render pattern, processes API responses, keyboard input, cache events, image updates
- **Keyboard handling** (`handlers/keyboard.rs`): Top-level match statement with confirmation dialogs processed first
- **State updates**: `model.syncthing.folders` updated via `client.get_folders()` after mutations (pause/resume, etc.)

**CRITICAL Architecture Rules:**
1. **UI Side Effects (toasts, dialogs) MUST be in `handlers/keyboard.rs`**
   - ❌ WRONG: Calling `show_toast()` in helper methods in `main.rs`
   - ✅ CORRECT: Calling `show_toast()` in keyboard handler where user action happens
   - Helper methods in `main.rs` should only do business logic (update state, call APIs)
   - All user feedback (toasts, error messages) belongs at the call site in keyboard handler

2. **Separation of Concerns:**
   - `src/api.rs`: Pure API client methods (no UI, no state mutation beyond return values)
   - `src/handlers/keyboard.rs`: Keyboard events → business logic → UI feedback (toasts, dialogs)
   - `src/main.rs`: Orchestration methods (pure business logic, no UI side effects)
   - `src/model/`: Pure state (cloneable, no side effects, no I/O)
   - `src/logic/`: Pure functions (testable, no state mutation, no I/O)
   - `src/ui/`: Pure rendering (takes state, returns widgets, no mutation)

3. **Adding UI Feedback Pattern:**
   ```rust
   // ❌ WRONG - toast in helper method
   fn cycle_sort_mode(&mut self) {
       self.model.ui.sort_mode = new_mode;
       self.model.ui.show_toast("Sort changed"); // WRONG!
   }

   // ✅ CORRECT - toast at call site
   KeyCode::Char('s') => {
       app.cycle_sort_mode(); // Pure business logic
       app.model.ui.show_toast(format!("Sort: {}", app.model.ui.sort_mode.as_str())); // UI feedback here
   }
   ```

### Event-Driven Cache Invalidation
- Long-polling `/rest/events` endpoint for real-time updates
- Granular invalidation: file-level, directory-level, folder-level
- Event types handled:
  - `LocalIndexUpdated` (local changes with `filenames` array)
  - `ItemStarted` (sync begins - shows Syncing state)
  - `ItemFinished` (sync completion)
  - `LocalChangeDetected`, `RemoteChangeDetected`
- Persistent event ID across app restarts
- Auto-recovery from stale event IDs (resets to 0 if high ID returns nothing)

### Performance Optimizations
- **Async API Service**: Channel-based request queue with priority levels (High/Medium/Low)
- **Cache-First Rendering**: SQLite cache for instant display, background updates
- **Sequence-Based Validation**: Only invalidates cache when Syncthing data changes
- **Non-Blocking Operations**: All background tasks run async without freezing UI
- **Request Deduplication**: Prevents duplicate in-flight API calls
- **Idle Detection & Non-Blocking UI**: 300ms idle threshold ensures keyboard input is never blocked by background prefetch operations; main event loop uses 250ms poll timeout to minimize CPU wakeups (~<1-2% CPU when idle)

### Caching Strategy
- **SQLite database**: `~/.cache/stui/cache.db` (Linux) or `/tmp/stui-cache` (fallback)
- **Browse cache**: Directory listings with folder sequence validation, includes `mod_time` and `size` fields
- **Sync state cache**: Per-file sync states with file sequence validation
- **Folder status cache**: Status with sequence, displayed stats (in_sync/total items)
- **Event ID persistence**: Survives app restarts
- **Schema migrations**: Manual cache clear required when database schema changes (`rm ~/.cache/stui/cache.db`)

### ANSI Art Rendering
- **Auto-Detection**: Content-based detection of ANSI escape codes (ESC[ sequences) in addition to .ans/.asc extensions
  - `contains_ansi_codes()` function scans file bytes for ESC[ followed by valid ANSI parameters
  - Enables ANSI rendering for any file containing ANSI codes, regardless of extension
- **CP437 Encoding**: Uses `codepage-437` crate to decode original IBM PC character set (box-drawing, extended ASCII)
- **80-Column Wrapping**: Standard ANSI art format - automatically wraps at column 80 (matching PabloDraw, Ansilove)
- **Line Buffer Algorithm**: Fixed-width buffer (80 chars) with cursor positioning support
  - Characters written at absolute column positions
  - Cursor forward (`ESC[nC`) moves position without inserting characters
  - When buffer reaches 80 chars, flushes as new line and resets
  - Later content can overwrite earlier content at same position
- **SGR Color Support**: Full ANSI color codes
  - Foreground: 30-37 (standard), 90-97 (bright)
  - Background: 40-47 (standard), 100-107 (bright)
  - Modifiers: bold (1), italic (3), underline (4)
- **SAUCE Metadata**: Strips binary metadata block after Ctrl-Z character (0x1A)
- **Line Endings**: Normalizes \r\n, \n, and \r to uniform format
- **Background Stripping**: Removes background colors from spacing characters to prevent bleeding
- **Fixed-Width Container**: 83 columns (80 + 3 for borders/padding), centered horizontally
- **Text Wrapping**: Enabled to handle lines that exceed 80 characters

### State Transition Validation
- **Logic-Based Protection**: Validates state transitions based on user actions, not arbitrary timeouts
- **Action Tracking**: `ManualStateChange` struct tracks what action was performed (SetIgnored/SetUnignored) with timestamp
- **Transition Rules**:
  - After **SetIgnored**: Only accept `Ignored` state (reject stale Synced/RemoteOnly/etc)
  - After **SetUnignored**: Accept any state except `Ignored` (reject stale Ignored state)
- **Safety Valve**: 10-second timeout prevents permanent blocking in edge cases
- **No Race Conditions**: Works regardless of network latency or event timing
- **Syncing State Tracking**: `syncing_files` HashSet tracks actively syncing files between ItemStarted/ItemFinished events

## Current State & Test Coverage

**Architecture Quality:**
- ✅ Clean separation: Model (pure state) vs Runtime (services)
- ✅ Pure business logic extracted (18 functions, 110+ tests)
- ✅ 169 total tests passing (110+ logic + 43+ model + 16 misc)
- ✅ Zero warnings, production-ready code
- ✅ Comprehensive refactoring complete (see ELM_REWRITE_PREP.md)
- ✅ Full ANSI art support with CP437 encoding and 80-column wrapping

### Known Limitations
- No async loading spinners (planned)
- No batch operations for multi-select
- Error handling and timeout management needs improvement

### Planned Features
- File type filtering and ignored-only view
- Event history viewer with persistent logging
- Optional filesystem diff view
- Batch operations (multi-select for ignore/delete/rescan)
- Cross-platform packaging (Linux, macOS, Windows)
- Better error states, handling, and timeout management

## Development Guidelines

### Safety & Best Practices
- **Safety First**: All destructive operations require confirmation (except `I` which is intentionally immediate)
- **Path Mapping**: Always translate container paths to host paths before file operations
- **Error Handling**: Graceful degradation, show errors in status bar or toast messages
- **Non-Blocking**: Keep UI responsive during all API calls
- **Cache Coherency**: Use sequence numbers to validate cached data
- **Testing - CRITICAL REQUIREMENT**:
  - **Test-Driven Development is MANDATORY** (see top of file for detailed TDD workflow)
  - **ALWAYS write tests when:**
    1. Adding new features (especially state management)
    2. Fixing bugs or edge cases
    3. Refactoring existing code
    4. Adding new model fields or business logic
  - **Test-Driven Development Pattern:**
    1. User reports bug or requests feature
    2. **IMMEDIATELY think: "What tests do I need?"**
    3. Write tests FIRST that cover:
       - Happy path (expected behavior)
       - Edge cases (boundary conditions)
       - Error cases (what happens when things go wrong)
       - State transitions (before/after)
    4. Implement the feature/fix
    5. Run tests to verify
    6. If tests fail, fix implementation (not tests)
  - **Test Coverage Requirements:**
    - Model state changes → tests in `src/model/*/tests`
    - Business logic → tests in `src/logic/*/tests`
    - Integration tests → `tests/*.rs` files (see `tests/reconnection_test.rs`)
    - Aim for 100% coverage of new code paths
  - **Real-World Success Story - Commit fcf4362:**
    - Problem: Folders not populating after reconnection (cost $20 debugging blind)
    - TDD Approach: Wrote 10 tests first exposing exact bug
    - Test `test_state_already_connected_before_system_status` revealed root cause
    - Solution: Simple 1-line fix guided by tests
    - Result: All 184 tests pass, bug fixed perfectly on first try
    - **Lesson: TDD saves time and money**
  - **When Claude forgets to write tests:**
    - User should immediately call it out
    - Claude should apologize and write tests before proceeding
    - This is a critical discipline for production code quality
  - **Existing test guidelines:**
    - Test with real Syncthing Docker instances with large datasets
    - Pure business logic in `src/logic/` should have comprehensive test coverage
    - Model state transitions should have tests in corresponding test modules
    - Run `cargo test` before committing to ensure all 184+ tests pass
    - Aim for zero compiler warnings (`cargo build` should be clean)
  - **Test Organization Standards:**
    - **Keep tests inline** using `#[cfg(test)] mod tests` at the bottom of each module
    - **Use section headers** for visual organization when files have >10 tests:
      ```rust
      // ========================================
      // SECTION NAME
      // ========================================
      ```
    - **Group tests logically** by feature/function being tested
    - **When to reorganize:**
      - File has >20 tests and they're randomly ordered → Major reorganization
      - File has >10 tests but well-ordered → Add section headers only
      - File has <10 tests → No changes needed
    - **Examples of well-organized test modules:**
      - `src/logic/file.rs` - 35 tests in 5 sections (Image Detection, Binary Detection, ANSI Code Detection, ANSI Parsing, Binary Text Extraction)
      - `src/logic/ignore.rs` - 13 tests in 4 sections (Pattern Matching, Find Matching, Validation Valid/Invalid/Edge Cases)
      - `src/model/ui.rs` - 16 tests in 4 sections (UI Model Creation, Search Mode, Search Query Operations, Search Origin Level)
      - `src/logic/navigation.rs` - 14 tests in 4 sections (Next Selection, Prev Selection, Edge Cases, Find Item By Name)
    - **Benefits:** Tests can be collapsed by section in IDEs, clear grouping makes finding related tests easy, maintains locality with implementation code
- **Debug Mode**: Use `--debug` flag for verbose logging to `/tmp/stui-debug.log`

### Adding New Features - Common Patterns

**Adding a new API endpoint:**
1. Add method to `SyncthingClient` in `src/api.rs` (follow existing patterns)
2. Add request type to `ApiRequest` enum in `src/services/api.rs` if using async service
3. Add response type to `ApiResponse` enum in `src/services/api.rs`
4. Add handler in `src/handlers/api.rs` to process response

**Adding a new keybinding:**
1. Add state to `Model` (usually `model.ui` for dialogs/popups)
2. Add keybinding handler in `src/handlers/keyboard.rs`
   - Confirmation dialogs go at top of match statement (processed first)
   - Regular keys go in main match block with conditional guards (e.g., `focus_level == 0`)
3. Add dialog rendering in `src/ui/dialogs.rs` (if confirmation needed)
4. Add rendering call in `src/ui/render.rs`
5. Update hotkey legend in `src/ui/legend.rs` with context guards

**Example 1: Pause/Resume Feature (confirmation dialog pattern)**
- API: `src/api.rs` - `set_folder_paused()` using PATCH `/rest/config/folders/{id}`
- State: `model.ui.confirm_pause_resume: Option<(folder_id, label, is_paused)>`
- Keybinding: `KeyCode::Char('p') if focus_level == 0` opens confirmation
- Confirmation: Handles 'y' (execute), 'n'/Esc (cancel)
- Execution: Call API, reload folders via `client.get_folders()`, update `model.syncthing.folders`
- Dialog: `render_pause_resume_confirmation()` with color-coded borders
- Legend: Shows "p:Pause/Resume" only in folder view
- Visual: Pause icon (⏸ emoji /  nerdfont) via `FolderState::Paused`

**Example 2: Change Folder Type (selection menu pattern)**
- API: `src/api.rs` - `set_folder_type()` using PATCH `/rest/config/folders/{id}` with `{"type": "sendonly|sendreceive|receiveonly"}`
- Data: `Folder` struct has `folder_type: String` field (serde renamed from "type")
- State: `model.ui.folder_type_selection: Option<FolderTypeSelectionState>` with `folder_id`, `folder_label`, `current_type`, `selected_index`
- Keybinding: `KeyCode::Char('c') if focus_level == 0` opens selection menu (context-aware - 'c' copies path in breadcrumbs)
- Selection Menu:
  - Uses `List` widget with ↑↓ navigation, Enter to select, Esc to cancel
  - Current type highlighted in cyan/italic
  - Handler at top of keyboard.rs (lines 317-384)
- Execution: Call API, reload folders, update `model.syncthing.folders`, show toast
- Dialog: `render_folder_type_selection()` shows 3 options with user-friendly names
- Legend: Shows "c:Change Type" only in folder view
- Status Bar: Displays folder type (Send Only, Send & Receive, Receive Only) before state field

**State management patterns:**
- **Model fields**: All application state lives in `Model` struct (pure, cloneable)
- **Runtime fields**: Services, channels, caches in `App` struct (not cloneable)
- **State updates**: Mutate `app.model.*` directly, reload from API when needed
- **Toast messages**: `app.model.ui.show_toast()` for user feedback
- **Modal dialogs**: Set `model.ui.confirm_*` field, handled at top of keyboard handler

**UI rendering patterns:**
- **Icon rendering**: Use `IconRenderer` with `FolderState` or `SyncState` enums
- **Scrollbars**: Automatically rendered by breadcrumb panels using Ratatui's `Scrollbar` widget
- **Context-aware display**: Check `focus_level` to show/hide keys in legend
- **Color coding**: Use `Color::Cyan` (focused), `Color::Blue` (parent), `Color::Gray` (inactive)
- **Text wrapping**:
  - Legend uses `.wrap(ratatui::widgets::Wrap { trim: false })` for text wrapping
  - Fixed height of 3 lines (`Constraint::Length(3)`) - wraps content within available space
  - System bar and status bar also use `Constraint::Length(3)` (fixed height)
  - Note: On very narrow terminals, some hotkeys may be clipped if content exceeds 3 lines
