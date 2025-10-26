# Syncthing CLI TUI Manager — Development Plan

## 📊 Current Status

**Architecture: Production-Ready ✅**
- Clean separation: Model (state), Logic (pure functions), App (orchestration), Handlers (events), Services (I/O)
- **453 tests passing** (216 library + 221 binary + 16 integration)
- Zero compiler warnings
- main.rs refactored: 3,582 → 1,397 lines (61% reduction)
- **New:** 4 critical algorithms extracted as testable pure functions (+22 tests)

**Features: Complete ✅**
- Folder management with real-time sync status
- Breadcrumb navigation with multi-pane display
- File preview (text, ANSI art with CP437, images)
- Search with wildcards and recursive filtering
- Multiple sort modes, ignore management, file operations
- Event-driven cache invalidation, non-blocking operations

**Recent Improvements (2025-11-09):**
- ✅ Extracted `aggregate_directory_state` - directory sync state calculation fully testable
- ✅ Extracted `find_item_index_by_name` - selection preservation logic isolated
- ✅ Extracted `compare_browse_items` - sort comparison as reusable pure function
- ✅ Extracted `should_cleanup_stale_pending` & `should_verify_pending` - time validation testable

---

## 🎯 Future Work

---

### Feature Enhancements

**High Priority:**
- Remote device panel (name, download/upload rates, shared folders)
- Event history viewer with persistent logging
- File type filtering (show only images, ignored files, etc.)
- Better error handling and timeout management

**Medium Priority:**
- Batch operations (multi-select for ignore/delete/rescan)
- Filesystem diff view (compare local vs remote)
- Configurable keybindings
- Performance testing with large datasets

**Low Priority:**
- Cross-platform packaging (Linux, macOS, Windows)
- Live disk usage stats
- Syncthing log viewer
- CLI flags for headless operations

---

**Bottom Line:** Synctui is production-ready. All items above are optional enhancements.
