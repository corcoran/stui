# Syncthing CLI TUI Manager: Development Plan

## 🎯 Future Work

---

### Feature Enhancements

**High Priority:**
- Syncthing log viewer

**Medium Priority:**
- Taskbar icon (plus ability to launch terminal + app)

**Low Priority:**
- Remote device panel (name, download/upload rates, shared folders, sync status, out of sync files). Needs a significant refactor: the UI is built around folders, so devices need their own view and state

---

### Maintenance

- Test coverage is lopsided: `logic/` is at about 2.4 lines of test per line of code, but `app/`, `services/` and `main.rs` (about 4,400 lines) have 19 tests between them, and most bugs found in Nov 2026 lived there. Test new orchestration through the handler pattern (`create_test_app_with_api_rx` builds a real `App`, feeds it responses and checks the requests it sends), and move decisions out of `main.rs` into `logic/` when touching it, as `folders_needing_refresh_after_cache_load` and `need_fetch_plan` did

