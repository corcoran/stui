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

### Core Elm Pattern

```rust
// Pure update function
fn update(state: AppState, msg: AppMessage) -> (AppState, Command) {
    match msg {
        AppMessage::KeyPress(key) => {
            // Pure state transition
            let new_state = ...;
            let command = Command::FetchFileInfo { ... };
            (new_state, command)
        }
        ...
    }
}

// Side effect execution (outside update loop)
async fn execute_command(cmd: Command, runtime: &Runtime) {
    match cmd {
        Command::FetchFileInfo { folder_id, path } => {
            let result = runtime.api.get_file_info(...).await;
            runtime.send_message(AppMessage::FileInfoReceived(result));
        }
        ...
    }
}
```

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

### Step 1: Create Command Enum
- Define all side effects as commands
- Start simple (no execution yet)

### Step 2: Extract One Handler
- Pick simplest handler (maybe `handle_cache_invalidation`)
- Convert to pure `update()` function
- Return `(AppState, Command)` instead of mutating
- Keep old handler for comparison

### Step 3: Add Command Executor
- Create `Runtime` struct
- Implement `execute_command()`
- Wire up to message loop

### Step 4: Migrate Handlers One-by-One
- `handle_cache_invalidation` (simplest)
- `handle_api_response` (medium complexity)
- `handle_key` (most complex)

### Step 5: Extract Domain Models
- Move business rules to `domain/`
- Pure state machine logic
- Testable without I/O

### Step 6: Pure App State
- Remove all `&mut self` from App
- All state changes through `update()`
- Side effects through commands

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
- ✅ 80%+ test coverage on business logic
- ✅ Complete testability (mock-free unit tests)
- ✅ Predictable state changes
- ✅ Easy debugging (all changes through update)
- ✅ Time-travel debugging possible
- ✅ Replay bugs from logs

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

## ✅ Ready for Phase 2?

### Checklist Before Starting

- [x] Phase 1 complete (27.7% reduction)
- [x] All tests passing (27 tests)
- [x] Clear module boundaries
- [x] Manual testing verified
- [x] Handlers extracted
- [x] Pure logic extracted
- [x] Services organized
- [ ] **Decision:** Commit to 2-3 session effort
- [ ] **Decision:** Acceptable migration risk
- [ ] **Preparation:** Read Elm Architecture guide
- [ ] **Preparation:** Review iced/relm examples

---

## 💡 Recommendation

**Current state is solid.** You have:
- Clear architecture
- Good test coverage (for extracted logic)
- Maintainable codebase
- Working application

**Phase 2 is optional.** Consider it if:
- ✅ You want 80%+ test coverage
- ✅ You're adding new developers (easier onboarding)
- ✅ You want time-travel debugging
- ✅ You're willing to invest 2-3 sessions

**Phase 2 can wait.** Continue with:
- Adding new features
- Improving existing functionality
- Building more tests incrementally
- Revisit Elm rewrite when needed

---

**Generated:** 2025-10-31
**Current Version:** Phase 1 Complete (27.7% reduction)
