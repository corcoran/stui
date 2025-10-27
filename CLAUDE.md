# Syncthing CLI TUI Manager

## Project Overview

Building a Rust Ratatui CLI that manages Syncthing via its REST API — listing folders, showing contents, adding .stignore rules, and deleting directories safely with Docker path-mapping support.

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

Display visual indicators for file/folder states:
- `✅` Synced
- `☁️` Remote-only
- `💻` Local-only
- `⚠️` Out-of-sync
- `⏸` Paused
- `🚫⚠️` Ignored (exists on disk)
- `🚫..` Ignored (doesn't exist)
- `🔄` Loading/Unknown

### User Actions

- `i`: Toggle ignore state (add/remove from `.stignore`) via `PUT /rest/db/ignores`
- `I`: Ignore AND delete locally (immediate action, no confirmation)
- `d`: Delete file/directory from disk (with confirmation prompt)
- `r`: Rescan folder via `POST /rest/db/scan`
- `R`: Restore deleted files (revert receive-only folder)
- `s`: Cycle sort mode (Icon → A-Z → DateTime → Size)
- `S`: Toggle reverse sort order
- `t`: Toggle info display (Off → TimestampOnly → TimestampAndSize → Off)
- `p`: Pause/resume folder (planned)
- Vim keybindings (optional): `hjkl`, `gg`, `G`, `Ctrl-d/u`, `Ctrl-f/b`

### Status Bar & UI Elements

- **Status Bar**: Shows folder state, data sizes, sync progress, in_sync/total items, current sort mode
- **Last Update Display**: Shows timestamp and filename of most recent change per folder
- **File Info Display**: Three-state toggle showing timestamp and/or size (human-readable: `1.2K`, `5.3M`, etc.)
  - Off: No info displayed
  - TimestampOnly: Shows modification time (e.g., `2025-10-26 20:58`)
  - TimestampAndSize: Shows size + timestamp for files (e.g., `1.2M 2025-10-26 20:58`), timestamp only for directories
- **Hotkey Legend**: Wrapping legend at bottom of breadcrumb panels showing all available keys (dynamic based on vim mode)
- **Confirmation Dialogs**: For destructive operations (delete, revert, ignore+delete)
- **Sorting**: Multi-mode sorting (Icon/A-Z/DateTime/Size) with visual indicators in status bar, directories always sorted first

### Configuration

YAML config file (currently `./config.yaml`, planned: `~/.config/synctui/config.yaml`) containing:
- API key
- Base URL
- `path_map` (container-to-host path translations)
- `vim_mode` (optional, boolean to enable vim keybindings)

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
  - `ItemFinished` (sync completion)
  - `LocalChangeDetected`, `RemoteChangeDetected`
- Persistent event ID across app restarts

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
- Config file at `~/.config/synctui/config.yaml` with CLI override
- Pause / Resume folder toggle hotkey + status (with confirmation)
- Change Folder Type toggle hotkey + status (with confirmation)
- File type filtering and ignored-only view
- Event history viewer with persistent logging
- File preview (text files and CLI image rendering)
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
