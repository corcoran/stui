# Elm Architecture Rewrite - Preparation Guide

This document outlines the current state of the refactoring and provides guidance for the Phase 2 Elm Architecture rewrite.

---

## ✅ Phase 1 Complete (27.7% Reduction)

### What We Accomplished

**Before:** 4,570 lines in monolithic `main.rs`
**After:** 3,302 lines with clear module boundaries
**Reduction:** -1,268 lines (27.7%)

### New Architecture

```
src/
├── handlers/           # Event handling (1,233 lines extracted)
│   ├── keyboard.rs     # Keyboard input (~554 lines)
│   ├── api.rs          # API responses (~481 lines)
│   └── events.rs       # Cache invalidation (~198 lines)
│
├── logic/              # Pure business logic (9 functions, 27 tests)
│   ├── ignore.rs       # Pattern matching (7 tests)
│   ├── sync_states.rs  # State priority & existence checking (6 tests)
│   └── path.rs         # Path translation (5 tests)
│
├── services/           # External I/O (736 lines reorganized)
│   ├── api.rs          # API request queue
│   └── events.rs       # Event stream listener
│
├── messages.rs         # Unified message enum (ready for Elm)
├── state.rs            # State modules (with tests)
└── main.rs             # App coordinator (3,302 lines)
```

### Test Coverage

- **27 tests passing** (all pure business logic)
- **Zero compilation warnings**
- **Manual testing verified** - all features working

---

## 🎯 What Remains in main.rs (3,302 lines)

### Legitimate Controller Logic (~2,800 lines)

**Large Orchestration Methods:**
- `load_root_level()` - 127 lines - Async folder loading
- `enter_directory()` - 173 lines - Breadcrumb navigation
- `toggle_ignore()` - 226 lines - Complex ignore workflow
- `ignore_and_delete()` - 139 lines - Filesystem + API coordination

**Background Operations:**
- Prefetch methods (~300 lines)
- State caching (~200 lines)
- Directory discovery

**Navigation & Sorting:**
- `next_item()`, `previous_item()`, etc. (~200 lines)
- Sorting coordination (~100 lines)

**State Management:**
- Pending delete tracking (~150 lines)
- Cache management (~200 lines)
- Ignored file tracking

### Why This Logic Stays (For Now)

These methods are the **controller layer** - they:
- Coordinate multiple services
- Manage complex async workflows
- Handle side effects
- Maintain application invariants
- Mutate App state directly

Forcing further extraction would scatter this orchestration logic and make the code worse.

---

## 📋 Phase 2: Elm Architecture Rewrite

### Why Model is the Foundation

In Elm Architecture, **everything flows from the Model**:

1. **Model defines what data exists** - Forces you to think about state first
2. **Msg defines what can change** - Based on what the Model needs
3. **Update defines how it changes** - Pure transformations of Model
4. **View renders Model** - Pure function of current state
5. **Cmd executes side effects** - But Model never sees them

**The key insight:** If you can't clone/serialize your state, you can't:
- Test it easily (need real services)
- Debug it (can't snapshot)
- Reason about it (hidden in services)

**Current App is backwards:** Services mixed with state, making it impossible to separate concerns.

**Elm Architecture fixes this:** Model is pure data, everything else serves the Model.

### The Three Pillars

Elm Architecture has three core concepts:

**1. Model** - Pure application state (the foundation)
```rust
#[derive(Clone, Debug)]
struct Model {
    folders: Vec<Folder>,
    breadcrumb_trail: Vec<BreadcrumbLevel>,
    sort_mode: SortMode,
    // ... all data, NO services, NO channels
}
```

**2. Msg** - All events that can happen
```rust
enum Msg {
    KeyPress(KeyEvent),
    FoldersLoaded(Result<Vec<Folder>>),
    BrowseResult { folder_id: String, items: Vec<BrowseItem> },
    // ... everything that can change state
}
```

**3. Update** - Pure state transitions
```rust
fn update(model: Model, msg: Msg) -> (Model, Cmd) {
    // Returns NEW model + side effects to execute
    // No &mut, no .await, completely pure
}
```

### The Critical Separation: Model vs Runtime

**Current Problem:**
```rust
// ❌ CURRENT: App mixes state with services
struct App {
    // STATE (belongs in Model):
    folders: Vec<Folder>,           // ✅
    breadcrumb_trail: Vec<...>,     // ✅

    // SERVICES (belongs in Runtime):
    client: SyncthingClient,        // ❌
    cache: CacheDb,                 // ❌
    api_tx: channels,               // ❌
}
```

**After Elm Pattern:**
```rust
// ✅ MODEL: Pure state (cloneable, serializable)
#[derive(Clone, Debug)]
struct Model {
    folders: Vec<Folder>,
    breadcrumb_trail: Vec<BreadcrumbLevel>,
    // ... all data, NO services
}

// ✅ RUNTIME: Services that do I/O
struct Runtime {
    client: SyncthingClient,
    cache: CacheDb,
    msg_tx: Sender<Msg>,
    // ... all I/O
}

// ✅ Pure update function
fn update(model: Model, msg: Msg) -> (Model, Cmd) {
    match msg {
        Msg::KeyPress(key) => {
            let mut model = model;
            model.last_user_action = Instant::now();

            let cmd = match key.code {
                KeyCode::Char('q') => {
                    model.should_quit = true;
                    Cmd::None
                }
                KeyCode::Char('i') => {
                    Cmd::ToggleIgnore {
                        folder_id: model.current_folder_id(),
                        item: model.selected_item(),
                    }
                }
                _ => Cmd::None,
            };

            (model, cmd)
        }
        Msg::BrowseResult { folder_id, items } => {
            let mut model = model;
            // Update breadcrumb with new items
            model.add_breadcrumb_level(folder_id, items);
            (model, Cmd::None)
        }
        // ...
    }
}

// ✅ Runtime executes commands
impl Runtime {
    async fn execute(&self, cmd: Cmd) {
        match cmd {
            Cmd::ToggleIgnore { folder_id, item } => {
                let result = self.client.set_ignore(...).await;
                self.msg_tx.send(Msg::IgnoreToggled { result });
            }
            // ...
        }
    }
}
```

**📖 See MODEL_DESIGN.md for complete concrete examples.**

### Required Changes

#### 1. **Command Enum** (New)

```rust
pub enum Command {
    // API commands
    FetchFileInfo { folder_id: String, path: String },
    FetchFolderStatus { folder_id: String },
    BrowseFolder { folder_id: String, prefix: String },
    SetIgnorePatterns { folder_id: String, patterns: Vec<String> },
    RescanFolder { folder_id: String },

    // Filesystem commands
    DeleteFile { path: PathBuf },
    CheckFileExists { path: PathBuf },

    // Composite commands
    Batch(Vec<Command>),
    None,
}
```

#### 2. **Pure Update Function**

Transform current handlers:

**Current (side effects mixed in):**
```rust
async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('i') => {
            self.toggle_ignore().await?; // Side effect!
        }
        ...
    }
}
```

**After (pure):**
```rust
fn update_key(state: AppState, key: KeyEvent) -> (AppState, Command) {
    match key.code {
        KeyCode::Char('i') => {
            let new_state = state.clone();
            let cmd = Command::ToggleIgnore {
                folder_id: state.current_folder(),
                path: state.selected_item_path()
            };
            (new_state, cmd)
        }
        ...
    }
}
```

#### 3. **Domain Models** (New)

Create `src/domain/`:

```rust
// domain/folder.rs
pub struct Folder {
    pub id: String,
    pub path: String,
    pub state: FolderState,
}

impl Folder {
    pub fn can_browse(&self) -> bool {
        !self.state.is_paused()
    }
}

// domain/sync_state.rs
pub enum SyncState {
    Synced,
    OutOfSync,
    // ... with state machine methods
}

impl SyncState {
    pub fn can_transition_to(&self, new_state: SyncState) -> bool {
        // State transition validation
    }
}
```

#### 4. **Runtime/Services Layer**

Separate pure state from I/O:

```rust
pub struct Runtime {
    api: Arc<SyncthingClient>,
    cache: Arc<CacheDb>,
    filesystem: Arc<dyn Filesystem>,
    message_tx: Sender<AppMessage>,
}

impl Runtime {
    async fn execute(&self, cmd: Command) {
        match cmd {
            Command::FetchFileInfo { folder_id, path } => {
                let result = self.api.get_file_info(&folder_id, &path).await;
                self.message_tx.send(AppMessage::FileInfoReceived(result));
            }
            Command::Batch(cmds) => {
                for cmd in cmds {
                    self.execute(cmd).await;
                }
            }
            ...
        }
    }
}
```

---

## 🚀 Migration Strategy

**Model-First Approach:** Start with the foundation and build up.

### Step 1: Extract Model Struct ⭐ ✅ COMPLETE
- ✅ Created `src/model.rs` with pure Model struct
- ✅ Model is `Clone + Debug` (490 lines, 7 tests passing)
- ✅ Supporting types: VimCommandState, PatternSelectionState, BreadcrumbLevel, PendingDeleteInfo
- ✅ Helper methods: selected_folder(), current_level(), is_idle(), has_modal(), etc.
- ✅ All tests passing (34 total)

**📖 See MODEL_DESIGN.md for complete Model structure**

### Step 2: Integrate Model into App ⭐ ✅ 100% COMPLETE

**What's Done:**
- ✅ Created sub-model architecture with 4 focused domains:
  - `SyncthingModel`: API data (folders, devices, statuses, system info)
  - `NavigationModel`: Breadcrumbs, focus level, folder selection
  - `UiModel`: Preferences, dialogs, popups, visual state
  - `PerformanceModel`: Loading tracking, metrics, pending operations
- ✅ Migrated ALL ~40 state fields to sub-models
- ✅ Converted all ListState to Option<usize> (pure state)
- ✅ Removed ui_dirty flag (always-render approach)
- ✅ Updated ~150+ references across codebase
- ✅ All 48 tests passing, zero compilation errors
- ✅ Fixed image preview regression (ImagePreviewState in Runtime)

**Current Structure:**
```rust
pub struct Model {
    pub syncthing: SyncthingModel,     // API data (12 fields)
    pub navigation: NavigationModel,   // Breadcrumbs, focus (3 fields)
    pub ui: UiModel,                   // Preferences, popups (13 fields)
    pub performance: PerformanceModel, // Tracking, metrics (13 fields)
}

pub struct App {
    // ✅ Pure application state (Elm Architecture Model)
    pub model: Model,  // ALL state fields migrated

    // 🔧 Services (Runtime) - NOT part of Model
    client: SyncthingClient,
    cache: CacheDb,
    api_tx/rx, event channels,
    icon_renderer, image_picker,
    image_state_map,  // ImagePreviewState not Clone
    // ... config, timing, etc.
}
```

### Step 2.5: Define Msg Enum ⭐ ✅ COMPLETE (UNUSED)

**What's Done:**
- ✅ Renamed AppMessage → Msg (Elm convention)
- ✅ Organized variants by domain concern
- ✅ Expanded ApiResponse into explicit variants:
  - BrowseResult, FileInfoResult, FolderStatusResult
  - RescanResult, SystemStatusResult, ConnectionStatsResult
- ✅ Added helper constructors for all variants

**Current State:**
- ✅ Msg enum fully defined and documented
- ⚠️ NOT integrated into main loop (still using separate channels)
- 📝 Marked with #[allow(dead_code)] for now
- 💡 Available for gradual integration

**Why Not Integrated:**
Full Elm Architecture requires:
1. Unified message channel (merge all rx channels)
2. Pure update() function (no &mut, no .await)
3. Command enum for side effects
4. Runtime executor loop

This is a MASSIVE refactoring. Instead, we'll take an **incremental approach**.

---

## 🔄 Next Phase: Incremental Functional Migration

Instead of a big-bang Elm Architecture rewrite, we'll gradually make the codebase more functional **without breaking existing code**.

### Philosophy

**Goal:** Make code more testable, predictable, and maintainable without rewriting everything.

**Approach:** Small, isolated refactorings that each provide immediate value.

**Non-Goal:** Perfect Elm Architecture purity (we're building a TUI app, not a web framework).

### Step 3: Extract Pure Business Logic (NEXT)

**Target:** Handler functions that do state calculations.

**Strategy:** Extract calculation logic into pure functions that return new state instead of mutating.

**Example Refactoring:**

```rust
// BEFORE: Mutation-based (current)
async fn handle_key(&mut self, key: KeyEvent) {
    match key.code {
        KeyCode::Char('s') => {
            // Inline state mutation
            self.model.ui.sort_mode = match self.model.ui.sort_mode {
                SortMode::Icon => SortMode::Alphabetical,
                SortMode::Alphabetical => SortMode::DateTime,
                SortMode::DateTime => SortMode::Size,
                SortMode::Size => SortMode::Icon,
            };
        }
    }
}

// AFTER: Pure calculation + mutation
async fn handle_key(&mut self, key: KeyEvent) {
    match key.code {
        KeyCode::Char('s') => {
            // Pure function returns new state
            self.model.ui.sort_mode = logic::cycle_sort_mode(self.model.ui.sort_mode);
        }
    }
}

// In src/logic/ui.rs (pure, testable)
pub fn cycle_sort_mode(current: SortMode) -> SortMode {
    match current {
        SortMode::Icon => SortMode::Alphabetical,
        SortMode::Alphabetical => SortMode::DateTime,
        SortMode::DateTime => SortMode::Size,
        SortMode::Size => SortMode::Icon,
    }
}

#[test]
fn test_cycle_sort_mode() {
    assert_eq!(cycle_sort_mode(SortMode::Icon), SortMode::Alphabetical);
    // ... more tests
}
```

**Benefits:**
- ✅ Pure logic is testable without App instance
- ✅ No behavior changes (still mutates, just calls pure function)
- ✅ Incremental (one function at a time)
- ✅ No refactoring debt (each extraction is immediately useful)

**Target Functions (Priority Order):**

1. **UI State Transitions** (~5 functions, easy wins):
   - `cycle_sort_mode()` - sort mode cycling
   - `toggle_sort_reverse()` - reverse sorting
   - `cycle_display_mode()` - info display cycling
   - `next_vim_command_state()` - vim key sequences
   - `should_dismiss_toast()` - toast timeout logic

2. **Navigation Logic** (~3 functions):
   - `calculate_scroll_position()` - breadcrumb scrolling
   - `next_focus_level()` / `prev_focus_level()` - focus navigation
   - `should_show_hotkey()` - context-aware hotkey display

3. **Validation Logic** (~4 functions):
   - `can_delete_file()` - delete permission checks
   - `can_revert_folder()` - revert validation
   - `should_show_restore()` - restore button visibility
   - `validate_ignore_pattern()` - pattern syntax checking

**Migration Process:**

1. Identify calculation in handler
2. Extract to `src/logic/[domain].rs`
3. Add tests for edge cases
4. Replace inline code with function call
5. Verify no behavior change
6. Commit

**Timeline:** ~1-2 functions per session, ~15 functions total = 2-3 weeks of incremental work.

### Step 4: Make Handlers Return Outcomes (LATER)

Once we have pure logic extracted, we can start making handlers more functional:

```rust
// Current: Mutation-based
async fn handle_browse_result(&mut self, folder_id: String, items: Vec<BrowseItem>) {
    self.model.navigation.breadcrumb_trail.push(...);
    self.cache.save_browse_result(...);
}

// Future: Outcome-based
async fn handle_browse_result(&mut self, folder_id: String, items: Vec<BrowseItem>) {
    let outcome = logic::process_browse_result(&self.model, folder_id, items);
    outcome.apply_to(&mut self.model);  // Still mutates, but via explicit outcome
    outcome.execute_side_effects(self).await;  // Side effects separated
}
```

This is OPTIONAL and can be done gradually after Step 3.

### Step 5: Full Elm Architecture (OPTIONAL)

If we want to go all the way:
1. Merge channels into unified Msg stream
2. Create pure `update(model, msg) -> (Model, Cmd)` function
3. Add Command enum for side effects
4. Add Runtime executor

**But this is NOT required** for the benefits we want (testability, maintainability, predictability).

---

## 📊 Current Status Summary

**Completed:**
- ✅ Phase 1: Modularization (27.7% code reduction)
- ✅ Step 1: Pure Model struct with sub-models
- ✅ Step 2: 100% Model migration (all state pure)
- ✅ Step 2.5: Msg enum defined (unused but ready)

**Current Architecture Quality:**
- ✅ Clean separation: Model (pure) vs Runtime (services)
- ✅ Sub-models organized by domain
- ✅ All tests passing (48 tests)
- ✅ Zero compilation errors
- ✅ Always-render approach (no dirty flags)

**Next Steps:**
- 🎯 Step 3: Extract pure business logic (15 functions)
  - Start with UI state transitions (easy wins)
  - Add tests for each extracted function
  - No behavior changes, just better organization

**Long-Term Options:**
- 🤔 Step 4: Outcome-based handlers (optional)
- 🤔 Step 5: Full Elm Architecture (optional)

### Step 3: Define Cmd Enum (DEPRECATED)
**Note:** Skipping this in favor of incremental approach. Command enum only needed for full Elm Architecture.

### Step 4: Create Pure Update Function (DEPRECATED)
**Note:** Skipping this in favor of incremental approach. Pure update() only needed for full Elm Architecture.
- Start with ONE message type (pick simplest)
- Write `update(model: Model, msg: Msg) -> (Model, Cmd)`
- Example: `Msg::KeyPress('q')` → quit
- Run alongside existing handlers (don't break anything)

### Step 5: Wire Up Main Loop
- Separate model from runtime in main loop
- Call `update()` for new message types
- Execute returned commands via Runtime
- Keep old handlers for unmigrated messages

### Step 6: Migrate Message-by-Message
- Convert one Msg type at a time to pure update
- Order: simplest → most complex
  1. `Msg::Tick` (just updates timestamps)
  2. `Msg::CacheInvalidation` (updates breadcrumb)
  3. `Msg::BrowseResult` (navigation state)
  4. `Msg::KeyPress` (most complex, do last)

### Step 7: Remove Old Handlers
- Once all messages migrated, delete old handler code
- Runtime now ONLY executes commands
- App becomes pure Model + Update + Runtime

**⚠️ Critical:** Each step should compile and work. Never break the app.

---

## 📊 Expected Outcome

**After Phase 2:**
- `main.rs` - ~50 lines (just event loop)
- `app.rs` - ~100 lines (coordinates update loop)
- `logic/` - Pure functions (tested)
- `domain/` - Business models (tested)
- `handlers/` - Pure update functions (tested)
- `services/` - I/O execution (integration tested)

**Benefits:**
- ✅ **Model is cloneable** → snapshot state for undo/redo
- ✅ **Model is serializable** → save/restore app state
- ✅ **Pure update function** → 80%+ test coverage (no mocking)
- ✅ **Predictable state changes** → all through `update()`
- ✅ **Time-travel debugging** → replay any sequence of Msg
- ✅ **Reproduce bugs** → serialize Msg history, replay exactly
- ✅ **Easy debugging** → inspect Model at any point
- ✅ **No side effects in tests** → test update() is pure function calls

---

## ⚠️ Challenges & Tradeoffs

### Challenges

1. **Async Complexity**
   - Elm is synchronous, Rust is async
   - Commands need careful async handling
   - Might need `Runtime` to spawn tasks

2. **Performance**
   - Pure updates might clone state
   - Consider `Rc<T>` or structural sharing
   - Profile hot paths

3. **Borrow Checker**
   - Pure functions easier than `&mut self`
   - But command execution might be tricky
   - May need interior mutability (`RefCell`)

4. **Migration Effort**
   - ~2-3 full sessions
   - Need comprehensive testing
   - Risk of introducing bugs

### Tradeoffs

**Pros:**
- Much more testable
- Easier to reason about
- Better for collaboration
- Cleaner architecture

**Cons:**
- More boilerplate (Command enum, execute functions)
- Steeper learning curve
- Potential performance overhead
- Migration risk

---

## 🎓 Resources

**Elm Architecture:**
- [The Elm Architecture Guide](https://guide.elm-lang.org/architecture/)
- [Redux (similar pattern)](https://redux.js.org/understanding/thinking-in-redux/motivation)

**Rust Examples:**
- [iced](https://github.com/iced-rs/iced) - Rust GUI using Elm pattern
- [relm](https://github.com/antoyo/relm) - GTK+ Elm pattern

**Relevant Patterns:**
- Command Pattern
- Event Sourcing
- CQRS (Command Query Responsibility Segregation)

---

## ✅ Phase 2 Progress Tracker

### Step 1: Extract Model Struct ✅ COMPLETE

**Completed:**
- [x] Phase 1 complete (27.7% reduction)
- [x] Created `src/model.rs` with pure Model struct
- [x] Model is Clone + Debug (490 lines)
- [x] All supporting types defined
- [x] Helper methods implemented
- [x] All tests passing (34 total, including 7 new model tests)

### Step 2: Integrate Model into App ✅ 90% COMPLETE

**Completed:**
- [x] Added model field to App struct
- [x] Migrated 21 major state fields
- [x] Updated 100+ references across codebase
- [x] All tests passing
- [x] Zero compilation errors
- [x] Manual testing verified (app runs correctly)

**Optional Remaining:**
- [ ] Migrate performance tracking fields (8 fields) - optional
- [ ] Convert ListState → Option<usize> (complex) - optional
- [ ] Migrate breadcrumb state management - optional

### Step 3: Define Cmd Enum ⏳ READY TO START

**Ready when:**
- Model migration is complete (90% done)
- Understanding of Elm Architecture patterns
- Ready to define side effects as commands

### Understanding Checklist

**Completed:**
- [x] **Read:** MODEL_DESIGN.md (understand Model vs Runtime)
- [x] **Understand:** Model = pure data (cloneable, serializable)
- [x] **Practice:** Successfully migrated 21 fields to Model
- [x] **Verify:** All tests passing with new architecture

**Still Learning:**
- [ ] **Understand:** Update = pure function (no &mut, no .await)
- [ ] **Understand:** Runtime executes Cmd, sends Msg back
- [ ] **Read:** [Elm Architecture Guide](https://guide.elm-lang.org/architecture/) (optional)

---

## 💡 Current Status & Recommendation

**Excellent progress! Phase 2 is 90% complete.** You now have:
- ✅ Pure, cloneable Model (21 fields migrated)
- ✅ Clear separation: Model (state) vs App (services)
- ✅ All tests passing (34 tests)
- ✅ Zero compilation errors
- ✅ Working application with cleaner architecture

**Current Architecture Benefits:**
- 🎯 Model can be cloned/snapshotted for debugging
- 🧪 Pure state easier to test (no mocking services)
- 📖 Clear separation of concerns (state vs I/O)
- 🔄 Foundation ready for pure update functions

**Next Steps - You Have Options:**

1. **Option A: Continue Elm Architecture (Recommended)**
   - Define Cmd enum (Step 3)
   - Create pure update function (Step 4)
   - Wire up main event loop (Step 5)
   - Full Elm Architecture benefits

2. **Option B: Finish Optional Migrations**
   - Migrate performance tracking fields
   - Convert ListState to Option<usize>
   - Complete Model purity (95%+)

3. **Option C: Stop Here and Use Current State**
   - Current architecture is already much cleaner
   - Model provides snapshot/debugging benefits
   - Can add features with current setup
   - Revisit full Elm pattern later if needed

**Recommendation:** The foundation is excellent. Proceeding to Step 3 (Cmd enum) would be the natural next step to unlock the full power of the Elm Architecture pattern. However, the current state is already a significant improvement and perfectly usable.

---

**Last Updated:** 2025-10-31
**Current Version:** Phase 2 - Step 2 (90% complete)
- Phase 1: ✅ Complete (27.7% reduction)
- Step 1: ✅ Complete (Model extraction)
- Step 2: ✅ 90% Complete (Model integration - 21 fields migrated)
