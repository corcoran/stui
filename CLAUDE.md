# stui - Syncthing TUI Manager

## Project Overview

Building a Rust Ratatui CLI that manages Syncthing via its REST API — listing folders, showing contents, adding .stignore rules, and deleting directories safely with Docker path-mapping support.

## claude instructions

### CRITICAL: Test-Driven Development is MANDATORY

**YOU MUST WRITE TESTS FIRST. NO EXCEPTIONS.**

Every code change requires this exact workflow:

1. **STOP and think**: "What tests do I need?"
2. **Write tests FIRST** that expose the bug or define the feature
3. **Verify tests fail** (proving they test the right thing)
4. **Implement** minimal code to pass tests
5. **Verify tests pass**
6. **Only then** commit

**Why this matters:**
- Without tests, you waste user's time and money debugging blind
- Tests document expected behavior and catch regressions
- TDD prevents over-engineering and scope creep
- Example: Commit fcf4362 (reconnection fix) - 10 tests written first, exposed exact bug, guided perfect solution

**Red flags that you're doing it wrong:**
- ❌ "Let me try this change and see if it works"
- ❌ "I'll add some debug logging to investigate"
- ❌ Making multiple attempts without tests
- ❌ Saying "I think this should work"

**What you should do instead:**
- ✅ "Let me write a test that reproduces this bug"
- ✅ "I'll write tests for these 3 scenarios first"
- ✅ "Here's a failing test - now I'll implement the fix"
- ✅ "All 10 tests pass, ready to commit"

**When to write tests:**
- Adding new features → Write feature tests first
- Fixing bugs → Write test that reproduces bug first
- Refactoring → Ensure existing tests pass, add coverage if missing
- Changing state logic → Write state transition tests first
- User reports "X doesn't work" → Write test showing X failing

**If you catch yourself coding before testing:**
1. STOP immediately
2. Delete/revert the code
3. Write tests first
4. Start over with proper TDD

This is not optional. This is not a suggestion. **This is how professional software is built.**

### Git Commit Commands

**CRITICAL: Avoid "STDIN" prefix in commit messages**

The user has `cat` aliased to `bat`, which adds "STDIN" label when reading from heredocs. **Always use `/bin/cat` instead of `cat` in git commit commands.**

**Bad pattern (adds "STDIN" prefix):**
```bash
git commit -m "$(cat <<'EOF'
commit message
EOF
)"
```

**Good pattern (no STDIN prefix):**
```bash
git commit -m "$(/bin/cat <<'EOF'
commit message
EOF
)"
```

**Why:** `cat` is aliased to `bat --style header --style snip --style changes --style header`, and bat labels stdin input as "STDIN".

**CRITICAL: Never add co-authored-by attribution**

Do not add "Co-Authored-By: Claude" or similar attribution lines to commit messages.

**Bad pattern:**
```bash
git commit -m "$(/bin/cat <<'EOF'
feat: Add new feature

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

**Good pattern:**
```bash
git commit -m "$(/bin/cat <<'EOF'
feat: Add new feature
EOF
)"
```

**Why:** Commit authorship is already tracked by Git. Additional attribution is redundant and clutters commit history.

### Syncthing API Testing with curl

**CRITICAL: Always read API credentials from user config file**

When testing Syncthing API endpoints with curl commands:

1. **Read the config file first**: `~/.config/stui/config.yaml`
2. **Extract `api_key` and `base_url` from the config**
3. **Use those values in curl commands**

**Example workflow:**
```bash
# Read config to get API key and base URL
cat ~/.config/stui/config.yaml

# Then use extracted values in curl
curl -s -H "X-API-Key: <key-from-config>" "<base_url-from-config>/rest/db/need?folder=lok75-7d42r"
```

**NEVER:**
- ❌ Use hardcoded API keys
- ❌ Guess at API credentials
- ❌ Use old/stale credentials from previous sessions

**ALWAYS:**
- ✅ Read config file first
- ✅ Use current credentials from config
- ✅ Verify base_url matches user's setup

### Preferred Tools for File Operations

**Use `ag` (The Silver Surfer) for content search:**
```bash
# Search for text pattern in code
ag "search_term"

# Search in specific file type
ag --rust "pattern"

# Search with context
ag -C 3 "pattern"

# Case-insensitive search
ag -i "pattern"
```

**CRITICAL: Always put ag options BEFORE the pattern, never after files:**
```bash
# ✅ CORRECT - options before pattern
ag -C 3 "pattern" src/

# ❌ WRONG - options after files will fail
ag "pattern" src/ -C 3
```

**Use `fd` for finding files:**
```bash
# Find files by name pattern
fd "pattern"

# Find in specific directory
fd "pattern" src/

# Find by file type
fd -e rs      # Rust files
fd -e toml    # TOML files

# Combine with other commands
fd "test" | head -10
```

**Why these tools:**
- `ag`: Faster than grep, respects .gitignore, better syntax highlighting
- `fd`: Faster than find, simpler syntax, respects .gitignore

**AVOID using:**
- ❌ `grep -r` - use `ag` instead
- ❌ `find` - use `fd` instead

### Code Formatting Requirements

**CRITICAL: All code must pass `cargo fmt` and `cargo clippy`**

These checks run on every PR and release - failures will block merging/releasing.

#### Rust Formatting with cargo fmt

**Before committing, always run:**
```bash
cargo fmt
```

This auto-formats all Rust code to match Rustfmt style. Never commit code that fails this check.

**Formatting rules (enforced by Rustfmt):**

**Imports:**
- Use alphabetical ordering: `use crate::...`, `use std::...`, `use external::...`
- Group related imports together with blank lines between groups
- Remove unused imports

Example:
```rust
// Good - alphabetical order, grouped
use crate::utils;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

// Bad - not alphabetical
use anyhow::{Context, Result};
use crate::utils;
use std::path::PathBuf;
```

**Line length:**
- Max 100 characters per line (Rustfmt default)
- Long function arguments and match arms split across multiple lines

Example:
```rust
// Good - split long argument list
log_debug(&format!(
    "Failed to get files for {}: {}",
    folder_id, error
));

// Bad - exceeds line length
log_debug(&format!("Failed to get files for {}: {}", folder_id, error));
```

**Spacing:**
- Use consistent spacing around operators and delimiters
- Single space after keywords (`if `, `for `, `match `, etc.)
- No space between function name and opening parenthesis

Example:
```rust
// Good
if x > 0 {
    do_something();
}
let result = calculate(a, b);

// Bad
if(x>0){
    do_something();
}
let result = calculate (a, b);
```

**Type annotations:**
- Always include explicit type annotations on public functions
- Use `:` with no space before type: `foo: String`

Example:
```rust
// Good
pub fn get_path() -> PathBuf {
    let cache_dir: Option<PathBuf> = dirs::cache_dir();
    cache_dir
}

// Bad
pub fn get_path() { // missing return type
    let cache_dir = dirs::cache_dir();
}
```

#### Clippy Linting

**Before committing, run:** `cargo clippy -- -D warnings` (failures block CI/CD)

**Key patterns:**
- Use `is_some_and`/`is_none_or` instead of `map_or` with booleans
- Use `.first()` not `.get(0)`, `contains_key()` not `.get().is_none()`
- Use pattern matching instead of unwrap after is_ok/is_err checks
- Collapse nested ifs with `&&`, use `if let` for single pattern matches
- Use `saturating_sub()` for safe subtraction
- Avoid `format!()` for static strings, use string literals or `.to_string()`
- Add `Default` impl for types with `new()`
- Box large enum variants (>3x size difference)
- Prefix unused variables with `_`
- Use `#[allow(clippy::too_many_arguments)]` for Ratatui render functions (idiomatic pattern)

#### GitHub Actions Checks

The CI pipeline runs these checks automatically:
1. **tests.yml** (on PRs and pushes):
   - `cargo test` - Run test suite
   - `cargo fmt -- --check` - Verify formatting
   - `cargo clippy -- -D warnings` - Run linter
   - `cargo build --release` - Build project

2. **release.yml** (on version tags):
   - `cargo test --verbose` - Run all tests before building

**If checks fail:**
1. Run locally to see the issue: `cargo fmt` and `cargo clippy`
2. Fix the issues (most cargo fmt issues auto-fix)
3. Commit the fixes
4. Push again to re-run checks

### Other Instructions

- If you make a change that doesn't work, do not just keep adding more things on. If a change didn't fix things, consider that and revert it before attempting a new solution.
- Use debug logs for general development and troubleshooting
- Make logging comprehensive but concise - debug logs should be informative without overwhelming
- When adding new features or fixes, git commit once user has confirmed working and tests are written

## CRITICAL: Data Type Constants

**BrowseItem.item_type values** (from Syncthing API):
- Directory: `"FILE_INFO_TYPE_DIRECTORY"` (the ONLY directory type)
- File: Anything else (various file types, but NOT equal to `"FILE_INFO_TYPE_DIRECTORY"`)

**NEVER use**:
- ❌ `"directory"`
- ❌ `"file"`
- ❌ `"dir"`

**ALWAYS use**:
- ✅ `item.item_type == "FILE_INFO_TYPE_DIRECTORY"` for checking directories
- ✅ `item.item_type != "FILE_INFO_TYPE_DIRECTORY"` for checking files

See `src/api.rs:43` for `BrowseItem` struct definition.


## Architecture Context

- **Syncthing Environment**: Runs in Docker container
- **CLI Environment**: Runs on host machine
- **Path Translation**: Container paths differ from host paths; use configurable `path_map` to translate container paths to host paths for file operations
- **Data Source**: Syncthing REST API (not direct filesystem scanning)

## Tech Stack

- **Language**: Rust
- **TUI Framework**: Ratatui
- **Dependencies**:
  - `reqwest` (HTTP client)
  - `serde` / `serde_json` / `serde_yaml` (serialization)
  - `crossterm` (terminal handling)
  - `tokio` (async runtime)
  - `rusqlite` (SQLite cache)
  - `ratatui-image` (terminal image rendering)
  - `image` (image processing and resizing)
  - `anyhow` (error handling)
  - `urlencoding` (URL encoding)
  - `dirs` (directory paths)
  - `glob` (pattern matching)
  - `clap` (CLI argument parsing)
  - `unicode-width` (text width calculations)
  - `codepage-437` (CP437 encoding for ANSI art)

## Core Features

### Display & Navigation

- List folders from `/rest/system/config` with real-time status icons
- Browse folder contents via `/rest/db/browse` with recursive traversal
- Multi-pane breadcrumb navigation showing directory hierarchy
- Keyboard navigation:
  - `↑` / `↓`: Navigate items
  - `Enter`: Preview file (if file) or drill into folder (if directory)
  - `←` / `Backspace`: Go back
  - `q`: Quit

### Sync State Icons

Display visual indicators for file/folder states following `<file|dir><status>` pattern:
- `📄✅` / `📁✅` Synced
- `📄☁️` / `📁☁️` Remote-only
- `📄💻` / `📁💻` Local-only
- `📄⚠️` / `📁⚠️` Out-of-sync OR Ignored (exists on disk)
- `📄⏸` / `📁⏸` Paused
- `📄🚫` / `📁🚫` Ignored (deleted from disk)
- `📄🔄` / `📁🔄` Syncing (actively downloading/uploading)
- `📄❓` / `📁❓` Unknown

### User Actions

- `?` or `Enter` (on files): Show detailed file info popup with metadata and preview:
  - **Text files**: Scrollable preview with vim keybindings (j/k, gg/G, Ctrl-d/u/f/b, PgUp/PgDn)
  - **ANSI art files**: Auto-detects ANSI codes, CP437 encoding, 80-column wrapping, SGR colors
  - **Image files**: Terminal rendering (Kitty/iTerm2/Sixel/Halfblocks), non-blocking load
  - **Binary files**: Extracted text or metadata
- `c`: **Context-aware action**:
  - **Folder view** (focus_level == 0): Change folder type (Send Only, Send & Receive, Receive Only) with selection menu
  - **Breadcrumb view** (focus_level > 0): Copy file/directory path to clipboard (uses mapped host paths)
- `i`: Toggle ignore state (add/remove from `.stignore`) via `PUT /rest/db/ignores`
- `I`: Ignore AND delete locally (immediate action, no confirmation)
- `o`: Open file/directory with configured command (e.g., `xdg-open`, `code`, `vim`)
- `d`: Delete file/directory from disk (with confirmation prompt)
- `r`: **Rescan folder** - Shows confirmation dialog with options:
  - `y`: Normal rescan - Trigger Syncthing scan, wait for sequence change to invalidate cache
  - `f`: Force refresh - Immediately invalidate cache and trigger rescan (useful for stale cache bugs)
  - `n` or `Esc`: Cancel
- `R`: Restore deleted files (revert receive-only folder)
- `s`: Cycle sort mode (Sync State → A-Z → Timestamp → Size)
- `S`: Toggle reverse sort order
- `t`: Toggle info display (Off → TimestampOnly → TimestampAndSize → Off)
- `p`: Pause/resume folder (folder view only, with confirmation)
- `u`: **Folder Update History** - Shows recent file updates for the selected folder with lazy-loading pagination
  - Loads files in batches of 100 as you scroll
  - Auto-loads when within 10 items of bottom
  - Press `Enter` on a file to jump directly to that file's location in breadcrumbs
- Vim keybindings (optional): `hjkl`, `gg`, `G`, `Ctrl-d/u`, `Ctrl-f/b`

### Search Feature

**Real-time recursive search** with wildcards (`*`), case-insensitive, shows parent dirs with matching descendants. Trigger with `Ctrl-F` or `/` (vim mode). Search persists when drilling down, context-aware clearing when backing out past origin. SQLite cache enables instant recursive queries across all subdirectories.

### Status Bar & UI Elements

**UI Layout:** System bar → Main content (folders + breadcrumb panels with smart sizing) → Hotkey legend → Status bar

**Folder List (focus_level == 0):**
- Card-based rendering with inline stats (3 lines per folder)
- Shows folder name, state icon, type, size, file count, and status message
- Out-of-sync folders show detailed breakdown (remote needed, local changes)
- Dynamic title shows counts (total, synced, syncing, paused)

**Status Bar (context-aware):**
- **Folder view (focus_level == 0)**: Activity feed + device count
  - Shows last sync activity with timestamp ("SYNCED file 'example.txt' • 5 sec ago")
  - Shows connected device count ("3 devices connected")
- **Breadcrumb view (focus_level > 0)**: Folder/file details + sort mode + filter status

**Key UI features:**
- Context-aware hotkey legend (folder view vs breadcrumb view)
- Three-state file info toggle (Off/TimestampOnly/TimestampAndSize)
- Multi-mode sorting (Sync State/A-Z/Timestamp/Size) with visual indicators
- Scrollbar indicators on breadcrumb panels when content exceeds viewport
- Confirmation dialogs for destructive operations

### Configuration

YAML config at `~/.config/stui/config.yaml` (Linux) with: API key, base URL, `path_map`, `vim_mode`, `icon_mode`, `open_command`, `clipboard_command`, image preview settings

CLI flags: `--debug`, `--vim`, `--config <path>`

## Syncthing REST API Endpoints

```
/rest/system/config                           # Get folders and devices
/rest/config/folders/<id>                     # PATCH to modify folder config (e.g., pause/resume, folder type)
/rest/db/status?folder=<id>                   # Folder sync status (with sequence numbers)
/rest/db/browse?folder=<id>[&prefix=subdir/]  # Browse contents
/rest/db/file?folder=<id>&file=<path>         # Get file sync details
/rest/db/ignores?folder=<id>                  # GET/PUT .stignore rules
/rest/db/scan?folder=<id>                     # Trigger folder rescan
/rest/db/revert?folder=<id>                   # Revert receive-only folder
/rest/db/localchanged?folder=<id>             # Get local changes (receive-only)
/rest/db/need?folder=<id>                     # Get files needed from remote
/rest/stats/folder                            # Folder statistics (matches web GUI)
/rest/events?since=<id>&timeout=60            # Event stream (long-polling)
/rest/system/status                           # System status (device info, uptime)
/rest/system/connections                      # Connection/transfer statistics
```

## Architecture Highlights

### Code Organization

**Main modules:**
- `src/main.rs` (~1,180 lines) - App struct, main event loop (starts ~line 909)
- `src/app/` - App orchestration: file_ops, filters, ignore, navigation, preview, sorting, sync_states
- `src/handlers/` - Event handlers: keyboard, api, events
- `src/services/` - Background: api (async queue), events (long-polling)
- `src/model/` - Pure state (Elm): syncthing, navigation, ui, performance, types
- `src/logic/` - Pure business logic (16 modules): file, folder, folder_card, formatting, ignore, layout, navigation, path, performance, platform, search, sorting, sync_states, ui, errors
- `src/ui/` - Rendering (13 modules): render, folder_list (card-based), breadcrumb, dialogs, icons, legend, search, status_bar (activity feed), system_bar, out_of_sync_summary (filter modal), toast, layout
- `src/api.rs`, `src/cache.rs`, `src/config.rs`, `src/utils.rs` - Core utilities

**Key patterns:** App initialization loads folders, spawns services. Main event loop (~line 909) processes API responses, keyboard, cache events. Keyboard handler has confirmation dialogs first.

**CRITICAL Architecture Rules:**
1. **UI Side Effects (toasts, dialogs) MUST be in `handlers/keyboard.rs`**
   - ❌ WRONG: Calling `show_toast()` in helper methods in `main.rs` or `src/app/`
   - ✅ CORRECT: Calling `show_toast()` in keyboard handler where user action happens
   - Helper methods in `main.rs` and `src/app/` should only do business logic (update state, call APIs)
   - All user feedback (toasts, error messages) belongs at the call site in keyboard handler

2. **Separation of Concerns:**
   - `src/api.rs`: Pure API client methods (no UI, no state mutation beyond return values)
   - `src/handlers/keyboard.rs`: Keyboard events → business logic → UI feedback (toasts, dialogs)
   - `src/main.rs` + `src/app/`: Orchestration methods (pure business logic, no UI side effects)
   - `src/model/`: Pure state (cloneable, no side effects, no I/O)
   - `src/logic/`: Pure functions (testable, no state mutation, no I/O)
   - `src/ui/`: Pure rendering (takes state, returns widgets, no mutation)

3. **Adding UI Feedback Pattern:**
   ```rust
   // ❌ WRONG - toast in helper method (main.rs or src/app/)
   fn cycle_sort_mode(&mut self) {
       self.model.ui.sort_mode = new_mode;
       self.model.ui.show_toast("Sort changed"); // WRONG!
   }

   // ✅ CORRECT - toast at call site in keyboard handler
   KeyCode::Char('s') => {
       app.cycle_sort_mode(); // Pure business logic
       app.model.ui.show_toast(format!("Sort: {}", app.model.ui.sort_mode.as_str())); // UI feedback here
   }
   ```

### Event-Driven Cache Invalidation
Long-polling `/rest/events` for real-time updates. Granular invalidation (file/dir/folder). Handles LocalIndexUpdated, ItemStarted, ItemFinished. Persistent event ID, auto-recovery.

**Activity Event Deduplication:** Activity events from ItemFinished are deduplicated by timestamp. Only events newer than existing activity are stored, preventing event replay from overwriting fresh data during event stream reconnection.

### Performance Optimizations
Async API service with priority queue, cache-first rendering, sequence-based validation, request deduplication, 300ms idle threshold, 250ms poll timeout (~1-2% CPU idle).

### Caching Strategy
SQLite at `~/.cache/stui/cache.db` with browse, sync state, folder status caches. Event ID persists. Manual clear on schema changes: `rm ~/.cache/stui/cache.db`

### ANSI Art Rendering
Auto-detects ANSI codes (ESC[ sequences), CP437 encoding, 80-column wrapping, line buffer with cursor positioning, SGR colors (fg 30-37/90-97, bg 40-47/100-107), SAUCE stripping.

### State Transition Validation
`ManualStateChange` tracks SetIgnored/SetUnignored. After SetIgnored → only accept Ignored. After SetUnignored → accept any except Ignored. 10s safety timeout. `syncing_files` HashSet tracks ItemStarted/ItemFinished.

## Current State

**603 tests passing**, zero warnings, clean Model/Runtime separation. Full ANSI/CP437 support. Version 0.10.0.

## Development Guidelines

### Safety & Best Practices
- **Safety First**: All destructive operations require confirmation (except `I` which is intentionally immediate)
- **Path Mapping**: Always translate container paths to host paths before file operations
- **Error Handling**: Graceful degradation, show errors in status bar or toast messages
- **Non-Blocking**: Keep UI responsive during all API calls
- **Cache Coherency**: Use sequence numbers to validate cached data
- **Testing - CRITICAL REQUIREMENT**:
  - **Test-Driven Development is MANDATORY** (see top of file for detailed TDD workflow)
  - **ALWAYS write tests when:**
    1. Adding new features (especially state management)
    2. Fixing bugs or edge cases
    3. Refactoring existing code
    4. Adding new model fields or business logic
  - **Test-Driven Development Pattern:**
    1. User reports bug or requests feature
    2. **IMMEDIATELY think: "What tests do I need?"**
    3. Write tests FIRST that cover:
       - Happy path (expected behavior)
       - Edge cases (boundary conditions)
       - Error cases (what happens when things go wrong)
       - State transitions (before/after)
    4. Implement the feature/fix
    5. Run tests to verify
    6. If tests fail, fix implementation (not tests)
  - **Test Coverage Requirements:**
    - Model state changes → tests in `src/model/*/tests`
    - Business logic → tests in `src/logic/*/tests`
    - Integration tests → `tests/*.rs` files (see `tests/reconnection_test.rs`)
    - Aim for 100% coverage of new code paths
  - **Real-World Success Story - Commit fcf4362:**
    - Problem: Folders not populating after reconnection (cost $20 debugging blind)
    - TDD Approach: Wrote 10 tests first exposing exact bug
    - Test `test_state_already_connected_before_system_status` revealed root cause
    - Solution: Simple 1-line fix guided by tests
    - Result: All tests pass, bug fixed perfectly on first try
    - **Lesson: TDD saves time and money**
  - **When Claude forgets to write tests:**
    - User should immediately call it out
    - Claude should apologize and write tests before proceeding
    - This is a critical discipline for production code quality
  - **Existing test guidelines:**
    - Test with real Syncthing Docker instances with large datasets
    - Pure business logic in `src/logic/` should have comprehensive test coverage
    - Model state transitions should have tests in corresponding test modules
    - Run `cargo test` before committing to ensure all 603+ tests pass
    - Aim for zero compiler warnings (`cargo build` should be clean)
  - **Test Organization Standards:**
    - **Keep tests inline** using `#[cfg(test)] mod tests` at the bottom of each module
    - **Use section headers** for visual organization when files have >10 tests:
      ```rust
      // ========================================
      // SECTION NAME
      // ========================================
      ```
    - **Group tests logically** by feature/function being tested
    - **When to reorganize:**
      - File has >20 tests and they're randomly ordered → Major reorganization
      - File has >10 tests but well-ordered → Add section headers only
      - File has <10 tests → No changes needed
    - **Examples of well-organized test modules:**
      - `src/logic/file.rs` - 35 tests in 5 sections (Image Detection, Binary Detection, ANSI Code Detection, ANSI Parsing, Binary Text Extraction)
      - `src/logic/ignore.rs` - 13 tests in 4 sections (Pattern Matching, Find Matching, Validation Valid/Invalid/Edge Cases)
      - `src/model/ui.rs` - 16 tests in 4 sections (UI Model Creation, Search Mode, Search Query Operations, Search Origin Level)
      - `src/logic/navigation.rs` - 14 tests in 4 sections (Next Selection, Prev Selection, Edge Cases, Find Item By Name)
    - **Benefits:** Tests can be collapsed by section in IDEs, clear grouping makes finding related tests easy, maintains locality with implementation code
- **Debug Mode**: Use `--debug` flag for verbose logging to `/tmp/stui-debug.log`

### Adding New Features - Common Patterns

**Adding a new API endpoint:**
1. Add method to `SyncthingClient` in `src/api.rs` (follow existing patterns)
2. Add request type to `ApiRequest` enum in `src/services/api.rs` if using async service
3. Add response type to `ApiResponse` enum in `src/services/api.rs`
4. Add handler in `src/handlers/api.rs` to process response

**Adding a new keybinding:**
1. Add state to `Model` (usually `model.ui` for dialogs/popups)
2. Add keybinding handler in `src/handlers/keyboard.rs`
   - Confirmation dialogs go at top of match statement (processed first)
   - Regular keys go in main match block with conditional guards (e.g., `focus_level == 0`)
3. Add dialog rendering in `src/ui/dialogs.rs` (if confirmation needed)
4. Add rendering call in `src/ui/render.rs`
5. Update hotkey legend in `src/ui/legend.rs` with context guards

**Example 1: Pause/Resume Feature (confirmation dialog pattern)**
- API: `src/api.rs` - `set_folder_paused()` using PATCH `/rest/config/folders/{id}`
- State: `model.ui.confirm_pause_resume: Option<(folder_id, label, is_paused)>`
- Keybinding: `KeyCode::Char('p') if focus_level == 0` opens confirmation
- Confirmation: Handles 'y' (execute), 'n'/Esc (cancel)
- Execution: Call API, reload folders via `client.get_folders()`, update `model.syncthing.folders`
- Dialog: `render_pause_resume_confirmation()` with color-coded borders
- Legend: Shows "p:Pause/Resume" only in folder view
- Visual: Pause icon (⏸ emoji /  nerdfont) via `FolderState::Paused`

**Example 2: Change Folder Type (selection menu pattern)**
- API: `src/api.rs` - `set_folder_type()` using PATCH `/rest/config/folders/{id}` with `{"type": "sendonly|sendreceive|receiveonly"}`
- Data: `Folder` struct has `folder_type: String` field (serde renamed from "type")
- State: `model.ui.folder_type_selection: Option<FolderTypeSelectionState>` with `folder_id`, `folder_label`, `current_type`, `selected_index`
- Keybinding: `KeyCode::Char('c') if focus_level == 0` opens selection menu (context-aware - 'c' copies path in breadcrumbs)
- Selection Menu:
  - Uses `List` widget with ↑↓ navigation, Enter to select, Esc to cancel
  - Current type highlighted in cyan/italic
  - Handler near top of keyboard.rs (check for `folder_type_selection` match)
- Execution: Call API, reload folders, update `model.syncthing.folders`, show toast
- Dialog: `render_folder_type_selection()` shows 3 options with user-friendly names
- Legend: Shows "c:Change Type" only in folder view
- Status Bar: Displays folder type (Send Only, Send & Receive, Receive Only) before state field

**State management patterns:**
- **Model fields**: All application state lives in `Model` struct (pure, cloneable)
- **Runtime fields**: Services, channels, caches in `App` struct (not cloneable)
- **State updates**: Mutate `app.model.*` directly, reload from API when needed
- **Toast messages**: `app.model.ui.show_toast()` for user feedback
- **Modal dialogs**: Set `model.ui.confirm_*` field, handled at top of keyboard handler

**UI rendering patterns:**
- **Icon rendering**: Use `IconRenderer` with `FolderState` or `SyncState` enums
- **Scrollbars**: Automatically rendered by breadcrumb panels using Ratatui's `Scrollbar` widget
- **Context-aware display**: Check `focus_level` to show/hide keys in legend
- **Color coding**: Use `Color::Cyan` (focused), `Color::Blue` (parent), `Color::Gray` (inactive)
- **Text wrapping**:
  - Legend uses `.wrap(ratatui::widgets::Wrap { trim: false })` for text wrapping
  - Fixed height of 3 lines (`Constraint::Length(3)`) - wraps content within available space
  - System bar and status bar also use `Constraint::Length(3)` (fixed height)
  - Note: On very narrow terminals, some hotkeys may be clipped if content exceeds 3 lines
