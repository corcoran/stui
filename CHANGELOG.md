# Changelog

All notable changes to Synctui will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 2025-11-09

### 🎨 Code Quality & Architecture

**Major Refactoring: Simplification Cascades (-733 lines)**
- Cleaned up codebase by removing 733 lines of unused and duplicate code
- Removed speculative architecture that was never integrated (Elm-style state system, message passing)
- Unified confirmation dialog handling - all confirmation prompts now use a single, extensible pattern
- Consolidated duplicate type definitions into canonical locations
- **Impact**: 5% smaller codebase, cleaner architecture, easier to maintain and extend

**Module Organization**
- Extracted business logic into focused modules for better testability
- Organized app methods by domain (navigation, file operations, preview, sorting)
- Main.rs reduced from 3,582 lines to 1,397 lines (61% reduction)
- Created dedicated logic modules with comprehensive test coverage (188+ tests)

### ✨ New Features

**ANSI/ASCII Art Viewer**
- View classic ANSI art files directly in the terminal
- Auto-detection of ANSI codes in any file (not just .ans/.asc extensions)
- Supports CP437 encoding (original IBM PC character set with box-drawing characters)
- 80-column automatic wrapping matches traditional ANSI art viewers
- Full color support (foreground colors 30-37, 90-97; background colors 40-47, 100-107)

**Quick Open Web UI**
- Press `o` in folder view to open Syncthing's web UI instantly
- Context-aware: same key opens files/folders when browsing (if configured)
- Automatically opens to the correct folder page

**Wildcard Search**
- Search with wildcards: `*jeff*`, `*.txt`, `photo*`
- Real-time filtering as you type - no lag on large directories
- Persistent search when navigating into subdirectories
- Smart context-aware clearing when backing out

### 🔧 Improvements

**Reliability & Error Handling**
- Fixed folder refresh after reconnection - folders now populate correctly
- Transient state polling: UI updates when folders finish scanning/syncing
- Better offline mode: gracefully handles network errors without crashing
- Cached device names eliminate "Unknown" flash on startup
- Clearer error messages (removed unhelpful technical context)

**Performance**
- Optimized folder status polling (medium priority vs high)
- Non-blocking operations keep UI responsive during network issues
- Efficient cache validation for offline browsing

### 📚 Documentation

**Test-Driven Development**
- Added comprehensive TDD guidelines to project documentation
- Real success story: 10 tests written first exposed exact bug, guided perfect solution
- Clear RED → GREEN → REFACTOR workflow
- Examples of what to do vs what not to do

### 🧪 Testing

- **215 tests passing** (up from 193)
- Added 6 tests for unified confirmation dialogs
- Added 10 comprehensive reconnection flow tests
- All refactoring covered by existing test suite
- Zero compiler warnings

---

## How to Read This Changelog

- **Features** (✨): New capabilities you can use
- **Improvements** (🔧): Enhancements to existing features
- **Fixes** (🐛): Bug fixes
- **Code Quality** (🎨): Behind-the-scenes improvements that make development faster
- **Documentation** (📚): Improvements to guides and instructions
- **Testing** (🧪): Test coverage improvements

---

## Previous Development

For earlier changes, see the git commit history:
```bash
git log --oneline --since="2025-10-01"
```

Key milestones:
- Folder pause/resume functionality
- Folder type changing (Send Only/Send & Receive/Receive Only)
- Dynamic UI with text wrapping
- Sort mode cycling with visual indicators
- Image preview with Kitty/iTerm2/Sixel/Halfblocks protocols
- Event-driven cache invalidation
- SQLite caching for instant navigation
