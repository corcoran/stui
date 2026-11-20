//! Paging of the full /rest/db/need list behind the out-of-sync filter.
//!
//! Page 1 is fetched as soon as the filter asks; later pages follow one at a time at
//! low priority until the list is exhausted, the folder status total is reached, or the page cap
//! is hit.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::model::syncthing::NeedPaging;
use crate::model::types::PendingSelection;

/// Entries requested per /rest/db/need page.
pub const NEED_LIST_PER_PAGE: u32 = 1000;

/// Most pages fetched for one folder (100k entries at the default page size). Pages are parsed
/// and cached on the UI thread; past the cap the indicator reports the list as partial.
pub const NEED_LIST_MAX_PAGES: u32 = 100;

/// Minimum gap between filter re-applies while a walk is running
pub const NEED_LIST_REAPPLY_INTERVAL: Duration = Duration::from_millis(500);

/// Minimum gap between TTL refreshes of a walk's cached rows
pub const NEED_LIST_TOUCH_INTERVAL: Duration = Duration::from_secs(10);

/// Minimum time from one walk's start to the next restart of the same folder
pub const NEED_LIST_RESTART_INTERVAL: Duration = Duration::from_secs(5);

/// Restart interval for a walk that stopped short: re-walking up to the page cap is costly
pub const NEED_LIST_PARTIAL_RESTART_INTERVAL: Duration = Duration::from_secs(30);

/// What to do after a need page arrives
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextNeedPage {
    /// The list is complete
    Done,
    /// Request this page next
    Fetch(u32),
    /// Entries remain but the page cap is reached
    Capped,
}

/// "loaded X of Y" for the status bar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedPagingProgress {
    pub loaded: u64,
    pub total: u64,
    /// Stopped before the end of the list
    pub partial: bool,
}

/// Whether a need page tagged `generation`/`sequence` belongs to the folder's current walk.
/// Responses for a walk that was stopped or restarted are dropped.
pub fn accept_need_page(paging: Option<&NeedPaging>, generation: u64, sequence: u64) -> bool {
    paging.is_some_and(|p| p.generation == generation && p.sequence == sequence)
}

/// Record that `page` arrived with `returned` entries, `new_distinct` of them not yet cached by
/// this walk (`total` from the latest folder status), and decide what comes next. A short page
/// means the server has nothing more, whatever the status total says.
pub fn advance_need_paging(
    paging: &mut NeedPaging,
    page: u32,
    returned: u64,
    new_distinct: u64,
    total: u64,
    perpage: u32,
    max_pages: u32,
) -> NextNeedPage {
    paging.loaded += new_distinct;
    paging.last_page = page;
    paging.total = total;
    if returned < u64::from(perpage) || paging.loaded >= total {
        paging.finished = true;
        return NextNeedPage::Done;
    }
    if page >= max_pages {
        paging.finished = true;
        paging.partial = true;
        return NextNeedPage::Capped;
    }
    NextNeedPage::Fetch(page + 1)
}

/// Whether a page should re-apply the filter: always when the walk
/// ends, otherwise at most every `NEED_LIST_REAPPLY_INTERVAL`
pub fn should_reapply_need_page(
    last_apply: Option<Instant>,
    now: Instant,
    next: NextNeedPage,
) -> bool {
    !matches!(next, NextNeedPage::Fetch(_))
        || last_apply.is_none_or(|at| now.duration_since(at) >= NEED_LIST_REAPPLY_INTERVAL)
}

/// Whether the walk's cached rows are due a TTL refresh
pub fn should_touch_need_rows(last_touch: Option<Instant>, now: Instant) -> bool {
    last_touch.is_none_or(|at| now.duration_since(at) >= NEED_LIST_TOUCH_INTERVAL)
}

/// Whether a finished walk should start over: the list changed (marked dirty, or the folder
/// moved to another sequence) and the last start is at least `NEED_LIST_RESTART_INTERVAL` ago
/// (`NEED_LIST_PARTIAL_RESTART_INTERVAL` for a partial walk).
/// A running walk never restarts; it would discard its progress.
pub fn should_restart_need_walk(paging: &NeedPaging, current_sequence: u64, now: Instant) -> bool {
    let interval = if paging.partial {
        NEED_LIST_PARTIAL_RESTART_INTERVAL
    } else {
        NEED_LIST_RESTART_INTERVAL
    };
    paging.finished
        && (paging.dirty || paging.sequence != current_sequence)
        && now.duration_since(paging.started_at) >= interval
}

/// Progress to show while later pages are loading, or after the walk stopped short. None
/// before page 1 arrives and once the list is complete.
pub fn need_paging_progress(paging: &NeedPaging) -> Option<NeedPagingProgress> {
    let complete = paging.finished && !paging.partial;
    (paging.last_page > 0 && !complete).then_some(NeedPagingProgress {
        loaded: paging.loaded,
        total: paging.total,
        partial: paging.partial,
    })
}

/// Status bar suffix for the filter label, e.g. "loading 2000/5400"
pub fn format_need_paging_progress(progress: &NeedPagingProgress) -> String {
    let verb = if progress.partial {
        "partial"
    } else {
        "loading"
    };
    format!("{} {}/{}", verb, progress.loaded, progress.total)
}

/// Every directory prefix (ending in '/') of `paths`. A directory holds an out-of-sync
/// descendant exactly when its prefix is in the set.
pub fn out_of_sync_dir_prefixes<'a>(paths: impl Iterator<Item = &'a String>) -> HashSet<String> {
    let mut prefixes = HashSet::new();
    for path in paths {
        for (i, _) in path.match_indices('/') {
            prefixes.insert(path[..=i].to_string());
        }
    }
    prefixes
}

/// Selection for a filtered level. `previous` is the selected name before filtering and
/// `pending` the selection still waiting for its item; returns the index to select and the
/// pending selection to keep.
pub fn select_after_filter(
    previous: Option<&str>,
    pending: Option<&PendingSelection>,
    prefix: Option<&str>,
    names: &[&str],
) -> (Option<usize>, Option<PendingSelection>) {
    let position = |name: &str| names.iter().position(|n| *n == name);
    if let Some(p) = pending
        && p.prefix.as_deref() == prefix
        && previous == Some(p.fallback.as_str())
    {
        if let Some(index) = position(&p.wanted) {
            return (Some(index), None);
        }
        let index = position(&p.fallback).unwrap_or(0);
        let fallback = names.get(index).map(|n| n.to_string()).unwrap_or_default();
        return (
            Some(index),
            Some(PendingSelection {
                fallback,
                ..p.clone()
            }),
        );
    }
    match previous {
        Some(name) => match position(name) {
            Some(index) => (Some(index), None),
            None => (
                Some(0),
                names.first().map(|first| PendingSelection {
                    prefix: prefix.map(str::to_string),
                    wanted: name.to_string(),
                    fallback: first.to_string(),
                }),
            ),
        },
        None => (Some(0), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paging(total: u64) -> NeedPaging {
        NeedPaging::new(7, 42, total, Instant::now())
    }

    /// `advance_need_paging` for a page whose entries are all new to the walk
    fn advance(
        p: &mut NeedPaging,
        page: u32,
        returned: u64,
        total: u64,
        perpage: u32,
        max: u32,
    ) -> NextNeedPage {
        advance_need_paging(p, page, returned, returned, total, perpage, max)
    }

    // ========================================
    // NEXT PAGE DECISION
    // ========================================

    #[test]
    fn test_short_first_page_is_done() {
        let mut p = paging(6);
        assert_eq!(advance(&mut p, 1, 6, 6, 1000, 100), NextNeedPage::Done);
        assert_eq!(p.loaded, 6);
        assert_eq!(p.last_page, 1);
        assert!(p.finished);
        assert!(!p.partial);
    }

    #[test]
    fn test_full_first_page_reaching_total_is_done() {
        let mut p = paging(1000);
        assert_eq!(
            advance(&mut p, 1, 1000, 1000, 1000, 100),
            NextNeedPage::Done
        );
    }

    #[test]
    fn test_total_above_returned_fetches_next_page() {
        let mut p = paging(5400);
        assert_eq!(
            advance(&mut p, 1, 1000, 5400, 1000, 100),
            NextNeedPage::Fetch(2)
        );
        assert_eq!(
            advance(&mut p, 2, 1000, 5400, 1000, 100),
            NextNeedPage::Fetch(3)
        );
        assert_eq!(p.loaded, 2000);
        assert_eq!(p.last_page, 2);
        assert!(!p.finished);
    }

    #[test]
    fn test_short_later_page_is_done_even_below_total() {
        // Items finished syncing between pages: the server ran out before the status total
        let mut p = paging(5400);
        advance(&mut p, 1, 1000, 5400, 1000, 100);
        assert_eq!(advance(&mut p, 2, 300, 5400, 1000, 100), NextNeedPage::Done);
    }

    #[test]
    fn test_total_refreshed_from_status() {
        let mut p = paging(5400);
        advance(&mut p, 1, 1000, 2000, 1000, 100);
        assert_eq!(p.total, 2000);
        assert_eq!(
            advance(&mut p, 2, 1000, 2000, 1000, 100),
            NextNeedPage::Done
        );
    }

    #[test]
    fn test_duplicate_entries_do_not_end_the_walk() {
        // The list shifted between pages: page 2 repeats an entry of page 1
        let mut p = paging(4);
        advance_need_paging(&mut p, 1, 2, 2, 4, 2, 100);
        assert_eq!(
            advance_need_paging(&mut p, 2, 2, 1, 4, 2, 100),
            NextNeedPage::Fetch(3)
        );
        assert_eq!(p.loaded, 3);
    }

    #[test]
    fn test_cap_stops_paging() {
        let mut p = paging(10_000);
        advance(&mut p, 1, 2, 10_000, 2, 3);
        advance(&mut p, 2, 2, 10_000, 2, 3);
        assert_eq!(advance(&mut p, 3, 2, 10_000, 2, 3), NextNeedPage::Capped);
        assert!(p.partial);
        assert!(p.finished);
    }

    #[test]
    fn test_cap_not_reported_when_last_page_completes_list() {
        let mut p = paging(6);
        advance(&mut p, 1, 2, 6, 2, 3);
        advance(&mut p, 2, 2, 6, 2, 3);
        assert_eq!(advance(&mut p, 3, 2, 6, 2, 3), NextNeedPage::Done);
        assert!(!p.partial);
    }

    // ========================================
    // STALENESS
    // ========================================

    #[test]
    fn test_accepts_current_fetch() {
        let p = paging(10);
        assert!(accept_need_page(Some(&p), 7, 42));
    }

    #[test]
    fn test_drops_other_generation() {
        let p = paging(10);
        assert!(!accept_need_page(Some(&p), 6, 42));
    }

    #[test]
    fn test_drops_other_sequence() {
        let p = paging(10);
        assert!(!accept_need_page(Some(&p), 7, 41));
    }

    #[test]
    fn test_drops_when_no_fetch() {
        assert!(!accept_need_page(None, 7, 42));
    }

    // ========================================
    // THROTTLING
    // ========================================

    #[test]
    fn test_reapply_first_page_of_walk() {
        assert!(should_reapply_need_page(
            None,
            Instant::now(),
            NextNeedPage::Fetch(2)
        ));
    }

    #[test]
    fn test_reapply_throttled_mid_walk() {
        let now = Instant::now();
        let just_now = now - Duration::from_millis(100);
        assert!(!should_reapply_need_page(
            Some(just_now),
            now,
            NextNeedPage::Fetch(3)
        ));
        let a_while_ago = now - Duration::from_millis(600);
        assert!(should_reapply_need_page(
            Some(a_while_ago),
            now,
            NextNeedPage::Fetch(3)
        ));
    }

    #[test]
    fn test_reapply_always_when_walk_ends() {
        let now = Instant::now();
        let just_now = now - Duration::from_millis(100);
        assert!(should_reapply_need_page(
            Some(just_now),
            now,
            NextNeedPage::Done
        ));
        assert!(should_reapply_need_page(
            Some(just_now),
            now,
            NextNeedPage::Capped
        ));
    }

    #[test]
    fn test_touch_at_most_every_interval() {
        let now = Instant::now();
        assert!(should_touch_need_rows(None, now));
        assert!(!should_touch_need_rows(
            Some(now - Duration::from_secs(3)),
            now
        ));
        assert!(should_touch_need_rows(
            Some(now - Duration::from_secs(11)),
            now
        ));
    }

    // ========================================
    // RESTARTS
    // ========================================

    fn finished(started_secs_ago: u64) -> (NeedPaging, Instant) {
        let now = Instant::now();
        let mut p = NeedPaging::new(7, 42, 6, now - Duration::from_secs(started_secs_ago));
        advance(&mut p, 1, 6, 6, 1000, 100);
        (p, now)
    }

    #[test]
    fn test_clean_finished_walk_at_same_sequence_does_not_restart() {
        let (p, now) = finished(60);
        assert!(!should_restart_need_walk(&p, 42, now));
    }

    #[test]
    fn test_finished_walk_restarts_on_new_sequence() {
        let (p, now) = finished(60);
        assert!(should_restart_need_walk(&p, 43, now));
    }

    #[test]
    fn test_dirty_finished_walk_restarts() {
        let (mut p, now) = finished(60);
        p.dirty = true;
        assert!(should_restart_need_walk(&p, 42, now));
    }

    #[test]
    fn test_restart_waits_for_interval() {
        let (mut p, now) = finished(1);
        p.dirty = true;
        assert!(!should_restart_need_walk(&p, 43, now));
    }

    #[test]
    fn test_partial_walk_restarts_less_often() {
        let now = Instant::now();
        let mut p = NeedPaging::new(7, 42, 10_000, now - Duration::from_secs(10));
        advance(&mut p, 1, 2, 10_000, 2, 1);
        assert!(p.partial);
        assert!(!should_restart_need_walk(&p, 43, now));

        p.started_at = now - Duration::from_secs(60);
        assert!(should_restart_need_walk(&p, 43, now));
    }

    #[test]
    fn test_running_walk_never_restarts() {
        let now = Instant::now();
        let mut p = NeedPaging::new(7, 42, 5000, now - Duration::from_secs(60));
        advance(&mut p, 1, 1000, 5000, 1000, 100);
        p.dirty = true;
        assert!(!should_restart_need_walk(&p, 43, now));
    }

    // ========================================
    // PROGRESS
    // ========================================

    #[test]
    fn test_no_progress_before_first_page() {
        assert_eq!(need_paging_progress(&paging(5400)), None);
    }

    #[test]
    fn test_progress_mid_paging() {
        let mut p = paging(5400);
        advance(&mut p, 1, 1000, 5400, 1000, 100);
        advance(&mut p, 2, 1000, 5400, 1000, 100);
        assert_eq!(
            need_paging_progress(&p),
            Some(NeedPagingProgress {
                loaded: 2000,
                total: 5400,
                partial: false
            })
        );
    }

    #[test]
    fn test_no_progress_once_complete() {
        let mut p = paging(6);
        advance(&mut p, 1, 6, 6, 1000, 100);
        assert_eq!(need_paging_progress(&p), None);
    }

    #[test]
    fn test_progress_partial_after_cap() {
        let mut p = paging(10_000);
        advance(&mut p, 1, 2, 10_000, 2, 1);
        assert_eq!(
            need_paging_progress(&p),
            Some(NeedPagingProgress {
                loaded: 2,
                total: 10_000,
                partial: true
            })
        );
    }

    #[test]
    fn test_format_loading() {
        let progress = NeedPagingProgress {
            loaded: 2000,
            total: 5400,
            partial: false,
        };
        assert_eq!(format_need_paging_progress(&progress), "loading 2000/5400");
    }

    #[test]
    fn test_format_partial() {
        let progress = NeedPagingProgress {
            loaded: 100_000,
            total: 250_000,
            partial: true,
        };
        assert_eq!(
            format_need_paging_progress(&progress),
            "partial 100000/250000"
        );
    }

    // ========================================
    // DIRECTORY MEMBERSHIP
    // ========================================

    #[test]
    fn test_dir_prefixes_match_starts_with() {
        let paths: Vec<String> = ["a/b/c.txt", "a/d/", "top.txt", "x/y/z/w", "deep/one/"]
            .iter()
            .map(|p| p.to_string())
            .collect();
        let prefixes = out_of_sync_dir_prefixes(paths.iter());
        for dir in [
            "a/",
            "a/b/",
            "a/d/",
            "a/e/",
            "top.txt/",
            "x/",
            "x/y/",
            "x/y/z/",
            "x/y/z/w/",
            "deep/",
            "deep/one/",
            "b/",
            "a/b/c.txt/",
        ] {
            let expected = paths.iter().any(|p| p.starts_with(dir));
            assert_eq!(prefixes.contains(dir), expected, "dir {}", dir);
        }
    }

    // ========================================
    // SELECTION
    // ========================================

    fn pending(prefix: Option<&str>, wanted: &str, fallback: &str) -> PendingSelection {
        PendingSelection {
            prefix: prefix.map(str::to_string),
            wanted: wanted.to_string(),
            fallback: fallback.to_string(),
        }
    }

    #[test]
    fn test_selection_kept_when_item_present() {
        assert_eq!(
            select_after_filter(Some("b"), None, None, &["a", "b"]),
            (Some(1), None)
        );
    }

    #[test]
    fn test_missing_selection_falls_back_and_waits() {
        assert_eq!(
            select_after_filter(Some("c"), None, None, &["a", "b"]),
            (Some(0), Some(pending(None, "c", "a")))
        );
    }

    #[test]
    fn test_pending_selection_restored_when_item_appears() {
        let p = pending(None, "c", "a");
        assert_eq!(
            select_after_filter(Some("a"), Some(&p), None, &["a", "b", "c"]),
            (Some(2), None)
        );
    }

    #[test]
    fn test_pending_selection_kept_while_item_missing() {
        let p = pending(None, "c", "a");
        assert_eq!(
            select_after_filter(Some("a"), Some(&p), None, &["a", "b"]),
            (Some(0), Some(p.clone()))
        );
    }

    #[test]
    fn test_user_move_drops_pending_selection() {
        let p = pending(None, "c", "a");
        assert_eq!(
            select_after_filter(Some("b"), Some(&p), None, &["a", "b", "c"]),
            (Some(1), None)
        );
    }

    #[test]
    fn test_other_prefix_drops_pending_selection() {
        let p = pending(None, "c", "a");
        assert_eq!(
            select_after_filter(Some("x"), Some(&p), Some("sub/"), &["x", "c"]),
            (Some(0), None)
        );
    }
}
