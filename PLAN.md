# Syncthing CLI TUI Manager — Development Plan

This document outlines the step-by-step plan for building a **Rust Ratatui CLI tool** that interfaces with **Syncthing’s REST API** to manage folders, view sync states, and control ignored/deleted files. The steps are structured to allow **progressive, testable milestones**, ideal for both human and LLM collaboration.

---

## Phase 1: Basic Prototype — Folder and Directory Listing

### Objective
Create a minimal working prototype that queries Syncthing’s REST API and lists folders and their contents in a simple Ratatui UI.

### Steps

1. **Setup Project**
   - Initialize a new Rust project:  
     ```bash
     cargo new syncthing-tui
     cd syncthing-tui
     ```
   - Add dependencies in `Cargo.toml`:
     ```toml
     [dependencies]
     ratatui = "0.27"
     crossterm = "0.27"
     reqwest = { version = "0.12", features = ["json"] }
     serde = { version = "1.0", features = ["derive"] }
     serde_json = "1.0"
     tokio = { version = "1", features = ["full"] }
     ```

2. **Implement Config Loader**
   - Create `config.yaml` with:
     ```yaml
     api_key: "YOUR_API_KEY"
     base_url: "http://localhost:8384"
     path_map:
       "/data/": "/mnt/docker/syncthing/data/"
     ```
   - Implement a loader in Rust to read and deserialize this YAML.

3. **Query Folders via Syncthing API**
   - Endpoint: `/rest/system/config`
   - Parse and extract folder IDs and labels.

4. **Render Folder List (TUI)**
   - Display a scrollable list of folder names.
   - Use icons to represent their state:
     - ✅ synced
     - ⚠️ out-of-sync
     - ⏸ paused
   - Navigation: ↑ ↓ to move, `q` to quit.

5. **Query Folder Contents**
   - On pressing `Enter` on a folder, fetch `/rest/db/browse?folder=<id>`.
   - Display the first-level directories (no recursion yet).
   - Render icons per file type:
     - 📁 directory
     - 💻 local-only
     - ☁️ remote-only
     - ⚠️ out-of-sync

6. **Basic Error Handling**
   - Graceful error display if API unavailable.
   - Handle timeouts and authentication errors.

---

## Phase 2: Folder State and Actions

### Objective
Add interactivity — rescan, pause/resume, and ignore actions.

### Steps

1. **Add Folder Status Queries**
   - Endpoint: `/rest/db/status?folder=<id>`.
   - Display “progress” or “needs rescan” state.

2. **Add Folder Controls**
   - `r` → POST `/rest/db/scan?folder=<id>` (rescan)
   - `p` → pause/resume folder (update via `/rest/system/config` PUT)
   - Confirm with small dialog.

3. **Add Ignoring Support**
   - `i` → edit `.stignore` via `/rest/db/ignores?folder=<id>` PUT.
   - Allow appending a line, e.g. `photos/**`.

4. **Add Delete + Ignore**
   - `d` → Confirm deletion → add path to `.stignore` and delete mapped host directory.
   - Use `path_map` to translate Syncthing path → host path.

---

## Phase 3: UX Improvements

### Objective
Make navigation smoother and display richer data.

### Steps

1. **Breadcrumb Navigation**
   - Allow traversing directories with `Enter` / `Backspace`.
   - Maintain a navigation stack per folder.

2. **Async Loading Indicators**
   - Show spinners during REST requests.

3. **Status Bar**
   - Show connection status, folder count, last API poll time.

4. **Keyboard Shortcuts Help**
   - Display modal on `?` showing all hotkeys.

---

## Phase 4: Event Listening and Live Updates

### Objective
Subscribe to `/rest/events` for live status updates.

### Steps

1. **Implement Event Listener (async task)**
   - Stream events and update UI reactively.
   - Detect folder rescans, sync completion, etc.

2. **Display Realtime Icons**
   - Automatically update states (✅, ⚠️, ⏸).

3. **Handle Connection Drops**
   - Reconnect and retry event stream automatically.

---

## Phase 5: Polishing and Extensions

### Objective
Add quality-of-life improvements and new modes.

### Steps

1. **Filesystem Diff Mode**
   - Compare local vs remote contents using `/rest/db/browse` and `/rest/db/file`.

2. **Batch Operations**
   - Multi-select directories for ignore/delete/rescan.

3. **Configurable Keybindings**
   - Optional TOML or YAML keymap file.

4. **Cross-Platform Packaging**
   - Build for Linux, macOS, and Windows with cross-compilation via `cross`.

---

## Future Considerations

- Live disk usage stats (`du`-like)
- Integration with Docker volumes
- CLI flags for headless operations
- Log viewer for Syncthing system logs
- Offline cache for quick folder browsing

---

### Summary of Phased Goals

| Phase | Goal | Core Feature |
|-------|------|---------------|
| 1 | Initial prototype | Display folders & directories |
| 2 | Control actions | Ignore, delete, rescan, pause/resume |
| 3 | UX polish | Navigation, help modal, status bar |
| 4 | Live updates | Event streaming and reactive icons |
| 5 | Advanced features | Diff view, batch actions, packaging |

---

**Final Deliverable:**  
A cross-platform, keyboard-driven TUI manager for Syncthing that provides complete visibility and control over folders and files using only the REST API.
