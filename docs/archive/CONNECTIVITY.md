# Network Error Handling - Implementation Status

## Overview
Comprehensive error handling for Syncthing API failures with graceful degradation, auto-retry with exponential backoff, and clear user feedback.

## ✅ COMPLETED FEATURES (11/12)

### 1. ✅ Error Classification Module (`src/logic/errors.rs`)
- `ErrorType` enum: ConnectionRefused, Timeout, Unauthorized, NotFound, ServerError, NetworkError, Other
- `classify_error()`: Classifies anyhow errors by type
- `format_error_message()`: User-friendly error messages
- **Tests**: 10 comprehensive tests covering all error types

### 2. ✅ Platform Helpers (`src/logic/platform.rs`)
- Created module as part of implementation plan
- **Note**: Helper functions ended up not being needed - existing code in `src/main.rs:get_config_path()` and `src/cache.rs:CacheDb::get_cache_dir()` already handles cross-platform path resolution
- Module kept as documentation of implementation approach

### 3. ✅ ConnectionState Enum (`src/model/syncthing.rs`)
- `Connected`: Successfully connected
- `Connecting { attempt, last_error }`: Retrying with attempt count
- `Disconnected { error_type, message }`: Failed with classified error
- **Tests**: 3 tests for state transitions and equality

### 4. ✅ Folder Cache Persistence (`src/cache.rs`)
- `save_folders()`: Persists folders as JSON to SQLite
- `get_all_folders()`: Loads folders from cache
- New `cached_folders` table in SQLite schema
- **Note**: First run with API down shows setup help (expected behavior)

### 5. ✅ Graceful Initialization (`src/main.rs:365-407`)
- API failure no longer crashes app
- Falls back to `cached_folders` table
- Sets appropriate ConnectionState (Connected/Connecting/Disconnected)
- **Debug logging** added for troubleshooting

### 6. ✅ System Bar Connection Indicator (`src/ui/system_bar.rs`)
- 🟢 **Connected** (green)
- 🟡 **Connecting (attempt N)** (yellow, shows retry count)
- 🔴/⏱️/🔒/❓ **Error type icons** (red, specific to error)
- Shows abbreviated error messages (Connection Refused, Timeout, etc.)
- **Smart error display**: When system status unavailable, shows connection error instead of "Unknown | Loading..."
  - `Disconnected`: Shows error message (e.g., "Connection refused - is Syncthing running?")
  - `Connecting`: Shows last error if available, otherwise "Connecting..."
  - `Connected`: Shows "Loading..." (normal startup)

### 7. ✅ Setup Help Dialog (`src/ui/dialogs.rs`, `src/handlers/keyboard.rs`)
- Shown when no cache and API unreachable
- Displays error message and config path
- **[r]** Retry connection
- **[c]** Copy config path to clipboard
- **[q]** Quit
- Integrated into render pipeline and keyboard handler

### 8. ✅ Simplified API Service (Removed Per-Request Retries) (`src/services/api.rs`)
- **Removed** `execute_request_with_retry()` - redundant with background reconnection
- API requests now fail fast (no per-request retries)
- Background reconnection loop (Task #11) handles all retry logic globally
- Simpler architecture with single retry mechanism using exponential backoff

### 9. ✅ All Tests Passing
- **183/183 tests pass** ✅
- Zero compiler errors
- Zero warnings

### 10. ✅ API Response Handlers Track Connection State (`src/handlers/api.rs`)
- Updated all API response handlers to track connection state:
  - **BrowseResult**: Updates to Connected on success, Disconnected on error (critical)
  - **FileInfoResult**: Updates to Connected on success, Disconnected on error (critical)
  - **FolderStatusResult**: Updates to Connected on success, Disconnected on error (critical)
  - **SystemStatusResult**: Updates to Connected on success, logs error but doesn't change state (non-critical)
  - **ConnectionStatsResult**: Updates to Connected on success, logs error but doesn't change state (non-critical)
- **Critical vs Non-Critical Endpoints**:
  - Critical endpoints (browse, file info, folder status) affect connection state on failure
  - Non-critical endpoints (system status, connection stats) only log errors - they're UI enhancements
  - This prevents transient failures in optional features from marking the entire connection as down
- Errors are classified using `logic::errors::classify_error()` for specific error types
- Connection state changes happen in real-time as API responses arrive

### 11. ✅ Background Reconnection Loop (`src/main.rs:3480-3528`)
- Added fields to `App` struct:
  - `last_reconnect_attempt: Instant` (line 193)
  - `reconnect_delay: Duration` (line 194)
- Initialized in `App::new()` (lines 530-531)
- Background reconnection logic in main event loop with **exponential backoff**:
  - Only triggers when `Disconnected` or `Connecting` (not when `Connected`)
  - Exponential backoff schedule: **5s → 10s → 20s → 40s → 60s (capped)**
  - Updates state to show retry attempt count
  - Calls `refresh_folder_statuses_nonblocking()` to test connection
  - Automatically transitions from `Disconnected` → `Connecting` (attempt 1)
  - Increments attempt counter on subsequent retries
  - Connection state updates to `Connected` on first successful API response
  - Resets delay back to 5s when connection is restored
- Debug logging shows current delay for troubleshooting

---

## 📋 REMAINING TASKS (1)

### 12. ⏳ Manual Testing
- Test with Syncthing stopped (connection refused)
- Test with wrong API key (401)
- Test with firewall blocking (timeout)
- Verify cache fallback works
- Verify reconnection succeeds

---

## Implementation Details (For Reference)

## Changes Required

### 1. **New Error Types & State** (`src/model/syncthing.rs`)
Add connection state tracking to `SyncthingModel`:
```rust
pub enum ConnectionState {
    Connected,
    Connecting { attempt: u32, last_error: Option<String> },
    Disconnected { error_type: ErrorType, message: String },
}

pub enum ErrorType {
    ConnectionRefused,
    Timeout,
    Unauthorized,
    NotFound,
    ServerError,
    NetworkError,
    Other,
}
```

Add field: `pub connection_state: ConnectionState`

### 2. **Graceful Initialization** (`src/main.rs:361-480`)
**Current (FATAL):**
```rust
let folders = client.get_folders().await?;  // Line 364 - crashes on failure
```

**New (GRACEFUL):**
```rust
// Try API first
let (folders, initial_connection_state) = match client.get_folders().await {
    Ok(folders) => (folders, ConnectionState::Connected),
    Err(e) => {
        // Fallback to cache
        let cached_folders = cache.get_all_folders().unwrap_or_default();
        if cached_folders.is_empty() {
            // No cache - show setup help
            (vec![], ConnectionState::Disconnected {
                error_type: classify_error(&e),
                message: format_error_message(&e),
            })
        } else {
            // Use cache, will auto-retry
            (cached_folders, ConnectionState::Connecting {
                attempt: 1,
                last_error: Some(e.to_string())
            })
        }
    }
};
```

Also make `get_devices()` follow same pattern (currently uses `unwrap_or_default()` which is good, but should update connection state).

### 3. **Error Classification** (`src/logic/errors.rs` - NEW FILE)
Pure functions to classify and format errors:

```rust
use anyhow::Error;

pub enum ErrorType {
    ConnectionRefused,
    Timeout,
    Unauthorized,    // HTTP 401
    NotFound,        // HTTP 404
    ServerError,     // HTTP 500+
    NetworkError,    // DNS, routing, etc.
    Other,
}

/// Classify an error based on its type and error chain
pub fn classify_error(error: &Error) -> ErrorType {
    let error_msg = error.to_string().to_lowercase();

    // Check for connection-specific errors
    if error_msg.contains("connection refused") {
        return ErrorType::ConnectionRefused;
    }
    if error_msg.contains("timeout") || error_msg.contains("timed out") {
        return ErrorType::Timeout;
    }

    // Check for HTTP status codes (via reqwest error chain)
    if let Some(reqwest_err) = error.downcast_ref::<reqwest::Error>() {
        if let Some(status) = reqwest_err.status() {
            return match status.as_u16() {
                401 => ErrorType::Unauthorized,
                404 => ErrorType::NotFound,
                500..=599 => ErrorType::ServerError,
                _ => ErrorType::Other,
            };
        }
    }

    // Network-level errors
    if error_msg.contains("dns") || error_msg.contains("network") {
        return ErrorType::NetworkError;
    }

    ErrorType::Other
}

/// Format user-friendly error message showing error type
pub fn format_error_message(error: &Error) -> String {
    match classify_error(error) {
        ErrorType::ConnectionRefused => "Connection refused - is Syncthing running?".to_string(),
        ErrorType::Timeout => "Connection timeout - check network or URL".to_string(),
        ErrorType::Unauthorized => "Unauthorized - check API key".to_string(),
        ErrorType::NotFound => "API endpoint not found - check Syncthing version".to_string(),
        ErrorType::ServerError => "Syncthing server error".to_string(),
        ErrorType::NetworkError => "Network error - check connectivity".to_string(),
        ErrorType::Other => format!("Connection error: {}", error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_connection_refused() {
        let err = anyhow::anyhow!("connection refused (os error 111)");
        assert!(matches!(classify_error(&err), ErrorType::ConnectionRefused));
    }

    #[test]
    fn test_classify_timeout() {
        let err = anyhow::anyhow!("request timed out");
        assert!(matches!(classify_error(&err), ErrorType::Timeout));
    }

    #[test]
    fn test_format_connection_refused() {
        let err = anyhow::anyhow!("connection refused");
        let msg = format_error_message(&err);
        assert!(msg.contains("Connection refused"));
        assert!(msg.contains("Syncthing running"));
    }
}
```

### 4. **Retry Logic** (`src/services/api.rs`)
Add retry wrapper around API calls with exponential backoff:

```rust
async fn execute_request_with_retry(
    client: &SyncthingClient,
    request: ApiRequest,
) -> ApiResponse {
    let max_retries = 3;
    let mut delay = Duration::from_secs(1);

    for attempt in 0..max_retries {
        let result = execute_request(client, request.clone()).await;

        // Check if we should retry
        let should_retry = match &result {
            ApiResponse::BrowseResult { items, .. } => items.is_err(),
            ApiResponse::FileInfoResult { info, .. } => info.is_err(),
            ApiResponse::FolderStatusResult { status, .. } => status.is_err(),
            ApiResponse::SystemStatusResult { status } => status.is_err(),
            ApiResponse::ConnectionStatsResult { stats } => stats.is_err(),
            _ => false,
        };

        // If success or last attempt, return result
        if !should_retry || attempt >= max_retries - 1 {
            return result;
        }

        // Wait before retry with exponential backoff
        tokio::time::sleep(delay).await;
        delay *= 2; // 1s -> 2s -> 4s
    }

    unreachable!("Loop should always return via early return")
}
```

Update `api_service()` to use `execute_request_with_retry()` instead of `execute_request()`.

### 5. **Cache Folder Persistence** (`src/cache.rs`)
Add methods to cache complete folder data (not just browse results):

```rust
impl CacheDb {
    /// Save folders to cache (for graceful degradation on startup)
    pub fn save_folders(&self, folders: &[Folder]) -> Result<()> {
        let json = serde_json::to_string(folders)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO cached_folders (id, data) VALUES (1, ?1)",
            params![json],
        )?;

        Ok(())
    }

    /// Get all cached folders
    pub fn get_all_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM cached_folders WHERE id = 1"
        )?;

        let json: String = stmt.query_row([], |row| row.get(0))?;
        let folders: Vec<Folder> = serde_json::from_str(&json)?;

        Ok(folders)
    }
}
```

Add to schema in `init_schema()`:
```sql
CREATE TABLE IF NOT EXISTS cached_folders (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    data TEXT NOT NULL
);
```

**Save folders after successful fetch** (in `src/handlers/api.rs` or during successful folder refresh):
```rust
// After successful folder fetch
if let Err(e) = app.cache.save_folders(&folders) {
    log_debug(&format!("Failed to cache folders: {}", e));
}
```

### 6. **Connection Status UI** (`src/ui/system_bar.rs`)
Add connection indicator **before** device name in system bar:

**Current layout:**
```
DeviceName | Uptime: 5d 3h | Local: 1234 items | ↓ 1.2 MB/s ↑ 512 KB/s
```

**New layout:**
```
🟢 Connected | DeviceName | Uptime: 5d 3h | Local: 1234 items | ↓ 1.2 MB/s ↑ 512 KB/s
🟡 Connecting (attempt 2) | DeviceName | Uptime: 5d 3h | ...
🔴 Connection Refused | Unknown | ...
```

```rust
fn render_connection_status(state: &ConnectionState) -> Span {
    match state {
        ConnectionState::Connected => {
            Span::styled("🟢 Connected", Style::default().fg(Color::Green))
        }
        ConnectionState::Connecting { attempt, .. } => {
            let text = if *attempt > 1 {
                format!("🟡 Connecting (attempt {})", attempt)
            } else {
                "🟡 Connecting...".to_string()
            };
            Span::styled(text, Style::default().fg(Color::Yellow))
        }
        ConnectionState::Disconnected { error_type, message } => {
            let icon = match error_type {
                ErrorType::ConnectionRefused => "🔴",
                ErrorType::Timeout => "⏱️",
                ErrorType::Unauthorized => "🔒",
                _ => "⚠️",
            };
            Span::styled(
                format!("{} {}", icon, message),
                Style::default().fg(Color::Red)
            )
        }
    }
}
```

### 7. **Setup Help Dialog** (`src/ui/dialogs.rs`)
New dialog when no cache exists and initial connection fails:

```rust
pub fn render_setup_help_dialog(
    error_type: &ErrorType,
    error_message: &str,
    config_path: &str,
) -> Paragraph {
    let text = vec![
        Line::from("Cannot connect to Syncthing API"),
        Line::from(""),
        Line::from(Span::styled(
            format!("Error: {}", error_message),
            Style::default().fg(Color::Red),
        )),
        Line::from(""),
        Line::from("Please check:"),
        Line::from("  • Is Syncthing running?"),
        Line::from("  • Is the API URL correct?"),
        Line::from("  • Is the API key valid?"),
        Line::from(""),
        Line::from(Span::styled(
            format!("Config: {}", config_path),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from("[r] Retry    [c] Copy config path    [q] Quit"),
    ];

    Paragraph::new(text)
        .block(
            Block::default()
                .title("Connection Failed - Setup Help")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
        )
        .wrap(ratatui::widgets::Wrap { trim: false })
}
```

**Add state to Model** (`src/model/ui.rs`):
```rust
pub show_setup_help: bool,
pub setup_help_config_path: String,
```

**Trigger during initialization** when no cache and connection fails - set `show_setup_help = true` and store the config path (use `dirs::config_dir()` for cross-platform support).

### 8. **Cross-Platform Config Path Helper** (`src/logic/path.rs` or new `src/logic/platform.rs`)
```rust
use std::path::PathBuf;

/// Get the expected config file path for the current platform
/// Linux: ~/.config/synctui/config.yaml
/// macOS: ~/Library/Application Support/synctui/config.yaml
/// Windows: %APPDATA%\synctui\config.yaml
pub fn get_default_config_path() -> String {
    if let Some(config_dir) = dirs::config_dir() {
        config_dir
            .join("synctui")
            .join("config.yaml")
            .display()
            .to_string()
    } else {
        // Fallback if dirs crate can't determine config dir
        "./config.yaml".to_string()
    }
}

/// Get cache directory path (matches CacheDb::get_cache_dir logic)
/// Linux: ~/.cache/synctui/cache.db
/// macOS: ~/Library/Caches/synctui/cache.db
/// Windows: %LOCALAPPDATA%\synctui\cache.db
pub fn get_cache_path() -> String {
    if let Some(cache_dir) = dirs::cache_dir() {
        cache_dir
            .join("synctui")
            .join("cache.db")
            .display()
            .to_string()
    } else {
        "/tmp/synctui-cache/cache.db".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_path_not_empty() {
        let path = get_default_config_path();
        assert!(!path.is_empty());
        assert!(path.contains("synctui"));
    }

    #[test]
    fn test_get_cache_path_not_empty() {
        let path = get_cache_path();
        assert!(!path.is_empty());
        assert!(path.contains("synctui"));
    }
}
```

**During initialization**, store the actual config path used (from `get_config_path()`) in the Model so setup help can display it.

### 9. **Background Reconnection** (`src/main.rs` event loop)
Add periodic reconnection attempts when disconnected:

```rust
// Add to App struct
last_reconnect_attempt: Instant,

// In event loop (around line 3050+)
if matches!(
    app.model.syncthing.connection_state,
    ConnectionState::Disconnected { .. } | ConnectionState::Connecting { .. }
) {
    if app.last_reconnect_attempt.elapsed() >= Duration::from_secs(5) {
        // Try to refresh folders to test connection
        let _ = app.api_tx.send(ApiRequest::RefreshFolders);
        app.last_reconnect_attempt = Instant::now();

        // Update state to show we're attempting
        if let ConnectionState::Disconnected { error_type, message } =
            &app.model.syncthing.connection_state
        {
            app.model.syncthing.connection_state = ConnectionState::Connecting {
                attempt: 1,
                last_error: Some(message.clone()),
            };
        } else if let ConnectionState::Connecting { attempt, last_error } =
            &app.model.syncthing.connection_state
        {
            app.model.syncthing.connection_state = ConnectionState::Connecting {
                attempt: attempt + 1,
                last_error: last_error.clone(),
            };
        }
    }
}
```

**Add RefreshFolders request type** to `ApiRequest` enum in `src/services/api.rs`.

### 10. **Update Connection State on API Results** (`src/handlers/api.rs`)
Update connection state based on API responses:

```rust
// In handle_api_response()
match response {
    ApiResponse::BrowseResult { items, .. } => {
        match items {
            Ok(_) => {
                // Successful API call - mark as connected
                app.model.syncthing.connection_state = ConnectionState::Connected;
            }
            Err(e) => {
                // Failed API call - update connection state
                let error = anyhow::anyhow!(e);
                app.model.syncthing.connection_state = ConnectionState::Disconnected {
                    error_type: logic::errors::classify_error(&error),
                    message: logic::errors::format_error_message(&error),
                };
            }
        }
    }
    // Similar for other response types
}
```

**Important:** Only update to `Disconnected` if the error is connection-related (not validation/parsing errors).

### 11. **Setup Help Dialog Keyboard Handler** (`src/handlers/keyboard.rs`)
Handle keys when setup help is shown:

```rust
// At top of keyboard handler (before other conditions)
if app.model.ui.show_setup_help {
    match key.code {
        KeyCode::Char('r') => {
            // Retry connection
            app.model.ui.show_setup_help = false;
            let _ = app.api_tx.send(ApiRequest::RefreshFolders);
            app.model.syncthing.connection_state = ConnectionState::Connecting {
                attempt: 1,
                last_error: None,
            };
        }
        KeyCode::Char('c') => {
            // Copy config path to clipboard
            if let Some(clipboard_cmd) = &app.config.clipboard_command {
                let path = &app.model.ui.setup_help_config_path;
                if let Err(e) = logic::clipboard::copy_to_clipboard(clipboard_cmd, path) {
                    app.model.ui.show_toast(format!("Failed to copy: {}", e));
                } else {
                    app.model.ui.show_toast("Config path copied to clipboard");
                }
            } else {
                app.model.ui.show_toast("No clipboard command configured");
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            // Quit app
            return Ok(true);
        }
        _ => {}
    }
    return Ok(false);
}
```

### 12. **Graceful Degradation**
- All runtime API failures update `connection_state` instead of crashing
- Keep showing last known good data when connection lost
- Event listener already has good retry logic (5s backoff) - keep it
- Connection stats errors update connection state but don't block UI
- Cache serves as fallback during disconnection

### 13. **Testing Requirements**
Write comprehensive tests:

**Unit tests** (`src/logic/errors.rs`):
- `classify_error()` with various reqwest error types
- `format_error_message()` output formatting
- Connection refused, timeout, 401, 404, 500 errors

**Integration tests**:
- Startup with Syncthing down (no cache) → setup help shown
- Startup with Syncthing down (with cache) → cache loaded, auto-retry
- Runtime disconnection → status bar updates, cache remains functional
- Reconnection success → status bar updates, fresh data loaded
- Retry backoff timing (1s, 2s, 4s)

**Manual test scenarios**:
1. Stop Syncthing, start app with empty cache
2. Stop Syncthing, start app with existing cache
3. Start app normally, kill Syncthing mid-session
4. Start app with wrong API key (401 error)
5. Start app with wrong port (connection refused)
6. Start app with firewall blocking (timeout)

## Files to Modify

1. **src/model/syncthing.rs** - Add `ConnectionState` enum and field
2. **src/logic/errors.rs** - NEW - Error classification/formatting functions
3. **src/logic/platform.rs** - NEW - Cross-platform config/cache path helpers
4. **src/main.rs** - Graceful init, background reconnection, store config path
5. **src/services/api.rs** - Retry wrapper with exponential backoff, `RefreshFolders` request
6. **src/cache.rs** - Add `save_folders()` and `get_all_folders()` methods
7. **src/ui/system_bar.rs** - Connection status indicator (before device name)
8. **src/ui/dialogs.rs** - Setup help dialog rendering
9. **src/handlers/keyboard.rs** - Handle setup dialog keys (r/c/q)
10. **src/handlers/api.rs** - Update connection state on API results

## User Experience Flows

### Scenario 1: First launch, API down, no cache
1. App starts, tries to fetch folders → connection refused
2. No cached folders → Show setup help dialog
3. User sees error type: "Connection refused - is Syncthing running?"
4. Config path displayed (cross-platform correct path)
5. User presses **'r'** → Retry immediately
6. Still fails → Dialog shown again
7. User presses **'c'** → Config path copied to clipboard
8. User fixes config, presses **'r'** → Success, app loads

### Scenario 2: Launch with cache, API down
1. App starts, tries to fetch folders → connection refused
2. Has cached folders → Load from cache immediately
3. System bar shows: "🟡 Connecting (attempt 1) | DeviceName | ..."
4. App functional with cached data
5. Background auto-retry every 5s: attempt 2, 3, 4...
6. User can browse, see status, use all features
7. When Syncthing comes back online → "🟢 Connected"
8. Fresh data loaded in background

### Scenario 3: Runtime disconnection
1. App running normally: "🟢 Connected | DeviceName | ..."
2. User stops Syncthing (or network fails)
3. Next API call fails (e.g., browse folder) after 3 retries
4. System bar updates: "🔴 Connection Refused | DeviceName | ..."
5. Cache keeps UI functional (browse cached directories)
6. Background auto-retry starts (every 5s)
7. User continues working with cached data
8. When reconnected → "🟢 Connected", fresh data loads

### Scenario 4: Wrong API key (401)
1. App starts → HTTP 401 Unauthorized
2. System bar: "🔒 Unauthorized - check API key | Unknown | ..."
3. Setup help dialog (if no cache) shows specific error
4. User presses 'c' to copy config path
5. User fixes API key in config
6. User presses 'r' to retry → Success

### Scenario 5: Timeout
1. Network slow/firewall blocking → request timeout
2. After 3 retries (1s + 2s + 4s = 7s total)
3. System bar: "⏱️ Connection timeout - check network or URL"
4. Auto-retry continues every 5s
5. When network recovers → reconnects

## Implementation Order

1. **Error classification** (`src/logic/errors.rs`) - Pure functions, fully testable
2. **Platform helpers** (`src/logic/platform.rs`) - Config/cache path helpers
3. **Connection state** (`src/model/syncthing.rs`) - Add enum and field
4. **Cache persistence** (`src/cache.rs`) - Folder save/load methods
5. **Graceful init** (`src/main.rs`) - Non-fatal startup, cache fallback
6. **Setup help UI** (`src/ui/dialogs.rs`, `src/handlers/keyboard.rs`) - Dialog and handlers
7. **System bar indicator** (`src/ui/system_bar.rs`) - Connection status display
8. **Retry logic** (`src/services/api.rs`) - Exponential backoff wrapper
9. **State updates** (`src/handlers/api.rs`) - Update connection state on results
10. **Background reconnection** (`src/main.rs`) - Periodic retry loop
11. **Testing** - Unit tests for logic, manual testing with stopped Syncthing

## Success Criteria

- ✅ App never crashes due to network errors
- ✅ Startup succeeds even when Syncthing is down
- ✅ Cached data displayed when offline
- ✅ Clear error messages showing error types
- ✅ Auto-retry with exponential backoff (1s, 2s, 4s)
- ✅ Persistent connection status in system bar
- ✅ Setup help shown when no cache and connection fails
- ✅ Config path displayed correctly for Linux/macOS/Windows
- ✅ Background reconnection every 5s when disconnected
- ✅ All 169+ existing tests still pass
- ✅ New unit tests for error classification (5+ tests)
