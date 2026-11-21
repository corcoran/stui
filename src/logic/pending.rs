//! Judging whether Syncthing's replies confirm or contradict a pending local change.

use crate::api::SyncState;
use crate::model::pending::{PendingChange, PendingFolderChange, PendingFolderKind, PendingKind};
use crate::model::syncthing::NeedIndex;
use std::time::Instant;

/// Something Syncthing (or a failed request) told us. `requested_at` is when the request was
/// sent: a reply sent before a change was recorded says nothing about it.
#[derive(Clone, Debug)]
pub enum Observation {
    FileInfo {
        state: SyncState,
        requested_at: Instant,
    },
    Listed {
        present: bool,
        requested_at: Instant,
    },
    LocalChanged {
        deleted: bool,
        requested_at: Instant,
    },
    ItemFinished {
        failed: bool,
        deleted: bool,
    },
    FolderStatus {
        receive_only_items: u64,
        requested_at: Instant,
    },
    FolderPaused,
    ActionFailed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    Confirmed,
    Contradicted,
}

impl Observation {
    fn requested_at(&self) -> Option<Instant> {
        match self {
            Observation::FileInfo { requested_at, .. }
            | Observation::Listed { requested_at, .. }
            | Observation::LocalChanged { requested_at, .. }
            | Observation::FolderStatus { requested_at, .. } => Some(*requested_at),
            Observation::ItemFinished { .. }
            | Observation::FolderPaused
            | Observation::ActionFailed => None,
        }
    }
}

/// Verdicts every change shares: an expired deadline, a failed action, a stale reply
fn common(
    started: Instant,
    deadline: Option<Instant>,
    obs: &Observation,
    now: Instant,
) -> Option<Verdict> {
    if deadline.is_some_and(|d| now >= d) || matches!(obs, Observation::ActionFailed) {
        return Some(Verdict::Contradicted);
    }
    if obs.requested_at().is_some_and(|r| r < started) {
        return Some(Verdict::Keep);
    }
    None
}

pub fn observe(change: &PendingChange, obs: &Observation, now: Instant) -> Verdict {
    if let Some(verdict) = common(change.started, change.deadline, obs, now) {
        return verdict;
    }
    use Observation as O;
    match (&change.kind, obs) {
        (PendingKind::Ignored { .. }, O::FileInfo { state, .. })
            if *state == SyncState::Ignored =>
        {
            Verdict::Confirmed
        }
        (PendingKind::Unignored, O::FileInfo { state, .. }) if *state != SyncState::Ignored => {
            Verdict::Confirmed
        }
        (PendingKind::DeletedLocally, O::LocalChanged { deleted: true, .. }) => Verdict::Confirmed,
        (PendingKind::DeletedLocally, O::FileInfo { state, .. })
            if *state == SyncState::RemoteOnly =>
        {
            Verdict::Confirmed
        }
        (PendingKind::Removed, O::Listed { present: false, .. }) => Verdict::Confirmed,
        (
            PendingKind::Restoring,
            O::ItemFinished {
                failed: false,
                deleted: false,
            },
        ) => Verdict::Confirmed,
        (PendingKind::Restoring, O::FileInfo { state, .. }) if *state == SyncState::Synced => {
            Verdict::Confirmed
        }
        (PendingKind::Restoring, O::ItemFinished { failed: true, .. }) => Verdict::Contradicted,
        (PendingKind::Restoring, O::FolderPaused) => Verdict::Contradicted,
        (PendingKind::Reverting, O::ItemFinished { failed: false, .. }) => Verdict::Confirmed,
        (PendingKind::Reverting, O::FileInfo { state, .. }) if *state == SyncState::Synced => {
            Verdict::Confirmed
        }
        (PendingKind::Reverting, O::Listed { present: false, .. }) => Verdict::Confirmed,
        (PendingKind::Reverting, O::ItemFinished { failed: true, .. }) => Verdict::Contradicted,
        (PendingKind::Reverting, O::FolderPaused) => Verdict::Contradicted,
        _ => Verdict::Keep,
    }
}

pub fn observe_folder(change: &PendingFolderChange, obs: &Observation, now: Instant) -> Verdict {
    if let Some(verdict) = common(change.started, change.deadline, obs, now) {
        return verdict;
    }
    match (&change.kind, obs) {
        (
            PendingFolderKind::Reverting,
            Observation::FolderStatus {
                receive_only_items: 0,
                ..
            },
        ) => Verdict::Confirmed,
        (PendingFolderKind::Reverting, Observation::FolderPaused) => Verdict::Contradicted,
        _ => Verdict::Keep,
    }
}

/// Re-impose pending deletions, restorations and reverts on a freshly fetched `index`
pub fn reapply_to_index<'a>(
    index: &mut NeedIndex,
    changes: impl IntoIterator<Item = (&'a str, &'a PendingKind)>,
) {
    let mut changed = false;
    for (path, kind) in changes {
        match kind {
            PendingKind::DeletedLocally => {
                index
                    .local_changed
                    .insert(path.to_string(), SyncState::RemoteOnly);
                changed = true;
            }
            PendingKind::Restoring | PendingKind::Reverting => {
                changed |= index.local_changed.remove(path).is_some();
            }
            _ => {}
        }
    }
    if changed {
        crate::logic::sync_states::rebuild_descendants(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SyncState;
    use crate::model::pending::{
        PendingChange, PendingFolderChange, PendingFolderKind, PendingKind,
    };
    use crate::model::syncthing::NeedIndex;
    use std::time::{Duration, Instant};

    fn change(kind: PendingKind, started: Instant) -> PendingChange {
        PendingChange {
            kind,
            started,
            deadline: None,
        }
    }

    fn base() -> Instant {
        Instant::now() + Duration::from_secs(1000)
    }

    fn fresh(started: Instant) -> Instant {
        started + Duration::from_millis(1)
    }

    fn judge(kind: PendingKind, obs: Observation) -> Verdict {
        let t0 = base();
        observe(&change(kind, t0), &obs, fresh(t0))
    }

    fn verdict(kind: PendingKind, make: impl Fn(Instant) -> Observation) -> Verdict {
        let t0 = Instant::now();
        observe(&change(kind, t0), &make(fresh(t0)), fresh(t0))
    }

    // ========================================
    // CONFIRMED BY
    // ========================================

    #[test]
    fn test_ignored_confirmed_by_ignored_file_info() {
        for exists in [true, false] {
            assert_eq!(
                verdict(
                    PendingKind::Ignored {
                        exists,
                        deleting: false
                    },
                    |r| Observation::FileInfo {
                        state: SyncState::Ignored,
                        requested_at: r
                    }
                ),
                Verdict::Confirmed
            );
        }
    }

    #[test]
    fn test_unignored_confirmed_by_non_ignored_file_info() {
        for state in [
            SyncState::Synced,
            SyncState::LocalOnly,
            SyncState::OutOfSync,
        ] {
            assert_eq!(
                verdict(PendingKind::Unignored, |r| Observation::FileInfo {
                    state,
                    requested_at: r
                }),
                Verdict::Confirmed
            );
        }
    }

    #[test]
    fn test_deleted_locally_confirmed_by_local_changed_deleted() {
        assert_eq!(
            verdict(PendingKind::DeletedLocally, |r| Observation::LocalChanged {
                deleted: true,
                requested_at: r
            }),
            Verdict::Confirmed
        );
    }

    #[test]
    fn test_deleted_locally_confirmed_by_remote_only_file_info() {
        assert_eq!(
            verdict(PendingKind::DeletedLocally, |r| Observation::FileInfo {
                state: SyncState::RemoteOnly,
                requested_at: r
            }),
            Verdict::Confirmed
        );
    }

    #[test]
    fn test_removed_confirmed_by_listing_without_path() {
        assert_eq!(
            verdict(PendingKind::Removed, |r| Observation::Listed {
                present: false,
                requested_at: r
            }),
            Verdict::Confirmed
        );
    }

    #[test]
    fn test_restoring_confirmed_by_item_finished_ok() {
        assert_eq!(
            judge(
                PendingKind::Restoring,
                Observation::ItemFinished {
                    failed: false,
                    deleted: false
                }
            ),
            Verdict::Confirmed
        );
    }

    #[test]
    fn test_restoring_confirmed_by_synced_file_info() {
        assert_eq!(
            verdict(PendingKind::Restoring, |r| Observation::FileInfo {
                state: SyncState::Synced,
                requested_at: r
            }),
            Verdict::Confirmed
        );
    }

    // ========================================
    // KEPT
    // ========================================

    #[test]
    fn test_fresh_unrelated_reply_keeps_change() {
        assert_eq!(
            verdict(
                PendingKind::Ignored {
                    exists: true,
                    deleting: false
                },
                |r| {
                    Observation::FileInfo {
                        state: SyncState::Synced,
                        requested_at: r,
                    }
                }
            ),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Unignored, |r| Observation::FileInfo {
                state: SyncState::Ignored,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::DeletedLocally, |r| Observation::LocalChanged {
                deleted: false,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Removed, |r| Observation::Listed {
                present: true,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Restoring, |r| Observation::FileInfo {
                state: SyncState::RemoteOnly,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            judge(
                PendingKind::Restoring,
                Observation::ItemFinished {
                    failed: false,
                    deleted: true
                }
            ),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Removed, |r| Observation::FolderStatus {
                receive_only_items: 0,
                requested_at: r
            }),
            Verdict::Keep
        );
    }

    #[test]
    fn test_reply_requested_before_change_keeps_it() {
        let t0 = Instant::now() + Duration::from_secs(10);
        let stale = t0 - Duration::from_millis(1);
        let cases: Vec<(PendingKind, Observation)> = vec![
            (
                PendingKind::Ignored {
                    exists: true,
                    deleting: false,
                },
                Observation::FileInfo {
                    state: SyncState::Ignored,
                    requested_at: stale,
                },
            ),
            (
                PendingKind::Unignored,
                Observation::FileInfo {
                    state: SyncState::Synced,
                    requested_at: stale,
                },
            ),
            (
                PendingKind::DeletedLocally,
                Observation::LocalChanged {
                    deleted: true,
                    requested_at: stale,
                },
            ),
            (
                PendingKind::DeletedLocally,
                Observation::FileInfo {
                    state: SyncState::RemoteOnly,
                    requested_at: stale,
                },
            ),
            (
                PendingKind::Removed,
                Observation::Listed {
                    present: false,
                    requested_at: stale,
                },
            ),
            (
                PendingKind::Restoring,
                Observation::FileInfo {
                    state: SyncState::Synced,
                    requested_at: stale,
                },
            ),
        ];
        for (kind, obs) in cases {
            assert_eq!(
                observe(&change(kind.clone(), t0), &obs, t0),
                Verdict::Keep,
                "{kind:?} {obs:?}"
            );
        }
        let folder = PendingFolderChange {
            kind: PendingFolderKind::Reverting,
            started: t0,
            deadline: None,
        };
        let obs = Observation::FolderStatus {
            receive_only_items: 0,
            requested_at: stale,
        };
        assert_eq!(observe_folder(&folder, &obs, t0), Verdict::Keep);
    }

    #[test]
    fn test_reply_requested_exactly_at_start_counts() {
        let t0 = Instant::now();
        let obs = Observation::Listed {
            present: false,
            requested_at: t0,
        };
        assert_eq!(
            observe(&change(PendingKind::Removed, t0), &obs, t0),
            Verdict::Confirmed
        );
    }

    // ========================================
    // CONTRADICTED
    // ========================================

    #[test]
    fn test_restoring_contradicted_by_failed_item_and_pause() {
        assert_eq!(
            judge(
                PendingKind::Restoring,
                Observation::ItemFinished {
                    failed: true,
                    deleted: false
                }
            ),
            Verdict::Contradicted
        );
        assert_eq!(
            judge(PendingKind::Restoring, Observation::FolderPaused),
            Verdict::Contradicted
        );
    }

    #[test]
    fn test_action_failed_contradicts_every_kind() {
        for kind in [
            PendingKind::DeletedLocally,
            PendingKind::Removed,
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
            PendingKind::Ignored {
                exists: false,
                deleting: false,
            },
            PendingKind::Unignored,
            PendingKind::Restoring,
        ] {
            assert_eq!(
                judge(kind.clone(), Observation::ActionFailed),
                Verdict::Contradicted,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn test_pause_does_not_touch_other_kinds() {
        assert_eq!(
            judge(PendingKind::Removed, Observation::FolderPaused),
            Verdict::Keep
        );
    }

    // ========================================
    // DEADLINES
    // ========================================

    #[test]
    fn test_deadline_passed_contradicts() {
        let t0 = Instant::now();
        let deadline = t0 + Duration::from_secs(5);
        let c = PendingChange {
            kind: PendingKind::Removed,
            started: t0,
            deadline: Some(deadline),
        };
        let unrelated = file_info_at(fresh(t0));
        assert_eq!(observe(&c, &unrelated, deadline), Verdict::Contradicted);
        assert_eq!(
            observe(&c, &unrelated, deadline - Duration::from_millis(1)),
            Verdict::Keep
        );
        // The deadline wins over a confirming observation.
        let confirming = Observation::Listed {
            present: false,
            requested_at: fresh(t0),
        };
        assert_eq!(observe(&c, &confirming, deadline), Verdict::Contradicted);
    }

    fn file_info_at(requested_at: Instant) -> Observation {
        Observation::FileInfo {
            state: SyncState::Synced,
            requested_at,
        }
    }

    #[test]
    fn test_no_deadline_never_expires() {
        let t0 = Instant::now();
        let c = change(PendingKind::Removed, t0);
        let far = t0 + Duration::from_secs(100_000);
        assert_eq!(observe(&c, &file_info_at(far), far), Verdict::Keep);
    }

    #[test]
    fn test_folder_deadline_and_action_failed_contradict() {
        let t0 = Instant::now();
        let f = PendingFolderChange {
            kind: PendingFolderKind::Reverting,
            started: t0,
            deadline: Some(t0 + Duration::from_secs(1)),
        };
        let status = Observation::FolderStatus {
            receive_only_items: 3,
            requested_at: fresh(t0),
        };
        assert_eq!(
            observe_folder(&f, &status, t0 + Duration::from_secs(1)),
            Verdict::Contradicted
        );
        assert_eq!(observe_folder(&f, &status, t0), Verdict::Keep);
        assert_eq!(
            observe_folder(&f, &Observation::ActionFailed, t0),
            Verdict::Contradicted
        );
    }

    // ========================================
    // FOLDER OBSERVATIONS
    // ========================================

    #[test]
    fn test_revert_confirmed_by_fresh_zero_receive_only_items() {
        let t0 = Instant::now();
        let f = PendingFolderChange {
            kind: PendingFolderKind::Reverting,
            started: t0,
            deadline: None,
        };
        let zero = Observation::FolderStatus {
            receive_only_items: 0,
            requested_at: fresh(t0),
        };
        let some = Observation::FolderStatus {
            receive_only_items: 2,
            requested_at: fresh(t0),
        };
        assert_eq!(observe_folder(&f, &zero, fresh(t0)), Verdict::Confirmed);
        assert_eq!(observe_folder(&f, &some, fresh(t0)), Verdict::Keep);
    }

    #[test]
    fn test_revert_contradicted_by_pause() {
        let t0 = Instant::now();
        let f = PendingFolderChange {
            kind: PendingFolderKind::Reverting,
            started: t0,
            deadline: None,
        };
        assert_eq!(
            observe_folder(&f, &Observation::FolderPaused, t0),
            Verdict::Contradicted
        );
        assert_eq!(
            observe_folder(
                &f,
                &Observation::ItemFinished {
                    failed: true,
                    deleted: false
                },
                t0
            ),
            Verdict::Keep
        );
    }

    // ========================================
    // REAPPLY TO INDEX
    // ========================================

    #[test]
    fn test_reapply_marks_deleted_and_forgets_restoring() {
        let mut index = NeedIndex::default();
        index
            .local_changed
            .insert("dir/back".to_string(), SyncState::RemoteOnly);
        crate::logic::sync_states::rebuild_descendants(&mut index);
        assert!(index.descendants.contains_key("dir"));

        let gone = PendingKind::DeletedLocally;
        let back = PendingKind::Restoring;
        let other = PendingKind::Removed;
        reapply_to_index(
            &mut index,
            [("other/gone", &gone), ("dir/back", &back), ("x", &other)],
        );

        assert_eq!(
            index.local_changed.get("other/gone"),
            Some(&SyncState::RemoteOnly)
        );
        assert!(!index.local_changed.contains_key("dir/back"));
        assert!(!index.local_changed.contains_key("x"));
        assert_eq!(index.descendants.get("other"), Some(&SyncState::RemoteOnly));
        assert!(!index.descendants.contains_key("dir"));
    }

    #[test]
    fn test_reapply_without_changes_leaves_index_alone() {
        let mut index = NeedIndex::default();
        index
            .descendants
            .insert("stale".to_string(), SyncState::OutOfSync);
        let other = PendingKind::Unignored;
        reapply_to_index(&mut index, [("a", &other)]);
        assert!(index.descendants.contains_key("stale"));
    }

    // ========================================
    // REVERTING PATH
    // ========================================

    #[test]
    fn test_reverting_confirmed_by_item_finished_ok_for_any_deletion() {
        for deleted in [false, true] {
            assert_eq!(
                judge(
                    PendingKind::Reverting,
                    Observation::ItemFinished {
                        failed: false,
                        deleted
                    }
                ),
                Verdict::Confirmed,
                "deleted={deleted}"
            );
        }
    }

    #[test]
    fn test_reverting_confirmed_by_synced_file_info_and_absent_listing() {
        assert_eq!(
            verdict(PendingKind::Reverting, |r| Observation::FileInfo {
                state: SyncState::Synced,
                requested_at: r
            }),
            Verdict::Confirmed
        );
        assert_eq!(
            verdict(PendingKind::Reverting, |r| Observation::Listed {
                present: false,
                requested_at: r
            }),
            Verdict::Confirmed
        );
    }

    #[test]
    fn test_reverting_contradicted_by_failed_item_and_pause() {
        for deleted in [false, true] {
            assert_eq!(
                judge(
                    PendingKind::Reverting,
                    Observation::ItemFinished {
                        failed: true,
                        deleted
                    }
                ),
                Verdict::Contradicted
            );
        }
        assert_eq!(
            judge(PendingKind::Reverting, Observation::FolderPaused),
            Verdict::Contradicted
        );
    }

    #[test]
    fn test_reverting_kept_by_unrelated_and_stale_replies() {
        assert_eq!(
            verdict(PendingKind::Reverting, |r| Observation::FileInfo {
                state: SyncState::LocalOnly,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Reverting, |r| Observation::Listed {
                present: true,
                requested_at: r
            }),
            Verdict::Keep
        );
        assert_eq!(
            verdict(PendingKind::Reverting, |r| Observation::LocalChanged {
                deleted: false,
                requested_at: r
            }),
            Verdict::Keep
        );
        let t0 = Instant::now() + Duration::from_secs(10);
        let stale = Observation::FileInfo {
            state: SyncState::Synced,
            requested_at: t0 - Duration::from_millis(1),
        };
        assert_eq!(
            observe(&change(PendingKind::Reverting, t0), &stale, t0),
            Verdict::Keep
        );
    }

    #[test]
    fn test_reverting_expires_and_fails_like_other_kinds() {
        let t0 = Instant::now();
        let mut c = change(PendingKind::Reverting, t0);
        c.deadline = Some(t0 + Duration::from_secs(1));
        let obs = Observation::FileInfo {
            state: SyncState::LocalOnly,
            requested_at: t0,
        };
        assert_eq!(
            observe(&c, &obs, t0 + Duration::from_secs(2)),
            Verdict::Contradicted
        );
        assert_eq!(
            judge(PendingKind::Reverting, Observation::ActionFailed),
            Verdict::Contradicted
        );
    }

    #[test]
    fn test_reapply_forgets_reverting_local_change() {
        let mut index = NeedIndex::default();
        index
            .local_changed
            .insert("dir/new".to_string(), SyncState::LocalOnly);
        crate::logic::sync_states::rebuild_descendants(&mut index);
        assert!(index.descendants.contains_key("dir"));

        let reverting = PendingKind::Reverting;
        reapply_to_index(&mut index, [("dir/new", &reverting)]);

        assert!(!index.local_changed.contains_key("dir/new"));
        assert!(!index.descendants.contains_key("dir"));
    }
}
