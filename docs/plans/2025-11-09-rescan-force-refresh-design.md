# Rescan Force Refresh Feature Design

**Date**: 2025-11-09
**Status**: Approved

## Problem Statement

The current rescan feature (`r` key) relies on Syncthing's sequence validation to detect when cache needs invalidation. This has two limitations:

1. **Stale cache bugs**: Sometimes cache gets out of sync and sequence validation doesn't catch it
2. **Force refresh need**: Users want to manually refresh even when Syncthing thinks nothing changed

## Solution Overview

Add a confirmation dialog when user presses `r` that offers two options:
- **(y) Normal rescan**: Existing behavior - trigger Syncthing scan, wait for sequence change
- **(f) Force refresh**: New behavior - invalidate cache immediately, then trigger Syncthing scan

## User Experience

### User Flow

1. User presses `r` in either folder list or breadcrumb view
2. Dialog appears showing the selected folder name
3. User chooses:
   - `y` → Normal rescan (existing behavior)
   - `f` → Force refresh (new behavior)
   - `n` or `Esc` → Cancel

### Dialog Content

```
┌─ Rescan Folder ─────────────────────────┐
│ Rescan folder "Movies"?                 │
│                                          │
│ (y) Rescan - Ask Syncthing to scan for  │
│              changes                     │
│ (f) Force Refresh - Clear cache + rescan│
│ (n) Cancel                               │
└──────────────────────────────────────────┘
```

### Dialog Styling
- Cyan border (matches other confirmation dialogs)
- Centered on screen
- Auto-sized to content
- Folder name in quotes for clarity

### Toast Messages
- Normal: "Rescanning Movies..."
- Force: "Force refreshing Movies..."

## Technical Design

### Data Structures

**New ConfirmAction Variant** (`src/model/types.rs`):
```rust
pub enum ConfirmAction {
    // ... existing variants ...
    Rescan {
        folder_id: String,
        folder_label: String,
    },
}
```

### State Flow

1. **Trigger**: User presses `r`
   - Get folder_id and folder_label via `get_rescan_folder_info()`
   - Set `app.model.ui.confirm_action = Some(ConfirmAction::Rescan { ... })`
   - Return early (don't execute immediately)

2. **Dialog Active**: User sees confirmation
   - `src/ui/render.rs` matches on `ConfirmAction::Rescan`
   - Calls `render_rescan_confirmation()` in `src/ui/dialogs.rs`

3. **User Response**: Handled at top of keyboard handler
   - `y` → Call `rescan_selected_folder()` (existing)
   - `f` → Call `force_refresh_folder()` (new)
   - `n`/`Esc` → Clear `confirm_action`

### Implementation Details

#### Code Reuse Strategy

**New Helper Method** (`src/app/file_ops.rs`):
```rust
fn get_rescan_folder_info(&self) -> Option<(String, String)> {
    if self.model.navigation.focus_level == 0 {
        // Folder list view
        let selected = self.model.navigation.folders_state_selection?;
        let folder = self.model.syncthing.folders.get(selected)?;
        Some((folder.id.clone(), folder.label.clone()))
    } else {
        // Breadcrumb view
        if self.model.navigation.breadcrumb_trail.is_empty() {
            return None;
        }
        let level = &self.model.navigation.breadcrumb_trail[0];
        Some((level.folder_id.clone(), level.folder_label.clone()))
    }
}
```

**Refactored Existing Method**:
```rust
pub(crate) fn rescan_selected_folder(&mut self) -> Result<()> {
    let (folder_id, _) = self.get_rescan_folder_info()
        .ok_or_else(|| anyhow::anyhow!("No folder selected"))?;

    log_debug(&format!(
        "DEBUG [rescan_selected_folder]: Requesting rescan for folder={}",
        folder_id
    ));

    let _ = self.api_tx.send(ApiRequest::RescanFolder { folder_id });
    Ok(())
}
```

**New Force Refresh Method**:
```rust
pub(crate) fn force_refresh_folder(&mut self, folder_id: &str) -> Result<()> {
    log_debug(&format!(
        "DEBUG [force_refresh_folder]: Force refreshing folder={}",
        folder_id
    ));

    // Step 1: Invalidate cache and refresh breadcrumbs
    self.invalidate_and_refresh_folder(folder_id);

    // Step 2: Trigger Syncthing rescan
    let _ = self.api_tx.send(ApiRequest::RescanFolder {
        folder_id: folder_id.to_string(),
    });

    Ok(())
}
```

**Shared Invalidation Logic** (extracted from `handlers/api.rs` FolderStatusResult):
```rust
fn invalidate_and_refresh_folder(&mut self, folder_id: &str) {
    // Invalidate cache
    let _ = self.cache.invalidate_folder(folder_id);
    let _ = self.cache.invalidate_out_of_sync_categories(folder_id);

    // Clear discovered directories
    self.model.performance.discovered_dirs
        .retain(|key| !key.starts_with(&format!("{}:", folder_id)));

    // Refresh breadcrumbs if viewing this folder
    if !self.model.navigation.breadcrumb_trail.is_empty()
        && self.model.navigation.breadcrumb_trail[0].folder_id == folder_id
    {
        for level in &self.model.navigation.breadcrumb_trail {
            let browse_key = format!(
                "{}:{}",
                folder_id,
                level.prefix.as_deref().unwrap_or("")
            );

            if !self.model.performance.loading_browse.contains(&browse_key) {
                self.model.performance.loading_browse.insert(browse_key);
                let _ = self.api_tx.send(ApiRequest::BrowseFolder {
                    folder_id: folder_id.to_string(),
                    prefix: level.prefix.clone(),
                    priority: Priority::High,
                });
            }
        }
    }
}
```

#### Keyboard Handler Changes (`src/handlers/keyboard.rs`)

**Confirmation Handler** (at top with other confirmations):
```rust
ConfirmAction::Rescan { folder_id, folder_label } => {
    match key.code {
        KeyCode::Char('y') => {
            // Normal rescan
            app.model.ui.confirm_action = None;
            let _ = app.rescan_selected_folder();
            app.model.ui.show_toast(format!("Rescanning {}...", folder_label));
        }
        KeyCode::Char('f') => {
            // Force refresh
            app.model.ui.confirm_action = None;
            let _ = app.force_refresh_folder(&folder_id);
            app.model.ui.show_toast(format!("Force refreshing {}...", folder_label));
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            // Cancel
            app.model.ui.confirm_action = None;
        }
        _ => {} // Ignore other keys
    }
    return Ok(());
}
```

**'r' Key Handler** (replace existing):
```rust
KeyCode::Char('r') => {
    // Get folder info using helper
    if let Some((folder_id, folder_label)) = app.get_rescan_folder_info() {
        app.model.ui.confirm_action = Some(ConfirmAction::Rescan {
            folder_id,
            folder_label,
        });
    }
}
```

#### Dialog Rendering (`src/ui/dialogs.rs`)

**New Dialog Function**:
```rust
pub fn render_rescan_confirmation(f: &mut Frame, folder_label: &str) {
    let text = vec![
        Line::from(format!("Rescan folder \"{}\"?", folder_label)),
        Line::from(""),
        Line::from("(y) Rescan - Ask Syncthing to scan for changes"),
        Line::from("(f) Force Refresh - Clear cache + rescan"),
        Line::from("(n) Cancel"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Rescan Folder ");

    let paragraph = Paragraph::new(text).block(block);
    let area = centered_rect(50, 9, f.size());
    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}
```

**Render Call** (`src/ui/render.rs`):
```rust
ConfirmAction::Rescan { folder_label, .. } => {
    dialogs::render_rescan_confirmation(f, folder_label);
}
```

## Behavior Comparison

### Normal Rescan (y)
1. Trigger `POST /rest/db/scan`
2. Wait for Syncthing to scan filesystem
3. Syncthing increments sequence if changes found
4. FolderStatusResult handler detects sequence change
5. Cache invalidated and breadcrumbs refresh

**When to use**: Most of the time - efficient, only refreshes when needed

### Force Refresh (f)
1. Immediately invalidate folder cache
2. Immediately invalidate out-of-sync categories
3. Clear discovered directories
4. Re-fetch all visible breadcrumb levels
5. Trigger `POST /rest/db/scan`

**When to use**:
- Cache appears stale (files missing/extra in UI)
- Want immediate refresh regardless of sequence
- Debugging cache issues

## Files Modified

1. `src/model/types.rs` - Add `ConfirmAction::Rescan` variant
2. `src/app/file_ops.rs` - Add helper methods and force refresh logic
3. `src/handlers/keyboard.rs` - Update 'r' handler and add confirmation handler
4. `src/ui/dialogs.rs` - Add `render_rescan_confirmation()`
5. `src/ui/render.rs` - Add render call for new dialog

## Testing Strategy

### Manual Testing
1. Test 'r' in folder list view → dialog appears with correct folder name
2. Test 'r' in breadcrumb view → dialog appears with correct folder name
3. Test 'y' → normal rescan executes, toast shows
4. Test 'f' → force refresh executes, breadcrumbs update immediately, toast shows
5. Test 'n' and 'Esc' → dialog closes without action
6. Verify force refresh clears stale cache
7. Verify normal rescan still works with sequence validation

### Unit Tests
- Test `get_rescan_folder_info()` returns correct values for both views
- Test `ConfirmAction::Rescan` variant can be created and pattern matched
- Test dialog rendering doesn't panic with various folder names

## Future Enhancements

- Add force refresh option to other operations (e.g., folder list refresh)
- Track force refresh usage metrics to identify cache bugs
- Consider auto-force-refresh on certain error conditions
