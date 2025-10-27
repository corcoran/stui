# Synctui

A fast, keyboard-driven terminal UI for managing [Syncthing](https://syncthing.net/) — browse folders, track sync states, manage ignore patterns, and control your files, all from the comfort of your terminal.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)

## Features

### 🚀 Real-Time Sync Monitoring
- **Live Status Updates**: Automatic cache invalidation using Syncthing's event stream
- **Visual Sync States**: Instant feedback with pastel-colored icons (synced, remote-only, local-only, out-of-sync, ignored)
- **Icon Modes**: Choose between emoji or Nerd Fonts icons via config
- **Smart Caching**: SQLite-backed cache for instant UI responsiveness with background updates
- **System Status Bar**: Device name, uptime, local state summary, and live transfer rates

### 📁 File & Folder Management
- **Multi-Pane Navigation**: Breadcrumb-style directory traversal with independent panels
- **Flexible Sorting**: Sort by sync state, name, timestamp, or size with one keypress
- **File Info Display**: Toggle between no info, timestamps only, or timestamps + human-readable sizes
- **Ignore Management**: Add or remove files from `.stignore` patterns interactively
- **Safe Deletions**: Confirmation prompts for all destructive operations

### ⌨️ Keyboard-First Interface
- **Arrow Key Navigation** (default) or **Vim Keybindings** (optional)
- **Multi-Mode Sorting**: `s` to cycle modes, `S` to reverse order
- **Info Toggle**: `t` cycles through Off → Timestamp → Size+Timestamp
- **Quick Actions**: Single-key commands for ignore, delete, rescan, restore
- **Wrapping Hotkey Legend**: Always visible, adapts to show active keybindings

### 🎯 Smart Features
- **Responsive Navigation**: Instant keyboard input with idle-aware background caching
- **Docker Path Mapping**: Automatic translation between container and host paths
- **Directory-Aware Display**: File sizes shown for files only, omitted for directories
- **Unicode-Aware Rendering**: Proper alignment even with emoji icons
- **Graceful Truncation**: Smart text trimming when terminal width is limited

## Installation

### From Source

```bash
git clone https://github.com/yourusername/synctui.git
cd synctui
cargo build --release
sudo cp target/release/synctui /usr/local/bin/
```

### Prerequisites
- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- A running Syncthing instance (local or remote)

## Configuration

Create `~/.config/synctui/config.yaml`:

```yaml
api_key: "your-syncthing-api-key"
base_url: "http://127.0.0.1:8384"

# Optional: Icon display mode ("emoji" or "nerdfont")
icon_mode: "nerdfont"

# Optional: Map container paths to host paths (for Docker setups)
path_map:
  "/data": "/home/user/syncthing-data"
  "/photos": "/mnt/photos"

# Optional: Enable vim keybindings by default
vim_mode: false
```

### Finding Your Syncthing API Key

1. Open Syncthing Web UI (usually http://127.0.0.1:8384)
2. Go to **Actions** → **Settings** → **General**
3. Copy the **API Key** value

### Docker Setup Example

If Syncthing runs in Docker with volume mounts, configure `path_map` to translate paths:

```yaml
path_map:
  "/data": "/home/user/Sync"        # Container path → Host path
  "/media": "/mnt/external/media"   # Multiple mappings supported
```

This allows synctui (running on host) to perform file operations on the correct paths.

## Usage

### Basic Commands

```bash
# Start synctui (reads config from ~/.config/synctui/config.yaml)
synctui

# Enable vim keybindings for this session
synctui --vim

# Enable debug logging
synctui --debug
```

### Navigation Keys

**Standard Navigation:**
- `↑` / `↓` — Navigate items
- `Enter` / `→` — Enter directory or open folder
- `←` / `Backspace` — Go back to parent directory
- `PageUp` / `PageDown` — Scroll by page (hidden feature)
- `Home` / `End` — Jump to first/last item (hidden feature)

**Vim Keybindings** (when enabled):
- `h` / `j` / `k` / `l` — Navigate left/down/up/right
- `gg` — Jump to first item
- `G` — Jump to last item
- `Ctrl-d` / `Ctrl-u` — Half-page down/up
- `Ctrl-f` / `Ctrl-b` — Full-page down/up

### Action Keys

| Key | Action | Confirmation |
|-----|--------|--------------|
| `i` | Toggle ignore pattern (add/remove from `.stignore`) | No |
| `I` | Ignore AND delete from disk | No (immediate) |
| `d` | Delete file/directory from disk | Yes |
| `r` | Rescan folder (refresh from disk) | No |
| `R` | Restore deleted files (revert receive-only folder) | Yes |
| `s` | Cycle sort mode (Icon → A-Z → DateTime → Size) | No |
| `S` | Reverse current sort order | No |
| `t` | Toggle info display (Off → Timestamp → Size+Timestamp) | No |
| `q` | Quit synctui | No |

### Display Modes

Press `t` to cycle through three information display modes:

1. **Off**: Clean view with just filenames and sync icons
2. **Timestamp Only**: Shows modification times (e.g., `2025-10-26 20:58`)
3. **Timestamp + Size**: Shows file sizes and timestamps (e.g., `1.2M 2025-10-26 20:58`)

File sizes are displayed in human-readable format:(e.g. `1.2K`, `5.3M`, `2.1G`, etc)

### Sorting

Press `s` to cycle through sort modes:
- **Icon** (Sync State): Groups by sync status, directories always first
- **A-Z** (Alphabetical): Standard alphabetical ordering
- **DateTime**: Sort by modification time (newest first)
- **Size**: Sort by file size (largest first)

Press `S` to reverse the current sort order. Current mode and direction are shown in the status bar (e.g., `Sort: DateTime↑`)

## Status Bars

### System Status Bar
Located at the bottom of the Folders panel, showing:
- **Device Name**: Your Syncthing device name
- **Uptime**: Time since Syncthing started (e.g., `Up: 3d 16h`)
- **Local State**: Total files, directories, and storage size across all folders
- **Transfer Rates**: Live download/upload speeds updated every 2.5 seconds

### Folder/Directory Status Bar
Located at the bottom of the screen, showing:
- **Folder Name**: Currently selected folder/directory
- **Sync State**: Folder status (Idle, Syncing, etc.)
- **Data Sizes**: Local/Global bytes, sync progress
- **Items**: In-sync count vs. total items (e.g., `125/125`)
- **Sort Mode**: Current sorting mode and direction
- **Last Event**: Most recent file change with timestamp

## Cache Management

Synctui uses SQLite caching for instant UI responsiveness:
- **Location**: `~/.cache/synctui/cache.db` (Linux) or `/tmp/synctui-cache` (fallback)
- **Contents**: Directory listings, file sync states, folder statuses, event IDs
- **Validation**: Automatic invalidation using Syncthing's sequence numbers
- **Persistence**: Cache survives app restarts for faster startup

### Manual Cache Clear

If you experience issues after an update, clear the cache:

```bash
rm ~/.cache/synctui/cache.db
```

## Architecture

- **Event-Driven**: Long-polls Syncthing's `/rest/events` endpoint for real-time updates
- **Async API Service**: Non-blocking request queue with priority levels
- **Cache-First Rendering**: Instant display from cache, background validation
- **Sequence-Based Validation**: Only refetches when Syncthing data actually changes

## Troubleshooting

### "Connection refused" error
- Check that Syncthing is running: `curl http://127.0.0.1:8384`
- Verify `base_url` in your config matches Syncthing's listen address

### API Key errors
- Ensure your API key in `config.yaml` matches Syncthing's settings
- API key is found in Syncthing Web UI: Actions → Settings → General

### Cache issues after update
- Run `rm ~/.cache/synctui/cache.db` to clear stale cache
- Required when database schema changes between versions

### Debug logging
- Run with `--debug` flag to enable verbose logging
- Check `/tmp/synctui-debug.log` for detailed operation traces

## Limitations

- No async loading spinners (UI may briefly pause on large operations)
- No file type filtering or batch operations yet
- Error handling and timeout management still being refined

## Contributing

Contributions welcome! This project is actively being developed. See [PLAN.md](PLAN.md) for roadmap and [CLAUDE.md](CLAUDE.md) for architecture details.

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with:
- [Ratatui](https://github.com/ratatui-org/ratatui) — Terminal UI framework
- [Syncthing](https://syncthing.net/) — Continuous file synchronization
- [Rust](https://www.rust-lang.org/) — Systems programming language
