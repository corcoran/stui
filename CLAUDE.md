# Syncthing CLI TUI Manager

## Project Overview

Building a Rust Ratatui CLI that manages Syncthing via its REST API — listing folders, showing contents, adding .stignore rules, and deleting directories safely with Docker path-mapping support.
What makes this app unique is the file management part (breadcrumbs, directories, files) part. Being able to navigate your files and selectively ignore and see if the file is ignored but still exists on the disk is a feature missing across most Syncthing apps. It is one of the most important features and should be modified with care ONLY.

## claude instructions

- If you make a change that doesn't work, do not just keep adding more things on. If a change didn't fix things, consider that and revert it before attempting a new solution.
- Use debug logs for general development and troubleshooting; use --bug logs sparingly for specific issues that need reproduction
- Make logging comprehensive but concise - debug logs should be informative without overwhelming
- Always clean up `bug` logs for one-off issues but keep helpful logs (convert into debug) that may be used later


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
  - `serde` (serialization)
  - `crossterm` (terminal handling)

## Core Features

### Display & Navigation

- List folders from `/rest/system/config` with real-time status icons
- Browse folder contents via `/rest/db/browse` with recursive traversal
- Multi-pane breadcrumb navigation showing directory hierarchy
- Keyboard navigation:
  - `↑` / `↓`: Navigate items
  - `Enter`: Drill into folder
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

- `?`: Show detailed file info popup with metadata (sync state, permissions, device availability) and text preview (scrollable with vim keybindings)
- `c`: Copy folder ID (folders) or file/directory path (files/folders, uses mapped host paths) to clipboard
- `i`: Toggle ignore state (add/remove from `.stignore`) via `PUT /rest/db/ignores`
- `I`: Ignore AND delete locally (immediate action, no confirmation)
- `o`: Open file/directory with configured command (e.g., `xdg-open`, `code`, `vim`)
- `d`: Delete file/directory from disk (with confirmation prompt)
- `r`: Rescan folder via `POST /rest/db/scan`
- `R`: Restore deleted files (revert receive-only folder)
- `s`: Cycle sort mode (Icon → A-Z → DateTime → Size)
- `S`: Toggle reverse sort order
- `t`: Toggle info display (Off → TimestampOnly → TimestampAndSize → Off)
- `p`: Pause/resume folder (planned)
- Vim keybindings (optional): `hjkl`, `gg`, `G`, `Ctrl-d/u`, `Ctrl-f/b`

### Status Bar & UI Elements

**UI Layout (top to bottom):**
- **System Bar** (full width): Device name, uptime, local state summary, transfer rates
- **Main Content**: Folders pane + Breadcrumb panels (horizontal split with smart sizing)
- **Hotkey Legend** (full width): Context-aware key display
- **Status Bar** (full width): Folder state, data sizes, sync progress, sort mode

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
  - Folder view: Shows navigation, Rescan, Quit (hides Sort, Info, Ignore, Delete)
  - Breadcrumb view: Shows all keys including file operations
  - Restore only appears when folder has local changes (receive_only_total_items > 0)
- **Confirmation Dialogs**: For destructive operations (delete, revert, ignore+delete)
- **Sorting**: Multi-mode sorting (Icon/A-Z/DateTime/Size) with visual indicators in status bar, directories always sorted first

### Configuration

YAML config file (currently `./config.yaml`, planned: `~/.config/synctui/config.yaml`) containing:
- API key
- Base URL
- `path_map` (container-to-host path translations)
- `vim_mode` (optional, boolean to enable vim keybindings)
- `open_command` (optional, string): Command to execute for opening files/directories (e.g., `xdg-open`, `code`, `vim`)
- `clipboard_command` (optional, string): Command to copy text to clipboard via stdin (e.g., `wl-copy`, `xclip`, `pbcopy`).

CLI flags:
- `--debug`: Enable debug logging to `/tmp/synctui-debug.log`
- `--vim`: Enable vim keybindings (overrides config file setting)

### Safety Features

- Confirmation prompts for destructive actions
- Optional folder pause before deletions
- Path mapping validation

## Syncthing REST API Endpoints

```
/rest/system/config                          # Get folders and devices
/rest/db/status?folder=<id>                  # Folder sync status (with sequence numbers)
/rest/db/browse?folder=<id>[&prefix=subdir/] # Browse contents
/rest/db/file?folder=<id>&file=<path>        # Get file sync details
/rest/db/ignores?folder=<id>                 # Get/set .stignore rules
/rest/db/scan?folder=<id>                    # Trigger folder rescan
/rest/db/revert?folder=<id>                  # Revert receive-only folder
/rest/events?since=<id>&timeout=60           # Event stream (long-polling, IMPLEMENTED)
```

## Architecture Highlights

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
- **SQLite database**: `~/.cache/synctui/cache.db` (Linux) or `/tmp/synctui-cache` (fallback)
- **Browse cache**: Directory listings with folder sequence validation, includes `mod_time` and `size` fields
- **Sync state cache**: Per-file sync states with file sequence validation
- **Folder status cache**: Status with sequence, displayed stats (in_sync/total items)
- **Event ID persistence**: Survives app restarts
- **Schema migrations**: Manual cache clear required when database schema changes (`rm ~/.cache/synctui/cache.db`)

### State Transition Validation
- **Logic-Based Protection**: Validates state transitions based on user actions, not arbitrary timeouts
- **Action Tracking**: `ManualStateChange` struct tracks what action was performed (SetIgnored/SetUnignored) with timestamp
- **Transition Rules**:
  - After **SetIgnored**: Only accept `Ignored` state (reject stale Synced/RemoteOnly/etc)
  - After **SetUnignored**: Accept any state except `Ignored` (reject stale Ignored state)
- **Safety Valve**: 10-second timeout prevents permanent blocking in edge cases
- **No Race Conditions**: Works regardless of network latency or event timing
- **Syncing State Tracking**: `syncing_files` HashSet tracks actively syncing files between ItemStarted/ItemFinished events

## Current Limitations & Future Goals

### Known Limitations
- Pause/resume folder not yet implemented
- No async loading spinners (planned)
- No filtering by file type or name (planned)
- No batch operations for multi-select
- Config file location hardcoded to `./config.yaml` (needs `~/.config/synctui/` support)
- Error handling and timeout management needs improvement
- Code needs refactoring for better modularity and readability
- No comprehensive test suite yet

### Planned Features
- Pause / Resume folder toggle hotkey + status (with confirmation)
- Change Folder Type toggle hotkey + status (with confirmation)
- File type filtering and ignored-only view
- Event history viewer with persistent logging
- Image preview in file info popup (CLI rendering)
- Optional filesystem diff view
- Batch operations (multi-select for ignore/delete/rescan)
- Configurable keybindings via YAML/TOML
- Cross-platform packaging (Linux, macOS, Windows)
- Comprehensive test suite
- Better error states, handling, and timeout management
- Code refactoring for improved modularity

## Development Guidelines

- **Safety First**: All destructive operations require confirmation (except `I` which is intentionally immediate)
- **Path Mapping**: Always translate container paths to host paths before file operations
- **Error Handling**: Graceful degradation, show errors in status bar
- **Non-Blocking**: Keep UI responsive during all API calls
- **Cache Coherency**: Use sequence numbers to validate cached data
- **Testing**: Test with real Syncthing Docker instances with large datasets
- **Debug Mode**: Set `DEBUG_MODE` environment variable for verbose logging to `/tmp/synctui-debug.log`
