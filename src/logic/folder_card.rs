//! Folder card formatting logic
//!
//! Pure functions for calculating folder card states and formatting card data

use crate::api::{Folder, FolderStatus};

/// Card state enum for visual rendering
#[derive(Debug, Clone, PartialEq)]
pub enum FolderCardState {
    /// Fully synced, no pending changes
    Synced,
    /// Out of sync (remote_needed files, local_changes files)
    OutOfSync {
        remote_needed: u64,
        local_changes: u64,
    },
    /// Currently syncing (remote_needed files, local_changes files)
    Syncing {
        remote_needed: u64,
        local_changes: u64,
    },
    /// `sync-preparing` or `sync-waiting`
    Preparing,
    /// `scanning`
    Scanning,
    /// `scan-waiting`
    WaitingToScan,
    /// `cleaning` or `clean-waiting`
    Cleaning,
    /// Folder is paused
    Paused,
    /// Folder has errors
    Error { errors: u64 },
    /// Status not yet loaded
    Loading,
}

/// Calculate the card state for a folder
pub fn calculate_folder_card_state(
    folder: &Folder,
    status: Option<&FolderStatus>,
) -> FolderCardState {
    if folder.paused {
        return FolderCardState::Paused;
    }

    let Some(status) = status else {
        return FolderCardState::Loading;
    };

    if status.errors > 0 {
        return FolderCardState::Error {
            errors: status.errors,
        };
    }

    let remote_needed = status.need_total_items;
    let local_changes = status.receive_only_total_items;

    match status.state.as_str() {
        "syncing" => {
            return FolderCardState::Syncing {
                remote_needed,
                local_changes,
            };
        }
        "sync-preparing" | "sync-waiting" => return FolderCardState::Preparing,
        "scanning" => return FolderCardState::Scanning,
        "scan-waiting" => return FolderCardState::WaitingToScan,
        "cleaning" | "clean-waiting" => return FolderCardState::Cleaning,
        _ => {}
    }

    if remote_needed > 0 || local_changes > 0 {
        FolderCardState::OutOfSync {
            remote_needed,
            local_changes,
        }
    } else {
        FolderCardState::Synced
    }
}

/// Format folder type to user-friendly string
pub fn format_folder_type(folder_type: &str) -> String {
    match folder_type {
        "sendonly" => "Send Only".to_string(),
        "sendreceive" => "Send & Receive".to_string(),
        "receiveonly" => "Receive Only".to_string(),
        _ => folder_type.to_string(),
    }
}

/// Format byte size to human-readable
pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, units[unit_index])
    }
}

/// Format file count to human-readable
pub fn format_file_count(count: u64) -> String {
    if count == 0 {
        return "0 files".to_string();
    }

    if count < 1000 {
        format!("{} files", count)
    } else if count < 1_000_000 {
        format!("{:.1}K files", count as f64 / 1000.0)
    } else {
        format!("{:.1}M files", count as f64 / 1_000_000.0)
    }
}

/// Percent of the folder's bytes already in sync while it syncs: floored and capped at
/// 99, since 100% would read as done while Syncthing still reports `syncing`.
pub fn sync_percent(global_bytes: u64, need_bytes: u64) -> u8 {
    if global_bytes == 0 || need_bytes > global_bytes {
        return 0;
    }
    let done = (global_bytes - need_bytes) as u128 * 100 / global_bytes as u128;
    done.min(99) as u8
}

/// Glyph for a folder type: ⇅ Send & Receive, ⇡ Send Only, ⇣ Receive Only
pub fn type_glyph(folder_type: &str) -> &'static str {
    match folder_type {
        "sendreceive" => "⇅",
        "sendonly" => "⇡",
        "receiveonly" => "⇣",
        _ => "?",
    }
}

/// How much detail a folder card shows; chosen per card from the pane width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardTier {
    Wide,
    Medium,
    Narrow,
}

/// Text of one folder card. `detail` and `pending` exclude their 5-column indent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLines {
    pub tier: CardTier,
    pub name: String,
    /// Right-aligned on line 1
    pub status: Option<String>,
    pub status_is_error: bool,
    /// Line 2: type, size and file count
    pub detail: Option<String>,
    /// Line 3: what's left to sync
    pub pending: Option<String>,
}

const CARD_INDENT: usize = 5;

fn text_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

/// `text` cut to `max` display columns, ending in `…` when cut.
fn truncate_to_width(text: &str, max: usize) -> String {
    if text_width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > max - 1 {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

fn card_status(
    state: &FolderCardState,
    status: Option<&FolderStatus>,
    tier: CardTier,
) -> (Option<String>, bool) {
    let wide = tier == CardTier::Wide;
    match state {
        FolderCardState::Syncing { .. } => {
            let pct = status.map_or(0, |s| sync_percent(s.global_bytes, s.need_bytes));
            let text = if wide {
                format!("Syncing {}%", pct)
            } else {
                format!("{}%", pct)
            };
            (Some(text), false)
        }
        FolderCardState::Error { errors } => {
            let text = match tier {
                CardTier::Narrow => errors.to_string(),
                _ if *errors == 1 => "1 error".to_string(),
                _ => format!("{} errors", errors),
            };
            (Some(text), true)
        }
        FolderCardState::Preparing if wide => (Some("Preparing".to_string()), false),
        FolderCardState::Scanning if wide => (Some("Scanning".to_string()), false),
        FolderCardState::WaitingToScan if wide => (Some("Waiting to scan".to_string()), false),
        FolderCardState::Cleaning if wide => (Some("Cleaning".to_string()), false),
        _ => (None, false),
    }
}

fn card_pending(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    tier: CardTier,
) -> Option<String> {
    let (FolderCardState::OutOfSync {
        remote_needed,
        local_changes,
    }
    | FolderCardState::Syncing {
        remote_needed,
        local_changes,
    }) = state
    else {
        return None;
    };
    let mut parts = Vec::new();
    if *remote_needed > 0 {
        let size = format_size(status.map_or(0, |s| s.need_bytes));
        parts.push(match tier {
            CardTier::Wide => format!("↓ {} files ({})", remote_needed, size),
            CardTier::Medium => format!("↓ {} ({})", remote_needed, size),
            CardTier::Narrow => format!("↓ {}", remote_needed),
        });
    }
    if *local_changes > 0 {
        let receive_only = folder.folder_type == "receiveonly";
        parts.push(match (tier, receive_only) {
            (CardTier::Wide, true) if *local_changes == 1 => {
                "✎ 1 file modified locally".to_string()
            }
            (CardTier::Wide, true) => format!("✎ {} files modified locally", local_changes),
            (CardTier::Medium, true) => format!("✎ {} modified", local_changes),
            (CardTier::Narrow, true) => format!("✎ {}", local_changes),
            (CardTier::Wide, false) => format!("↑ {} files", local_changes),
            (_, false) => format!("↑ {}", local_changes),
        });
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn card_detail(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    tier: CardTier,
) -> Option<String> {
    let glyph = type_glyph(&folder.folder_type);
    let type_words = format_folder_type(&folder.folder_type);
    let figures = status.map(|s| {
        (
            format_size(s.global_bytes),
            format_file_count(s.global_files),
        )
    });
    match (state, figures) {
        (FolderCardState::Paused, _) => match tier {
            CardTier::Wide => Some(format!("{} {}", glyph, type_words)),
            CardTier::Medium => Some(glyph.to_string()),
            CardTier::Narrow => None,
        },
        (_, None) => match tier {
            CardTier::Wide => Some(format!("{} {}", glyph, type_words)),
            _ => Some(glyph.to_string()),
        },
        (_, Some((size, count))) => Some(match tier {
            CardTier::Wide => format!("{} {} · {} · {}", glyph, type_words, size, count),
            CardTier::Medium => format!("{} {} · {}", glyph, size, count),
            CardTier::Narrow => size,
        }),
    }
}

fn build_card(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    tier: CardTier,
) -> CardLines {
    let (card_status_text, status_is_error) = card_status(state, status, tier);
    let pending = card_pending(folder, status, state, tier);
    let mut detail = card_detail(folder, status, state, tier);
    if tier == CardTier::Narrow && pending.is_some() {
        detail = None;
    }
    CardLines {
        tier,
        name: folder.label.clone().unwrap_or_else(|| folder.id.clone()),
        status: card_status_text,
        status_is_error,
        detail,
        pending,
    }
}

fn card_fits(card: &CardLines, inner_width: usize, icon_width: usize) -> bool {
    let line1 =
        icon_width + text_width(&card.name) + card.status.as_ref().map_or(0, |s| 1 + text_width(s));
    let fits_indented = |line: &Option<String>| {
        line.as_ref()
            .is_none_or(|l| CARD_INDENT + text_width(l) <= inner_width)
    };
    line1 <= inner_width && fits_indented(&card.detail) && fits_indented(&card.pending)
}

/// The card text for a pane `inner_width` columns wide, using the widest tier that fits.
/// `icon_width` is the display width of the icon prefix drawn before the name.
pub fn card_lines(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    inner_width: u16,
    icon_width: u16,
) -> CardLines {
    let (width, icon) = (inner_width as usize, icon_width as usize);
    for tier in [CardTier::Wide, CardTier::Medium] {
        let card = build_card(folder, status, state, tier);
        if card_fits(&card, width, icon) {
            return card;
        }
    }

    let mut card = build_card(folder, status, state, CardTier::Narrow);
    if width <= icon {
        return CardLines {
            tier: CardTier::Narrow,
            name: String::new(),
            status: None,
            status_is_error: false,
            detail: None,
            pending: None,
        };
    }
    let room = width - icon;
    let status_room = card.status.as_ref().map_or(0, |s| 1 + text_width(s));
    if status_room >= room {
        card.status = None;
        card.status_is_error = false;
    }
    let status_room = card.status.as_ref().map_or(0, |s| 1 + text_width(s));
    card.name = truncate_to_width(&card.name, room - status_room);
    let room_indented = width.saturating_sub(CARD_INDENT);
    card.detail = card
        .detail
        .filter(|_| room_indented > 0)
        .map(|d| truncate_to_width(&d, room_indented));
    card.pending = card
        .pending
        .filter(|_| room_indented > 0)
        .map(|p| truncate_to_width(&p, room_indented));
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // CARD STATE CALCULATION
    // ========================================

    #[test]
    fn test_calculate_folder_card_state_paused() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: true,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let state = calculate_folder_card_state(&folder, None);
        assert_eq!(state, FolderCardState::Paused);
    }

    #[test]
    fn test_calculate_folder_card_state_loading() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let state = calculate_folder_card_state(&folder, None);
        assert_eq!(state, FolderCardState::Loading);
    }

    #[test]
    fn test_calculate_folder_card_state_synced() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let status = FolderStatus {
            state: "idle".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 0,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 0,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(state, FolderCardState::Synced);
    }

    #[test]
    fn test_calculate_folder_card_state_out_of_sync() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let mut status = FolderStatus {
            state: "idle".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 1024,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 5,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::OutOfSync {
                remote_needed: 5,
                local_changes: 0
            }
        );

        // Test with local changes
        status.need_total_items = 0;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::OutOfSync {
                remote_needed: 0,
                local_changes: 3
            }
        );
    }

    #[test]
    fn test_calculate_folder_card_state_syncing_with_counts() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        // Test syncing with remote needed files
        let mut status = FolderStatus {
            state: "syncing".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 1024,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 5,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 0,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 0
            }
        );

        // Test syncing with local changes
        status.need_total_items = 0;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 0,
                local_changes: 3
            }
        );

        // Test syncing with both
        status.need_total_items = 5;
        status.receive_only_total_items = 3;
        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(
            state,
            FolderCardState::Syncing {
                remote_needed: 5,
                local_changes: 3
            }
        );
    }

    #[test]
    fn test_calculate_folder_card_state_sync_preparing_with_counts() {
        let folder = Folder {
            id: "test".to_string(),
            label: None,
            path: "/test".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        };

        let status = FolderStatus {
            state: "sync-preparing".to_string(),
            sequence: 0,
            global_bytes: 0,
            global_deleted: 0,
            global_directories: 0,
            global_files: 10,
            global_symlinks: 0,
            global_total_items: 10,
            in_sync_bytes: 0,
            in_sync_files: 0,
            local_bytes: 0,
            local_deleted: 0,
            local_directories: 0,
            local_files: 10,
            local_symlinks: 0,
            local_total_items: 10,
            need_bytes: 2048,
            need_deletes: 0,
            need_directories: 0,
            need_files: 0,
            need_symlinks: 0,
            need_total_items: 8,
            receive_only_changed_bytes: 0,
            receive_only_changed_deletes: 0,
            receive_only_changed_directories: 0,
            receive_only_changed_files: 0,
            receive_only_changed_symlinks: 0,
            receive_only_total_items: 2,
            errors: 0,
        };

        let state = calculate_folder_card_state(&folder, Some(&status));
        assert_eq!(state, FolderCardState::Preparing);
    }

    // ========================================
    // FORMATTING FUNCTIONS
    // ========================================

    #[test]
    fn test_format_folder_type() {
        assert_eq!(format_folder_type("sendonly"), "Send Only");
        assert_eq!(format_folder_type("sendreceive"), "Send & Receive");
        assert_eq!(format_folder_type("receiveonly"), "Receive Only");
        assert_eq!(format_folder_type("unknown"), "unknown");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_file_count() {
        assert_eq!(format_file_count(0), "0 files");
        assert_eq!(format_file_count(1), "1 files");
        assert_eq!(format_file_count(500), "500 files");
        assert_eq!(format_file_count(1500), "1.5K files");
        assert_eq!(format_file_count(1_500_000), "1.5M files");
    }

    // ========================================
    // CARD LINES
    // ========================================

    const MIB: u64 = 1024 * 1024;

    fn st(state: &str) -> FolderStatus {
        serde_json::from_value(serde_json::json!({
            "state": state, "sequence": 0, "globalBytes": 0, "globalDeleted": 0,
            "globalDirectories": 0, "globalFiles": 0, "globalSymlinks": 0, "globalTotalItems": 0,
            "inSyncBytes": 0, "inSyncFiles": 0, "localBytes": 0, "localDeleted": 0,
            "localDirectories": 0, "localFiles": 0, "localSymlinks": 0, "localTotalItems": 0,
            "needBytes": 0, "needDeletes": 0, "needDirectories": 0, "needFiles": 0,
            "needSymlinks": 0, "needTotalItems": 0, "receiveOnlyChangedBytes": 0,
            "receiveOnlyChangedDeletes": 0, "receiveOnlyChangedDirectories": 0,
            "receiveOnlyChangedFiles": 0, "receiveOnlyChangedSymlinks": 0,
            "receiveOnlyTotalItems": 0
        }))
        .unwrap()
    }

    fn fld(label: &str, folder_type: &str, paused: bool) -> Folder {
        serde_json::from_value(serde_json::json!({
            "id": label.to_lowercase(), "label": label, "path": "/data",
            "paused": paused, "type": folder_type
        }))
        .unwrap()
    }

    /// Media, mid-sync: 600 MiB, 6 files, 4 files / 231 MiB still needed
    fn media_syncing() -> (Folder, FolderStatus) {
        let mut s = st("syncing");
        s.global_bytes = 600 * MIB;
        s.global_files = 6;
        s.need_bytes = 231 * MIB;
        s.need_total_items = 4;
        (fld("Media", "sendreceive", false), s)
    }

    fn lines(f: &Folder, s: Option<&FolderStatus>, width: u16) -> CardLines {
        let state = calculate_folder_card_state(f, s);
        card_lines(f, s, &state, width, 5)
    }

    fn w(text: &str) -> usize {
        unicode_width::UnicodeWidthStr::width(text)
    }

    fn fits(c: &CardLines, width: u16, icon: u16) -> bool {
        let width = width as usize;
        let line1 = if c.name.is_empty() && c.status.is_none() {
            0
        } else {
            icon as usize + w(&c.name) + c.status.as_ref().map_or(0, |t| 1 + w(t))
        };
        line1 <= width
            && c.detail.as_ref().is_none_or(|d| 5 + w(d) <= width)
            && c.pending.as_ref().is_none_or(|p| 5 + w(p) <= width)
    }

    #[test]
    fn test_sync_percent() {
        assert_eq!(sync_percent(600, 12), 98);
        assert_eq!(sync_percent(600, 0), 99);
        assert_eq!(sync_percent(0, 0), 0);
        assert_eq!(sync_percent(10, 20), 0);
        assert_eq!(sync_percent(1000, 381), 61);
    }

    #[test]
    fn test_type_glyph() {
        assert_eq!(type_glyph("sendreceive"), "⇅");
        assert_eq!(type_glyph("sendonly"), "⇡");
        assert_eq!(type_glyph("receiveonly"), "⇣");
        assert_eq!(type_glyph("weird"), "?");
    }

    #[test]
    fn test_card_state_from_syncthing_states() {
        let f = fld("Photos", "sendreceive", false);
        let state = |name: &str| calculate_folder_card_state(&f, Some(&st(name)));
        assert_eq!(state("sync-preparing"), FolderCardState::Preparing);
        assert_eq!(state("sync-waiting"), FolderCardState::Preparing);
        assert_eq!(state("scanning"), FolderCardState::Scanning);
        assert_eq!(state("scan-waiting"), FolderCardState::WaitingToScan);
        assert_eq!(state("cleaning"), FolderCardState::Cleaning);
        assert_eq!(state("clean-waiting"), FolderCardState::Cleaning);
        let mut errored = st("scanning");
        errored.errors = 2;
        assert_eq!(
            calculate_folder_card_state(&f, Some(&errored)),
            FolderCardState::Error { errors: 2 }
        );
    }

    #[test]
    fn test_card_lines_wide_syncing() {
        let (f, s) = media_syncing();
        let c = lines(&f, Some(&s), 56);
        assert_eq!(c.tier, CardTier::Wide);
        assert_eq!(c.name, "Media");
        assert_eq!(c.status.as_deref(), Some("Syncing 61%"));
        assert_eq!(
            c.detail.as_deref(),
            Some("⇅ Send & Receive · 600.0 MB · 6 files")
        );
        assert_eq!(c.pending.as_deref(), Some("↓ 4 files (231.0 MB)"));
    }

    #[test]
    fn test_card_lines_medium_and_narrow_syncing() {
        let (f, s) = media_syncing();
        let c = lines(&f, Some(&s), 36);
        assert_eq!(c.tier, CardTier::Medium);
        assert_eq!(c.status.as_deref(), Some("61%"));
        assert_eq!(c.detail.as_deref(), Some("⇅ 600.0 MB · 6 files"));
        assert_eq!(c.pending.as_deref(), Some("↓ 4 (231.0 MB)"));

        let c = lines(&f, Some(&s), 24);
        assert_eq!(c.tier, CardTier::Narrow);
        assert_eq!(c.status.as_deref(), Some("61%"));
        assert_eq!(c.detail, None);
        assert_eq!(c.pending.as_deref(), Some("↓ 4"));
    }

    #[test]
    fn test_card_lines_scanning_words_only_when_wide() {
        let mut s = st("scanning");
        s.global_bytes = 2 * MIB;
        s.global_files = 7;
        let f = fld("Photos", "sendreceive", false);
        assert_eq!(lines(&f, Some(&s), 56).status.as_deref(), Some("Scanning"));
        assert_eq!(lines(&f, Some(&s), 36).status, None);
    }

    #[test]
    fn test_card_lines_paused_hides_size_and_count() {
        let f = fld("Vault", "sendreceive", true);
        let c = lines(&f, Some(&st("idle")), 56);
        assert_eq!(c.detail.as_deref(), Some("⇅ Send & Receive"));
        assert_eq!(c.status, None);
        let medium = lines(&f, Some(&st("idle")), 16);
        assert_eq!(
            (medium.tier, medium.detail.as_deref()),
            (CardTier::Medium, Some("⇅"))
        );
        let narrow = lines(&f, Some(&st("idle")), 9);
        assert_eq!((narrow.tier, narrow.detail), (CardTier::Narrow, None));
    }

    #[test]
    fn test_card_lines_synced_has_no_status_words() {
        let mut s = st("idle");
        s.global_bytes = 2 * MIB;
        s.global_files = 7;
        let c = lines(&fld("Photos", "sendreceive", false), Some(&s), 56);
        assert_eq!(c.status, None);
        assert_eq!(c.pending, None);
        assert_eq!(
            c.detail.as_deref(),
            Some("⇅ Send & Receive · 2.0 MB · 7 files")
        );
    }

    #[test]
    fn test_card_lines_receive_only_local_changes() {
        let mut s = st("idle");
        s.global_bytes = 6 * 1024;
        s.global_files = 6;
        s.receive_only_total_items = 3;
        let f = fld("Documents", "receiveonly", false);
        let c = lines(&f, Some(&s), 56);
        assert_eq!(c.pending.as_deref(), Some("✎ 3 files modified locally"));
        assert!(c.detail.as_deref().unwrap().starts_with("⇣ Receive Only"));
        assert_eq!(
            lines(&f, Some(&s), 36).pending.as_deref(),
            Some("✎ 3 modified")
        );
    }

    #[test]
    fn test_card_lines_errors() {
        let mut s = st("idle");
        s.errors = 2;
        s.global_bytes = 315 * 1024;
        s.global_files = 27;
        let f = fld("Projects", "sendreceive", false);
        let c = lines(&f, Some(&s), 56);
        assert_eq!(c.status.as_deref(), Some("2 errors"));
        assert!(c.status_is_error);
        assert_eq!(lines(&f, Some(&s), 24).status.as_deref(), Some("2"));
    }

    #[test]
    fn test_card_lines_long_name_drops_to_next_tier() {
        let (_, s) = media_syncing();
        let f = fld(&"n".repeat(40), "sendreceive", false);
        let c = lines(&f, Some(&s), 56);
        assert_ne!(c.tier, CardTier::Wide);
        assert!(fits(&c, 56, 5), "{:?}", c);
    }

    #[test]
    fn test_card_lines_never_exceed_width() {
        let (_, syncing) = media_syncing();
        let mut errored = st("idle");
        errored.errors = 12;
        let synced = st("idle");
        let names = [
            "Media".to_string(),
            "Фото и видео 2024".to_string(),
            "x".repeat(60),
        ];
        for name in &names {
            for (paused, status) in [
                (false, &synced),
                (false, &syncing),
                (true, &synced),
                (false, &errored),
            ] {
                let f = fld(name, "sendreceive", paused);
                for width in 0..=60u16 {
                    let c = lines(&f, Some(status), width);
                    assert!(fits(&c, width, 5), "width {} name {} {:?}", width, name, c);
                }
            }
        }
    }
}
