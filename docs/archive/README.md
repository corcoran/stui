# Archive - Historical Documentation

This directory contains completed/superseded documentation from the refactoring process and implemented feature designs.

## Refactoring Documentation

### MODEL_DESIGN.md
**Status:** Reference documentation (optional future use)

Complete Elm Architecture pattern guide showing Model/Msg/Update/Runtime separation. Contains detailed examples of pure update functions and command execution.

**Why archived:** Full Elm Architecture was explored but deemed unnecessary for this TUI application. Current architecture (pure Model + logic modules) provides sufficient testability and maintainability without the additional ceremony.

**Use case:** Reference if considering future migration to pure functional architecture.

---

### NEXT_STEPS.md
**Status:** Complete (all tasks done)

Detailed roadmap for Phase 3 pure business logic extraction. Documents the step-by-step process for extracting 15 functions across 3 phases (A/B/C).

**Why archived:** All 15 functions have been successfully extracted with comprehensive tests (118 tests passing). The extraction process is complete.

**Historical value:** Shows the incremental refactoring approach used, provides template for future extractions.

---

## Feature Design Documentation

### FEATURE_PREVIEW_FILE.md (718 lines)
**Status:** ✅ Complete - Implemented

Comprehensive design document for the file info popup feature (`?` key):
- Two-column layout (metadata + preview)
- Text file preview with vim keybindings (j/k, gg/G, Ctrl-d/u/f/b)
- Terminal image rendering (Kitty/iTerm2/Sixel/Halfblocks)
- Binary file detection and text extraction
- Non-blocking background loading

**Implementation completed:** October 2025 (see CLAUDE.md user actions)

---

### FEATURE_OPTIMIZE.md (791 lines)
**Status:** ✅ Complete - Implemented

Performance optimization plan addressing:
- **Problem:** 10-30 seconds for 100+ file directories (SQLite write bottleneck + unconditional UI redraws)
- **Solution:** Batched database writes, idle detection, non-blocking prefetch
- **Results:** <1-2% idle CPU, instant keyboard responsiveness, 60-90% fewer redraws

**Implementation completed:** October 31, 2025 (see PLAN.md performance section)

**Key optimizations:**
- 250ms poll timeout (vs 100ms)
- 300ms idle threshold for background operations
- Request deduplication
- Cache-first rendering with async updates

---

## What's Still Active

- **CLAUDE.md** - Project documentation for AI assistants (active, updated regularly)
- **ELM_REWRITE_PREP.md** - Primary refactoring reference showing complete journey and current status
- **PLAN.md** - Original development plan (updated to reflect completed refactoring)
- **README.md** - User-facing documentation

---

**Last Updated:** 2025-10-31
**Archive Created:** During markdown consolidation cleanup
