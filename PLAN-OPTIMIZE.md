# Synctui Performance Optimization Plan

## Executive Summary

**Current Performance Issues:**
- Building fresh cache on slow machines: 10-30 seconds for directories with 100+ files
- Un-ignoring deleted directories: 3+ seconds before state updates visible
- Root cause: **100+ individual SQLite writes** and **unconditional UI redraws** every 250ms

**Expected Improvements:**
- **Database operations:** 30-50x faster (100 individual writes → 1-3 batched transactions)
- **UI rendering:** 60-90% fewer redraws (unconditional 4 FPS → on-demand only)
- **User experience:** Near-instant on fast machines (<500ms), 2-5 seconds on slow machines (vs current 10-30s)

---

## Performance Bottleneck Analysis

### 1. Individual Database Writes (Critical Bottleneck)

**Location:** `src/cache.rs:348-367`

**Problem:** Each sync state is saved with a separate SQL INSERT statement, no transaction batching.

```rust
pub fn save_sync_state(
    &self,
    folder_id: &str,
    file_path: &str,
    state: SyncState,
    file_sequence: u64,
) -> Result<()> {
    self.conn.execute(
        "INSERT OR REPLACE INTO sync_states (folder_id, file_path, file_sequence, sync_state)
         VALUES (?1, ?2, ?3, ?4)",
        params![folder_id, file_path, file_sequence, state.to_string()],
    )?;
    Ok(())
}
```

**Called from:** `src/main.rs:810-812` in `handle_api_response()` for each `FileInfoResult`

**Impact:**
- Browsing directory with 100 files → 100 separate database writes
- Each write has full overhead: parse SQL, acquire lock, write, release lock
- On slower machines with slower I/O, this becomes multiplicative

**Comparison with browse cache (good example):**
- `cache.rs:251-323` - `save_browse_items()` uses transactions: `let tx = self.conn.unchecked_transaction()?;` (line 270)
- Saves 100+ items in single transaction: 1 DELETE + 100 INSERTs wrapped in `tx.commit()`

### 2. Unconditional UI Redraws (Critical Bottleneck)

**Location:** `src/main.rs:4188-4306` - `run_app()` event loop

**Problem:** UI redraws every loop iteration regardless of state changes.

```rust
async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        // ALWAYS draws, no dirty flag check
        terminal.draw(|f| {
            ui::render(f, app);
        })?;  // Line 4195

        // Process API responses
        while let Ok(response) = app.api_rx.try_recv() {
            app.handle_api_response(response);
        }

        // Poll for input (250ms timeout)
        if event::poll(Duration::from_millis(250))? {
            // Handle keyboard...
        }
    }
}
```

**Impact:**
- Runs at ~4 FPS (250ms poll interval)
- Processing 100 FileInfo responses over 5-10 seconds = 20-40 redraws
- Terminal rendering is not free - layout calculation, text formatting, ANSI sequences
- On some terminals (especially over SSH), redraws can be expensive

### 3. Bulk Operation Flow (100-file directory)

**Scenario:** User navigates into directory with 100 files, cold cache

**Phase 1: Browse Request** (`main.rs:579-760`)
```
1. API call: GET /rest/db/browse?folder=X&prefix=dir/
2. Response arrives → handle_api_response() → BrowseResult
3. Save browse cache: ✅ 1 transaction (DELETE + 100 INSERTs)
4. Queue 100 GetFileInfo API requests (lines 736-752)
```

**Phase 2: FileInfo Responses Trickle In** (`main.rs:762-863`)
```
For each of 100 responses:
1. Determine sync state
2. ❌ Individual DB write: cache.save_sync_state() (line 810)
3. Update in-memory HashMap: level.file_sync_states.insert()
4. UI redraws naturally on next loop iteration (~250ms later)

Timeline: 5-10 seconds for all responses to process
Database operations: 100 individual writes
UI redraws during this period: 20-40 times
```

**Code Evidence:**
```rust
// main.rs:762-863 - ApiResponse::FileInfoResult handler
ApiResponse::FileInfoResult { folder_id, file_path, details } => {
    let state = file_details.determine_sync_state();

    // ❌ INDIVIDUAL DATABASE WRITE (NO BATCHING)
    let _ = self.cache.save_sync_state(&folder_id, &file_path, state, file_sequence);

    // Update in-memory state
    for (_level_idx, level) in self.breadcrumb_trail.iter_mut().enumerate() {
        if level.folder_id == folder_id {
            if file_path.starts_with(level_prefix) {
                level.file_sync_states.insert(item_name.to_string(), state);
                updated = true;
            }
        }
    }
    // NO explicit UI redraw trigger
}
```

### 4. Cache Invalidation During Un-ignore

**Location:** `src/main.rs:3224-3429` - `toggle_ignore()`

**Problem:** Multiple systems trigger simultaneously without coordination:

1. **Un-ignore operation** (lines 3278-3360):
   - Updates .stignore via API
   - Spawns background task with 3-second delay
   - Clears cached state

2. **Event listener** fires `LocalIndexUpdated` events
   - Invalidates cache (`main.rs:1041-1219`)
   - Triggers API requests for fresh data

3. **Prefetch system** kicks in during idle periods (`main.rs:4277-4298`)
   - Fires up to 31 API requests every 300ms
   - Continues even during heavy operations

**Code Evidence:**
```rust
// main.rs:3328-3360 - Background rescan after un-ignore
tokio::spawn(async move {
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = client.rescan_folder(&folder_id_clone).await;
    tokio::time::sleep(Duration::from_secs(3)).await;  // ⚠️ 3-second wait
    let _ = api_tx.send(ApiRequest::GetFileInfo { ... });
});
```

**Impact:**
- User waits 3+ seconds to see state change
- Meanwhile, prefetch system may be querying for same data
- Multiple database writes for invalidations
- Constant UI redraws showing intermediate states

---

## Optimization Strategy

### Optimization 1: Batched Database Writes (CRITICAL - 30-50x improvement)

**File:** `src/cache.rs`

Add new batched method after `save_sync_state()`:

```rust
/// Save multiple sync states in a single transaction for better performance
pub fn save_sync_states_batch(
    &self,
    states: &[(String, String, SyncState, u64)], // (folder_id, file_path, state, sequence)
) -> Result<()> {
    if states.is_empty() {
        return Ok(());
    }

    let tx = self.conn.unchecked_transaction()?;
    let mut stmt = tx.prepare(
        "INSERT OR REPLACE INTO sync_states (folder_id, file_path, file_sequence, sync_state)
         VALUES (?1, ?2, ?3, ?4)"
    )?;

    for (folder_id, file_path, state, seq) in states {
        stmt.execute(params![folder_id, file_path, *seq, state.to_string()])?;
    }

    tx.commit()?;
    Ok(())
}
```

**File:** `src/main.rs`

Add to `App` struct (around line 50-100):

```rust
pub struct App {
    // ... existing fields ...

    /// Pending sync state writes to be batched
    pending_sync_state_writes: Vec<(String, String, SyncState, u64)>,

    /// Last time we flushed pending writes to database
    last_db_flush: Instant,
}
```

Initialize in `App::new()` (around line 348-514):

```rust
pending_sync_state_writes: Vec::new(),
last_db_flush: Instant::now(),
```

Add flush method (add near other App methods):

```rust
impl App {
    /// Flush pending database writes in a single transaction
    fn flush_pending_db_writes(&mut self) {
        if self.pending_sync_state_writes.is_empty() {
            return;
        }

        if let Some(cache) = &self.cache {
            if let Err(e) = cache.save_sync_states_batch(&self.pending_sync_state_writes) {
                log_error!("Failed to flush sync state batch: {}", e);
            }
        }

        self.pending_sync_state_writes.clear();
        self.last_db_flush = Instant::now();
    }

    /// Check if we should flush pending writes based on batch size or time
    fn should_flush_db(&self) -> bool {
        const MAX_BATCH_SIZE: usize = 50;
        const MAX_BATCH_AGE_MS: u64 = 100;

        if self.pending_sync_state_writes.is_empty() {
            return false;
        }

        self.pending_sync_state_writes.len() >= MAX_BATCH_SIZE
            || self.last_db_flush.elapsed() > Duration::from_millis(MAX_BATCH_AGE_MS)
    }
}
```

**Modify `handle_api_response()` for FileInfo** (around line 810):

Replace:
```rust
let _ = self.cache.save_sync_state(&folder_id, &file_path, state, file_sequence);
```

With:
```rust
// Queue for batched write instead of immediate write
self.pending_sync_state_writes.push((
    folder_id.clone(),
    file_path.clone(),
    state,
    file_sequence,
));
```

**Add flush triggers in event loop** (`run_app()`, around line 4188-4306):

```rust
async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| { ui::render(f, app); })?;

        // Process API responses
        while let Ok(response) = app.api_rx.try_recv() {
            app.handle_api_response(response);
        }

        // ✅ NEW: Flush if batch is ready
        if app.should_flush_db() {
            app.flush_pending_db_writes();
        }

        // Process cache invalidations...

        // ✅ NEW: Flush before processing user input (ensures consistency)
        if event::poll(Duration::from_millis(250))? {
            app.flush_pending_db_writes();  // Flush before user action
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await?;
            }
        }

        // ✅ NEW: Flush before idle operations
        if idle_time >= Duration::from_millis(300) {
            app.flush_pending_db_writes();
            // ... existing prefetch code ...
        }
    }
}
```

**Expected Impact:**
- 100 individual writes → 1-3 batched transactions
- Each batch completes in ~5-10ms (vs 500-1000ms for individual writes)
- Visible improvement on slow machines with slower I/O

---

### Optimization 2: Dirty Flag UI Rendering (CRITICAL - 60-90% fewer redraws)

**File:** `src/main.rs`

Add to `App` struct:

```rust
pub struct App {
    // ... existing fields ...

    /// Flag indicating UI needs redrawing
    ui_dirty: bool,
}
```

Initialize in `App::new()`:

```rust
ui_dirty: true,  // Start dirty to draw initial frame
```

**Set dirty flag in key locations:**

1. **After processing API responses** (in `run_app()` loop):
```rust
while let Ok(response) = app.api_rx.try_recv() {
    app.handle_api_response(response);
    app.ui_dirty = true;  // ✅ Mark dirty after state update
}
```

2. **After cache invalidations** (in `run_app()` loop):
```rust
while let Ok(invalidation) = app.invalidation_rx.try_recv() {
    app.handle_cache_invalidation(invalidation);
    app.ui_dirty = true;  // ✅ Mark dirty after invalidation
    events_processed += 1;
    if events_processed >= MAX_EVENTS_PER_FRAME { break; }
}
```

3. **After user input** (after `handle_key()`):
```rust
if event::poll(Duration::from_millis(250))? {
    app.flush_pending_db_writes();
    if let Event::Key(key) = event::read()? {
        app.handle_key(key).await?;
        app.ui_dirty = true;  // ✅ Mark dirty after input
    }
}
```

4. **Timer-based updates** (for status bar uptime, transfer rates):
```rust
// Add periodic dirty flag for live stats (every 1 second)
let mut last_stats_update = Instant::now();

// In event loop:
if last_stats_update.elapsed() > Duration::from_secs(1) {
    app.ui_dirty = true;  // Update for live stats (uptime, transfer rates)
    last_stats_update = Instant::now();
}
```

**Modify rendering in event loop:**

Replace:
```rust
terminal.draw(|f| { ui::render(f, app); })?;
```

With:
```rust
if app.ui_dirty {
    terminal.draw(|f| { ui::render(f, app); })?;
    app.ui_dirty = false;
}
```

**Expected Impact:**
- Unconditional 4 FPS → on-demand only
- During heavy operations: ~20-40 redraws → ~5-10 redraws (only when state changes)
- Idle CPU usage drops significantly

---

### Optimization 3: Response Accumulation Window (MEDIUM - reduces overhead)

**Rationale:** When FileInfo responses arrive in bursts, process them together to enable better batching.

**File:** `src/main.rs`

Add to `App` struct:

```rust
pub struct App {
    // ... existing fields ...

    /// Accumulated API responses waiting to be processed
    pending_api_responses: Vec<ApiResponse>,

    /// Time when first response in current batch arrived
    last_response_time: Option<Instant>,
}
```

Initialize in `App::new()`:

```rust
pending_api_responses: Vec::new(),
last_response_time: None,
```

**Modify event loop to accumulate responses:**

Replace:
```rust
while let Ok(response) = app.api_rx.try_recv() {
    app.handle_api_response(response);
}
```

With:
```rust
// Accumulate responses
while let Ok(response) = app.api_rx.try_recv() {
    app.pending_api_responses.push(response);
    if app.last_response_time.is_none() {
        app.last_response_time = Some(Instant::now());
    }
}

// Process batch when window closes or batch is large enough
const RESPONSE_WINDOW_MS: u64 = 50;
const RESPONSE_BATCH_SIZE: usize = 20;

let should_process = if let Some(first_response_time) = app.last_response_time {
    // Window closed (50ms elapsed) or batch is large
    first_response_time.elapsed() > Duration::from_millis(RESPONSE_WINDOW_MS)
        || app.pending_api_responses.len() >= RESPONSE_BATCH_SIZE
} else {
    false
};

if should_process {
    for response in app.pending_api_responses.drain(..) {
        app.handle_api_response(response);
    }
    app.last_response_time = None;
    app.flush_pending_db_writes();  // Flush after processing batch
    app.ui_dirty = true;  // Single dirty flag for entire batch
}
```

**Expected Impact:**
- Naturally enables larger database write batches
- Reduces per-response overhead
- Single UI redraw per batch instead of per response
- Adds max 50ms latency (acceptable for background operations)

---

### Optimization 4: SQLite WAL Mode (LOW - easy win)

**File:** `src/cache.rs`

**Location:** In `Cache::new()` method (around line 71-85), after opening connection

Add:
```rust
// Enable Write-Ahead Logging for better concurrency
conn.execute("PRAGMA journal_mode=WAL", [])?;
conn.execute("PRAGMA synchronous=NORMAL", [])?;
```

**Benefits:**
- Readers don't block on writers
- Better performance for write-heavy workloads
- Safer than `synchronous=OFF` (still crash-safe)
- Creates `.db-wal` and `.db-shm` files alongside main database

**Note:** WAL files are automatically checkpointed (merged back) by SQLite. No manual cleanup needed.

---

## Edge Cases & Safety Considerations

### 1. Flush Before Destructive Operations

**Critical:** Must flush pending writes before user performs destructive actions.

**Locations to add flush:**

- **Before delete** (`main.rs` - in `handle_key()` for 'd' key):
```rust
KeyCode::Char('d') => {
    self.flush_pending_db_writes();  // ✅ Ensure state is saved
    // ... existing delete logic ...
}
```

- **Before ignore+delete** (`main.rs` - in `handle_key()` for 'I' key):
```rust
KeyCode::Char('I') => {
    self.flush_pending_db_writes();  // ✅ Ensure state is saved
    // ... existing ignore+delete logic ...
}
```

- **Before app shutdown** (in `main()` or cleanup code):
```rust
// Before returning from run_app() or in cleanup
app.flush_pending_db_writes();
```

### 2. Batch Size Limits

**Consideration:** Very large directories (1000+ files) could accumulate huge batches.

**Mitigation:**
- `MAX_BATCH_SIZE = 50` ensures batches are flushed regularly
- `MAX_BATCH_AGE_MS = 100` ensures timely flushes even for small batches
- Consider adding absolute max: `Vec::with_capacity(100)` and hard limit checks

**Code addition:**
```rust
fn flush_pending_db_writes(&mut self) {
    const ABSOLUTE_MAX_BATCH: usize = 100;

    if self.pending_sync_state_writes.len() > ABSOLUTE_MAX_BATCH {
        log_debug!("Warning: Large batch size {}, flushing in chunks",
                   self.pending_sync_state_writes.len());

        // Flush in chunks if extremely large
        for chunk in self.pending_sync_state_writes.chunks(ABSOLUTE_MAX_BATCH) {
            if let Some(cache) = &self.cache {
                let _ = cache.save_sync_states_batch(chunk);
            }
        }
        self.pending_sync_state_writes.clear();
    } else {
        // Normal flush path
        // ... existing code ...
    }

    self.last_db_flush = Instant::now();
}
```

### 3. User Action Consistency

**Problem:** User navigates away before pending writes flush.

**Solution:** Flush before processing keyboard input (already in plan).

**Additional consideration:** Track which folder/path pending writes belong to. If user navigates away, flush immediately.

```rust
// In handle_key() before navigation commands
KeyCode::Enter | KeyCode::Left | KeyCode::Backspace => {
    self.flush_pending_db_writes();  // ✅ Save before navigation
    // ... navigation logic ...
}
```

### 4. Crash Safety

**WAL Mode Benefits:**
- Even if app crashes mid-write, WAL provides crash recovery
- Committed transactions are durable
- Uncommitted transactions are rolled back automatically

**Additional Safety:**
- Consider adding signal handler to flush on SIGTERM/SIGINT
- Document that cache can be safely deleted if corrupted

### 5. Race Conditions with Event System

**Scenario:** Event listener invalidates cache while pending writes are queued.

**Current behavior:**
- Cache invalidation deletes entry from database
- Pending write will recreate it (INSERT OR REPLACE)
- This is actually correct - pending write has fresher data

**No changes needed** - race is benign.

### 6. Memory Usage

**Consideration:** Pending writes consume memory.

**Worst case:**
- 100 pending writes × ~100 bytes each = ~10KB
- Negligible for modern systems

**If concerned, add memory-based flush trigger:**
```rust
const MAX_BATCH_MEMORY: usize = 50_000; // ~50KB

fn should_flush_db(&self) -> bool {
    let estimated_size = self.pending_sync_state_writes.len() * 100;
    estimated_size > MAX_BATCH_MEMORY || /* ... existing conditions ... */
}
```

---

## Implementation Order

### Phase 1: Core Batching (Highest Impact)
1. Add `save_sync_states_batch()` to `cache.rs`
2. Add `pending_sync_state_writes` and flush methods to `App` in `main.rs`
3. Modify `handle_api_response()` to queue writes instead of immediate save
4. Add flush triggers in event loop (batch ready, before user input, before idle)
5. Add flush calls before destructive operations

**Test checkpoint:** Verify no database errors, all states still save correctly

### Phase 2: UI Optimization
6. Add `ui_dirty` flag to `App`
7. Set dirty flag after state changes (API responses, invalidations, user input)
8. Add periodic dirty flag for live stats (1 second interval)
9. Make rendering conditional on dirty flag

**Test checkpoint:** Verify UI updates correctly, no missed redraws

### Phase 3: Response Accumulation (Optional)
10. Add response accumulation fields to `App`
11. Modify event loop to accumulate responses for 50ms or 20 items
12. Process accumulated responses in batches

**Test checkpoint:** Verify no increased latency, better batching behavior

### Phase 4: SQLite Optimization
13. Enable WAL mode in `Cache::new()`

**Test checkpoint:** Verify database still works, check for `.db-wal` files

### Phase 5: Testing & Validation
14. Test with large directories (100+, 500+, 1000+ files)
15. Test un-ignore operations
16. Test on slow machines (add artificial delays if needed)
17. Monitor performance metrics with `--debug` flag

---

## Testing & Validation Plan

### Test Scenarios

#### Scenario 1: Browse Large Directory (Cold Cache)
```bash
# Setup: Clear cache
rm ~/.cache/synctui/cache.db

# Test: Browse into directory with 100+ files
# Navigate to large directory, observe behavior

# Measure:
# - Time to see all sync states populate
# - Database write count (check debug logs)
# - UI redraw frequency
```

**Expected Results:**
- **Before:** 10-30 seconds on slow machines, 100+ individual DB writes
- **After:** 2-5 seconds on slow machines, 1-3 batched DB writes

#### Scenario 2: Un-ignore Deleted Directory
```bash
# Setup: Ignore a directory, delete it locally
# Test: Un-ignore via 'i' key
# Observe how quickly state updates appear

# Measure:
# - Time to see state change from Ignored to other state
# - Background operation interference
```

**Expected Results:**
- **Before:** 3+ seconds delay (due to hardcoded sleep)
- **After:** Sub-second response (event-driven)

#### Scenario 3: Idle Behavior
```bash
# Test: Leave app idle on folder list view
# Observe CPU usage and UI redraw frequency

# Measure:
# - CPU percentage when idle
# - UI redraws per second
```

**Expected Results:**
- **Before:** ~1-2% CPU, 4 redraws/second
- **After:** <0.5% CPU, 1 redraw/second (for live stats only)

### Performance Metrics (--debug mode)

Add logging to track:

```rust
// In flush_pending_db_writes()
let start = Instant::now();
cache.save_sync_states_batch(&self.pending_sync_state_writes)?;
let elapsed = start.elapsed();
log_debug!("Flushed {} sync states in {:?}", self.pending_sync_state_writes.len(), elapsed);
```

```rust
// In run_app() event loop
let mut frame_count = 0;
let mut last_fps_log = Instant::now();

// After terminal.draw()
if app.ui_dirty {
    terminal.draw(|f| { ui::render(f, app); })?;
    app.ui_dirty = false;
    frame_count += 1;
}

// Log FPS every 10 seconds
if last_fps_log.elapsed() > Duration::from_secs(10) {
    let fps = frame_count as f64 / last_fps_log.elapsed().as_secs_f64();
    log_debug!("Average FPS: {:.2}", fps);
    frame_count = 0;
    last_fps_log = Instant::now();
}
```

### Success Criteria

- **Database writes reduced by 90%+** for bulk operations
- **UI redraws reduced by 60%+** during normal usage
- **No visible regressions** in functionality
- **No data loss** under normal and crash scenarios
- **Perceived performance** feels instant on fast machines, responsive on slow machines

---

## Future Optimizations (Out of Scope)

1. **Adaptive Performance Monitoring**: Auto-tune batch sizes based on observed performance
2. **Lazy Sync State Loading**: Only load sync states for visible items (deeper change)
3. **Background Thread for Cache**: Offload all database I/O to dedicated thread
4. **Render Diffing**: Only redraw changed portions of UI (requires ratatui internal changes)
5. **Progressive Loading**: Show "Loading..." indicators instead of waiting for all states

---

## References

### Key Files
- `src/cache.rs` - Database operations
- `src/main.rs` - App logic and event loop
- `src/api_service.rs` - API request handling
- `src/event_listener.rs` - Event stream processing

### Key Line Numbers
- `cache.rs:348-367` - Individual sync state save (bottleneck)
- `cache.rs:251-323` - Browse cache save (good example of batching)
- `main.rs:810-812` - FileInfo response handling (triggers individual writes)
- `main.rs:4195` - Unconditional UI rendering
- `main.rs:4188-4306` - Event loop structure

### SQLite Transaction Reference
- Unchecked transactions: `conn.unchecked_transaction()?`
- WAL mode: https://www.sqlite.org/wal.html
- Synchronous modes: https://www.sqlite.org/pragma.html#pragma_synchronous
