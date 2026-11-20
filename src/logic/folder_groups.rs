//! Folder groups (Syncthing 2.1+)
//!
//! Syncthing stores a group as a `group` label on each folder; there is no list of groups.
//! Older Syncthing has no `group` field at all.

use crate::api::Folder;

/// Header shown above folders that have no group
pub const UNGROUPED: &str = "Ungrouped";

/// Whether the connected Syncthing has folder groups: true when any folder carries the
/// `group` field, which Syncthing older than 2.1 never sends.
pub fn groups_supported(folders: &[Folder]) -> bool {
    folders.iter().any(|f| f.group.is_some())
}

fn group_of(folder: &Folder) -> &str {
    folder.group.as_deref().unwrap_or("")
}

/// Folders sorted by group A–Z (case-insensitive, exact spelling breaking ties), ungrouped
/// last, keeping Syncthing's config order within a group. Unchanged when groups are
/// unsupported or no folder has one.
pub fn order_by_group(mut folders: Vec<Folder>) -> Vec<Folder> {
    if !folders.iter().any(|f| !group_of(f).is_empty()) {
        return folders;
    }
    folders.sort_by(|a, b| {
        let key = |f: &Folder| {
            let group = group_of(f);
            (group.is_empty(), group.to_lowercase(), group.to_string())
        };
        key(a).cmp(&key(b))
    });
    folders
}

/// One row of the folder list: a group header, or the folder at that index
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderRow {
    Header(String),
    Folder(usize),
}

/// Rows for an already ordered folder list. Without group support there are no headers;
/// with it, every group gets a header, including a lone "Ungrouped" when no folder has
/// a group yet.
pub fn folder_rows(folders: &[Folder]) -> Vec<FolderRow> {
    if !groups_supported(folders) {
        return (0..folders.len()).map(FolderRow::Folder).collect();
    }
    let mut rows = Vec::with_capacity(folders.len() + 2);
    let mut current: Option<&str> = None;
    for (idx, folder) in folders.iter().enumerate() {
        let group = group_of(folder);
        if current != Some(group) {
            let name = if group.is_empty() { UNGROUPED } else { group };
            rows.push(FolderRow::Header(name.to_string()));
            current = Some(group);
        }
        rows.push(FolderRow::Folder(idx));
    }
    rows
}

/// Row index of the folder at `folder_idx`
pub fn row_of_folder(rows: &[FolderRow], folder_idx: usize) -> Option<usize> {
    rows.iter()
        .position(|r| *r == FolderRow::Folder(folder_idx))
}

/// Distinct non-empty group names, sorted case-insensitively with exact spelling breaking ties
pub fn group_names(folders: &[Folder]) -> Vec<String> {
    let mut names: Vec<String> = folders
        .iter()
        .map(group_of)
        .filter(|g| !g.is_empty())
        .map(str::to_string)
        .collect();
    names.sort_by(|a, b| (a.to_lowercase(), a).cmp(&(b.to_lowercase(), b)));
    names.dedup();
    names
}

/// Folder group menu rows: existing groups, then "No group", then "New group…"
pub fn group_menu_options(folders: &[Folder]) -> Vec<crate::model::types::GroupMenuOption> {
    use crate::model::types::GroupMenuOption;
    let mut options: Vec<GroupMenuOption> = group_names(folders)
        .into_iter()
        .map(GroupMenuOption::Group)
        .collect();
    options.push(GroupMenuOption::NoGroup);
    options.push(GroupMenuOption::NewGroup);
    options
}

/// A typed group name, trimmed; an existing group matching it regardless of case keeps
/// its spelling. `None` when the name is empty.
pub fn resolve_group_name(input: &str, existing: &[String]) -> Option<String> {
    let name = input.trim();
    if name.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    Some(
        existing
            .iter()
            .find(|g| g.to_lowercase() == lower)
            .cloned()
            .unwrap_or_else(|| name.to_string()),
    )
}

/// Warning shown when setting a group on Syncthing without folder groups
pub fn groups_unsupported_message(version: Option<&str>) -> String {
    match version {
        Some(v) => format!(
            "Folder groups need Syncthing 2.1 or newer (this device runs {})",
            v
        ),
        None => "Folder groups need Syncthing 2.1 or newer".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, group: Option<&str>) -> Folder {
        let mut json = serde_json::json!({
            "id": id, "label": id, "path": "/data", "paused": false, "type": "sendreceive"
        });
        if let Some(group) = group {
            json["group"] = serde_json::json!(group);
        }
        serde_json::from_value(json).unwrap()
    }

    fn ids(folders: &[Folder]) -> Vec<&str> {
        folders.iter().map(|f| f.id.as_str()).collect()
    }

    fn grouped() -> Vec<Folder> {
        vec![
            folder("docs", Some("")),
            folder("media", Some("")),
            folder("photos", Some("Personal")),
            folder("projects", Some("Work")),
            folder("vault", Some("Personal")),
        ]
    }

    #[test]
    fn test_groups_supported() {
        assert!(!groups_supported(&[folder("a", None), folder("b", None)]));
        assert!(!groups_supported(&[]));
        assert!(groups_supported(&[
            folder("a", None),
            folder("b", Some(""))
        ]));
    }

    #[test]
    fn test_order_unsupported_unchanged() {
        let folders = vec![folder("b", None), folder("a", None)];
        assert_eq!(ids(&order_by_group(folders)), ["b", "a"]);
    }

    #[test]
    fn test_order_supported_without_groups_unchanged() {
        let folders = vec![folder("b", Some("")), folder("a", Some(""))];
        assert_eq!(ids(&order_by_group(folders)), ["b", "a"]);
    }

    #[test]
    fn test_order_groups_az_ungrouped_last_stable() {
        assert_eq!(
            ids(&order_by_group(grouped())),
            ["photos", "vault", "projects", "docs", "media"]
        );
    }

    #[test]
    fn test_case_differing_groups_stay_separate() {
        let folders = vec![
            folder("a", Some("work")),
            folder("b", Some("Work")),
            folder("c", Some("work")),
        ];
        let ordered = order_by_group(folders);
        assert_eq!(ids(&ordered), ["b", "a", "c"]);
        assert_eq!(group_names(&ordered), ["Work", "work"]);
        let headers = folder_rows(&ordered)
            .into_iter()
            .filter(|r| matches!(r, FolderRow::Header(_)))
            .count();
        assert_eq!(headers, 2);
    }

    #[test]
    fn test_rows_unsupported_have_no_headers() {
        let folders = vec![folder("a", None), folder("b", None)];
        assert_eq!(
            folder_rows(&folders),
            [FolderRow::Folder(0), FolderRow::Folder(1)]
        );
    }

    #[test]
    fn test_rows_supported_without_groups_single_ungrouped_header() {
        let folders = vec![folder("a", Some("")), folder("b", Some(""))];
        assert_eq!(
            folder_rows(&folders),
            [
                FolderRow::Header(UNGROUPED.to_string()),
                FolderRow::Folder(0),
                FolderRow::Folder(1)
            ]
        );
    }

    #[test]
    fn test_rows_with_groups() {
        let ordered = order_by_group(grouped());
        assert_eq!(
            folder_rows(&ordered),
            [
                FolderRow::Header("Personal".to_string()),
                FolderRow::Folder(0),
                FolderRow::Folder(1),
                FolderRow::Header("Work".to_string()),
                FolderRow::Folder(2),
                FolderRow::Header("Ungrouped".to_string()),
                FolderRow::Folder(3),
                FolderRow::Folder(4),
            ]
        );
    }

    #[test]
    fn test_row_of_folder() {
        let rows = folder_rows(&order_by_group(grouped()));
        assert_eq!(row_of_folder(&rows, 2), Some(4));
        assert_eq!(row_of_folder(&rows, 9), None);
    }

    #[test]
    fn test_rows_empty() {
        assert_eq!(folder_rows(&[]), []);
    }

    // ========================================
    // GROUP MENU
    // ========================================

    #[test]
    fn test_group_menu_options() {
        use crate::model::types::GroupMenuOption as O;
        let folders = [
            folder("a", Some("Work")),
            folder("b", Some("Personal")),
            folder("c", Some("")),
        ];
        assert_eq!(
            group_menu_options(&folders),
            [
                O::Group("Personal".into()),
                O::Group("Work".into()),
                O::NoGroup,
                O::NewGroup
            ]
        );
    }

    #[test]
    fn test_resolve_group_name() {
        assert_eq!(
            resolve_group_name("  Clients ", &[]),
            Some("Clients".to_string())
        );
        assert_eq!(
            resolve_group_name("work", &["Work".to_string()]),
            Some("Work".to_string())
        );
        assert_eq!(resolve_group_name("   ", &[]), None);
    }

    #[test]
    fn test_groups_unsupported_message() {
        assert_eq!(
            groups_unsupported_message(Some("v2.0.3")),
            "Folder groups need Syncthing 2.1 or newer (this device runs v2.0.3)"
        );
        assert_eq!(
            groups_unsupported_message(None),
            "Folder groups need Syncthing 2.1 or newer"
        );
    }
}
