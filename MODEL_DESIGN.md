# Elm Architecture - Model Design for Synctui

This document shows the proper separation between **Model** (pure state) and **Runtime** (services).

---

## The Core Pattern

```rust
// MODEL: Pure, immutable application state
struct Model { /* all data lives here */ }

// MSG: All events that can happen
enum Msg { /* ... */ }

// UPDATE: Pure state transition
fn update(model: Model, msg: Msg) -> (Model, Cmd) {
    // Returns NEW model + side effects to execute
}

// RUNTIME: Services that execute commands
struct Runtime {
    // api, cache, filesystem, etc.
}

impl Runtime {
    async fn execute(&self, cmd: Cmd) {
        // Performs I/O, sends Msg back to update
    }
}
```

---

## Current Problem: App Struct Mixes Concerns

```rust
// ❌ CURRENT: Everything mixed together
struct App {
    // SERVICES (should be in Runtime):
    client: SyncthingClient,        // ❌
    cache: CacheDb,                 // ❌
    api_tx: UnboundedSender<...>,   // ❌
    api_rx: UnboundedReceiver<...>, // ❌
    invalidation_rx: ...,           // ❌
    event_id_rx: ...,               // ❌
    path_map: HashMap<String, String>, // ❌ (config)

    // STATE (should be in Model):
    folders: Vec<Folder>,           // ✅
    devices: Vec<Device>,           // ✅
    folder_statuses: HashMap<...>,  // ✅
    breadcrumb_trail: Vec<...>,     // ✅
    focus_level: usize,             // ✅
    sort_mode: SortMode,            // ✅
    // ... etc
}
```

**The problem:** Can't clone App, can't serialize it, can't test it without mocking everything.

---

## Proper Separation: Model vs Runtime

### 1. Model (Pure State)

```rust
/// Pure application state - no services, no I/O
/// This is cloneable, serializable, and easy to test
#[derive(Clone, Debug)]
pub struct Model {
    // ============================================
    // CORE DATA
    // ============================================

    /// List of all Syncthing folders
    pub folders: Vec<Folder>,

    /// List of all known devices
    pub devices: Vec<Device>,

    /// Sync status for each folder (keyed by folder_id)
    pub folder_statuses: HashMap<String, FolderStatus>,

    /// When folder statuses were last loaded
    pub statuses_loaded: bool,

    /// System status (device name, uptime, etc.)
    pub system_status: Option<SystemStatus>,

    /// Connection statistics (download/upload rates)
    pub connection_stats: Option<ConnectionStats>,


    // ============================================
    // NAVIGATION STATE
    // ============================================

    /// Breadcrumb trail for directory navigation
    pub breadcrumb_trail: Vec<BreadcrumbLevel>,

    /// Currently focused breadcrumb level (0 = folder list)
    pub focus_level: usize,

    /// Selected folder in the folder list
    pub folders_state_selection: Option<usize>,


    // ============================================
    // UI PREFERENCES
    // ============================================

    /// Current sort mode
    pub sort_mode: SortMode,

    /// Whether sort is reversed
    pub sort_reverse: bool,

    /// Display mode for file info
    pub display_mode: DisplayMode,

    /// Whether vim keybindings are enabled
    pub vim_mode: bool,

    /// Vim command state (for 'gg' double-key)
    pub vim_command_state: VimCommandState,


    // ============================================
    // UI DIALOGS & POPUPS
    // ============================================

    /// Confirmation dialog for revert operation
    pub confirm_revert: Option<(String, u64)>, // (folder_id, items_count)

    /// Confirmation dialog for delete operation
    pub confirm_delete: Option<(String, String, bool)>, // (folder_id, item_name, is_dir)

    /// Confirmation dialog for ignore+delete operation
    pub confirm_ignore_delete: Option<(String, String, bool)>,

    /// Pattern selection menu for un-ignore
    pub pattern_selection: Option<PatternSelectionState>,

    /// File info popup (metadata + preview)
    pub file_info_popup: Option<FileInfoPopupState>,

    /// Toast message (text, timestamp)
    pub toast_message: Option<(String, Instant)>,


    // ============================================
    // OPERATIONAL STATE
    // ============================================

    /// Folders currently being loaded
    pub folders_loading: HashSet<String>,

    /// Pending ignore+delete operations (blocks un-ignore)
    pub pending_ignore_deletes: HashMap<String, PendingDeleteInfo>,

    /// Last update timestamp for each folder (folder_id -> (timestamp, filename))
    pub last_folder_updates: HashMap<String, (SystemTime, String)>,

    /// Last time user interacted with UI
    pub last_user_action: Instant,

    /// Whether app should quit
    pub should_quit: bool,
}

/// A single level in the breadcrumb trail
#[derive(Clone, Debug)]
pub struct BreadcrumbLevel {
    pub folder_id: String,
    pub prefix: Option<String>,
    pub items: Vec<BrowseItem>,
    pub selected_index: Option<usize>,
    pub file_sync_states: HashMap<String, SyncState>,
    pub ignored_exists: HashMap<String, bool>,
    pub translated_base_path: String,
}

impl Model {
    /// Create initial empty model
    pub fn new(config: &Config) -> Self {
        Self {
            folders: Vec::new(),
            devices: Vec::new(),
            folder_statuses: HashMap::new(),
            statuses_loaded: false,
            system_status: None,
            connection_stats: None,

            breadcrumb_trail: Vec::new(),
            focus_level: 0,
            folders_state_selection: None,

            sort_mode: SortMode::VisualIndicator,
            sort_reverse: false,
            display_mode: DisplayMode::TimestampAndSize,
            vim_mode: config.vim_mode,
            vim_command_state: VimCommandState::None,

            confirm_revert: None,
            confirm_delete: None,
            confirm_ignore_delete: None,
            pattern_selection: None,
            file_info_popup: None,
            toast_message: None,

            folders_loading: HashSet::new(),
            pending_ignore_deletes: HashMap::new(),
            last_folder_updates: HashMap::new(),
            last_user_action: Instant::now(),
            should_quit: false,
        }
    }

    /// Get currently selected folder (if any)
    pub fn selected_folder(&self) -> Option<&Folder> {
        self.folders_state_selection
            .and_then(|idx| self.folders.get(idx))
    }

    /// Get current breadcrumb level (if navigating)
    pub fn current_level(&self) -> Option<&BreadcrumbLevel> {
        if self.focus_level == 0 {
            None
        } else {
            self.breadcrumb_trail.get(self.focus_level - 1)
        }
    }

    /// Check if we're idle (no user action in 300ms)
    pub fn is_idle(&self) -> bool {
        self.last_user_action.elapsed() > Duration::from_millis(300)
    }
}
```

**Key points:**
- ✅ Cloneable (all fields are owned data)
- ✅ Serializable (no channels, no services)
- ✅ Easy to test (just data structures)
- ✅ Pure accessors (no side effects)
- ✅ Can snapshot for debugging/undo

---

### 2. Runtime (Services)

```rust
/// Runtime environment - handles all I/O and side effects
/// This is NOT cloneable, NOT part of Model
pub struct Runtime {
    // ============================================
    // SERVICES
    // ============================================

    /// Syncthing HTTP client
    client: Arc<SyncthingClient>,

    /// SQLite cache database
    cache: Arc<CacheDb>,

    /// Configuration (paths, API keys, etc.)
    config: Arc<Config>,


    // ============================================
    // MESSAGE CHANNELS
    // ============================================

    /// Send messages back to update loop
    msg_tx: UnboundedSender<Msg>,

    /// Receive API responses from background service
    api_rx: UnboundedReceiver<ApiResponse>,

    /// Receive cache invalidations from event listener
    invalidation_rx: UnboundedReceiver<CacheInvalidation>,

    /// Receive event IDs from event listener
    event_id_rx: watch::Receiver<u64>,


    // ============================================
    // BACKGROUND SERVICES
    // ============================================

    /// API request queue handle (for sending requests)
    api_tx: UnboundedSender<ApiRequest>,

    /// Event listener handle (for lifecycle management)
    _event_listener_handle: JoinHandle<()>,
}

impl Runtime {
    /// Create runtime and spawn background services
    pub async fn new(config: Config, msg_tx: UnboundedSender<Msg>) -> Result<Self> {
        let client = Arc::new(SyncthingClient::new(
            config.base_url.clone(),
            config.api_key.clone()
        ));

        let cache = Arc::new(CacheDb::new()?);

        // Spawn API service
        let (api_tx, api_rx) = services::api::spawn_api_service(client.clone());

        // Spawn event listener
        let (invalidation_rx, event_id_rx, event_handle) =
            services::events::spawn_event_listener(
                client.clone(),
                cache.clone(),
                msg_tx.clone(),
            );

        Ok(Self {
            client,
            cache,
            config: Arc::new(config),
            msg_tx,
            api_rx,
            invalidation_rx,
            event_id_rx,
            api_tx,
            _event_listener_handle: event_handle,
        })
    }

    /// Execute a command (side effect)
    pub async fn execute(&self, cmd: Cmd) {
        match cmd {
            Cmd::LoadFolders => {
                let result = self.client.get_folders().await;
                self.msg_tx.send(Msg::FoldersLoaded(result));
            }

            Cmd::LoadFolderStatus { folder_id } => {
                let _ = self.api_tx.send(ApiRequest::GetFolderStatus {
                    folder_id,
                    priority: Priority::High,
                });
            }

            Cmd::BrowseFolder { folder_id, prefix } => {
                // Check cache first
                if let Some(cached) = self.cache.get_browse(&folder_id, &prefix) {
                    self.msg_tx.send(Msg::BrowseResult {
                        folder_id: folder_id.clone(),
                        prefix: prefix.clone(),
                        items: cached,
                    });
                }

                // Then fetch fresh data
                let _ = self.api_tx.send(ApiRequest::BrowseFolder {
                    folder_id,
                    prefix,
                    priority: Priority::High,
                });
            }

            Cmd::SetIgnorePatterns { folder_id, patterns } => {
                let result = self.client
                    .set_ignore_patterns(&folder_id, patterns)
                    .await;
                self.msg_tx.send(Msg::IgnorePatternsSet {
                    folder_id,
                    result,
                });
            }

            Cmd::DeleteFile { path } => {
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                self.msg_tx.send(Msg::FileDeleted {
                    path,
                    result: result.map_err(|e| e.to_string()),
                });
            }

            Cmd::CheckFileExists { path } => {
                let exists = path.exists();
                self.msg_tx.send(Msg::FileExistenceChecked {
                    path,
                    exists,
                });
            }

            Cmd::SaveToCache { folder_id, prefix, items } => {
                let _ = self.cache.save_browse(&folder_id, &prefix, &items);
            }

            Cmd::Batch(cmds) => {
                // Execute commands in parallel
                for cmd in cmds {
                    let runtime = self.clone_ref();
                    tokio::spawn(async move {
                        runtime.execute(cmd).await;
                    });
                }
            }

            Cmd::None => {}
        }
    }

    /// Clone references (for spawning tasks)
    fn clone_ref(&self) -> Self {
        Self {
            client: self.client.clone(),
            cache: self.cache.clone(),
            config: self.config.clone(),
            msg_tx: self.msg_tx.clone(),
            api_rx: self.api_rx.clone(), // Note: can't actually clone receiver
            invalidation_rx: self.invalidation_rx.clone(), // This needs refactoring
            event_id_rx: self.event_id_rx.clone(),
            api_tx: self.api_tx.clone(),
            _event_listener_handle: /* ??? */,
        }
    }

    /// Poll background services for messages
    pub async fn poll_messages(&mut self) -> Option<Msg> {
        tokio::select! {
            Some(api_response) = self.api_rx.recv() => {
                Some(Msg::ApiResponse(api_response))
            }
            Some(invalidation) = self.invalidation_rx.recv() => {
                Some(Msg::CacheInvalidation(invalidation))
            }
            else => None,
        }
    }

    /// Translate container path to host path
    pub fn translate_path(&self, folder_path: &str, relative_path: &str) -> String {
        logic::path::translate_path(folder_path, relative_path, &self.config.path_map)
    }
}
```

**Key points:**
- ❌ NOT cloneable (has channels, services)
- ❌ NOT serializable (stateful I/O)
- ✅ All side effects live here
- ✅ Executes commands
- ✅ Polls background services

---

## Example: Update Function

```rust
/// Pure state transition - no side effects
pub fn update(model: Model, msg: Msg) -> (Model, Cmd) {
    match msg {
        // ================================================
        // USER INPUT
        // ================================================

        Msg::KeyPress(key) => {
            let mut model = model;
            model.last_user_action = Instant::now();

            match key.code {
                KeyCode::Char('q') => {
                    model.should_quit = true;
                    (model, Cmd::None)
                }

                KeyCode::Char('i') => {
                    // Get selected item
                    let Some(level) = model.current_level() else {
                        return (model, Cmd::None);
                    };

                    let Some(item) = level.selected_item() else {
                        return (model, Cmd::None);
                    };

                    // Check current sync state
                    let sync_state = level.file_sync_states
                        .get(&item.name)
                        .copied()
                        .unwrap_or(SyncState::Unknown);

                    if sync_state == SyncState::Ignored {
                        // Un-ignore: Need to fetch patterns first
                        (model, Cmd::FetchIgnorePatterns {
                            folder_id: level.folder_id.clone(),
                        })
                    } else {
                        // Ignore: Can do immediately
                        let relative_path = level.relative_path(&item.name);
                        let new_pattern = format!("/{}", relative_path);

                        // Optimistically update UI
                        model.set_sync_state(&level.folder_id, &item.name, SyncState::Ignored);

                        (model, Cmd::AddIgnorePattern {
                            folder_id: level.folder_id.clone(),
                            pattern: new_pattern,
                        })
                    }
                }

                KeyCode::Down => {
                    model.move_selection_down();
                    (model, Cmd::None)
                }

                KeyCode::Up => {
                    model.move_selection_up();
                    (model, Cmd::None)
                }

                KeyCode::Enter => {
                    if model.focus_level == 0 {
                        // Entering a folder
                        let Some(folder) = model.selected_folder() else {
                            return (model, Cmd::None);
                        };

                        if folder.paused {
                            model.toast_message = Some((
                                "Cannot browse paused folder".to_string(),
                                Instant::now()
                            ));
                            return (model, Cmd::None);
                        }

                        // Mark as loading
                        model.folders_loading.insert(folder.id.clone());

                        (model, Cmd::BrowseFolder {
                            folder_id: folder.id.clone(),
                            prefix: String::new(),
                        })
                    } else {
                        // Entering a subdirectory
                        // ... similar logic
                        (model, Cmd::None)
                    }
                }

                _ => (model, Cmd::None),
            }
        }

        // ================================================
        // API RESPONSES
        // ================================================

        Msg::FoldersLoaded(result) => {
            match result {
                Ok(folders) => {
                    let mut model = model;
                    model.folders = folders;

                    // Load statuses for all folders
                    let cmds: Vec<Cmd> = model.folders.iter()
                        .map(|f| Cmd::LoadFolderStatus {
                            folder_id: f.id.clone()
                        })
                        .collect();

                    (model, Cmd::Batch(cmds))
                }
                Err(e) => {
                    let mut model = model;
                    model.toast_message = Some((
                        format!("Failed to load folders: {}", e),
                        Instant::now()
                    ));
                    (model, Cmd::None)
                }
            }
        }

        Msg::BrowseResult { folder_id, prefix, items } => {
            let mut model = model;

            // Stop loading indicator
            model.folders_loading.remove(&folder_id);

            // If this is the folder we're waiting for, create breadcrumb
            if model.focus_level == 0 &&
               model.selected_folder().map(|f| &f.id) == Some(&folder_id)
            {
                // Create root level breadcrumb
                let level = BreadcrumbLevel {
                    folder_id: folder_id.clone(),
                    prefix: None,
                    items: items.clone(),
                    selected_index: Some(0),
                    file_sync_states: HashMap::new(),
                    ignored_exists: HashMap::new(),
                    translated_base_path: String::new(), // Will be filled by runtime
                };

                model.breadcrumb_trail = vec![level];
                model.focus_level = 1;

                // Fetch sync states for visible items
                let cmds: Vec<Cmd> = items.iter()
                    .take(20) // Only fetch visible items
                    .map(|item| Cmd::LoadFileInfo {
                        folder_id: folder_id.clone(),
                        file_path: item.name.clone(),
                    })
                    .collect();

                (model, Cmd::Batch(cmds))
            } else {
                // Update existing breadcrumb or cache
                (model, Cmd::SaveToCache { folder_id, prefix, items })
            }
        }

        Msg::FileDeleted { path, result } => {
            let mut model = model;

            match result {
                Ok(()) => {
                    model.toast_message = Some((
                        format!("Deleted: {}", path.display()),
                        Instant::now()
                    ));

                    // Refresh current directory
                    if let Some(level) = model.current_level() {
                        (model, Cmd::BrowseFolder {
                            folder_id: level.folder_id.clone(),
                            prefix: level.prefix.clone().unwrap_or_default(),
                        })
                    } else {
                        (model, Cmd::None)
                    }
                }
                Err(e) => {
                    model.toast_message = Some((
                        format!("Delete failed: {}", e),
                        Instant::now()
                    ));
                    (model, Cmd::None)
                }
            }
        }

        // ... more message handlers

        _ => (model, Cmd::None),
    }
}
```

**Key points:**
- ✅ Pure function (no `&mut self`)
- ✅ Returns NEW model (immutable updates)
- ✅ Returns commands for side effects
- ✅ Easy to test (no mocking)
- ✅ Easy to reason about

---

## Main Event Loop

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Load config
    let config = Config::load()?;

    // Create message channel
    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

    // Initialize model
    let mut model = Model::new(&config);

    // Initialize runtime
    let mut runtime = Runtime::new(config, msg_tx.clone()).await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    // Initial commands
    let mut pending_cmds = vec![
        Cmd::LoadFolders,
        Cmd::LoadSystemStatus,
    ];

    // Main loop
    loop {
        // Execute pending commands
        for cmd in pending_cmds.drain(..) {
            runtime.execute(cmd).await;
        }

        // Render UI
        terminal.draw(|f| ui::render(f, &model))?;

        if model.should_quit {
            break;
        }

        // Wait for next event
        tokio::select! {
            // Keyboard input
            Ok(Event::Key(key)) = crossterm::event::read() => {
                let (new_model, cmd) = update(model, Msg::KeyPress(key));
                model = new_model;
                if !cmd.is_none() {
                    pending_cmds.push(cmd);
                }
            }

            // Background service messages
            Some(msg) = runtime.poll_messages() => {
                let (new_model, cmd) = update(model, msg);
                model = new_model;
                if !cmd.is_none() {
                    pending_cmds.push(cmd);
                }
            }

            // Periodic tick
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                let (new_model, cmd) = update(model, Msg::Tick);
                model = new_model;
                if !cmd.is_none() {
                    pending_cmds.push(cmd);
                }
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    terminal.clear()?;

    Ok(())
}
```

**Key points:**
- Model and Runtime are separate
- Update is pure, returns commands
- Runtime executes commands asynchronously
- Clean event loop

---

## Benefits of This Design

### Testability
```rust
#[test]
fn test_ignore_file() {
    let mut model = Model::new(&Config::default());
    model.folders = vec![test_folder()];
    model.focus_level = 1;
    model.breadcrumb_trail = vec![test_level()];

    let (new_model, cmd) = update(model, Msg::KeyPress(KeyEvent {
        code: KeyCode::Char('i'),
        modifiers: KeyModifiers::NONE,
    }));

    // Assert state changed correctly
    assert_eq!(new_model.get_sync_state("test.txt"), SyncState::Ignored);

    // Assert correct command emitted
    assert!(matches!(cmd, Cmd::AddIgnorePattern { .. }));
}
```

### Time-Travel Debugging
```rust
// Record all messages
let mut history: Vec<(Model, Msg)> = vec![];

// In main loop:
history.push((model.clone(), msg.clone()));

// Later: replay from any point
let mut model = history[50].0.clone();
for (_, msg) in &history[51..] {
    let (new_model, _) = update(model, msg.clone());
    model = new_model;
}
```

### Serialization
```rust
// Save app state
let json = serde_json::to_string(&model)?;
std::fs::write("state.json", json)?;

// Restore app state
let json = std::fs::read_to_string("state.json")?;
let model: Model = serde_json::from_str(&json)?;
```

---

## Next Steps for Migration

1. **Extract Model struct** - Move pure data from App
2. **Keep Runtime in App** - Rename App → Runtime
3. **Create update function** - Start with one message type
4. **Define Cmd enum** - Start with a few commands
5. **Wire up main loop** - Separate model/runtime
6. **Migrate incrementally** - One message type at a time

---

This is the **proper** Elm Architecture pattern. The Model is the foundation, and everything else flows from it.
