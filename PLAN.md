# Syncthing CLI TUI Manager: Development Plan

## 🎯 Future Work

---

### Feature Enhancements

**High Priority:**
- Pending local changes: one model for "the user did X, show the result until Syncthing confirms it". Today each action has its own patch (`restoring` set, `record_local_deletion`, `ItemFinished` marking Synced, ignore-toggle state kept, pause flag set locally, group edits applied locally), and Syncthing replies built before an action still overwrite its result. Record pending changes per path, draw them over Syncthing's data, clear them on confirmation, failure or timeout, and drop replies requested before the action for that path. Reverting a whole Receive Only folder (`R`) shows heavy icon churn that this would remove
- Syncthing log viewer

**Medium Priority:**
- `blockIndexing` toggle (Syncthing v2.1.0+): per-folder bool, defaults to true. Off shrinks the Syncthing database but loses block reuse across files. Fits next to the folder type menu
- Taskbar icon (plus ability to launch terminal + app)

**Low Priority:**
- Remote device panel (name, download/upload rates, shared folders, sync status, out of sync files). Needs a significant refactor: the UI is built around folders, so devices need their own view and state

---

### Known Bugs

- While a folder is syncing, `overlay_syncing_states` clones each breadcrumb level's state map every frame. Fine at normal sizes, costly for directories with tens of thousands of entries. Computing per visible row with `item_sync_state` would avoid it

---

### Syncthing Compatibility

- Directories report 0 bytes instead of 128 from v2.1.2, so byte totals in `/rest/db/status`, `/rest/db/file`, need and localchanged drop slightly. Check nothing assumes the old size once the local container is on 2.1

---

### Maintenance

- Test coverage is lopsided: `logic/` is at about 2.4 lines of test per line of code, but `app/`, `services/` and `main.rs` (about 4,400 lines) have 19 tests between them, and most bugs found in Nov 2026 lived there. Test new orchestration through the handler pattern (`create_test_app_with_api_rx` builds a real `App`, feeds it responses and checks the requests it sends), and move decisions out of `main.rs` into `logic/` when touching it, as `folders_needing_refresh_after_cache_load` and `need_fetch_plan` did

- `serde_yaml` is deprecated upstream (0.9.34+deprecated). Pick a maintained replacement for config parsing
- `cargo clippy --all-targets -- -D warnings` fails on old lints in `tests/*.rs` (`useless_vec`, `const_is_empty`, `useless_asref`). CI only lints lib and bin, so it passes there
