# Syncthing CLI TUI Manager: Development Plan

## 🎯 Future Work

---

### Feature Enhancements

**High Priority:**
- Markdown rendering in the file preview (headings, emphasis, lists, code blocks) instead of raw text
- Syncthing log viewer
- In the file preview popup, a key to switch the preview to the next/previous file in the current directory. Up/down and `j`/`k` scroll text, and PgUp/PgDn already page through text previews too, so ←/→ (`h`/`l` in vim mode) may be the free pair

**Medium Priority:**
- Set the terminal title (OSC 2) to the current context: folder, directory path, or the file being previewed. Restore the previous title on exit
- Folder groups (Syncthing v2.1.0+): `group` field on folder config, read from `/rest/system/config` and set with PATCH `/rest/config/folders/<id>`. Group folder cards by it and add a "set group" action
- `blockIndexing` toggle (Syncthing v2.1.0+): per-folder bool, defaults to true. Off shrinks the Syncthing database but loses block reuse across files. Fits next to the folder type menu
- Taskbar icon (plus ability to launch terminal + app)

**Low Priority:**
- Remote device panel (name, download/upload rates, shared folders, sync status, out of sync files). Needs a significant refactor: the UI is built around folders, so devices need their own view and state

---

### Known Bugs

- `parse_event_time_public` falls back to `SystemTime::now()` for an unparseable timestamp, so a malformed `lastFile.at` shows as "just now"
- While a folder is syncing, `overlay_syncing_states` clones each breadcrumb level's state map every frame. Fine at normal sizes, costly for directories with tens of thousands of entries. Computing per visible row with `item_sync_state` would avoid it
- `toggle_ignore` sets the "Cannot un-ignore: deletion in progress" toast itself instead of returning a result for `handlers/keyboard.rs` to report
- `create_test_app_with_api_rx` passes `SyncthingClient::new` its `base_url` and `api_key` in swapped order. Harmless for current tests

---

### Syncthing Compatibility

- Directories report 0 bytes instead of 128 from v2.1.2, so byte totals in `/rest/db/status`, `/rest/db/file`, need and localchanged drop slightly. Check nothing assumes the old size once the local container is on 2.1
- Folder cards still show `scanning`, `scan-waiting` and `cleaning` folders as synced. Only the status bar labels them

---

### Maintenance

- Test coverage is lopsided: `logic/` is at about 2.4 lines of test per line of code, but `app/`, `services/` and `main.rs` (about 4,400 lines) have 19 tests between them, and most bugs found in Nov 2026 lived there. Test new orchestration through the handler pattern (`create_test_app_with_api_rx` builds a real `App`, feeds it responses and checks the requests it sends), and move decisions out of `main.rs` into `logic/` when touching it, as `folders_needing_refresh_after_cache_load` and `need_fetch_plan` did

- `serde_yaml` is deprecated upstream (0.9.34+deprecated). Pick a maintained replacement for config parsing
- `cargo clippy --all-targets -- -D warnings` fails on old lints in `tests/*.rs` (`useless_vec`, `const_is_empty`, `useless_asref`). CI only lints lib and bin, so it passes there
- Before/after screenshot check of the dependency upgrade (ratatui 0.30, ratatui-image 11) with the demo harness, old binary vs new
