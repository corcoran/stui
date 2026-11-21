//! Sync State Logic
//!
//! This module contains logic for sync state management and transitions.

use crate::api::{BrowseItem, FolderStatus, LocalChangedFile, NeedResponse, SyncState};
use crate::model::syncthing::{NeedFetch, NeedIndex, PathEvent, PathEventKind};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Get the priority of a sync state for sorting
///
/// Lower number = higher priority (displayed first)
///
/// Priority order:
/// 1. OutOfSync (⚠️) - Most important
/// 2. Syncing (🔄) - Active operation
/// 3. RemoteOnly (☁️)
/// 4. LocalOnly (💻)
/// 5. Ignored (🚫)
/// 6. Unknown (❓)
/// 7. Synced (✅) - Least important
pub fn sync_state_priority(state: SyncState) -> u8 {
    match state {
        SyncState::OutOfSync => 0,  // ⚠️ Most important
        SyncState::Syncing => 1,    // 🔄 Active operation
        SyncState::RemoteOnly => 2, // ☁️
        SyncState::LocalOnly => 3,  // 💻
        SyncState::Ignored => 4,    // 🚫
        SyncState::Unknown => 5,    // ❓
        SyncState::Synced => 6,     // ✅ Least important
    }
}

/// Check which ignored files exist on the filesystem
///
/// For all items with Ignored sync state, checks if the corresponding file/directory
/// exists on disk. This is used to distinguish between ignored files that still exist
/// (⚠️ warning icon) vs. those that have been deleted (🚫 ban icon).
///
/// # Arguments
/// * `items` - List of browse items to check
/// * `file_sync_states` - Current sync states for items
/// * `translated_base_path` - The host path to the directory
/// * `parent_exists` - Optional optimization: if parent doesn't exist, skip checks
///
/// # Returns
/// HashMap mapping item names to whether they exist on disk (only for ignored items)
pub fn check_ignored_existence(
    items: &[BrowseItem],
    file_sync_states: &HashMap<String, SyncState>,
    translated_base_path: &str,
    parent_exists: Option<bool>,
) -> HashMap<String, bool> {
    let mut ignored_exists = HashMap::new();

    for item in items {
        if let Some(SyncState::Ignored) = file_sync_states.get(&item.name) {
            // Optimization: If parent directory doesn't exist, children can't either
            if parent_exists == Some(false) {
                ignored_exists.insert(item.name.clone(), false);
                continue;
            }

            // Check filesystem for this item
            let host_path = format!(
                "{}/{}",
                translated_base_path.trim_end_matches('/'),
                item.name
            );
            let exists = std::path::Path::new(&host_path).exists();
            ignored_exists.insert(item.name.clone(), exists);
        }
    }

    ignored_exists
}

/// Aggregate directory sync state based on direct state and children states
///
/// Determines a directory's overall sync state by considering:
/// 1. The directory's own direct state (from FileInfo)
/// 2. The states of all its children
///
/// # Priority Rules
/// - If directory itself is RemoteOnly or Ignored → use that (takes precedence)
/// - Otherwise, use highest priority child state:
///   - Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced
/// - If all children synced → use directory's direct state
///
/// # Arguments
/// * `direct_state` - The directory's own sync state (None = default to Synced)
/// * `child_states` - Slice of all children's sync states
///
/// # Returns
/// The aggregated sync state for the directory
pub fn aggregate_directory_state(
    direct_state: Option<SyncState>,
    child_states: &[SyncState],
) -> SyncState {
    let direct = direct_state.unwrap_or(SyncState::Synced);

    // If directory itself is RemoteOnly or Ignored, that takes precedence
    if matches!(direct, SyncState::RemoteOnly | SyncState::Ignored) {
        return direct;
    }

    // Collect child state flags
    let mut has_syncing = false;
    let mut has_remote_only = false;
    let mut has_out_of_sync = false;
    let mut has_local_only = false;

    for state in child_states {
        match state {
            SyncState::Syncing => has_syncing = true,
            SyncState::RemoteOnly => has_remote_only = true,
            SyncState::OutOfSync => has_out_of_sync = true,
            SyncState::LocalOnly => has_local_only = true,
            _ => {}
        }
    }

    // Priority order: Syncing > RemoteOnly > OutOfSync > LocalOnly > Synced
    if has_syncing {
        SyncState::Syncing
    } else if has_remote_only {
        SyncState::RemoteOnly
    } else if has_out_of_sync {
        SyncState::OutOfSync
    } else if has_local_only {
        SyncState::LocalOnly
    } else {
        // All children synced, use directory's direct state
        direct
    }
}

/// Folder states during which Syncthing may have transfers in flight.
/// Any other state means nothing is downloading, so the folder's syncing set is stale.
const ACTIVE_SYNC_STATES: [&str; 2] = ["syncing", "sync-preparing"];

/// How often to re-seed a folder's syncing set while it stays in "syncing".
/// ItemStarted/ItemFinished keep it current in between; this catches missed events.
pub const SYNCING_FILES_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/// Paths Syncthing is actively downloading (the `progress` list). Queued files are not included.
pub fn syncing_paths_from_need(need: &NeedResponse) -> HashSet<String> {
    need.progress
        .iter()
        .map(|f| f.name.trim_end_matches('/').to_string())
        .collect()
}

/// The syncing set for a need response requested at `requested_at`: its `progress` paths,
/// minus those an event reported finished since, plus those an event reported started since.
pub fn syncing_from_need(
    progress: &[String],
    requested_at: Instant,
    events: Option<&HashMap<String, PathEvent>>,
) -> HashSet<String> {
    let mut syncing: HashSet<String> = progress.iter().cloned().collect();
    for (path, event) in events.into_iter().flatten() {
        if event.at <= requested_at {
            continue;
        }
        match event.kind {
            PathEventKind::Started => syncing.insert(path.clone()),
            PathEventKind::Finished => syncing.remove(path),
        };
    }
    syncing
}

/// Path of a breadcrumb item relative to the folder root.
///
/// Breadcrumb prefixes carry a trailing slash (`"sub/"`), so this is a plain concatenation.
pub fn breadcrumb_item_path(prefix: Option<&str>, name: &str) -> String {
    format!("{}{}", prefix.unwrap_or(""), name)
}

/// Name of `path` as an item of the level listing `prefix`, if it lies directly in it
pub fn name_in_level<'a>(prefix: Option<&str>, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix.unwrap_or(""))?;
    (!rest.is_empty() && !rest.contains('/')).then_some(rest)
}

/// Overlay the in-flight transfer set on a base sync state.
///
/// `path` is relative to the folder root with no trailing slash. A directory is Syncing
/// when any syncing path lies beneath it.
pub fn effective_sync_state(
    base: SyncState,
    path: &str,
    is_dir: bool,
    syncing: &HashSet<String>,
) -> SyncState {
    if syncing.contains(path) {
        return SyncState::Syncing;
    }
    if is_dir {
        let dir_prefix = format!("{}/", path);
        if syncing.iter().any(|p| p.starts_with(&dir_prefix)) {
            return SyncState::Syncing;
        }
    }
    base
}

/// Displayed sync state of one breadcrumb item, with in-flight transfers overlaid.
///
/// `None` (state not loaded yet) stays `None` unless the item is syncing.
pub fn item_sync_state(
    base: Option<SyncState>,
    prefix: Option<&str>,
    item: &BrowseItem,
    syncing: Option<&HashSet<String>>,
) -> Option<SyncState> {
    let Some(syncing) = syncing else {
        return base;
    };
    let path = breadcrumb_item_path(prefix, &item.name);
    let is_dir = item.item_type == "FILE_INFO_TYPE_DIRECTORY";
    match effective_sync_state(base.unwrap_or(SyncState::Unknown), &path, is_dir, syncing) {
        SyncState::Syncing => Some(SyncState::Syncing),
        _ => base,
    }
}

/// Apply `effective_sync_state` to every item in a breadcrumb level.
///
/// Borrows `base` unchanged when nothing in the folder is syncing, so the common case
/// costs no allocation per frame.
pub fn overlay_syncing_states<'a>(
    items: &[BrowseItem],
    base: &'a HashMap<String, SyncState>,
    prefix: Option<&str>,
    syncing: Option<&HashSet<String>>,
) -> Cow<'a, HashMap<String, SyncState>> {
    let Some(syncing) = syncing.filter(|s| !s.is_empty()) else {
        return Cow::Borrowed(base);
    };

    let mut overlaid = base.clone();
    for item in items {
        let state = base.get(&item.name).copied();
        if item_sync_state(state, prefix, item, Some(syncing)) == Some(SyncState::Syncing) {
            overlaid.insert(item.name.clone(), SyncState::Syncing);
        }
    }
    Cow::Owned(overlaid)
}

/// How long `update_directory_states` reuses a directory's child states read from the cache.
/// The aggregate costs a cache query per child, and the function runs on every browse result.
pub const DIR_CHILD_STATES_TTL: Duration = Duration::from_secs(2);

/// Max FileInfo requests queued when a breadcrumb level opens.
///
/// No frame height is known outside rendering, and a breadcrumb panel on a 60-row terminal
/// shows about 50 rows. Items past the cap are picked up by the idle-time fetchers.
pub const NAVIGATION_FILE_INFO_CAP: usize = 50;

/// Max FileInfo requests queued for the folder-list preview panel.
///
/// Every j/k in the folder list opens a new preview, so this stays small: 20 rows cover the
/// top of the panel on a 30-row terminal. The rest load when the folder is entered.
pub const PREVIEW_FILE_INFO_CAP: usize = 20;

/// Sync states for a breadcrumb level after a fresh browse.
///
/// A cached state wins, then the previous in-memory state for names still listed, so a
/// cache wiped by invalidation never blanks an item. Names no longer in `items` are dropped.
/// Local-only items are always `LocalOnly`.
pub fn merge_level_states(
    prev: &HashMap<String, SyncState>,
    cached: HashMap<String, SyncState>,
    items: &[BrowseItem],
    local_only: &[String],
) -> HashMap<String, SyncState> {
    let mut merged = HashMap::with_capacity(items.len());
    for item in items {
        if let Some(state) = cached.get(&item.name).or_else(|| prev.get(&item.name)) {
            merged.insert(item.name.clone(), *state);
        }
    }
    for name in local_only {
        merged.insert(name.clone(), SyncState::LocalOnly);
    }
    merged
}

/// Whether a FileInfo response belongs to a folder shown in the breadcrumb trail.
///
/// The trail exists while the folder list is focused (preview panel), so focus is irrelevant.
pub fn is_file_info_relevant<'a>(
    folder_id: &str,
    trail_folder_ids: impl IntoIterator<Item = &'a str>,
) -> bool {
    trail_folder_ids.into_iter().any(|id| id == folder_id)
}

/// Item name of `file_path` within the level at `level_prefix`, if it is a direct child.
pub fn level_item_name<'a>(file_path: &'a str, level_prefix: Option<&str>) -> Option<&'a str> {
    file_path
        .strip_prefix(level_prefix.unwrap_or(""))
        .filter(|name| !name.is_empty() && !name.contains('/'))
}

/// Folder-relative paths to request FileInfo for when a level opens.
///
/// Covers up to `cap` items of `display_items` around `selected` that have no state yet.
pub fn file_info_paths_to_request(
    display_items: &[BrowseItem],
    states: &HashMap<String, SyncState>,
    prefix: Option<&str>,
    selected: Option<usize>,
    cap: usize,
) -> Vec<String> {
    let len = display_items.len();
    let start = selected
        .unwrap_or(0)
        .saturating_sub(cap / 2)
        .min(len.saturating_sub(cap));
    display_items
        .iter()
        .skip(start)
        .take(cap)
        .filter(|item| !states.contains_key(&item.name))
        .map(|item| breadcrumb_item_path(prefix, &item.name))
        .collect()
}

// ========================================
// DERIVED STATES (need index)
// ========================================

/// Index states of the queued and rest entries of a need page.
///
/// Queued and rest entries are RemoteOnly: a need entry carries only the global FileInfo, so an
/// outdated local copy (which FileInfo reports as OutOfSync) can't be told apart from a missing
/// one. A deleted entry is a file the remote deleted that still exists here, which FileInfo also
/// reports as LocalOnly. Progress entries are left out: they are Syncing, and `syncing_files` is
/// the only source of that state.
pub fn need_index_states(need: &NeedResponse) -> HashMap<String, SyncState> {
    need.queued
        .iter()
        .chain(&need.rest)
        .map(|f| {
            let state = if f.deleted {
                SyncState::LocalOnly
            } else {
                SyncState::RemoteOnly
            };
            (f.name.trim_end_matches('/').to_string(), state)
        })
        .collect()
}

/// Index states of /rest/db/localchanged entries.
///
/// A local deletion leaves the file on the remote only; anything else is a local addition or edit.
pub fn local_changed_states(files: &[LocalChangedFile]) -> HashMap<String, SyncState> {
    files
        .iter()
        .map(|f| {
            let state = if f.deleted {
                SyncState::RemoteOnly
            } else {
                SyncState::LocalOnly
            };
            (f.name.trim_end_matches('/').to_string(), state)
        })
        .collect()
}

/// Drop `path` from the index's local changes, e.g. once an undelete starts for it
pub fn forget_local_change(index: &mut NeedIndex, path: &str) {
    if index.local_changed.remove(path).is_some() {
        rebuild_descendants(index);
    }
}

/// Record `path` as deleted on this device, before /rest/db/localchanged reports it
pub fn record_local_deletion(index: &mut NeedIndex, path: &str) {
    index.local_changed.insert(
        path.trim_end_matches('/').to_string(),
        SyncState::RemoteOnly,
    );
    rebuild_descendants(index);
}

/// Whether `path` was deleted on this device in a Receive Only folder: /rest/db/localchanged
/// lists it as deleted. Such a file's sync state is RemoteOnly. A file Syncthing is downloading
/// or has queued is not: while it restores a file, Syncthing lists it both as a local deletion
/// and as needed.
pub fn is_locally_deleted(
    index: Option<&NeedIndex>,
    syncing: Option<&HashSet<String>>,
    path: &str,
) -> bool {
    let Some(index) = index else {
        return false;
    };
    index.local_changed.get(path) == Some(&SyncState::RemoteOnly)
        && !index.need.contains_key(path)
        && !syncing.is_some_and(|s| s.contains(path))
}

/// Preview text for a file deleted on this device in a Receive Only folder
pub fn locally_deleted_message() -> &'static str {
    "Deleted on this device. If you wish to sync this file to your device, close the preview and \
     press:\n\u{2022} u to restore this file\n\u{2022} R to revert all local changes in this folder"
}

/// Rank used to combine states into a directory state, matching `aggregate_directory_state`.
/// Lower is worse.
fn aggregation_rank(state: SyncState) -> u8 {
    match state {
        SyncState::Syncing => 0,
        SyncState::RemoteOnly => 1,
        SyncState::OutOfSync => 2,
        SyncState::LocalOnly => 3,
        SyncState::Ignored => 4,
        SyncState::Synced => 5,
        SyncState::Unknown => 6,
    }
}

fn worse(a: Option<SyncState>, b: SyncState) -> SyncState {
    match a {
        Some(a) if aggregation_rank(a) <= aggregation_rank(b) => a,
        _ => b,
    }
}

fn rebuild_descendants(index: &mut NeedIndex) {
    let mut descendants: HashMap<String, SyncState> = HashMap::new();
    for (path, state) in index.need.iter().chain(&index.local_changed) {
        let mut end = 0;
        while let Some(pos) = path[end..].find('/') {
            end += pos;
            let dir = &path[..end];
            let merged = worse(descendants.get(dir).copied(), *state);
            descendants.insert(dir.to_string(), merged);
            end += 1;
        }
    }
    index.descendants = descendants;
}

/// Number of entries on a need page (progress, queued and rest).
pub fn need_entry_count(need: &NeedResponse) -> u64 {
    (need.progress.len() + need.queued.len() + need.rest.len()) as u64
}

/// Replace the need part of `index` with a fresh need page.
pub fn set_index_need(
    index: &mut NeedIndex,
    need: &NeedResponse,
    sequence: u64,
    complete: bool,
    ignore_patterns: Option<Vec<String>>,
) {
    index.need = need_index_states(need);
    index.sequence = Some(sequence);
    index.complete = complete;
    index.ignore = ignore_patterns
        .as_deref()
        .map(crate::logic::ignore::compile_ignore_patterns);
    rebuild_descendants(index);
}

/// Forget the need part of `index`, as if it had never been fetched.
pub fn clear_index_need(index: &mut NeedIndex) {
    index.need.clear();
    index.sequence = None;
    index.complete = false;
    index.requested_at = None;
    rebuild_descendants(index);
}

/// Replace the local-changes part of `index`.
pub fn set_index_local_changed(index: &mut NeedIndex, files: &[LocalChangedFile], sequence: u64) {
    index.local_changed = local_changed_states(files);
    index.local_changed_sequence = Some(sequence);
    rebuild_descendants(index);
}

/// State the index lists for `path`: its own entry, plus for a directory the worst state
/// among indexed paths beneath it.
pub fn indexed_state(index: &NeedIndex, path: &str, is_dir: bool) -> Option<SyncState> {
    let mut state = None;
    for own in [index.need.get(path), index.local_changed.get(path)]
        .into_iter()
        .flatten()
    {
        state = Some(worse(state, *own));
    }
    if is_dir && let Some(below) = index.descendants.get(path) {
        state = Some(worse(state, *below));
    }
    state
}

/// Whether an item the index doesn't list is known to be Synced (ignore status aside).
///
/// Requires an idle, unpaused folder whose need list was fetched completely for the current
/// sequence, and whose receive-only local changes (if any) were fetched for it too.
fn index_proves_synced(ctx: &DeriveContext) -> bool {
    let (Some(index), Some(status)) = (ctx.index, ctx.status) else {
        return false;
    };
    let local_changes_known = status.receive_only_total_items == 0
        || (index.local_changed_sequence == Some(status.sequence)
            && index.local_changed.len() as u64 == status.receive_only_total_items);
    status.state == "idle"
        && index.sequence == Some(status.sequence)
        && index.complete
        && local_changes_known
}

/// What the derivation knows about a folder.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeriveContext<'a> {
    pub index: Option<&'a NeedIndex>,
    pub status: Option<&'a FolderStatus>,
    pub paused: bool,
}

/// State of an item derived from the folder's need index.
pub fn derived_state(
    path: &str,
    is_dir: bool,
    ctx: &DeriveContext,
    known: Option<SyncState>,
) -> Option<SyncState> {
    let paused = ctx.paused || ctx.status.is_some_and(|s| s.state == "paused");
    if paused {
        return None;
    }
    if let Some(state) = ctx
        .index
        .and_then(|index| indexed_state(index, path, is_dir))
    {
        return Some(state);
    }
    if !index_proves_synced(ctx) {
        return None;
    }
    // Ignored items are never in the need list, so "unlisted" says nothing about them
    let not_ignored = match known {
        Some(SyncState::Ignored) => false,
        Some(_) => true,
        None => ctx
            .index
            .and_then(|index| index.ignore.as_ref())
            .is_some_and(|matcher| !crate::logic::ignore::may_be_ignored(matcher, path)),
    };
    not_ignored.then_some(SyncState::Synced)
}

/// State shown for a directory given its own state and its derived state.
pub fn resolve_directory_state(
    direct: Option<SyncState>,
    derived: Option<SyncState>,
) -> Option<SyncState> {
    let direct = direct.filter(|s| *s != SyncState::Unknown);
    match (direct, derived) {
        (Some(s @ (SyncState::RemoteOnly | SyncState::Ignored)), _) => Some(s),
        (Some(d), Some(derived)) => Some(worse(Some(d), derived)),
        (direct, derived) => derived.or(direct),
    }
}

/// The entries of `states` that belong to directory items.
pub fn directory_states(
    items: &[BrowseItem],
    states: &HashMap<String, SyncState>,
) -> HashMap<String, SyncState> {
    items
        .iter()
        .filter(|item| item.item_type == "FILE_INFO_TYPE_DIRECTORY")
        .filter_map(|item| states.get(&item.name).map(|s| (item.name.clone(), *s)))
        .collect()
}

/// A directory's own state for resolution: its FileInfo/cache state, except that a shown
/// Ignored (set by an ignore action before FileInfo confirms it) wins.
pub fn directory_own_state(
    direct: Option<SyncState>,
    shown: Option<SyncState>,
) -> Option<SyncState> {
    match shown {
        Some(SyncState::Ignored) => shown,
        _ => direct,
    }
}

/// Apply derived states to one breadcrumb level's state map.
///
/// Files keep any state they have (FileInfo is authoritative) and only gain a derived one.
/// Directories are recomputed as `resolve_directory_state` of their own state in `direct` and
/// the derived state, keeping the shown state only when neither is known.
pub fn apply_derived_states(
    items: &[BrowseItem],
    prefix: Option<&str>,
    states: &mut HashMap<String, SyncState>,
    direct: &HashMap<String, SyncState>,
    ctx: &DeriveContext,
) {
    for item in items {
        let is_dir = item.item_type == "FILE_INFO_TYPE_DIRECTORY";
        let current = states.get(&item.name).copied();
        if !is_dir && current.is_some() {
            continue;
        }
        let path = breadcrumb_item_path(prefix, &item.name);
        let state = if is_dir {
            // The shown state may itself be derived, so only the directory's own state (or an
            // Ignored set by a user action) feeds the resolution
            let own = directory_own_state(direct.get(&item.name).copied(), current);
            let derived = derived_state(&path, true, ctx, own);
            resolve_directory_state(own, derived).or(current)
        } else {
            derived_state(&path, false, ctx, None)
        };
        if let Some(state) = state
            && current != Some(state)
        {
            states.insert(item.name.clone(), state);
        }
    }
}

/// What a folder status poll should do about the folder's need fetch and syncing set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NeedFetchPlan {
    pub fetch: bool,
    pub clear_syncing: bool,
    /// Drop responses to requests already sent (the folder left syncing)
    pub discard_pending: bool,
}

/// Whether a folder's need index is worth fetching: some breadcrumb level (preview included)
/// shows it, or it is syncing (the syncing overlay comes from the same fetch).
pub fn need_index_wanted(folder_open: bool, state: &str) -> bool {
    folder_open || ACTIVE_SYNC_STATES.contains(&state)
}

/// Whether to request /rest/db/localchanged for `sequence`: not yet requested for it, and
/// not within the refresh interval of a failure.
pub fn local_changed_fetch_due(
    requested: Option<u64>,
    failed_at: Option<Instant>,
    sequence: u64,
    now: Instant,
) -> bool {
    match failed_at {
        Some(at) => now.saturating_duration_since(at) >= SYNCING_FILES_REFRESH_INTERVAL,
        None => requested != Some(sequence),
    }
}

/// Whether a need response sent at `requested_at` should be applied.
///
/// Superseded responses still count when they are newer than what the index holds, so a
/// slow endpoint can't starve the index while each refresh supersedes the last.
pub fn accept_need_response(
    fetch: Option<&NeedFetch>,
    index_requested_at: Option<Instant>,
    requested_at: Instant,
) -> bool {
    fetch.is_some_and(|f| requested_at >= f.floor)
        && index_requested_at.is_none_or(|built| requested_at > built)
}

/// Decide whether a folder status poll should refetch /rest/db/need and clear the syncing set.
pub fn need_fetch_plan(
    prev_state: Option<&str>,
    new_state: &str,
    last_fetch: Option<NeedFetch>,
    sequence: u64,
    now: Instant,
) -> NeedFetchPlan {
    let was_active = prev_state.is_some_and(|s| ACTIVE_SYNC_STATES.contains(&s));
    let backoff = last_fetch.is_some_and(|f| {
        f.failed && now.saturating_duration_since(f.at) < SYNCING_FILES_REFRESH_INTERVAL
    });
    if !ACTIVE_SYNC_STATES.contains(&new_state) {
        let stale = last_fetch.is_none_or(|f| f.failed || f.sequence != sequence);
        return NeedFetchPlan {
            fetch: !backoff && (was_active || stale),
            clear_syncing: true,
            discard_pending: was_active,
        };
    }
    let fetch = match last_fetch {
        None => true,
        Some(f) if f.failed => !backoff,
        Some(_) if new_state != "syncing" => false,
        Some(_) if prev_state != Some("syncing") => true,
        Some(f) => now.saturating_duration_since(f.at) >= SYNCING_FILES_REFRESH_INTERVAL,
    };
    NeedFetchPlan {
        fetch,
        clear_syncing: false,
        discard_pending: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: Instant, kind: PathEventKind) -> PathEvent {
        PathEvent { at, kind }
    }

    fn events(entries: &[(&str, PathEvent)]) -> HashMap<String, PathEvent> {
        entries
            .iter()
            .map(|(path, event)| (path.to_string(), *event))
            .collect()
    }

    fn progress(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn test_syncing_from_need_drops_path_finished_after_request() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let events = events(&[("a", event(t1, PathEventKind::Finished))]);

        let syncing = syncing_from_need(&progress(&["a", "b"]), t0, Some(&events));

        assert_eq!(syncing, HashSet::from(["b".to_string()]));
    }

    #[test]
    fn test_syncing_from_need_adds_path_started_after_request() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let events = events(&[("c", event(t1, PathEventKind::Started))]);

        let syncing = syncing_from_need(&progress(&["a"]), t0, Some(&events));

        assert_eq!(syncing, HashSet::from(["a".to_string(), "c".to_string()]));
    }

    #[test]
    fn test_syncing_from_need_keeps_events_older_than_request_out() {
        let t0 = Instant::now();
        let before = t0 - Duration::from_millis(1);
        let events = events(&[
            ("a", event(before, PathEventKind::Finished)),
            ("c", event(before, PathEventKind::Started)),
        ]);

        let syncing = syncing_from_need(&progress(&["a"]), t0, Some(&events));

        assert_eq!(syncing, HashSet::from(["a".to_string()]));
    }

    #[test]
    fn test_syncing_from_need_without_events_is_progress() {
        let syncing = syncing_from_need(&progress(&["a", "b"]), Instant::now(), None);

        assert_eq!(syncing.len(), 2);
    }

    #[test]
    fn test_sync_state_priority_order() {
        // OutOfSync has highest priority (lowest number)
        assert_eq!(sync_state_priority(SyncState::OutOfSync), 0);
        assert_eq!(sync_state_priority(SyncState::Syncing), 1);
        assert_eq!(sync_state_priority(SyncState::RemoteOnly), 2);
        assert_eq!(sync_state_priority(SyncState::LocalOnly), 3);
        assert_eq!(sync_state_priority(SyncState::Ignored), 4);
        assert_eq!(sync_state_priority(SyncState::Unknown), 5);

        // Synced has lowest priority (highest number)
        assert_eq!(sync_state_priority(SyncState::Synced), 6);
    }

    #[test]
    fn test_priority_ordering() {
        // OutOfSync should come before Synced
        assert!(sync_state_priority(SyncState::OutOfSync) < sync_state_priority(SyncState::Synced));

        // Syncing should come before RemoteOnly
        assert!(
            sync_state_priority(SyncState::Syncing) < sync_state_priority(SyncState::RemoteOnly)
        );

        // Unknown should come before Synced
        assert!(sync_state_priority(SyncState::Unknown) < sync_state_priority(SyncState::Synced));
    }

    #[test]
    fn test_check_ignored_existence_empty_items() {
        let items: Vec<BrowseItem> = vec![];
        let file_sync_states = HashMap::new();

        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_check_ignored_existence_no_ignored_items() {
        let items = vec![BrowseItem {
            name: "file1.txt".to_string(),
            size: 100,
            mod_time: "2023-01-01T00:00:00Z".to_string(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        }];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("file1.txt".to_string(), SyncState::Synced);

        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_check_ignored_existence_parent_not_exists() {
        let items = vec![BrowseItem {
            name: "ignored.txt".to_string(),
            size: 100,
            mod_time: "2023-01-01T00:00:00Z".to_string(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        }];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("ignored.txt".to_string(), SyncState::Ignored);

        // Parent doesn't exist - should optimize and return false without checking
        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", Some(false));
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("ignored.txt"), Some(&false));
    }

    #[test]
    fn test_check_ignored_existence_mixed_states() {
        let items = vec![
            BrowseItem {
                name: "synced.txt".to_string(),
                size: 100,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
            BrowseItem {
                name: "ignored.txt".to_string(),
                size: 200,
                mod_time: "2023-01-01T00:00:00Z".to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
            },
        ];
        let mut file_sync_states = HashMap::new();
        file_sync_states.insert("synced.txt".to_string(), SyncState::Synced);
        file_sync_states.insert("ignored.txt".to_string(), SyncState::Ignored);

        // Only ignored items should be in the result
        let result = check_ignored_existence(&items, &file_sync_states, "/tmp", None);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("ignored.txt"));
        assert!(!result.contains_key("synced.txt"));
    }
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;

    #[test]
    fn test_aggregate_directory_state_all_synced() {
        // When all children are synced, use direct state
        let direct_state = Some(SyncState::Synced);
        let child_states = vec![SyncState::Synced, SyncState::Synced];

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Synced);
    }

    #[test]
    fn test_aggregate_directory_state_one_syncing() {
        // Syncing has highest priority
        let direct_state = Some(SyncState::Synced);
        let child_states = vec![SyncState::Synced, SyncState::Syncing, SyncState::OutOfSync];

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Syncing);
    }

    #[test]
    fn test_aggregate_directory_state_mixed_priorities() {
        // RemoteOnly > OutOfSync > LocalOnly
        let direct_state = Some(SyncState::Synced);

        // RemoteOnly wins
        let child_states1 = vec![
            SyncState::RemoteOnly,
            SyncState::OutOfSync,
            SyncState::LocalOnly,
        ];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states1),
            SyncState::RemoteOnly
        );

        // OutOfSync wins (no RemoteOnly)
        let child_states2 = vec![SyncState::OutOfSync, SyncState::LocalOnly];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states2),
            SyncState::OutOfSync
        );

        // LocalOnly wins (no higher priority)
        let child_states3 = vec![SyncState::LocalOnly, SyncState::Synced];
        assert_eq!(
            aggregate_directory_state(direct_state, &child_states3),
            SyncState::LocalOnly
        );
    }

    #[test]
    fn test_aggregate_directory_state_remote_only_direct() {
        // Directory itself is RemoteOnly - takes precedence
        let direct_state = Some(SyncState::RemoteOnly);
        let child_states = vec![SyncState::Synced]; // Doesn't matter

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::RemoteOnly);
    }

    #[test]
    fn test_aggregate_directory_state_ignored_direct() {
        // Directory itself is Ignored - takes precedence
        let direct_state = Some(SyncState::Ignored);
        let child_states = vec![SyncState::Syncing]; // Doesn't matter

        let result = aggregate_directory_state(direct_state, &child_states);
        assert_eq!(result, SyncState::Ignored);
    }
}

#[cfg(test)]
mod syncing_tests {
    use super::*;

    // ========================================
    // SYNCING OVERLAY
    // ========================================

    fn need_file(name: &str) -> crate::api::FileInfo {
        crate::api::FileInfo {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn browse_item(name: &str, is_dir: bool) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: String::new(),
            item_type: if is_dir {
                "FILE_INFO_TYPE_DIRECTORY".to_string()
            } else {
                "FILE_INFO_TYPE_FILE".to_string()
            },
        }
    }

    #[test]
    fn test_syncing_paths_from_need_uses_progress_only() {
        let need = NeedResponse {
            progress: vec![need_file("a.mkv"), need_file("sub/b.mkv")],
            queued: vec![need_file("queued.mkv")],
            rest: vec![need_file("rest.mkv")],
            page: 1,
            perpage: 100,
        };

        assert_eq!(syncing_paths_from_need(&need), set(&["a.mkv", "sub/b.mkv"]));
    }

    #[test]
    fn test_syncing_paths_from_need_strips_trailing_slash() {
        let need = NeedResponse {
            progress: vec![need_file("dir/")],
            queued: vec![],
            rest: vec![],
            page: 1,
            perpage: 100,
        };

        assert_eq!(syncing_paths_from_need(&need), set(&["dir"]));
    }

    #[test]
    fn test_effective_sync_state_file_in_set_is_syncing() {
        let syncing = set(&["a.mkv"]);
        assert_eq!(
            effective_sync_state(SyncState::RemoteOnly, "a.mkv", false, &syncing),
            SyncState::Syncing
        );
    }

    #[test]
    fn test_effective_sync_state_queued_file_stays_remote_only() {
        let need = NeedResponse {
            progress: vec![need_file("a.mkv")],
            queued: vec![need_file("queued.mkv")],
            rest: vec![],
            page: 1,
            perpage: 100,
        };
        let syncing = syncing_paths_from_need(&need);

        assert_eq!(
            effective_sync_state(SyncState::RemoteOnly, "queued.mkv", false, &syncing),
            SyncState::RemoteOnly
        );
        assert_eq!(
            effective_sync_state(SyncState::RemoteOnly, "a.mkv", false, &syncing),
            SyncState::Syncing
        );
    }

    #[test]
    fn test_effective_sync_state_dir_with_syncing_descendant() {
        let syncing = set(&["sub/a.mkv"]);
        assert_eq!(
            effective_sync_state(SyncState::Synced, "sub", true, &syncing),
            SyncState::Syncing
        );
    }

    #[test]
    fn test_effective_sync_state_dir_prefix_trap() {
        let syncing = set(&["subway/x.mkv"]);
        assert_eq!(
            effective_sync_state(SyncState::Synced, "sub", true, &syncing),
            SyncState::Synced
        );
    }

    #[test]
    fn test_effective_sync_state_nested_dir_descendant() {
        let syncing = set(&["a/b/c/d.mkv"]);
        assert_eq!(
            effective_sync_state(SyncState::Synced, "a/b", true, &syncing),
            SyncState::Syncing
        );
    }

    #[test]
    fn test_effective_sync_state_file_does_not_match_as_prefix() {
        // A file can't contain children, so "sub" as a file ignores "sub/a.mkv"
        let syncing = set(&["sub/a.mkv"]);
        assert_eq!(
            effective_sync_state(SyncState::Synced, "sub", false, &syncing),
            SyncState::Synced
        );
    }

    #[test]
    fn test_effective_sync_state_empty_set_preserves_base() {
        let syncing = HashSet::new();
        for base in [
            SyncState::Synced,
            SyncState::RemoteOnly,
            SyncState::OutOfSync,
            SyncState::Ignored,
            SyncState::Unknown,
        ] {
            assert_eq!(effective_sync_state(base, "a.mkv", false, &syncing), base);
            assert_eq!(effective_sync_state(base, "sub", true, &syncing), base);
        }
    }

    #[test]
    fn test_breadcrumb_item_path_root() {
        assert_eq!(breadcrumb_item_path(None, "a.mkv"), "a.mkv");
    }

    #[test]
    fn test_breadcrumb_item_path_nested_prefix_has_trailing_slash() {
        assert_eq!(breadcrumb_item_path(Some("sub/"), "a.mkv"), "sub/a.mkv");
        assert_eq!(breadcrumb_item_path(Some("a/b/"), "c"), "a/b/c");
    }

    #[test]
    fn test_overlay_syncing_states_marks_items_under_prefix() {
        let items = vec![
            browse_item("a.mkv", false),
            browse_item("b.mkv", false),
            browse_item("deeper", true),
        ];
        let mut base = HashMap::new();
        base.insert("a.mkv".to_string(), SyncState::RemoteOnly);
        base.insert("b.mkv".to_string(), SyncState::Synced);
        base.insert("deeper".to_string(), SyncState::Synced);
        let syncing = set(&["sub/a.mkv", "sub/deeper/x.mkv"]);

        let overlaid = overlay_syncing_states(&items, &base, Some("sub/"), Some(&syncing));

        assert_eq!(overlaid.get("a.mkv"), Some(&SyncState::Syncing));
        assert_eq!(overlaid.get("b.mkv"), Some(&SyncState::Synced));
        assert_eq!(overlaid.get("deeper"), Some(&SyncState::Syncing));
        // Base map is untouched
        assert_eq!(base.get("a.mkv"), Some(&SyncState::RemoteOnly));
    }

    #[test]
    fn test_overlay_syncing_states_item_without_base_state() {
        let items = vec![browse_item("new.mkv", false)];
        let base = HashMap::new();
        let syncing = set(&["new.mkv"]);

        let overlaid = overlay_syncing_states(&items, &base, None, Some(&syncing));

        assert_eq!(overlaid.get("new.mkv"), Some(&SyncState::Syncing));
    }

    #[test]
    fn test_item_sync_state_overlays_syncing() {
        let syncing = set(&["sub/a.mkv"]);
        assert_eq!(
            item_sync_state(
                Some(SyncState::RemoteOnly),
                Some("sub/"),
                &browse_item("a.mkv", false),
                Some(&syncing)
            ),
            Some(SyncState::Syncing)
        );
        assert_eq!(
            item_sync_state(
                None,
                Some("sub/"),
                &browse_item("a.mkv", false),
                Some(&syncing)
            ),
            Some(SyncState::Syncing)
        );
    }

    #[test]
    fn test_item_sync_state_keeps_unloaded_as_none() {
        let syncing = set(&["other.mkv"]);
        let item = browse_item("a.mkv", false);
        assert_eq!(item_sync_state(None, None, &item, Some(&syncing)), None);
        assert_eq!(item_sync_state(None, None, &item, None), None);
        assert_eq!(
            item_sync_state(Some(SyncState::Synced), None, &item, None),
            Some(SyncState::Synced)
        );
    }

    #[test]
    fn test_overlay_syncing_states_borrows_when_nothing_syncing() {
        let items = vec![browse_item("a.mkv", false)];
        let mut base = HashMap::new();
        base.insert("a.mkv".to_string(), SyncState::RemoteOnly);

        assert!(matches!(
            overlay_syncing_states(&items, &base, None, None),
            Cow::Borrowed(_)
        ));
        let empty = HashSet::new();
        assert!(matches!(
            overlay_syncing_states(&items, &base, None, Some(&empty)),
            Cow::Borrowed(_)
        ));
    }

    // ========================================
    // LEVEL STATE MERGING
    // ========================================

    fn named_file(name: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
            mod_time: String::new(),
            size: 0,
        }
    }

    fn states(pairs: &[(&str, SyncState)]) -> HashMap<String, SyncState> {
        pairs.iter().map(|(n, s)| (n.to_string(), *s)).collect()
    }

    #[test]
    fn test_merge_level_states_keeps_prev_on_cache_miss() {
        let prev = states(&[("a", SyncState::Synced), ("b", SyncState::RemoteOnly)]);
        let items = vec![named_file("a"), named_file("b")];
        let merged = merge_level_states(&prev, HashMap::new(), &items, &[]);
        assert_eq!(merged, prev);
    }

    #[test]
    fn test_merge_level_states_prefers_cached() {
        let prev = states(&[("a", SyncState::Synced)]);
        let cached = states(&[("a", SyncState::OutOfSync)]);
        let merged = merge_level_states(&prev, cached, &[named_file("a")], &[]);
        assert_eq!(merged.get("a"), Some(&SyncState::OutOfSync));
    }

    #[test]
    fn test_merge_level_states_drops_removed_items() {
        let prev = states(&[("a", SyncState::Synced), ("gone", SyncState::Synced)]);
        let cached = states(&[("gone2", SyncState::Synced)]);
        let merged = merge_level_states(&prev, cached, &[named_file("a")], &[]);
        assert_eq!(merged, states(&[("a", SyncState::Synced)]));
    }

    #[test]
    fn test_merge_level_states_local_only_wins() {
        let prev = states(&[("new", SyncState::Synced)]);
        let cached = states(&[("new", SyncState::RemoteOnly)]);
        let merged = merge_level_states(&prev, cached, &[named_file("new")], &["new".to_string()]);
        assert_eq!(merged.get("new"), Some(&SyncState::LocalOnly));
    }

    #[test]
    fn test_merge_level_states_new_item_without_state_stays_missing() {
        let merged = merge_level_states(&HashMap::new(), HashMap::new(), &[named_file("a")], &[]);
        assert!(merged.is_empty());
    }

    // ========================================
    // FILE INFO ROUTING
    // ========================================

    #[test]
    fn test_is_file_info_relevant_when_folder_in_trail() {
        assert!(is_file_info_relevant("media", ["media", "media"]));
    }

    #[test]
    fn test_is_file_info_relevant_rejects_other_folder() {
        assert!(!is_file_info_relevant("docs", ["media"]));
        assert!(!is_file_info_relevant("docs", std::iter::empty::<&str>()));
    }

    #[test]
    fn test_level_item_name_root_accepts_direct_child() {
        assert_eq!(level_item_name("a.txt", None), Some("a.txt"));
    }

    #[test]
    fn test_level_item_name_root_rejects_nested_path() {
        assert_eq!(level_item_name("dir/a.txt", None), None);
    }

    #[test]
    fn test_level_item_name_prefix_accepts_direct_child() {
        assert_eq!(level_item_name("notes/x", Some("notes/")), Some("x"));
    }

    #[test]
    fn test_level_item_name_prefix_rejects_grandchild() {
        assert_eq!(level_item_name("notes/sub/x", Some("notes/")), None);
    }

    #[test]
    fn test_level_item_name_rejects_unrelated_and_empty() {
        assert_eq!(level_item_name("other/x", Some("notes/")), None);
        assert_eq!(level_item_name("notes/", Some("notes/")), None);
        assert_eq!(level_item_name("notesx", Some("notes/")), None);
    }

    // ========================================
    // NAVIGATION FILE INFO REQUESTS
    // ========================================

    #[test]
    fn test_file_info_paths_skip_items_with_state() {
        let items = vec![named_file("a"), named_file("b"), named_file("c")];
        let have = states(&[("b", SyncState::Synced)]);
        assert_eq!(
            file_info_paths_to_request(&items, &have, Some("sub/"), Some(0), 50),
            vec!["sub/a".to_string(), "sub/c".to_string()]
        );
    }

    #[test]
    fn test_file_info_paths_root_has_no_prefix() {
        let items = vec![named_file("a")];
        assert_eq!(
            file_info_paths_to_request(&items, &HashMap::new(), None, None, 50),
            vec!["a".to_string()]
        );
    }

    #[test]
    fn test_file_info_paths_capped() {
        let items: Vec<BrowseItem> = (0..100).map(|i| named_file(&format!("f{i}"))).collect();
        let paths = file_info_paths_to_request(&items, &HashMap::new(), None, Some(0), 10);
        assert_eq!(paths.len(), 10);
        assert_eq!(paths[0], "f0");
        assert_eq!(paths[9], "f9");
    }

    #[test]
    fn test_file_info_paths_window_follows_selection() {
        let items: Vec<BrowseItem> = (0..100).map(|i| named_file(&format!("f{i}"))).collect();
        let paths = file_info_paths_to_request(&items, &HashMap::new(), None, Some(80), 10);
        assert_eq!(paths.len(), 10);
        assert!(paths.contains(&"f80".to_string()));
    }

    #[test]
    fn test_file_info_paths_window_clamped_at_end() {
        let items: Vec<BrowseItem> = (0..100).map(|i| named_file(&format!("f{i}"))).collect();
        let paths = file_info_paths_to_request(&items, &HashMap::new(), None, Some(99), 10);
        assert_eq!(paths.len(), 10);
        assert_eq!(paths[9], "f99");
    }
}

#[cfg(test)]
mod derived_tests {
    use super::*;

    fn info(name: &str, deleted: bool) -> crate::api::FileInfo {
        crate::api::FileInfo {
            name: name.to_string(),
            deleted,
            ..Default::default()
        }
    }

    fn need(progress: &[&str], queued: &[&str], rest: &[&str]) -> NeedResponse {
        let infos = |names: &[&str]| names.iter().map(|n| info(n, false)).collect();
        NeedResponse {
            progress: infos(progress),
            queued: infos(queued),
            rest: infos(rest),
            page: 1,
            perpage: 1000,
        }
    }

    fn changed(name: &str, deleted: bool) -> LocalChangedFile {
        LocalChangedFile {
            name: name.to_string(),
            deleted,
        }
    }

    fn status(
        state: &str,
        sequence: u64,
        need_total: u64,
        receive_only_total: u64,
    ) -> FolderStatus {
        let mut json = serde_json::json!({});
        for key in [
            "globalBytes",
            "globalDeleted",
            "globalDirectories",
            "globalFiles",
            "globalSymlinks",
            "globalTotalItems",
            "inSyncBytes",
            "inSyncFiles",
            "localBytes",
            "localDeleted",
            "localDirectories",
            "localFiles",
            "localSymlinks",
            "localTotalItems",
            "needBytes",
            "needDeletes",
            "needDirectories",
            "needFiles",
            "needSymlinks",
            "receiveOnlyChangedBytes",
            "receiveOnlyChangedDeletes",
            "receiveOnlyChangedDirectories",
            "receiveOnlyChangedFiles",
            "receiveOnlyChangedSymlinks",
        ] {
            json[key] = serde_json::json!(0);
        }
        json["state"] = serde_json::json!(state);
        json["sequence"] = serde_json::json!(sequence);
        json["needTotalItems"] = serde_json::json!(need_total);
        json["receiveOnlyTotalItems"] = serde_json::json!(receive_only_total);
        serde_json::from_value(json).expect("valid FolderStatus")
    }

    /// Index built for `sequence` from a complete need page with no ignore patterns
    fn index_with_need(n: &NeedResponse, sequence: u64) -> NeedIndex {
        let mut index = NeedIndex::default();
        set_index_need(&mut index, n, sequence, true, Some(vec![]));
        index
    }

    fn ctx<'a>(index: &'a NeedIndex, status: &'a FolderStatus) -> DeriveContext<'a> {
        DeriveContext {
            index: Some(index),
            status: Some(status),
            paused: false,
        }
    }

    // ========================================
    // INDEX CONTENTS
    // ========================================

    #[test]
    fn test_queued_and_rest_files_are_remote_only() {
        let n = need(&[], &["q.mkv"], &["r.mkv"]);
        let index = index_with_need(&n, 7);
        let st = status("syncing", 7, 2, 0);

        assert_eq!(
            derived_state("q.mkv", false, &ctx(&index, &st), None),
            Some(SyncState::RemoteOnly)
        );
        assert_eq!(
            derived_state("r.mkv", false, &ctx(&index, &st), None),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_progress_file_displays_syncing_from_syncing_set_only() {
        let n = need(&["a.mkv"], &["q.mkv"], &[]);
        let index = index_with_need(&n, 7);
        let st = status("syncing", 7, 2, 0);
        let syncing = syncing_paths_from_need(&n);

        assert!(!index.need.contains_key("a.mkv"), "Syncing has one source");
        let base = derived_state("a.mkv", false, &ctx(&index, &st), None);
        let item = BrowseItem {
            name: "a.mkv".to_string(),
            size: 0,
            mod_time: String::new(),
            item_type: "FILE_INFO_TYPE_FILE".to_string(),
        };
        assert_eq!(
            item_sync_state(base, None, &item, Some(&syncing)),
            Some(SyncState::Syncing)
        );
    }

    #[test]
    fn test_need_deleted_entry_is_local_only() {
        let mut n = need(&[], &[], &[]);
        n.rest.push(info("gone.txt", true));
        let states = need_index_states(&n);
        assert_eq!(states.get("gone.txt"), Some(&SyncState::LocalOnly));
    }

    #[test]
    fn test_need_entry_trailing_slash_is_trimmed() {
        let states = need_index_states(&need(&[], &["dir/"], &[]));
        assert_eq!(states.get("dir"), Some(&SyncState::RemoteOnly));
    }

    #[test]
    fn test_local_changed_states_map_deleted_and_added() {
        let states = local_changed_states(&[
            changed("manuals/router-setup.txt", true),
            changed("recipes/pancakes.md", false),
        ]);
        assert_eq!(
            states.get("manuals/router-setup.txt"),
            Some(&SyncState::RemoteOnly)
        );
        assert_eq!(
            states.get("recipes/pancakes.md"),
            Some(&SyncState::LocalOnly)
        );
    }

    // ========================================
    // DIRECTORY AGGREGATES
    // ========================================

    fn documents_index(sequence: u64) -> NeedIndex {
        let mut index = index_with_need(&need(&[], &[], &[]), sequence);
        set_index_local_changed(
            &mut index,
            &[
                changed("todo.txt", false),
                changed("manuals/router-setup.txt", true),
                changed("recipes/pancakes.md", false),
            ],
            sequence,
        );
        index
    }

    #[test]
    fn test_dir_with_deleted_local_change_is_remote_only() {
        let index = documents_index(3);
        let st = status("idle", 3, 0, 3);
        assert_eq!(
            derived_state("manuals", true, &ctx(&index, &st), Some(SyncState::Synced)),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_dir_with_added_local_change_is_local_only() {
        let index = documents_index(3);
        let st = status("idle", 3, 0, 3);
        assert_eq!(
            derived_state("recipes", true, &ctx(&index, &st), None),
            Some(SyncState::LocalOnly)
        );
    }

    #[test]
    fn test_nested_path_marks_every_ancestor() {
        let index = index_with_need(&need(&[], &["a/b/c.txt"], &[]), 1);
        let st = status("idle", 1, 1, 0);
        for dir in ["a", "a/b"] {
            assert_eq!(
                derived_state(dir, true, &ctx(&index, &st), None),
                Some(SyncState::RemoteOnly),
                "{dir}"
            );
        }
    }

    #[test]
    fn test_sibling_with_shared_prefix_is_not_marked() {
        let index = index_with_need(&need(&[], &["sub/x.txt"], &[]), 1);
        let st = status("idle", 1, 1, 0);
        assert_eq!(
            derived_state("sub", true, &ctx(&index, &st), None),
            Some(SyncState::RemoteOnly)
        );
        assert_eq!(
            derived_state("subway", true, &ctx(&index, &st), None),
            Some(SyncState::Synced)
        );
    }

    #[test]
    fn test_dir_takes_worst_descendant_state() {
        let mut index = index_with_need(&need(&[], &["d/remote.txt"], &[]), 1);
        set_index_local_changed(&mut index, &[changed("d/local.txt", false)], 1);
        let st = status("idle", 1, 1, 1);
        assert_eq!(
            derived_state("d", true, &ctx(&index, &st), None),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_file_is_not_aggregated_over_paths_beneath_its_name() {
        let index = index_with_need(&need(&[], &["a/b.txt"], &[]), 1);
        let st = status("syncing", 1, 1, 0);
        assert_eq!(derived_state("a", false, &ctx(&index, &st), None), None);
    }

    // ========================================
    // SAFE SYNCED DEFAULT
    // ========================================

    #[test]
    fn test_unlisted_item_is_synced_when_idle_complete_and_unchanged() {
        let index = index_with_need(&need(&[], &[], &[]), 5);
        let st = status("idle", 5, 0, 0);
        assert_eq!(
            derived_state("notes.txt", false, &ctx(&index, &st), None),
            Some(SyncState::Synced)
        );
        assert_eq!(
            derived_state("dir", true, &ctx(&index, &st), None),
            Some(SyncState::Synced)
        );
    }

    #[test]
    fn test_known_ignored_item_is_not_defaulted_to_synced() {
        let index = index_with_need(&need(&[], &[], &[]), 5);
        let st = status("idle", 5, 0, 0);
        assert_eq!(
            derived_state("x.log", false, &ctx(&index, &st), Some(SyncState::Ignored)),
            None
        );
    }

    #[test]
    fn test_item_matching_ignore_pattern_is_not_defaulted() {
        let mut index = NeedIndex::default();
        set_index_need(
            &mut index,
            &need(&[], &[], &[]),
            5,
            true,
            Some(vec![
                "scratch.txt".to_string(),
                "/website/build.log".to_string(),
            ]),
        );
        let st = status("idle", 5, 0, 0);
        let c = ctx(&index, &st);
        assert_eq!(derived_state("notes/scratch.txt", false, &c, None), None);
        assert_eq!(derived_state("website/build.log", false, &c, None), None);
        assert_eq!(
            derived_state("notes", true, &c, None),
            Some(SyncState::Synced)
        );
    }

    #[test]
    fn test_unknown_ignore_patterns_disable_default() {
        let mut index = NeedIndex::default();
        set_index_need(&mut index, &need(&[], &[], &[]), 5, true, None);
        let st = status("idle", 5, 0, 0);
        assert_eq!(derived_state("a.txt", false, &ctx(&index, &st), None), None);
    }

    #[test]
    fn test_syncing_folder_unlisted_item_is_none() {
        let index = index_with_need(&need(&[], &["q.mkv"], &[]), 5);
        let st = status("syncing", 5, 1, 0);
        assert_eq!(
            derived_state("other.mkv", false, &ctx(&index, &st), None),
            None
        );
    }

    #[test]
    fn test_stale_sequence_unlisted_is_none_listed_still_derived() {
        let index = index_with_need(&need(&[], &["q.mkv"], &[]), 5);
        let st = status("idle", 6, 1, 0);
        let c = ctx(&index, &st);
        assert_eq!(derived_state("other.mkv", false, &c, None), None);
        assert_eq!(
            derived_state("q.mkv", false, &c, None),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_incomplete_index_derives_listed_paths_only() {
        let mut index = NeedIndex::default();
        set_index_need(
            &mut index,
            &need(&[], &["d/q.mkv"], &[]),
            5,
            false,
            Some(vec![]),
        );
        let st = status("idle", 5, 2000, 0);
        let c = ctx(&index, &st);
        assert_eq!(
            derived_state("d/q.mkv", false, &c, None),
            Some(SyncState::RemoteOnly)
        );
        assert_eq!(
            derived_state("d", true, &c, None),
            Some(SyncState::RemoteOnly)
        );
        assert_eq!(derived_state("other.mkv", false, &c, None), None);
    }

    #[test]
    fn test_paused_folder_is_none() {
        let index = index_with_need(&need(&[], &[], &[]), 5);
        let st = status("idle", 5, 0, 0);
        let c = DeriveContext {
            paused: true,
            ..ctx(&index, &st)
        };
        assert_eq!(derived_state("a.txt", false, &c, None), None);
        let st = status("paused", 5, 0, 0);
        assert_eq!(derived_state("a.txt", false, &ctx(&index, &st), None), None);
    }

    #[test]
    fn test_receive_only_default_requires_matching_local_changes() {
        let st = status("idle", 3, 0, 3);
        // Local changes never fetched
        let index = index_with_need(&need(&[], &[], &[]), 3);
        assert_eq!(derived_state("a.txt", false, &ctx(&index, &st), None), None);

        // Fetched for an older sequence
        let index = documents_index(2);
        let mut index = index;
        index.sequence = Some(3);
        assert_eq!(derived_state("a.txt", false, &ctx(&index, &st), None), None);

        // Count mismatch
        let mut index = documents_index(3);
        set_index_local_changed(&mut index, &[changed("todo.txt", false)], 3);
        assert_eq!(derived_state("a.txt", false, &ctx(&index, &st), None), None);

        // Matching
        let index = documents_index(3);
        assert_eq!(
            derived_state("a.txt", false, &ctx(&index, &st), None),
            Some(SyncState::Synced)
        );
    }

    #[test]
    fn test_no_index_is_none() {
        let st = status("idle", 5, 0, 0);
        let c = DeriveContext {
            index: None,
            status: Some(&st),
            paused: false,
        };
        assert_eq!(derived_state("a.txt", false, &c, None), None);
    }

    // ========================================
    // DIRECTORY RESOLUTION
    // ========================================

    #[test]
    fn test_resolve_aggregate_beats_own_synced() {
        assert_eq!(
            resolve_directory_state(Some(SyncState::Synced), Some(SyncState::RemoteOnly)),
            Some(SyncState::RemoteOnly)
        );
        assert_eq!(
            resolve_directory_state(None, Some(SyncState::LocalOnly)),
            Some(SyncState::LocalOnly)
        );
    }

    #[test]
    fn test_resolve_own_remote_only_and_ignored_win() {
        assert_eq!(
            resolve_directory_state(Some(SyncState::Ignored), Some(SyncState::Synced)),
            Some(SyncState::Ignored)
        );
        assert_eq!(
            resolve_directory_state(Some(SyncState::RemoteOnly), Some(SyncState::LocalOnly)),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_resolve_synced_default_does_not_hide_own_out_of_sync() {
        assert_eq!(
            resolve_directory_state(Some(SyncState::OutOfSync), Some(SyncState::Synced)),
            Some(SyncState::OutOfSync)
        );
        assert_eq!(
            resolve_directory_state(Some(SyncState::Unknown), Some(SyncState::Synced)),
            Some(SyncState::Synced)
        );
        assert_eq!(resolve_directory_state(None, None), None);
    }

    #[test]
    fn test_apply_derived_states_fills_files_and_resolves_dirs() {
        let mut index = documents_index(3);
        set_index_need(
            &mut index,
            &need(&[], &["manuals/new.pdf"], &[]),
            3,
            true,
            Some(vec![]),
        );
        let st = status("idle", 3, 1, 3);
        let item = |name: &str, dir: bool| BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: String::new(),
            item_type: if dir {
                "FILE_INFO_TYPE_DIRECTORY".to_string()
            } else {
                "FILE_INFO_TYPE_FILE".to_string()
            },
        };
        let items = vec![
            item("manuals", true),
            item("router-setup.txt", false),
            item("new.pdf", false),
            item("guide.txt", false),
        ];
        let mut states = HashMap::from([("router-setup.txt".to_string(), SyncState::OutOfSync)]);

        apply_derived_states(
            &items,
            Some("manuals/"),
            &mut states,
            &HashMap::new(),
            &ctx(&index, &st),
        );

        // FileInfo state kept; listed file derived; unlisted file defaulted
        assert_eq!(states.get("router-setup.txt"), Some(&SyncState::OutOfSync));
        assert_eq!(states.get("new.pdf"), Some(&SyncState::RemoteOnly));
        assert_eq!(states.get("guide.txt"), Some(&SyncState::Synced));

        let direct = HashMap::from([("manuals".to_string(), SyncState::Synced)]);
        let mut root = direct.clone();
        apply_derived_states(&items[..1], None, &mut root, &direct, &ctx(&index, &st));
        assert_eq!(root.get("manuals"), Some(&SyncState::RemoteOnly));
    }

    fn dir(name: &str) -> BrowseItem {
        BrowseItem {
            name: name.to_string(),
            size: 0,
            mod_time: String::new(),
            item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
        }
    }

    #[test]
    fn test_derived_dir_state_clears_when_index_no_longer_lists_it() {
        let index = index_with_need(&need(&[], &[], &[]), 2);
        let st = status("idle", 2, 0, 0);
        let items = vec![dir("with_direct"), dir("derived_only")];
        let direct = HashMap::from([("with_direct".to_string(), SyncState::Synced)]);
        let mut shown = HashMap::from([
            ("with_direct".to_string(), SyncState::RemoteOnly),
            ("derived_only".to_string(), SyncState::LocalOnly),
        ]);

        apply_derived_states(&items, None, &mut shown, &direct, &ctx(&index, &st));

        assert_eq!(shown.get("with_direct"), Some(&SyncState::Synced));
        assert_eq!(shown.get("derived_only"), Some(&SyncState::Synced));
    }

    #[test]
    fn test_derived_dir_falls_back_to_direct_when_nothing_derivable() {
        // Syncing folder, nothing listed under the dirs: no derived state
        let index = index_with_need(&need(&[], &["other.mkv"], &[]), 2);
        let st = status("syncing", 2, 1, 0);
        let items = vec![dir("known"), dir("unknown"), dir("ignored")];
        let direct = HashMap::from([("known".to_string(), SyncState::Synced)]);
        let mut shown = HashMap::from([
            ("known".to_string(), SyncState::RemoteOnly),
            ("unknown".to_string(), SyncState::RemoteOnly),
            ("ignored".to_string(), SyncState::Ignored),
        ]);

        apply_derived_states(&items, None, &mut shown, &direct, &ctx(&index, &st));

        assert_eq!(shown.get("known"), Some(&SyncState::Synced));
        // Nothing better known: keep what is shown rather than blanking it
        assert_eq!(shown.get("unknown"), Some(&SyncState::RemoteOnly));
        // An Ignored set by a user action stays until FileInfo says otherwise
        assert_eq!(shown.get("ignored"), Some(&SyncState::Ignored));
    }

    // ========================================
    // NEED FETCH PLAN
    // ========================================

    fn fetch(at: Instant, sequence: u64) -> Option<NeedFetch> {
        Some(NeedFetch::new(at, sequence))
    }

    fn failed_fetch(at: Instant, sequence: u64) -> Option<NeedFetch> {
        Some(NeedFetch {
            failed: true,
            ..NeedFetch::new(at, sequence)
        })
    }

    #[test]
    fn test_plan_fetches_on_first_poll() {
        let now = Instant::now();
        for state in ["idle", "syncing", "sync-preparing"] {
            assert!(need_fetch_plan(None, state, None, 1, now).fetch, "{state}");
        }
    }

    #[test]
    fn test_plan_fetches_on_transition_into_syncing() {
        let now = Instant::now();
        assert!(need_fetch_plan(Some("idle"), "syncing", fetch(now, 1), 1, now).fetch);
        assert!(need_fetch_plan(Some("sync-preparing"), "syncing", fetch(now, 1), 1, now).fetch);
    }

    #[test]
    fn test_plan_throttles_while_syncing_even_if_sequence_changes() {
        let now = Instant::now();
        let recent = now - Duration::from_secs(1);
        let plan = need_fetch_plan(Some("syncing"), "syncing", fetch(recent, 1), 2, now);
        assert_eq!(plan, NeedFetchPlan::default());
    }

    #[test]
    fn test_plan_refreshes_syncing_after_interval() {
        let now = Instant::now();
        let stale = now - SYNCING_FILES_REFRESH_INTERVAL - Duration::from_secs(1);
        assert!(need_fetch_plan(Some("syncing"), "syncing", fetch(stale, 1), 1, now).fetch);
    }

    #[test]
    fn test_plan_leaving_syncing_clears_and_refetches() {
        let now = Instant::now();
        for state in ["idle", "scanning", "error"] {
            let plan = need_fetch_plan(Some("syncing"), state, fetch(now, 1), 1, now);
            assert_eq!(
                plan,
                NeedFetchPlan {
                    fetch: true,
                    clear_syncing: true,
                    discard_pending: true,
                },
                "{state}"
            );
        }
    }

    #[test]
    fn test_plan_idle_refetches_only_on_sequence_change() {
        let now = Instant::now();
        let plan = need_fetch_plan(Some("idle"), "idle", fetch(now, 4), 4, now);
        assert!(!plan.fetch);
        assert!(plan.clear_syncing);
        assert!(!plan.discard_pending);
        assert!(need_fetch_plan(Some("idle"), "idle", fetch(now, 4), 5, now).fetch);
        assert!(need_fetch_plan(Some("idle"), "idle", None, 4, now).fetch);
    }

    #[test]
    fn test_plan_backs_off_after_failed_fetch() {
        let now = Instant::now();
        let recent = now - Duration::from_secs(2);
        let stale = now - SYNCING_FILES_REFRESH_INTERVAL - Duration::from_secs(1);
        assert!(
            !need_fetch_plan(Some("syncing"), "syncing", failed_fetch(recent, 1), 1, now).fetch
        );
        assert!(!need_fetch_plan(Some("idle"), "idle", failed_fetch(recent, 1), 2, now).fetch);
        assert!(need_fetch_plan(Some("syncing"), "syncing", failed_fetch(stale, 1), 1, now).fetch);
        assert!(need_fetch_plan(Some("idle"), "idle", failed_fetch(stale, 1), 1, now).fetch);
    }

    #[test]
    fn test_clear_index_need_forgets_need_part_only() {
        let mut index = documents_index(3);
        set_index_need(
            &mut index,
            &need(&[], &["d/q.mkv"], &[]),
            3,
            true,
            Some(vec![]),
        );
        clear_index_need(&mut index);
        let st = status("idle", 3, 0, 3);
        assert_eq!(
            derived_state("d/q.mkv", false, &ctx(&index, &st), None),
            None
        );
        assert_eq!(derived_state("d", true, &ctx(&index, &st), None), None);
        assert_eq!(
            derived_state("manuals", true, &ctx(&index, &st), None),
            Some(SyncState::RemoteOnly)
        );
    }

    #[test]
    fn test_need_index_wanted_when_open_or_syncing() {
        assert!(need_index_wanted(true, "idle"));
        assert!(need_index_wanted(false, "syncing"));
        assert!(need_index_wanted(false, "sync-preparing"));
        assert!(!need_index_wanted(false, "idle"));
        assert!(!need_index_wanted(false, "scanning"));
    }

    #[test]
    fn test_local_changed_fetch_due_rules() {
        let now = Instant::now();
        let recent = now - Duration::from_secs(2);
        let stale = now - SYNCING_FILES_REFRESH_INTERVAL - Duration::from_secs(1);
        assert!(local_changed_fetch_due(None, None, 1, now));
        assert!(!local_changed_fetch_due(Some(1), None, 1, now));
        assert!(local_changed_fetch_due(Some(1), None, 2, now));
        assert!(!local_changed_fetch_due(Some(1), Some(recent), 1, now));
        assert!(!local_changed_fetch_due(Some(1), Some(recent), 2, now));
        assert!(local_changed_fetch_due(Some(1), Some(stale), 1, now));
    }

    #[test]
    fn test_accept_need_response_rules() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(10);
        let t2 = t0 + Duration::from_millis(20);
        let latest = NeedFetch {
            floor: t0,
            ..NeedFetch::new(t2, 1)
        };
        // Superseded but newer than the index
        assert!(accept_need_response(Some(&latest), None, t1));
        assert!(accept_need_response(Some(&latest), Some(t0), t1));
        // Not newer than what the index holds
        assert!(!accept_need_response(Some(&latest), Some(t1), t1));
        assert!(!accept_need_response(Some(&latest), Some(t2), t1));
        // Sent before the floor, or no fetch recorded
        let floored = NeedFetch {
            floor: t2,
            ..latest
        };
        assert!(!accept_need_response(Some(&floored), None, t1));
        assert!(accept_need_response(Some(&floored), None, t2));
        assert!(!accept_need_response(None, None, t1));
    }

    #[test]
    fn test_plan_keeps_during_sync_preparing() {
        let now = Instant::now();
        let plan = need_fetch_plan(Some("syncing"), "sync-preparing", fetch(now, 1), 2, now);
        assert_eq!(plan, NeedFetchPlan::default());
    }

    // ========================================
    // LOCAL DELETIONS
    // ========================================

    #[test]
    fn test_is_locally_deleted() {
        let index = documents_index(3);
        assert!(is_locally_deleted(
            Some(&index),
            None,
            "manuals/router-setup.txt"
        ));
        assert!(!is_locally_deleted(
            Some(&index),
            None,
            "recipes/pancakes.md"
        ));
        assert!(!is_locally_deleted(
            Some(&index),
            None,
            "manuals/printer.txt"
        ));
        assert!(!is_locally_deleted(None, None, "manuals/router-setup.txt"));
    }

    #[test]
    fn test_file_being_downloaded_is_not_locally_deleted() {
        // Mid-restore, Syncthing lists the file both as a local deletion and as needed
        let path = "manuals/router-setup.txt";
        let mut queued = documents_index(3);
        set_index_need(&mut queued, &need(&[], &[path], &[]), 3, true, None);
        assert!(!is_locally_deleted(Some(&queued), None, path));

        let syncing: HashSet<String> = [path.to_string()].into();
        assert!(!is_locally_deleted(
            Some(&documents_index(3)),
            Some(&syncing),
            path
        ));
    }

    #[test]
    fn test_locally_deleted_message_points_at_revert() {
        let message = locally_deleted_message();
        assert!(
            message.starts_with("Deleted on this device."),
            "{}",
            message
        );
        assert!(message.contains("u to restore this file"), "{}", message);
        assert!(
            message.contains("R to revert all local changes"),
            "{}",
            message
        );
    }
}
