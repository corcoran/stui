//! Local actions the user took that Syncthing hasn't confirmed yet.

use crate::logic::pending::PATH_DEADLINE;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingKind {
    DeletedLocally,
    Removed,
    Ignored { exists: bool },
    Unignored,
    Restoring,
}

#[derive(Clone, Debug)]
pub struct PendingChange {
    pub kind: PendingKind,
    pub started: Instant,
    /// None until the rescan that follows the action returns; never set for `Restoring`
    pub deadline: Option<Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingFolderKind {
    Reverting,
}

#[derive(Clone, Debug)]
pub struct PendingFolderChange {
    pub kind: PendingFolderKind,
    pub started: Instant,
    pub deadline: Option<Instant>,
}

/// Pending changes by folder id and path (no trailing slash), plus whole-folder changes
#[derive(Clone, Debug, Default)]
pub struct PendingChanges {
    paths: HashMap<String, HashMap<String, PendingChange>>,
    folders: HashMap<String, PendingFolderChange>,
}

impl PendingChanges {
    /// Record `kind` for `path`, replacing any earlier change (and its deadline) for it
    pub fn record(&mut self, folder: &str, path: &str, kind: PendingKind, now: Instant) {
        self.paths.entry(folder.to_string()).or_default().insert(
            path.to_string(),
            PendingChange {
                kind,
                started: now,
                deadline: None,
            },
        );
    }

    pub fn get(&self, folder: &str, path: &str) -> Option<&PendingChange> {
        self.paths.get(folder)?.get(path)
    }

    pub fn remove(&mut self, folder: &str, path: &str) -> Option<PendingChange> {
        let in_folder = self.paths.get_mut(folder)?;
        let removed = in_folder.remove(path);
        if in_folder.is_empty() {
            self.paths.remove(folder);
        }
        removed
    }

    pub fn in_folder(&self, folder: &str) -> impl Iterator<Item = (&str, &PendingChange)> {
        self.paths
            .get(folder)
            .into_iter()
            .flat_map(|m| m.iter().map(|(p, c)| (p.as_str(), c)))
    }

    pub fn restoring_in(&self, folder: &str) -> impl Iterator<Item = &str> {
        self.in_folder(folder)
            .filter(|(_, c)| c.kind == PendingKind::Restoring)
            .map(|(p, _)| p)
    }

    /// Whether `path` or an ancestor is being ignored and deleted
    pub fn blocks_unignore(&self, folder: &str, path: &str) -> bool {
        let Some(in_folder) = self.paths.get(folder) else {
            return false;
        };
        let deleting = |p: &str| {
            in_folder
                .get(p)
                .is_some_and(|c| c.kind == PendingKind::Ignored { exists: false })
        };
        if deleting(path) {
            return true;
        }
        path.match_indices('/').any(|(i, _)| deleting(&path[..i]))
    }

    pub fn ignore_delete_count(&self) -> usize {
        self.paths
            .values()
            .flat_map(|m| m.values())
            .filter(|c| c.kind == PendingKind::Ignored { exists: false })
            .count()
    }

    pub fn record_folder(&mut self, folder: &str, kind: PendingFolderKind, now: Instant) {
        self.folders.insert(
            folder.to_string(),
            PendingFolderChange {
                kind,
                started: now,
                deadline: None,
            },
        );
    }

    pub fn folder(&self, folder: &str) -> Option<&PendingFolderChange> {
        self.folders.get(folder)
    }

    pub fn folder_mut(&mut self, folder: &str) -> Option<&mut PendingFolderChange> {
        self.folders.get_mut(folder)
    }

    pub fn remove_folder(&mut self, folder: &str) -> Option<PendingFolderChange> {
        self.folders.remove(folder)
    }

    /// Give each deadline-less, non-`Restoring` change of `folder` that started no later than
    /// `rescan_returned` a deadline of `PATH_DEADLINE` after it.
    pub fn start_deadlines(&mut self, folder: &str, rescan_returned: Instant) {
        let Some(in_folder) = self.paths.get_mut(folder) else {
            return;
        };
        for change in in_folder.values_mut() {
            if change.kind != PendingKind::Restoring
                && change.deadline.is_none()
                && change.started <= rescan_returned
            {
                change.deadline = Some(rescan_returned + PATH_DEADLINE);
            }
        }
    }

    /// Remove and return the path changes (folder, path, kind) and folder changes whose
    /// deadline is at or before `now`
    pub fn drain_expired(
        &mut self,
        now: Instant,
    ) -> (Vec<(String, String, PendingKind)>, Vec<String>) {
        let due = |d: Option<Instant>| d.is_some_and(|d| d <= now);
        let mut paths = Vec::new();
        for (folder, in_folder) in self.paths.iter_mut() {
            in_folder.retain(|path, change| {
                if due(change.deadline) {
                    paths.push((folder.clone(), path.clone(), change.kind.clone()));
                    false
                } else {
                    true
                }
            });
        }
        self.paths.retain(|_, m| !m.is_empty());
        let folders: Vec<String> = self
            .folders
            .iter()
            .filter(|(_, c)| due(c.deadline))
            .map(|(f, _)| f.clone())
            .collect();
        for f in &folders {
            self.folders.remove(f);
        }
        (paths, folders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::pending::PATH_DEADLINE;
    use std::time::Duration;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    // ========================================
    // RECORDING
    // ========================================

    #[test]
    fn test_record_replaces_existing_change() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "a", PendingKind::Ignored { exists: true }, t0);
        p.start_deadlines("f", ms(t0, 5));
        assert!(p.get("f", "a").unwrap().deadline.is_some());

        p.record("f", "a", PendingKind::Unignored, ms(t0, 10));
        let c = p.get("f", "a").unwrap();
        assert_eq!(c.kind, PendingKind::Unignored);
        assert_eq!(c.started, ms(t0, 10));
        assert_eq!(c.deadline, None);
        assert_eq!(p.in_folder("f").count(), 1);
    }

    #[test]
    fn test_get_and_remove() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        assert!(p.get("f", "a").is_none());
        p.record("f", "a", PendingKind::Removed, t0);
        assert!(p.get("g", "a").is_none());
        assert_eq!(p.remove("f", "a").unwrap().kind, PendingKind::Removed);
        assert!(p.remove("f", "a").is_none());
        assert_eq!(p.in_folder("f").count(), 0);
    }

    #[test]
    fn test_in_folder_and_restoring_in() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "a", PendingKind::Restoring, t0);
        p.record("f", "b", PendingKind::Removed, t0);
        p.record("g", "c", PendingKind::Restoring, t0);
        let mut all: Vec<&str> = p.in_folder("f").map(|(path, _)| path).collect();
        all.sort();
        assert_eq!(all, vec!["a", "b"]);
        let restoring: Vec<&str> = p.restoring_in("f").collect();
        assert_eq!(restoring, vec!["a"]);
        assert_eq!(p.restoring_in("nope").count(), 0);
    }

    // ========================================
    // IGNORE AND DELETE
    // ========================================

    #[test]
    fn test_blocks_unignore_for_ancestor_ignore_delete() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "dir", PendingKind::Ignored { exists: false }, t0);
        assert!(p.blocks_unignore("f", "dir"));
        assert!(p.blocks_unignore("f", "dir/sub/file"));
        assert!(!p.blocks_unignore("f", "dir2/file"));
        assert!(!p.blocks_unignore("f", "other"));
        assert!(!p.blocks_unignore("g", "dir"));
    }

    #[test]
    fn test_blocks_unignore_ignores_other_kinds() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "dir", PendingKind::Ignored { exists: true }, t0);
        p.record("f", "x", PendingKind::Removed, t0);
        assert!(!p.blocks_unignore("f", "dir/file"));
        assert!(!p.blocks_unignore("f", "x"));
    }

    #[test]
    fn test_ignore_delete_count_spans_folders() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "a", PendingKind::Ignored { exists: false }, t0);
        p.record("g", "b", PendingKind::Ignored { exists: false }, t0);
        p.record("g", "c", PendingKind::Ignored { exists: true }, t0);
        p.record("g", "d", PendingKind::Removed, t0);
        assert_eq!(p.ignore_delete_count(), 2);
    }

    // ========================================
    // FOLDER CHANGES
    // ========================================

    #[test]
    fn test_folder_change_lifecycle() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        assert!(p.folder("f").is_none());
        p.record_folder("f", PendingFolderKind::Reverting, t0);
        assert_eq!(p.folder("f").unwrap().kind, PendingFolderKind::Reverting);
        assert_eq!(p.folder("f").unwrap().deadline, None);
        p.folder_mut("f").unwrap().deadline = Some(ms(t0, 1));
        assert_eq!(p.folder("f").unwrap().deadline, Some(ms(t0, 1)));
        assert!(p.remove_folder("f").is_some());
        assert!(p.folder("f").is_none());
        assert!(p.remove_folder("f").is_none());
    }

    // ========================================
    // DEADLINES
    // ========================================

    #[test]
    fn test_start_deadlines_skips_restoring_and_later_changes() {
        let t0 = Instant::now();
        let returned = ms(t0, 100);
        let mut p = PendingChanges::default();
        p.record("f", "early", PendingKind::Removed, t0);
        p.record("f", "exact", PendingKind::Removed, returned);
        p.record("f", "late", PendingKind::Removed, ms(t0, 101));
        p.record("f", "restoring", PendingKind::Restoring, t0);
        p.record("g", "other-folder", PendingKind::Removed, t0);
        p.start_deadlines("f", returned);

        assert_eq!(
            p.get("f", "early").unwrap().deadline,
            Some(returned + PATH_DEADLINE)
        );
        assert_eq!(
            p.get("f", "exact").unwrap().deadline,
            Some(returned + PATH_DEADLINE)
        );
        assert_eq!(p.get("f", "late").unwrap().deadline, None);
        assert_eq!(p.get("f", "restoring").unwrap().deadline, None);
        assert_eq!(p.get("g", "other-folder").unwrap().deadline, None);
    }

    #[test]
    fn test_start_deadlines_keeps_existing_deadline() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "a", PendingKind::Removed, t0);
        p.start_deadlines("f", ms(t0, 10));
        p.start_deadlines("f", ms(t0, 50));
        assert_eq!(
            p.get("f", "a").unwrap().deadline,
            Some(ms(t0, 10) + PATH_DEADLINE)
        );
    }

    #[test]
    fn test_drain_expired_returns_and_removes() {
        let t0 = Instant::now();
        let mut p = PendingChanges::default();
        p.record("f", "due", PendingKind::Removed, t0);
        p.record("f", "later", PendingKind::Unignored, t0);
        p.record("f", "none", PendingKind::Restoring, t0);
        p.record_folder("f", PendingFolderKind::Reverting, t0);
        p.record_folder("g", PendingFolderKind::Reverting, t0);
        p.paths
            .get_mut("f")
            .unwrap()
            .get_mut("due")
            .unwrap()
            .deadline = Some(ms(t0, 10));
        p.paths
            .get_mut("f")
            .unwrap()
            .get_mut("later")
            .unwrap()
            .deadline = Some(ms(t0, 11));
        p.folder_mut("f").unwrap().deadline = Some(ms(t0, 10));

        let (paths, folders) = p.drain_expired(ms(t0, 10));
        assert_eq!(
            paths,
            vec![("f".to_string(), "due".to_string(), PendingKind::Removed)]
        );
        assert_eq!(folders, vec!["f".to_string()]);
        assert!(p.get("f", "due").is_none());
        assert!(p.get("f", "later").is_some());
        assert!(p.get("f", "none").is_some());
        assert!(p.folder("f").is_none());
        assert!(p.folder("g").is_some());
    }
}
