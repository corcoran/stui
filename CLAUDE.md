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

- List folders from `/rest/system/config`
- Browse folder contents via `/rest/db/browse`
- Keyboard navigation:
  - `↑` / `↓`: Navigate items
  - `Enter`: Drill into folder
  - `q`: Quit

### Sync State Icons

Display visual indicators for file/folder states:
- `✅` Synced
- `☁️` Remote-only
- `💻` Local-only
- `⚠️` Out-of-sync
- `⏸` Paused

### User Actions

- `i`: Toggle directory in `.stignore` via `PUT /rest/db/ignores`
- `I`: Add to `.stignore` AND delete locally (with confirmation prompt)
- `r`: Rescan folder via `POST /rest/db/scan`
- `p`: Pause/resume folder

### Configuration

YAML config file containing:
- API key
- Base URL
- `path_map` (container-to-host path translations)

### Safety Features

- Confirmation prompts for destructive actions
- Optional folder pause before deletions
- Path mapping validation

## Syncthing REST API Endpoints

```
/rest/system/config              # Get folders and devices
/rest/db/status?folder=<id>      # Folder sync status
/rest/db/browse?folder=<id>[&prefix=subdir/]  # Browse contents
/rest/db/ignores?folder=<id>     # Get/set .stignore rules
/rest/db/scan?folder=<id>        # Trigger folder rescan
/rest/events                     # Event stream (future: live updates)
```

## Future Goals

- Live updates via `/rest/events` streaming
- Optional filesystem diff view
- Cross-platform support (Linux, macOS)

## Development Guidelines

- Prioritize safety for destructive operations
- Handle API errors gracefully
- Validate path mappings before file deletions
- Keep TUI responsive during API calls
- Test with real Syncthing Docker instances
