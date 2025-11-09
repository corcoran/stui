# Syncthing CLI TUI Manager — Development Plan

## 📊 Current Status

**Architecture: Production-Ready ✅**
- Clean separation: Model (state), Logic (pure functions), App (orchestration), Handlers (events), Services (I/O)
- 204 tests passing (188 unit + 16 integration)
- Zero compiler warnings
- main.rs refactored: 3,582 → 1,397 lines (61% reduction)

**Features: Complete ✅**
- Folder management with real-time sync status
- Breadcrumb navigation with multi-pane display
- File preview (text, ANSI art with CP437, images)
- Search with wildcards and recursive filtering
- Multiple sort modes, ignore management, file operations
- Event-driven cache invalidation, non-blocking operations

---

## 🎯 Future Work

### Optional: Additional Pure Logic Extraction

Extract remaining testable business logic from orchestration methods (not required for production):

1. **aggregate_directory_state** (from `app/sync_states.rs`)
   - Pure function: `logic::sync_states::aggregate_directory_state(direct_state, child_states) -> SyncState`
   - Tests: 5 (all synced, one syncing, mixed states, priority order)

2. **find_item_index_by_name** (from `app/sorting.rs`)
   - Pure function: `logic::navigation::find_item_index_by_name(items, name) -> Option<usize>`
   - Tests: 4 (found, not found, empty list, edge cases)

3. **sort comparison function** (from `app/sorting.rs`)
   - Pure function: `logic::sorting::compare_browse_items(...) -> Ordering`
   - Tests: 8 (dir vs file, each sort mode, reverse, tie-breaking)

4. **time validation functions** (from main.rs pending deletes)
   - Pure functions: `logic::performance::should_cleanup_stale_pending(...)`, `should_verify_pending(...)`
   - Tests: 6 (time thresholds, edge cases, rescan combinations)

**Impact:** +23 tests, better coverage on critical algorithms

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
