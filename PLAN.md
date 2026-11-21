# Syncthing CLI TUI Manager: Development Plan

## 🎯 Future Work

---

### Feature Enhancements

**High Priority:**
- Syncthing log viewer

**Medium Priority:**
- `blockIndexing` toggle (Syncthing v2.1.0+): per-folder bool, defaults to true. Off shrinks the Syncthing database but loses block reuse across files. Fits next to the folder type menu
- Taskbar icon (plus ability to launch terminal + app)

**Low Priority:**
- Remote device panel (name, download/upload rates, shared folders, sync status, out of sync files). Needs a significant refactor: the UI is built around folders, so devices need their own view and state

---

### Known Bugs

- The out-of-sync walk writes its cache rows with `INSERT OR REPLACE` (`src/cache.rs`, near line 881). These rows can overwrite file states that other code wrote
- A folder paused outside stui (web GUI or REST) goes unnoticed for 10 s or more. The folder list's paused flag refreshes only after stui's own actions

---

### Maintenance

- Test coverage is lopsided: `logic/` is at about 2.4 lines of test per line of code, but `app/`, `services/` and `main.rs` (about 4,400 lines) have 19 tests between them, and most bugs found in Nov 2026 lived there. Test new orchestration through the handler pattern (`create_test_app_with_api_rx` builds a real `App`, feeds it responses and checks the requests it sends), and move decisions out of `main.rs` into `logic/` when touching it, as `folders_needing_refresh_after_cache_load` and `need_fetch_plan` did

- `serde_yaml` is deprecated upstream (0.9.34+deprecated). Pick a maintained replacement for config parsing
- `cargo clippy --all-targets -- -D warnings` fails on old lints in `tests/*.rs` (`useless_vec`, `const_is_empty`, `useless_asref`) and in the tests of `src/model/ui.rs` (`type_complexity`). CI only lints lib and bin, so it passes there
