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

### Step 1: Extract Model Struct ⭐ CRITICAL
- Identify all data fields in `App`
- Create `struct Model` with ONLY data (no services)
- Model must be `Clone + Debug`
- Move fields one-by-one:
  - ✅ `folders`, `devices`, `breadcrumb_trail`
  - ✅ `sort_mode`, `display_mode`, `focus_level`
  - ✅ Dialog states, toast messages
  - ❌ `client`, `cache`, channels (stay in App/Runtime)
- Add helper methods to Model (pure functions)

**📖 See MODEL_DESIGN.md for complete Model structure**

### Step 2: Rename App → Runtime
- `App` becomes `Runtime` (it's really a service container)
- Runtime holds Model + services:
  ```rust
  struct Runtime {
      model: Model,  // The state
      client: SyncthingClient,
      cache: CacheDb,
      // ... services
  }
  ```

### Step 3: Define Cmd Enum
- Enumerate all side effects:
  - API calls (FetchFileInfo, BrowseFolder, etc.)
  - Filesystem ops (DeleteFile, CheckExists)
  - Cache ops (SaveToCache)
- Start with 5-10 most common commands

### Step 4: Create Pure Update Function
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

## ✅ Ready for Phase 2?

### Checklist Before Starting

**Completed:**
- [x] Phase 1 complete (27.7% reduction)
- [x] All tests passing (27 tests)
- [x] Clear module boundaries
- [x] Manual testing verified
- [x] Handlers extracted
- [x] Pure logic extracted
- [x] Services organized
- [x] Model design documented (MODEL_DESIGN.md)

**Understanding Required:**
- [ ] **Read:** MODEL_DESIGN.md (understand Model vs Runtime)
- [ ] **Understand:** Model = pure data (cloneable, serializable)
- [ ] **Understand:** Update = pure function (no &mut, no .await)
- [ ] **Understand:** Runtime executes Cmd, sends Msg back
- [ ] **Read:** [Elm Architecture Guide](https://guide.elm-lang.org/architecture/)
- [ ] **Review:** iced or relm examples (optional)

**Commitment:**
- [ ] **Decision:** Commit to 2-3 session effort
- [ ] **Decision:** Acceptable migration risk (will break things temporarily)
- [ ] **Strategy:** Model extraction FIRST (foundation)
- [ ] **Strategy:** Incremental migration (one Msg at a time)

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
