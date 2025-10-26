# Status Bar State Indicators Design

**Date:** 2025-11-09
**Status:** Approved

## Problem Statement

The status bar currently shows incomplete state information:

- **Folders (focus_level == 0):** Raw API strings like "idle", "waiting-to-scan" without visual indicators
- **Files/Directories (focus_level > 0):** Only shows "Ignored" states in red; all other states (Synced, OutOfSync, LocalOnly, RemoteOnly, Syncing) are invisible

Users cannot quickly identify file/directory sync states or understand folder states at a glance.

## Solution Overview

Add visual state indicators with icons to both folder and breadcrumb views, using the existing `IconRenderer` system for consistency.

### Goals

1. **Match Syncthing Web UI:** Use same terminology (e.g., "Local Additions" for receive-only folders)
2. **Visual Consistency:** Reuse existing icon system (emoji/nerdfont toggle)
3. **Clean Display:** Icons + labels, no "State:" prefix (saves space)
4. **User-Friendly:** Transform raw API strings to readable labels

### Display Format

**Folders:**
```
Folder: Movies | ✅ Idle | Up to date | Send & Receive | 50GB | 45/50 items
```

**Files/Directories:**
```
Folder: Movies | 42 items | Sort: A-Z↑ | 💻 Local Only | Selected: photo.jpg
```

## Folder State Mapping

For folders (focus_level == 0), map the 10 Syncthing API states to `FolderState` enum variants with user-friendly labels:

| API State | FolderState Enum | Icon (emoji/nerd) | Display Label |
|-----------|------------------|-------------------|---------------|
| `"idle"` | `Synced` | ✅ /  | Idle |
| `"scanning"` | `Scanning` | 🔍 /  | Scanning |
| `"syncing"` | `Syncing` | 🔄 /  | Syncing |
| `"preparing"` | `Syncing` | 🔄 /  | Preparing |
| `"waiting-to-scan"` | `Scanning` | 🔍 /  | Waiting to Scan |
| `"outofsync"` | `OutOfSync` | ⚠️ /  | Out of Sync |
| `"error"` | `Error` | ❌ /  | Error |
| `"stopped"` | `Paused` | ⏸️ /  | Stopped |
| `"paused"` | `Paused` | ⏸️ /  | Paused |
| `"unshared"` | `Unknown` | ❓ /  | Unshared |

### Special Case: Local Additions

When `status.receive_only_total_items > 0`, override the API state with:
- **FolderState:** `LocalOnly` (reusing existing enum variant)
- **Icon:** 💻 /
- **Display:** "Local Additions"

This matches the Syncthing web UI behavior and clearly indicates the folder needs attention.

### Field Order

Current order:
```
Folder: {name} | {type} | {state} | {bytes} | {items} | {need_display}
```

New order:
```
Folder: {name} | {state} | {need_display} | {type} | {bytes} | {items}
```

**Rationale:** Puts the most important information (state and sync status) immediately after the folder name for faster scanning.

## File/Directory State Display

For files and directories (focus_level > 0), display all `SyncState` values with icons:

| SyncState | Icon (emoji/nerd) | Display Label |
|-----------|-------------------|---------------|
| `Synced` | ✅ /  | Synced |
| `OutOfSync` | ⚠️ /  | Out of Sync |
| `LocalOnly` | 💻 /  | Local Only |
| `RemoteOnly` | ☁️ /  | Remote Only |
| `Ignored` | 🚫 /  | Ignored |
| `Syncing` | 🔄 /  | Syncing |
| `Unknown` | ❓ /  | Unknown |

### Ignored State Enhancement

Preserve existing red text behavior for ignored items:
- When `sync_state == Ignored && !exists_on_disk`: **"🚫 Ignored"** (red text)
- When `sync_state == Ignored && exists_on_disk`: **"🚫 Ignored, not deleted!"** (red text)

### Position in Status Bar

State appears after "Sort:" and before "Selected:" for better visual flow:

```
Folder: Movies | 42 items | Sort: A-Z↑ | 💻 Local Only | Selected: photo.jpg
```

## Implementation Plan

### 1. Create Folder State Mapping Helper

**Location:** `src/ui/status_bar.rs`

```rust
/// Map Syncthing API state to FolderState enum and user-friendly label
fn map_folder_state(
    api_state: &str,
    receive_only_items: u64,
) -> (FolderState, &'static str) {
    // Special case: Local Additions takes precedence
    if receive_only_items > 0 {
        return (FolderState::LocalOnly, "Local Additions");
    }

    match api_state {
        "idle" => (FolderState::Synced, "Idle"),
        "scanning" => (FolderState::Scanning, "Scanning"),
        "syncing" => (FolderState::Syncing, "Syncing"),
        "preparing" => (FolderState::Syncing, "Preparing"),
        "waiting-to-scan" => (FolderState::Scanning, "Waiting to Scan"),
        "outofsync" => (FolderState::OutOfSync, "Out of Sync"),
        "error" => (FolderState::Error, "Error"),
        "stopped" => (FolderState::Paused, "Stopped"),
        "paused" => (FolderState::Paused, "Paused"),
        "unshared" => (FolderState::Unknown, "Unshared"),
        _ => (FolderState::Unknown, api_state), // Fallback
    }
}
```

### 2. Update Folder Status Bar Format

**Location:** `build_status_line()` in `src/ui/status_bar.rs` (lines 99-177)

**Changes:**
- Add `IconRenderer` parameter to function signature
- Call `map_folder_state()` to get enum + label
- Use `IconRenderer::render_folder_state()` to get icon string
- Reorder format string: `name | state | need_display | type | bytes | items`

### 3. Add File/Directory State Display

**Location:** `build_status_line()` in `src/ui/status_bar.rs` (lines 184-240)

**Changes:**
- Add state field after "Sort:" in metrics vec
- Use `IconRenderer::render_sync_state()` to get icon string
- Format: `"{icon} {label}"` where label comes from state
- Preserve red text for Ignored states (apply after icon rendering)

### 4. Update Function Signatures

**Files to modify:**
- `build_status_paragraph()` - add `icon_renderer: &IconRenderer` parameter
- `build_status_line()` - add `icon_renderer: &IconRenderer` parameter
- `render_status_bar()` - add `icon_renderer: &IconRenderer` parameter, pass through
- `calculate_status_height()` - add `icon_renderer: &IconRenderer` parameter, pass through

**Callers to update:**
- `src/ui/render.rs` - pass `app.icon_renderer` when calling status bar functions

### 5. Update Tests

**Location:** `src/ui/status_bar.rs` (if tests exist)

- Update expected strings to match new field order
- Add test cases for folder state mapping (all 10 API states + local additions)
- Verify icon rendering works with both emoji and nerdfont modes

## Testing Checklist

**Manual Testing:**

1. **Folder States:**
   - [ ] Idle folder shows "✅ Idle"
   - [ ] Scanning folder shows "🔍 Scanning"
   - [ ] Syncing folder shows "🔄 Syncing"
   - [ ] Out of sync folder shows "⚠️ Out of Sync"
   - [ ] Paused folder shows "⏸️ Paused"
   - [ ] Receive-only folder with local additions shows "💻 Local Additions"
   - [ ] Field order: name → state → sync status → type → size → items

2. **File/Directory States:**
   - [ ] Synced file shows "✅ Synced"
   - [ ] Out of sync file shows "⚠️ Out of Sync"
   - [ ] Local only file shows "💻 Local Only"
   - [ ] Remote only file shows "☁️ Remote Only"
   - [ ] Ignored file (deleted) shows "🚫 Ignored" in red
   - [ ] Ignored file (exists) shows "🚫 Ignored, not deleted!" in red
   - [ ] Syncing file shows "🔄 Syncing"
   - [ ] State appears after "Sort:" and before "Selected:"

3. **Icon Modes:**
   - [ ] Emoji mode renders correctly
   - [ ] Nerdfont mode renders correctly (test with `icon_mode: "nerdfont"` in config)

4. **Edge Cases:**
   - [ ] Unknown states handled gracefully
   - [ ] Empty folder status (loading state)
   - [ ] Very long folder names don't break layout
   - [ ] Status bar wraps correctly on narrow terminals

## Non-Goals

- Changing existing sync progress display (arrows, byte counts)
- Modifying folder type selection logic
- Adding new state enum variants (reuse existing)
- Changing icon designs (use existing IconRenderer)

## Future Enhancements

- Clickable states to show detailed sync information
- Tooltips with state explanations
- Color coding states beyond just icons (green for synced, yellow for pending, etc.)
