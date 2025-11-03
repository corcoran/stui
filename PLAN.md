# Syncthing CLI TUI Manager — Development Plan

## 📊 Current Status (2025-11-02)

### ✅ Architecture: Excellent

**Refactoring Complete** — main.rs successfully modularized:
- **Original**: 3,582 lines (monolithic)
- **Current**: 1,397 lines (**61% reduction**)
- **Extracted**: 6 modules under `src/app/` (2,298 lines)

**Code Organization:**
```
src/
├── main.rs (1,397 lines)      # Event loop, App struct, core orchestration
├── app/                        # App orchestration methods (6 modules, 2,298 lines)
│   ├── navigation.rs (517)    # Breadcrumb traversal, load/enter/back
│   ├── sync_states.rs (513)   # State fetching, prefetching, directory aggregation
│   ├── file_ops.rs (368)      # Delete, rescan, restore, open, clipboard
│   ├── preview.rs (398)       # Text/ANSI/image preview
│   ├── ignore.rs (337)        # Toggle ignore, ignore+delete
│   └── sorting.rs (146)       # Sort modes, reverse, selection preservation
├── handlers/                   # Event processing
│   ├── keyboard.rs            # Keyboard input handling
│   ├── api.rs                 # API response processing
│   └── events.rs              # Cache invalidation events
├── services/                   # Background services
│   ├── api.rs                 # API request queue
│   └── events.rs              # Event stream listener
├── logic/                      # Pure business logic (18 functions, 133 tests)
│   ├── path.rs                # Path translation, validation
│   ├── folder.rs              # Folder validation, aggregation
│   ├── file.rs                # File type detection, ANSI parsing, text extraction
│   ├── ignore.rs              # Pattern matching
│   ├── sync_states.rs         # State priority, validation
│   ├── navigation.rs          # Selection logic
│   ├── search.rs              # Search pattern matching
│   ├── ui.rs                  # UI state transitions
│   ├── formatting.rs          # Data formatting
│   ├── layout.rs              # Layout calculations
│   ├── performance.rs         # Batching, throttling
│   └── errors.rs              # Error classification
├── model/                      # Pure application state
│   ├── mod.rs                 # Main Model struct
│   ├── syncthing.rs           # Syncthing data (folders, devices, connection)
│   ├── navigation.rs          # Breadcrumb trail, focus
│   ├── ui.rs                  # UI preferences, dialogs, popups
│   ├── performance.rs         # Loading tracking, metrics
│   └── types.rs               # Shared types
└── ui/                         # Rendering (12 modules)
    ├── render.rs, breadcrumb.rs, folder_list.rs, dialogs.rs,
    ├── icons.rs, legend.rs, search.rs, status_bar.rs, system_bar.rs, etc.
```

### ✅ Test Coverage: Comprehensive

- **188 unit tests** (logic + model + UI components)
- **16 integration tests** (folder status + reconnection flows)
- **Zero compiler warnings**
- **All tests passing**

### ✅ Features: Production-Ready

**Core Features:**
- ✅ Folder list with real-time sync status
- ✅ Breadcrumb navigation with multi-pane display
- ✅ File preview (text, ANSI art with CP437, images with Kitty/iTerm2/Sixel/Halfblocks)
- ✅ Ignore management (.stignore patterns)
- ✅ File operations (delete, rescan, restore for receive-only folders)
- ✅ Search with wildcards and recursive filtering
- ✅ Multiple sort modes (sync state, A-Z, timestamp, size)
- ✅ Context-aware hotkeys
- ✅ 'o' key: Open Syncthing web UI (folder view) or open file/dir (breadcrumb view)

**Performance:**
- ✅ Event-driven cache invalidation (file/directory/folder granularity)
- ✅ Non-blocking operations (idle CPU <1-2%)
- ✅ Instant keyboard responsiveness
- ✅ Smart prefetching and request deduplication

---

## 🎯 Next Steps

### Immediate: TDD Pure Logic Extraction (Phases 4-7)

Continue extracting testable business logic from orchestration methods:

**Phase 4: Extract aggregate_directory_state** (from `app/sync_states.rs`)
- Pure function: `logic::sync_states::aggregate_directory_state(direct_state, child_states) -> SyncState`
- Tests: 5 (all synced, one syncing, mixed states, priority order, RemoteOnly/Ignored handling)
- Benefit: Makes directory state aggregation algorithm testable

**Phase 5: Extract find_item_index_by_name** (from `app/sorting.rs`)
- Pure function: `logic::navigation::find_item_index_by_name(items, name) -> Option<usize>`
- Tests: 4 (found, not found, empty list, edge cases)
- Benefit: Reusable selection preservation logic

**Phase 6: Extract sort comparison function** (from `app/sorting.rs`)
- Pure function: `logic::sorting::compare_browse_items(...) -> Ordering`
- Tests: 8 (dir vs file, each sort mode, reverse, tie-breaking)
- Benefit: Testable sort comparison logic (currently in closure)

**Phase 7: Extract time validation functions** (from main.rs pending deletes)
- Pure functions: `logic::performance::should_cleanup_stale_pending(...)`, `should_verify_pending(...)`
- Tests: 6 (time thresholds, edge cases, rescan combinations)
- Benefit: Testable business rules for pending operation cleanup

**Impact:** +23 tests, better test coverage on critical algorithms

---

### Future: Feature Enhancements

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

## 📈 Progress Tracking

### Refactoring Progress

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| main.rs | 3,582 lines | 1,397 lines | **-61%** ✅ |
| app/ modules | 0 | 6 modules | +2,298 lines |
| Test coverage | 174 tests | 188 tests | +14 tests ✅ |
| Compiler warnings | 0 | 0 | Clean ✅ |

### Architecture Quality

- ✅ **Separation of Concerns**: Model (state), Logic (pure functions), App (orchestration), Handlers (events), Services (I/O)
- ✅ **Testability**: Pure logic fully tested, orchestration methods validated via integration tests
- ✅ **Maintainability**: Small focused modules (<600 lines each)
- ✅ **Discoverability**: Clear module naming by domain (navigation, sorting, ignore, etc.)
- ✅ **Consistency**: Follows existing patterns (handlers/, services/, model/, logic/, ui/)

---

## Recent Accomplishments (Session 2025-11-02)

### Main.rs Refactoring — Phases 1 & 2 Complete

**Phase 1:**
- ✅ Extracted navigation.rs (11 methods, 517 lines)
- ✅ Extracted sync_states.rs (7 methods, 513 lines)
- ✅ Extracted ignore.rs (2 methods, 337 lines)
- Result: 3,582 → 2,269 lines (36.6% reduction)

**Phase 2:**
- ✅ Extracted sorting.rs (6 methods, 146 lines)
- ✅ Extracted file_ops.rs (6 methods, 368 lines)
- ✅ Extracted preview.rs (5 methods, 398 lines)
- Result: 2,269 → 1,397 lines (additional 38% reduction, 61% total)

### TDD Pure Logic Extraction — Phases 1-3 Complete

Following strict RED → GREEN → REFACTOR cycle:

**Phase 1:**
- ✅ Extracted `logic::path::is_path_or_parent_in_set` with 5 tests
- Tests path hierarchy validation for pending deletes

**Phase 2:**
- ✅ Extracted `logic::folder::calculate_local_state_summary` with 4 tests
- Tests folder statistics aggregation

**Phase 3:**
- ✅ Extracted `logic::file::extract_text_from_binary` with 5 tests
- Tests binary text extraction algorithm

### Feature: Open Syncthing Web UI

- ✅ Added 'o' key context-aware behavior:
  - Folder view: Opens Syncthing web UI in browser
  - Breadcrumb view: Opens selected file/directory
- ✅ Always shows hotkey for discoverability (error toast if command not configured)
- ✅ 5 legend display tests added
- ✅ Updated README.md documentation

### Cleanup

- ✅ Removed `--bug` CLI flag and `log_bug()` infrastructure (36+ calls removed)
- ✅ Simplified debugging to single `--debug` flag
- ✅ Updated CLAUDE.md documentation

---

## Success Criteria

### Architecture ✅
- [x] main.rs < 1,500 lines (target achieved: 1,397 lines)
- [x] Clear module boundaries by domain
- [x] Zero behavior regressions
- [x] All tests passing

### Testing ✅
- [x] 180+ tests (target achieved: 204 tests)
- [x] Pure logic fully tested
- [x] Integration tests for critical flows
- [x] Zero compiler warnings

### Code Quality ✅
- [x] Consistent architecture patterns
- [x] Well-documented modules
- [x] Clean git history
- [x] Production-ready

---

**Bottom Line:** Synctui is feature-complete, well-architected, comprehensively tested, and ready for production use. Future work focuses on enhancements and additional features.
