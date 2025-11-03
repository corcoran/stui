# Synctui

A fast, keyboard-driven terminal UI for managing [Syncthing](https://syncthing.net/) — browse folders, track sync states, manage ignore patterns, and control your files, all from the comfort of your terminal.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)

## Features

### 🚀 Real-Time Sync Monitoring
- **Live Status Updates**: Automatic cache invalidation using Syncthing's event stream
- **Visual Sync States**: Instant feedback with `<file|dir><status>` icon pattern (e.g., `📄✅`, `📁☁️`)
- **Active Sync Indicator**: Files show spinning icon (🔄) during active downloads/uploads
- **Ignored File Detection**: Separate icons for ignored files that exist (`📄⚠️`) vs deleted (`📄🚫`)
- **Icon Modes**: Choose between emoji or Nerd Fonts icons via config
- **Terminal Theme Support**: All colors use standard terminal colors, customizable through your terminal theme
- **Smart Caching**: SQLite-backed cache for instant UI responsiveness with background updates
- **System Status Bar**: Device name, uptime, local state summary, and live transfer rates at top of screen

### 📁 File & Folder Management
- **Multi-Pane Navigation**: Breadcrumb-style directory traversal with smart sizing (current folder gets 50-60% width)
- **Ancestor Highlighting**: All parent folders stay highlighted (blue border) when drilling deeper
- **Real-Time Recursive Search**: Fast wildcard search across all cached files/directories
  - Trigger with `Ctrl-F` (normal) or `/` (vim mode)
  - Instant filtering as you type with match count display
  - Wildcard patterns: `*jeff*`, `*.txt`, `photo*`
  - Context-aware clearing: only clears when backing out past search origin
  - Shows parent directories if they contain matching descendants
- **Flexible Sorting**: Sort by sync state, name, timestamp, or size with one keypress
- **File Info Display**: Toggle between no info, timestamps only, or timestamps + human-readable sizes
- **Detailed File Preview**: Press `?` on any file to open a comprehensive popup showing:
  - **Metadata**: Sync state, permissions, resolution (images), device availability
  - **Text Preview**: Scrollable with vim keybindings (j/k, gg/G, Ctrl-d/u/f/b)
  - **ANSI Art Rendering**: Full-featured viewer with auto-detection
    - Auto-detects ANSI codes in any file (not just .ans/.asc extensions)
    - CP437 encoding (original IBM PC character set)
    - 80-column automatic wrapping (matches PabloDraw standard)
    - Full SGR color support (16 foreground + 16 background colors)
    - Proper cursor positioning and SAUCE metadata handling
  - **Image Preview**: Terminal graphics rendering (Kitty/iTerm2/Sixel/Halfblocks)
    - Non-blocking load (40-200ms)
    - Smart centering and aspect ratio preservation
    - Adaptive quality/performance balance
- **Ignore Management**: Add or remove files from `.stignore` patterns interactively
- **Safe Deletions**: Confirmation prompts for all destructive operations
- **Ignore+Delete Protection**: Prevents accidental un-ignore during active deletion operations
  - Blocks un-ignore until files are verified deleted and Syncthing has processed changes
  - Smart path hierarchy blocking (parent paths block child un-ignore)
  - Status bar shows pending operations count
  - 5-second verification buffer after filesystem deletion
  - 60-second timeout fallback for edge cases

### ⌨️ Keyboard-First Interface
- **Arrow Key Navigation** (default) or **Vim Keybindings** (optional)
- **Multi-Mode Sorting**: `s` to cycle modes, `S` to reverse order
- **Info Toggle**: `t` cycles through Off → Timestamp → Size+Timestamp
- **Quick Actions**: Single-key commands for ignore, delete, rescan, restore, pause/resume, change folder type
- **Smart Hotkey Legend**: Context-aware display with text wrapping - hides irrelevant keys, shows Restore only when applicable
- **Folder Type Management**: Change folder types (Send Only, Send & Receive, Receive Only) with interactive selection menu

### 🎯 Smart Features
- **Responsive Navigation**: Instant keyboard input with idle-aware background caching
- **Docker Path Mapping**: Automatic translation between container and host paths
- **Directory-Aware Display**: File sizes shown for files only, omitted for directories
- **Unicode-Aware Rendering**: Proper alignment even with emoji icons
- **Graceful Truncation**: Smart text trimming when terminal width is limited

### ⚡ Performance Optimizations
- **Batched Database Writes**: 30-50x faster than individual writes
  - Groups sync state updates into single transactions
  - Processes 100+ file updates in 5-10ms vs 500-1000ms
  - Automatic batch flushing based on size (50 items) or time (100ms)
- **Smart UI Rendering**: 60-90% fewer redraws
  - Dirty flag system - only redraws when state actually changes
  - Reduces from unconditional 4 FPS to on-demand only
  - Periodic updates for live stats (uptime, transfer rates) every 1 second
- **SQLite WAL Mode**: Write-Ahead Logging for better concurrency
  - Readers don't block on writers
  - Crash-safe with automatic recovery
  - Better performance for write-heavy workloads
- **Idle-Aware Operations**: 300ms idle detection prevents blocking keyboard input
  - Background prefetch only runs when user is idle
  - Minimizes CPU usage (~1-2% when idle)
  - All operations non-blocking with channel-based async architecture

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

Create a config file at the platform-specific location:
- **Linux**: `~/.config/synctui/config.yaml`
- **macOS**: `~/Library/Application Support/synctui/config.yaml`
- **Windows**: `%APPDATA%\synctui\config.yaml`

```yaml
api_key: "your-syncthing-api-key"
base_url: "http://127.0.0.1:8384"

# Optional: Icon display mode ("emoji" or "nerdfont")
icon_mode: "nerdfont"

# Optional: Command to open files/directories (e.g., xdg-open, code, vim)
# Linux: "xdg-open", macOS: "open", Windows: "explorer"
open_command: "xdg-open"

# Optional: Command to copy to clipboard (receives text via stdin)
# Wayland: "wl-copy", X11: "xclip" or "xsel", macOS: "pbcopy", Windows: "clip.exe"
clipboard_command: "wl-copy"

# Optional: Map container paths to host paths (for Docker setups)
path_map:
  "/data": "/home/user/syncthing-data"
  "/photos": "/mnt/photos"

# Optional: Enable vim keybindings by default
vim_mode: false

# Optional: Image preview settings
image_preview_enabled: true        # Enable/disable image preview (default: true)
image_protocol: "auto"             # auto|kitty|iterm2|sixel|halfblocks (default: "auto")
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
- `Enter` / `→` — Preview file (if file) or enter directory (if folder)
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
| `Ctrl-F` / `/` | **Search**: Enter search mode (recursive wildcard search) | No |
| `?` | Show detailed file info popup (metadata, sync state, preview). Note: `Enter` on files also opens preview. | No |
| `c` | **Context-aware**: Change folder type (folder view) OR Copy path (breadcrumb view) | Selection menu / No |
| `p` | Pause/resume folder (folder view only) | Yes |
| `i` | Toggle ignore pattern (add/remove from `.stignore`) | No |
| `I` | Ignore AND delete from disk | No (immediate) |
| `o` | **Context-aware**: Open Syncthing web UI (folder view) OR Open file/directory with configured command (breadcrumb view) | No |
| `d` | Delete file/directory from disk | Yes |
| `r` | Rescan folder (refresh from disk) | No |
| `R` | Restore deleted files (revert receive-only folder) | Yes |
| `s` | Cycle sort mode (Sync State → A-Z → Timestamp → Size) | No |
| `S` | Reverse current sort order | No |
| `t` | Toggle info display (Off → Timestamp → Size+Timestamp) | No |
| `q` | Quit synctui | No |

**Search Mode Keys** (when search is active):
- Type to filter results in real-time
- `Enter` — Accept search (keep filtering, deactivate input)
- `Backspace` — Delete character (auto-exits when query becomes empty)
- `Esc` — Clear search and restore all items

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

## UI Layout

The interface is organized top to bottom:

### System Status Bar (Top)
Full-width bar showing:
- **Device Name**: Your Syncthing device name
- **Uptime**: Time since Syncthing started (e.g., `Up: 3d 16h`)
- **Local State**: Total files, directories, and storage size across all folders
- **Transfer Rates**: Live download/upload speeds updated every 2.5 seconds

### Main Content (Middle)
- **Folders Panel**: Left side, lists all Syncthing folders
- **Breadcrumb Panels**: Right side, current folder gets 50-60% width, parents share remaining space
- **Ancestor Highlighting**: All parent folders stay highlighted (blue) when drilling deeper

### Search Input (Above Legend)
Dynamic search box that appears when triggered:
- **Cyan border** when actively typing, **gray** when accepted
- **Match count** in title (e.g., `Search (3 matches) - Esc to clear`)
- **Blinking cursor** shows current input position
- Filters results recursively across all cached subdirectories

### Hotkey Legend (Above Status)
Full-width bar with context-aware key display:
- **Folder View**: Shows navigation, Change Type, Pause/Resume, Rescan, Quit
- **Breadcrumb View**: Shows all keys including Copy, Sort, Info, Ignore, Delete
- **Dynamic Restore**: Only appears when folder has local changes to restore
- **Text Wrapping**: Wraps text within fixed height on narrow terminals

### Status Bar (Bottom)
Full-width bar showing:
- **Folder Name**: Currently selected folder/directory
- **Folder Type**: Send Only, Send & Receive, or Receive Only
- **Sync State**: Folder status (Idle, Syncing, Paused, etc.)
- **Data Sizes**: Local/Global bytes, sync progress
- **Items**: In-sync count vs. total items (e.g., `125/125`)
- **Sort Mode**: Current sorting mode and direction
- **Pending Operations**: Shows count when ignore+delete operations are processing (e.g., `⏳ 2 deletions processing`)
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

- **Event-Driven**: Long-polls Syncthing's `/rest/events` endpoint for real-time updates with auto-recovery
- **Async API Service**: Non-blocking request queue with priority levels
- **Cache-First Rendering**: Instant display from cache, background validation
- **Sequence-Based Validation**: Only refetches when Syncthing data actually changes
- **Batched Database Operations**: Groups writes into transactions for 30-50x performance improvement
- **Dirty Flag UI System**: Only redraws when state changes, reducing CPU usage by 60-90%
- **Robust State Transitions**: Logic-based validation prevents flickering during ignore/unignore operations
- **Operation Safety Tracking**: Prevents destructive race conditions with filesystem verification

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

- No async loading spinners (planned)
- No batch operations for multi-select yet (planned)

## Contributing

Contributions welcome! This project is actively being developed. See [PLAN.md](PLAN.md) for roadmap and [CLAUDE.md](CLAUDE.md) for architecture details.

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with:
- [Ratatui](https://github.com/ratatui-org/ratatui) — Terminal UI framework
- [Syncthing](https://syncthing.net/) — Continuous file synchronization
- [Rust](https://www.rust-lang.org/) — Systems programming language
