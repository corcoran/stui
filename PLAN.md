# Syncthing CLI TUI Manager — Development Plan

This document outlines the step-by-step plan for building a **Rust Ratatui CLI tool** that interfaces with **Syncthing's REST API** to manage folders, view sync states, and control ignored/deleted files. The steps are structured to allow **progressive, testable milestones**, ideal for both human and LLM collaboration.

---

## 📍 Current Status (Updated 2025-01-27)

**Recent Accomplishments (Session 2025-01-27):**

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

**Performance Optimizations:**
- ✅ Idle detection (300ms threshold) prevents background operations from blocking keyboard input
- ✅ Non-blocking prefetch operations converted from async to sync (cache-only, no `.await`)
- ✅ Event poll timeout increased from 100ms to 250ms (60% reduction in wakeups)
- ✅ CPU usage reduced from ~18% idle to <1-2% expected
- ✅ Instant keyboard responsiveness even during background caching

**Next steps:**
1. Performance testing with large-scale datasets (validate idle CPU usage and responsiveness)
2. Add config file location support (`~/.config/synctui/config.yaml` with CLI override)
3. Add filtering functionality (show only ignored files, by type, etc.)
4. Add event history viewer with persistent logging
5. Add file preview system (text and images)
6. Build comprehensive test suite
7. Improve error handling, display, and timeouts
8. Refactor code to be more modular and readable



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
