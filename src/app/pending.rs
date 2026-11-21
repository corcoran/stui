//! Recording pending local changes and judging Syncthing replies against them

use crate::logic::pending::{Observation, Verdict, observe, observe_folder, reapply_to_index};
use crate::model::pending::PendingKind;
use crate::services::api::{ApiRequest, Priority};
use crate::{App, log_debug};
use std::time::Instant;

impl App {
    /// Record `kind` for `path` and impose it on the folder's need index
    #[allow(dead_code)]
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

    /// Drop changes whose deadline is at or before `now` and ask Syncthing for the real state
    pub(crate) fn expire_pending(&mut self, now: Instant) {
        let (paths, folders) = self.model.pending.drain_expired(now);
        for (folder, path, kind) in paths {
            log_debug(&format!(
                "DEBUG [Pending]: expired folder={} path={} kind={:?}",
                folder, path, kind
            ));
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
