# Stui

A fast, keyboard-driven terminal UI for managing [Syncthing](https://syncthing.net/) — browse folders, track sync states, manage ignore patterns, and control your files, all from the comfort of your terminal.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)

## Features

### 🚀 Real-Time Sync Monitoring
- **Live Status Updates**: See sync state changes instantly with visual icons (`📄✅` synced, `📁☁️` remote-only, `📄🔄` syncing)
- **Ignored File Detection**: Distinct icons for ignored files that exist (`📄🔇`) vs deleted (`📄🚫`)
- **Icon Modes**: Choose between emoji or Nerd Fonts icons
- **System Dashboard**: View device name, uptime, storage usage, and live transfer rates

### 📁 File & Folder Management
- **Breadcrumb Navigation**: Multi-pane directory browsing with ancestor highlighting
- **Recursive Search**: Fast wildcard search (`*jeff*`, `*.txt`) with instant filtering as you type
- **Out-of-Sync Filter**: Press `f` to show only files that need attention
  - Shows remote files you need to download
  - Shows local changes in receive-only folders (added/deleted/modified files)
  - Works recursively across entire folder hierarchy
- **Flexible Sorting**: Sort by sync state, name, date, or size
- **File Preview Popup**: View file details, text content, ANSI art, or images directly in terminal
  - **Text files**: Scrollable with vim keybindings
  - **ANSI art**: Auto-detection with CP437 encoding and 80-column wrapping
  - **Images**: Terminal graphics (Kitty/iTerm2/Sixel/Halfblocks protocols)
- **Ignore Management**: Add/remove files from `.stignore` patterns
- **Folder Control**: Pause/resume sync, change folder type (Send Only/Send & Receive/Receive Only)
- **Safe Operations**: Confirmation prompts for delete, restore, and other destructive actions

### ⌨️ Keyboard-First Interface
- **Arrow Keys or Vim Mode**: Choose your preferred navigation style
- **Single-Key Actions**: Quick commands for all operations (sort, ignore, delete, search, etc.)
- **Context-Aware Help**: Smart hotkey legend shows only relevant keys for current view

## Installation

### From Source

```bash
git clone https://github.com/yourusername/stui.git
cd stui
cargo build --release
sudo cp target/release/stui /usr/local/bin/
```

### Prerequisites
- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- A running Syncthing instance (local or remote)

## Configuration

Create a config file at the platform-specific location:
- **Linux**: `~/.config/stui/config.yaml`
- **macOS**: `~/Library/Application Support/stui/config.yaml`
- **Windows**: `%APPDATA%\stui\config.yaml`

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

This allows stui (running on host) to perform file operations on the correct paths.

## Usage

### Basic Commands

```bash
# Start stui (reads config from ~/.config/stui/config.yaml)
stui

# Enable vim keybindings for this session
stui --vim

# Enable debug logging
stui --debug
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
| `f` | **Filter**: Toggle out-of-sync filter (shows remote needed files + local changes) | No |
| `?` | Show detailed file info popup (metadata, sync state, preview). Note: `Enter` on files also opens preview. | No |
| `c` | **Context-aware**: Change folder type (folder view) OR Copy path (breadcrumb view) | Selection menu / No |
| `p` | Pause/resume folder (folder view only) | Yes |
| `i` | Toggle ignore pattern (add/remove from `.stignore`) | No |
| `I` | Ignore AND delete from disk | No (immediate) |
| `o` | **Context-aware**: Open Syncthing web UI (folder view) OR Open file/directory with configured command (breadcrumb view) | No |
| `d` | Delete file/directory from disk | Yes |
| `r` | Rescan folder (refresh from disk) | Yes |
| `R` | Restore deleted files (revert receive-only folder) | Yes |
| `s` | Cycle sort mode (Sync State → A-Z → Timestamp → Size) | No |
| `S` | Reverse current sort order | No |
| `t` | Toggle info display (Off → Timestamp → Size+Timestamp) | No |
| `q` | Quit stui | No |

**Search Mode Keys** (when search is active):
- Type to filter results in real-time
- `Enter` — Accept search (keep filtering, deactivate input)
- `Backspace` — Delete character (auto-exits when query becomes empty)
- `Esc` — Clear search and restore all items

**Filter Mode** (when out-of-sync filter is active):
- Press `f` again to toggle filter off and show all files
- Status bar shows "Filter: Remote + Local" (receive-only) or "Filter: Remote" (other folder types)
- Filter persists when navigating into subdirectories

## Cache Management

Stui caches data for instant UI performance. Cache locations:
- **Linux**: `~/.cache/stui/cache.db`
- **macOS**: `~/Library/Caches/stui/cache.db`
- **Windows**: `%LOCALAPPDATA%\stui\cache\cache.db`

To clear cache if you experience issues:
```bash
# Linux
rm ~/.cache/stui/cache.db

# macOS
rm ~/Library/Caches/stui/cache.db

# Windows
del %LOCALAPPDATA%\stui\cache\cache.db
```

## Troubleshooting

### "Connection refused" error
- Check that Syncthing is running: `curl http://127.0.0.1:8384`
- Verify `base_url` in your config matches Syncthing's listen address

### API Key errors
- Ensure your API key in `config.yaml` matches Syncthing's settings
- API key is found in Syncthing Web UI: Actions → Settings → General

### Cache issues after update
- Run `rm ~/.cache/stui/cache.db` to clear stale cache
- Required when database schema changes between versions

### Debug logging
- Run with `--debug` flag to enable verbose logging
- Check `/tmp/stui-debug.log` for detailed operation traces

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
