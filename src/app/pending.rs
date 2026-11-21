//! Recording pending local changes and judging Syncthing replies against them

use crate::api::LocalChangedFile;
use crate::logic::pending::{Observation, Verdict, observe, observe_folder, reapply_to_index};
use crate::logic::sync_states::{local_changed_states, rebuild_descendants};
use crate::model::pending::{PendingFolderKind, PendingKind, REVERT_DEADLINE};
use crate::services::api::{ApiRequest, Priority};
use crate::{App, log_debug};
use std::time::Instant;

impl App {
    /// Record `kind` for `path` and impose it on the folder's need index
    pub(crate) fn record_pending(&mut self, folder: &str, path: &str, kind: PendingKind) {
        log_debug(&format!(
            "DEBUG [Pending]: record folder={} path={} kind={:?}",
            folder, path, kind
        ));
        let index = if kind == PendingKind::DeletedLocally {
            Some(
                self.model
                    .syncthing
                    .need_index
                    .entry(folder.to_string())
                    .or_default(),
            )
        } else {
            self.model.syncthing.need_index.get_mut(folder)
        };
        if let Some(index) = index {
            reapply_to_index(index, [(path, &kind)]);
        }
        self.model
            .pending
            .record(folder, path, kind, Instant::now());
    }

    /// Revert `folder`'s local changes. The changes and the folder revert are recorded before
    /// the request so icons change at once; a failed request drops them again.
    pub(crate) async fn revert_folder_changes(
        &mut self,
        folder: &str,
        changes: &[LocalChangedFile],
    ) -> anyhow::Result<()> {
        for change in changes {
            let kind = if change.deleted {
                PendingKind::Restoring
            } else {
                PendingKind::Reverting
            };
            self.record_pending(folder, &change.name, kind);
        }
        self.model
            .pending
            .record_folder(folder, PendingFolderKind::Reverting, Instant::now());
        self.refresh_derived_states(folder);

        if let Err(error) = self.client.revert_folder(folder).await {
            for change in changes {
                self.observe_path(folder, &change.name, Observation::ActionFailed);
            }
            self.observe_folder_change(folder, Observation::ActionFailed);
            if let Some(index) = self.model.syncthing.need_index.get_mut(folder) {
                index.local_changed.extend(local_changed_states(changes));
                rebuild_descendants(index);
            }
            self.refresh_derived_states(folder);
            return Err(error);
        }
        let deadline = Instant::now() + REVERT_DEADLINE;
        if let Some(change) = self.model.pending.folder_mut(folder) {
            change.deadline = Some(deadline);
        }
        for change in changes {
            let reverting = self
                .model
                .pending
                .get(folder, &change.name)
                .is_some_and(|c| c.kind == PendingKind::Reverting);
            if reverting {
                self.model
                    .pending
                    .set_deadline(folder, &change.name, deadline);
            }
        }
        Ok(())
    }

    /// Judge `obs` against `path`'s pending change, removing it once confirmed or contradicted.
    /// `Keep` when `path` has no pending change.
    pub(crate) fn observe_path(&mut self, folder: &str, path: &str, obs: Observation) -> Verdict {
        let Some(change) = self.model.pending.get(folder, path) else {
            return Verdict::Keep;
        };
        let verdict = observe(change, &obs, Instant::now());
        if verdict != Verdict::Keep {
            log_debug(&format!(
                "DEBUG [Pending]: {:?} folder={} path={} kind={:?} by {:?}",
                verdict, folder, path, change.kind, obs
            ));
            self.model.pending.remove(folder, path);
        }
        verdict
    }

    /// `observe_path` for the folder's whole-folder change
    pub(crate) fn observe_folder_change(&mut self, folder: &str, obs: Observation) -> Verdict {
        let Some(change) = self.model.pending.folder(folder) else {
            return Verdict::Keep;
        };
        let verdict = observe_folder(change, &obs, Instant::now());
        if verdict != Verdict::Keep {
            log_debug(&format!(
                "DEBUG [Pending]: {:?} folder={} kind={:?} by {:?}",
                verdict, folder, change.kind, obs
            ));
            self.model.pending.remove_folder(folder);
        }
        verdict
    }

    /// Whether an open breadcrumb level of `folder` lists `path`
    fn level_open_for(&self, folder: &str, path: &str) -> bool {
        let prefix = path.rfind('/').map(|i| &path[..=i]);
        self.model
            .navigation
            .breadcrumb_trail
            .iter()
            .any(|l| l.folder_id == folder && l.prefix.as_deref() == prefix)
    }

    /// Re-browse the open level of `folder` that lists `path`, if any
    fn relist_level_of(&mut self, folder: &str, path: &str) {
        if !self.level_open_for(folder, path) {
            return;
        }
        let prefix = path.rfind('/').map(|i| path[..=i].to_string());
        self.model.performance.loading_browse.insert(format!(
            "{}:{}",
            folder,
            prefix.as_deref().unwrap_or("")
        ));
        let _ = self.api_tx.send(ApiRequest::BrowseFolder {
            folder_id: folder.to_string(),
            prefix,
            priority: Priority::High,
            requested_at: Instant::now(),
        });
    }

    /// Drop changes whose deadline is at or before `now` and ask Syncthing for the real state
    /// of those an open level lists
    pub(crate) fn expire_pending(&mut self, now: Instant) {
        let (paths, folders) = self.model.pending.drain_expired(now);
        for (folder, path, kind) in paths {
            log_debug(&format!(
                "DEBUG [Pending]: expired folder={} path={} kind={:?}",
                folder, path, kind
            ));
            if kind == PendingKind::Removed {
                self.relist_level_of(&folder, &path);
            }
            if !self.level_open_for(&folder, &path) {
                continue;
            }
            self.model
                .performance
                .loading_sync_states
                .insert(format!("{}:{}", folder, path));
            let _ = self.api_tx.send(ApiRequest::GetFileInfo {
                folder_id: folder,
                file_path: path,
                priority: Priority::High,
                requested_at: Instant::now(),
            });
        }
        for folder in &folders {
            log_debug(&format!(
                "DEBUG [Pending]: expired folder change folder={}",
                folder
            ));
        }
        if !folders.is_empty() {
            self.refresh_folder_statuses_nonblocking();
        }
    }
}
