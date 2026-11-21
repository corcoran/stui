//! How an item is shown: one function turns everything known about an item into its icon
//! and status-bar state, so every place that draws or tests an item agrees.

use crate::api::{BrowseItem, SyncState};
use crate::logic::sync_states::{breadcrumb_item_path, is_locally_deleted, item_sync_state};
use crate::model::BreadcrumbLevel;
use crate::model::pending::{PendingChanges, PendingKind};
use crate::model::syncthing::{NeedIndex, SyncthingModel};
use std::collections::HashSet;

/// How a file or directory is shown: its icon in the list and its state in the status bar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemDisplay {
    State(SyncState),
    /// Ignored; `exists` picks the on-disk or deleted icon
    Ignored {
        exists: bool,
    },
    /// Deleted on this device in a Receive Only folder
    LocallyDeleted,
}

/// Everything `item_display` reads about one item of a breadcrumb level
pub struct ItemInput<'a> {
    /// Stored state; `None` while it hasn't loaded
    pub base: Option<SyncState>,
    pub item: &'a BrowseItem,
    /// Path of the level the item is listed in
    pub prefix: Option<&'a str>,
    /// Paths Syncthing is downloading in the item's folder
    pub syncing: Option<&'a HashSet<String>>,
    pub index: Option<&'a NeedIndex>,
    /// Whether an ignored item exists on disk
    pub ignored_exists: Option<bool>,
    /// The item's pending local change
    pub pending: Option<&'a PendingKind>,
}

/// Display for an item: a pending change beats everything, then a locally deleted item, then
/// in-flight transfers over the stored state. `None` means the state hasn't loaded, or the item
/// is pending removal or an unignore Syncthing hasn't confirmed.
pub fn item_display(input: &ItemInput) -> Option<ItemDisplay> {
    match input.pending {
        Some(PendingKind::DeletedLocally) => return Some(ItemDisplay::LocallyDeleted),
        Some(PendingKind::Ignored { exists, .. }) => {
            return Some(ItemDisplay::Ignored { exists: *exists });
        }
        Some(PendingKind::Restoring | PendingKind::Reverting) => {
            return Some(ItemDisplay::State(SyncState::Syncing));
        }
        Some(PendingKind::Removed) => return None,
        Some(PendingKind::Unignored) if input.base == Some(SyncState::Ignored) => return None,
        Some(PendingKind::Unignored) | None => {}
    }
    let path = breadcrumb_item_path(input.prefix, &input.item.name);
    if is_locally_deleted(input.index, input.syncing, &path) {
        return Some(ItemDisplay::LocallyDeleted);
    }
    let state = item_sync_state(input.base, input.prefix, input.item, input.syncing)?;
    Some(match state {
        SyncState::Ignored => ItemDisplay::Ignored {
            exists: input.ignored_exists.unwrap_or(false),
        },
        other => ItemDisplay::State(other),
    })
}

/// `item_display` for an item listed in `level`
pub fn level_item_display(
    syncthing: &SyncthingModel,
    level: &BreadcrumbLevel,
    item: &BrowseItem,
    pending: &PendingChanges,
) -> Option<ItemDisplay> {
    let path = breadcrumb_item_path(level.prefix.as_deref(), &item.name);
    item_display(&ItemInput {
        base: level.file_sync_states.get(&item.name).copied(),
        item,
        prefix: level.prefix.as_deref(),
        syncing: syncthing.syncing_files.get(&level.folder_id),
        index: syncthing.need_index.get(&level.folder_id),
        ignored_exists: level.ignored_exists.get(&item.name).copied(),
        pending: pending
            .get(&level.folder_id, &path)
            .map(|c| &c.kind)
            .or_else(|| {
                let is_dir = item.item_type == "FILE_INFO_TYPE_DIRECTORY";
                (is_dir && pending.transfer_below(&level.folder_id, &path))
                    .then_some(&PendingKind::Reverting)
            }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SyncState;
    use crate::model::BreadcrumbLevel;
    use crate::model::pending::{PendingChanges, PendingKind};
    use crate::model::syncthing::{NeedIndex, SyncthingModel};
    use std::collections::{HashMap, HashSet};

    fn browse_item(name: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: String::new(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        }
    }

    fn input<'a>(
        base: Option<SyncState>,
        item: &'a BrowseItem,
        ignored_exists: Option<bool>,
        index: Option<&'a NeedIndex>,
    ) -> ItemInput<'a> {
        ItemInput {
            base,
            item,
            prefix: None,
            syncing: None,
            index,
            ignored_exists,
            pending: None,
        }
    }

    // ========================================
    // ITEM DISPLAY
    // ========================================

    #[test]
    fn test_item_display_precedence() {
        use SyncState::*;
        let item = browse_item("a.txt");
        let mut index = NeedIndex::default();
        index.local_changed.insert("a.txt".to_string(), RemoteOnly);

        assert_eq!(item_display(&input(None, &item, None, None)), None);
        assert_eq!(
            item_display(&input(Some(Synced), &item, None, None)),
            Some(ItemDisplay::State(Synced))
        );
        assert_eq!(
            item_display(&input(Some(Ignored), &item, Some(true), None)),
            Some(ItemDisplay::Ignored { exists: true })
        );
        assert_eq!(
            item_display(&input(Some(Ignored), &item, None, None)),
            Some(ItemDisplay::Ignored { exists: false })
        );
        assert_eq!(
            item_display(&input(Some(RemoteOnly), &item, None, Some(&index))),
            Some(ItemDisplay::LocallyDeleted)
        );
        assert_eq!(
            item_display(&input(None, &item, None, Some(&index))),
            Some(ItemDisplay::LocallyDeleted)
        );
    }

    #[test]
    fn test_item_display_overlays_syncing_before_locally_deleted() {
        let item = browse_item("a.txt");
        let mut index = NeedIndex::default();
        index
            .local_changed
            .insert("a.txt".to_string(), SyncState::RemoteOnly);
        let syncing: HashSet<String> = ["a.txt".to_string()].into();
        let mut inp = input(Some(SyncState::RemoteOnly), &item, None, Some(&index));
        inp.syncing = Some(&syncing);
        assert_eq!(
            item_display(&inp),
            Some(ItemDisplay::State(SyncState::Syncing))
        );
    }

    #[test]
    fn test_display_maps_each_pending_kind() {
        use SyncState::*;
        let item = browse_item("a.txt");
        let shown = |base: Option<SyncState>, kind: PendingKind| {
            let mut inp = input(base, &item, Some(true), None);
            inp.pending = Some(&kind);
            item_display(&inp)
        };

        assert_eq!(
            shown(Some(Synced), PendingKind::DeletedLocally),
            Some(ItemDisplay::LocallyDeleted)
        );
        assert_eq!(
            shown(
                Some(Synced),
                PendingKind::Ignored {
                    exists: false,
                    deleting: false
                }
            ),
            Some(ItemDisplay::Ignored { exists: false })
        );
        assert_eq!(
            shown(
                None,
                PendingKind::Ignored {
                    exists: true,
                    deleting: false
                }
            ),
            Some(ItemDisplay::Ignored { exists: true })
        );
        assert_eq!(
            shown(Some(RemoteOnly), PendingKind::Restoring),
            Some(ItemDisplay::State(Syncing))
        );
        assert_eq!(
            shown(Some(LocalOnly), PendingKind::Reverting),
            Some(ItemDisplay::State(Syncing))
        );
        assert_eq!(shown(Some(Ignored), PendingKind::Unignored), None);
        assert_eq!(
            shown(Some(OutOfSync), PendingKind::Unignored),
            Some(ItemDisplay::State(OutOfSync))
        );
        assert_eq!(shown(Some(Synced), PendingKind::Removed), None);
    }

    #[test]
    fn test_level_item_display_reads_pending_change_by_folder_path() {
        let model = SyncthingModel::new();
        let level = BreadcrumbLevel {
            folder_id: "f".to_string(),
            folder_label: "f".to_string(),
            folder_path: "/data/f".to_string(),
            prefix: Some("sub/".to_string()),
            items: vec![],
            filtered_items: None,
            selected_index: None,
            file_sync_states: HashMap::from([
                ("a.txt".to_string(), SyncState::Synced),
                ("b.txt".to_string(), SyncState::Synced),
            ]),
            direct_states: HashMap::new(),
            ignored_exists: HashMap::new(),
            translated_base_path: "/data/f".to_string(),
        };
        let mut pending = PendingChanges::default();
        pending.record(
            "f",
            "sub/a.txt",
            PendingKind::Ignored {
                exists: true,
                deleting: false,
            },
            std::time::Instant::now(),
        );

        assert_eq!(
            level_item_display(&model, &level, &browse_item("a.txt"), &pending),
            Some(ItemDisplay::Ignored { exists: true })
        );
        pending.record(
            "g",
            "sub/b.txt",
            PendingKind::Removed,
            std::time::Instant::now(),
        );
        assert_eq!(
            level_item_display(&model, &level, &browse_item("b.txt"), &pending),
            Some(ItemDisplay::State(SyncState::Synced))
        );
    }

    #[test]
    fn test_level_item_display_matches_list_and_status_bar_inputs() {
        let mut model = SyncthingModel::new();
        let level = BreadcrumbLevel {
            folder_id: "f".to_string(),
            folder_label: "f".to_string(),
            folder_path: "/data/f".to_string(),
            prefix: Some("sub/".to_string()),
            items: vec![],
            filtered_items: None,
            selected_index: None,
            file_sync_states: HashMap::from([
                ("downloading.txt".to_string(), SyncState::RemoteOnly),
                ("gone.txt".to_string(), SyncState::RemoteOnly),
                ("skipped.txt".to_string(), SyncState::Ignored),
            ]),
            direct_states: HashMap::new(),
            ignored_exists: HashMap::from([("skipped.txt".to_string(), true)]),
            translated_base_path: "/data/f".to_string(),
        };
        model.syncing_files.insert(
            "f".to_string(),
            HashSet::from(["sub/downloading.txt".to_string()]),
        );
        let mut index = NeedIndex::default();
        index
            .local_changed
            .insert("sub/gone.txt".to_string(), SyncState::RemoteOnly);
        model.need_index.insert("f".to_string(), index);

        assert_eq!(
            level_item_display(
                &model,
                &level,
                &browse_item("downloading.txt"),
                &PendingChanges::default()
            ),
            Some(ItemDisplay::State(SyncState::Syncing))
        );
        assert_eq!(
            level_item_display(
                &model,
                &level,
                &browse_item("gone.txt"),
                &PendingChanges::default()
            ),
            Some(ItemDisplay::LocallyDeleted)
        );
        assert_eq!(
            level_item_display(
                &model,
                &level,
                &browse_item("skipped.txt"),
                &PendingChanges::default()
            ),
            Some(ItemDisplay::Ignored { exists: true })
        );
        assert_eq!(
            level_item_display(
                &model,
                &level,
                &browse_item("unloaded.txt"),
                &PendingChanges::default()
            ),
            None
        );
    }
}
