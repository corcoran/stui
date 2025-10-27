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

**Timestamp Display:**
- ✅ File/folder modification timestamps displayed on right side (on by default)
- ✅ Toggle timestamps with `t` key
- ✅ Smart truncation: Full (16 chars) → Medium (10 chars) → Time only (5 chars)
- ✅ Timestamps in dark gray for subtle appearance
- ✅ Unicode-aware alignment (handles emoji widths correctly)
- ✅ Graceful degradation when panel width is limited

**Database Schema Updates:**
- ✅ Added `mod_time` and `size` fields to `browse_cache` table
- ✅ Proper cache invalidation when schema changes (requires manual cache clear)

**UI Improvements:**
- ✅ Hotkey legend now wraps automatically to multiple lines
- ✅ Updated legend with all new keys: `s`, `S`, `t`
- ✅ Cache clearing fix for schema migrations

**Next steps:**
1. Add config file location support (`~/.config/synctui/config.yaml` with CLI override)
2. Add filtering functionality (show only ignored files, by type, etc.)
3. Add event history viewer with persistent logging
4. Add file preview system (text and images)
5. Build comprehensive test suite
6. Improve error handling and display
7. Performance testing with large-scale datasets



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
