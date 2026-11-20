use super::icons::{FolderState, IconRenderer};
use crate::api::{Folder, FolderStatus};
use crate::logic::folder_card::{FolderCardState, calculate_folder_card_state, card_lines};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

/// Render folder cards with inline stats and status
#[allow(clippy::too_many_arguments)]
pub fn render_folder_list(
    f: &mut Frame,
    area: Rect,
    folders: &[Folder],
    folder_statuses: &HashMap<String, FolderStatus>,
    _statuses_loaded: bool,
    folders_state: &mut ListState,
    is_focused: bool,
    icon_renderer: &IconRenderer,
    _last_folder_updates: &HashMap<String, (std::time::SystemTime, String)>,
) {
    let inner_width = area.width.saturating_sub(2);
    let title = folder_list_title(folders, folder_statuses, inner_width);

    let folder_items: Vec<ListItem> = folders
        .iter()
        .map(|folder| {
            let status = folder_statuses.get(&folder.id);
            let card_state = calculate_folder_card_state(folder, status);
            render_folder_card(folder, status, &card_state, icon_renderer, inner_width)
        })
        .collect();

    let highlight = if is_focused {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let folders_list = List::new(folder_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if is_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                }),
        )
        .highlight_style(highlight);

    f.render_stateful_widget(folders_list, area, folders_state);
}

/// Folder list title with counts, shortened to fit `inner_width` columns
fn folder_list_title(
    folders: &[Folder],
    statuses: &HashMap<String, FolderStatus>,
    inner_width: u16,
) -> String {
    let total = folders.len();
    let synced = folders
        .iter()
        .filter(|f| {
            !f.paused
                && statuses.get(&f.id).is_some_and(|status| {
                    status.state == "idle"
                        && status.need_total_items == 0
                        && status.receive_only_total_items == 0
                })
        })
        .count();
    let syncing = folders
        .iter()
        .filter(|f| {
            statuses
                .get(&f.id)
                .is_some_and(|status| status.state == "syncing" || status.state == "sync-preparing")
        })
        .count();
    let paused = folders.iter().filter(|f| f.paused).count();

    let counts = |sep: &str, with_paused: bool, total_word: bool| {
        let mut parts = vec![if total_word {
            format!("{} total", total)
        } else {
            total.to_string()
        }];
        if synced > 0 {
            parts.push(format!("{} synced", synced));
        }
        if syncing > 0 {
            parts.push(format!("{} syncing", syncing));
        }
        if with_paused && paused > 0 {
            parts.push(format!("{} paused", paused));
        }
        parts.join(sep)
    };

    let wide = format!(" Folders ({}) ", counts(", ", true, true));
    if wide.width() <= inner_width as usize {
        return wide;
    }
    let medium = format!(" Folders ({}) ", counts(" · ", false, false));
    if medium.width() <= inner_width as usize {
        return medium;
    }
    format!(" Folders {} ", total)
}

/// Map a card state to the icon state used by `IconRenderer`
fn card_state_to_folder_state(state: &FolderCardState) -> FolderState {
    match state {
        FolderCardState::Synced => FolderState::Synced,
        FolderCardState::OutOfSync { .. } => FolderState::OutOfSync,
        FolderCardState::Syncing { .. } => FolderState::Syncing,
        FolderCardState::Paused => FolderState::Paused,
        FolderCardState::Preparing => FolderState::Syncing,
        FolderCardState::Scanning | FolderCardState::WaitingToScan | FolderCardState::Cleaning => {
            FolderState::Scanning
        }
        FolderCardState::Error { .. } => FolderState::Error,
        FolderCardState::Loading => FolderState::Loading,
    }
}

/// Render a single folder card as a multi-line ListItem sized for `inner_width` columns
fn render_folder_card(
    folder: &Folder,
    status: Option<&FolderStatus>,
    state: &FolderCardState,
    icon_renderer: &IconRenderer,
    inner_width: u16,
) -> ListItem<'static> {
    let mut line1 = icon_renderer.folder_with_status(card_state_to_folder_state(state));
    line1.push(Span::raw(" "));
    let icon_width: usize = line1.iter().map(|span| span.content.width()).sum();
    let card = card_lines(folder, status, state, inner_width, icon_width as u16);

    let mut lines = Vec::new();
    line1.push(Span::raw(card.name.clone()));
    if let Some(text) = &card.status {
        let used = icon_width + card.name.width() + text.width();
        let pad = (inner_width as usize).saturating_sub(used).max(1);
        line1.push(Span::raw(" ".repeat(pad)));
        let style = if card.status_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        line1.push(Span::styled(text.clone(), style));
    }
    lines.push(Line::from(line1));

    if let Some(detail) = card.detail {
        lines.push(Line::from(Span::styled(
            format!("     {}", detail),
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(pending) = card.pending {
        lines.push(Line::from(format!("     {}", pending)));
    }

    // Blank separator between folders
    lines.push(Line::from(""));

    ListItem::new(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::{FolderState, IconMode, IconTheme};
    use ratatui::{Terminal, backend::TestBackend};

    fn test_folder() -> Folder {
        Folder {
            id: "abc".to_string(),
            label: Some("Docs".to_string()),
            path: "/data/docs".to_string(),
            paused: false,
            folder_type: "sendreceive".to_string(),
            devices: vec![],
        }
    }

    fn render_card_text(mode: IconMode, state: &FolderCardState) -> String {
        let renderer = IconRenderer::new(mode, IconTheme::default());
        let item = render_folder_card(&test_folder(), None, state, &renderer, 58);
        let backend = TestBackend::new(60, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(List::new(vec![item]), f.area()))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect()
    }

    // ========================================
    // CARD STATE MAPPING
    // ========================================

    #[test]
    fn test_card_state_to_folder_state_mapping() {
        let oos = FolderCardState::OutOfSync {
            remote_needed: 1,
            local_changes: 0,
        };
        let syncing = FolderCardState::Syncing {
            remote_needed: 1,
            local_changes: 0,
        };
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Synced),
            FolderState::Synced
        );
        assert_eq!(card_state_to_folder_state(&oos), FolderState::OutOfSync);
        assert_eq!(card_state_to_folder_state(&syncing), FolderState::Syncing);
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Paused),
            FolderState::Paused
        );
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Error { errors: 1 }),
            FolderState::Error
        );
        assert_eq!(
            card_state_to_folder_state(&FolderCardState::Loading),
            FolderState::Loading
        );
    }

    // ========================================
    // CARD ICON RENDERING
    // ========================================

    #[test]
    fn test_card_nerdfont_mode_has_no_emoji_folder() {
        let text = render_card_text(IconMode::NerdFont, &FolderCardState::Synced);
        assert!(!text.contains('📁'), "emoji folder leaked: {text:?}");
        assert!(!text.contains('📂'), "emoji folder leaked: {text:?}");
        assert!(
            text.contains('\u{F07C}'),
            "nerdfont glyph missing: {text:?}"
        );
    }

    #[test]
    fn test_card_emoji_mode_has_emoji_folder() {
        let text = render_card_text(IconMode::Emoji, &FolderCardState::Synced);
        assert!(text.contains('📂'), "emoji folder missing: {text:?}");
        assert!(!text.contains('\u{F07C}'));
    }

    // ========================================
    // FOLDER LIST RENDERING
    // ========================================

    const MIB: u64 = 1024 * 1024;

    fn folder(id: &str, label: &str, folder_type: &str, paused: bool) -> Folder {
        serde_json::from_value(serde_json::json!({
            "id": id, "label": label, "path": "/data", "paused": paused, "type": folder_type
        }))
        .unwrap()
    }

    fn status(state: &str, bytes: u64, files: u64) -> FolderStatus {
        serde_json::from_value(serde_json::json!({
            "state": state, "sequence": 0, "globalBytes": bytes, "globalDeleted": 0,
            "globalDirectories": 0, "globalFiles": files, "globalSymlinks": 0,
            "globalTotalItems": files, "inSyncBytes": 0, "inSyncFiles": 0, "localBytes": 0,
            "localDeleted": 0, "localDirectories": 0, "localFiles": 0, "localSymlinks": 0,
            "localTotalItems": 0, "needBytes": 0, "needDeletes": 0, "needDirectories": 0,
            "needFiles": 0, "needSymlinks": 0, "needTotalItems": 0,
            "receiveOnlyChangedBytes": 0, "receiveOnlyChangedDeletes": 0,
            "receiveOnlyChangedDirectories": 0, "receiveOnlyChangedFiles": 0,
            "receiveOnlyChangedSymlinks": 0, "receiveOnlyTotalItems": 0
        }))
        .unwrap()
    }

    /// The demo's five folders: Documents (3 local changes), Media (syncing), Photos,
    /// Projects, Vault (paused)
    fn demo() -> (Vec<Folder>, HashMap<String, FolderStatus>) {
        let folders = vec![
            folder("documents", "Documents", "receiveonly", false),
            folder("media", "Media", "sendreceive", false),
            folder("photos", "Photos", "sendreceive", false),
            folder("projects", "Projects", "sendreceive", false),
            folder("vault", "Vault", "sendreceive", true),
        ];
        let mut docs = status("idle", 6246, 6);
        docs.receive_only_total_items = 3;
        let mut media = status("syncing", 600 * MIB, 6);
        media.need_bytes = 231 * MIB;
        media.need_total_items = 4;
        let statuses = HashMap::from([
            ("documents".to_string(), docs),
            ("media".to_string(), media),
            ("photos".to_string(), status("idle", 2_831_155, 7)),
            ("projects".to_string(), status("idle", 314_880, 27)),
        ]);
        (folders, statuses)
    }

    fn draw(
        width: u16,
        folders: &[Folder],
        statuses: &HashMap<String, FolderStatus>,
        selected: Option<usize>,
        focused: bool,
    ) -> ratatui::buffer::Buffer {
        let renderer = IconRenderer::new(IconMode::Emoji, IconTheme::default());
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        let mut state = ListState::default();
        state.select(selected);
        terminal
            .draw(|f| {
                render_folder_list(
                    f,
                    f.area(),
                    folders,
                    statuses,
                    true,
                    &mut state,
                    focused,
                    &renderer,
                    &HashMap::new(),
                )
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    fn find_row(buf: &ratatui::buffer::Buffer, needle: &str) -> u16 {
        (0..buf.area.height)
            .find(|&y| row_text(buf, y).contains(needle))
            .unwrap_or_else(|| panic!("no row contains {:?}", needle))
    }

    fn find_col(buf: &ratatui::buffer::Buffer, y: u16, symbol: &str) -> u16 {
        (0..buf.area.width)
            .find(|&x| buf[(x, y)].symbol() == symbol)
            .unwrap_or_else(|| panic!("no {:?} on row {}", symbol, y))
    }

    #[test]
    fn test_wide_card_lines_render() {
        let (folders, statuses) = demo();
        let buf = draw(58, &folders, &statuses, None, true);
        let media = row_text(&buf, find_row(&buf, "Media"));
        assert!(
            media.trim_end_matches(['│', ' ']).ends_with("Syncing 61%"),
            "{}",
            media
        );
        find_row(&buf, "⇅ Send & Receive · 600.0 MB · 6 files");
        find_row(&buf, "↓ 4 files (231.0 MB)");
    }

    #[test]
    fn test_line_two_is_dimmed() {
        let (folders, statuses) = demo();
        let buf = draw(58, &folders, &statuses, None, true);
        let y = find_row(&buf, "⇅ Send & Receive · 2.7 MB");
        let x = find_col(&buf, y, "⇅");
        assert_eq!(buf[(x, y)].fg, Color::DarkGray);
    }

    #[test]
    fn test_selected_card_has_background_bar_when_focused() {
        let (folders, statuses) = demo();
        let buf = draw(58, &folders, &statuses, Some(1), true);
        let y = find_row(&buf, "Media");
        assert_eq!(buf[(1, y)].bg, Color::DarkGray);
        assert_eq!(buf[(56, y)].bg, Color::DarkGray);
    }

    #[test]
    fn test_selected_card_is_cyan_text_when_unfocused() {
        let (folders, statuses) = demo();
        let buf = draw(58, &folders, &statuses, Some(1), false);
        let y = find_row(&buf, "Media");
        let x = find_col(&buf, y, "M");
        assert_eq!(buf[(x, y)].fg, Color::Cyan);
        assert_ne!(buf[(x, y)].bg, Color::DarkGray);
    }

    #[test]
    fn test_error_status_is_red() {
        let folders = vec![folder("projects", "Projects", "sendreceive", false)];
        let mut errored = status("idle", 314_880, 27);
        errored.errors = 2;
        let statuses = HashMap::from([("projects".to_string(), errored)]);
        let buf = draw(58, &folders, &statuses, None, true);
        let y = find_row(&buf, "2 errors");
        let x = (0..buf.area.width)
            .find(|&x| buf[(x, y)].symbol() == "2" && buf[(x + 2, y)].symbol() == "e")
            .unwrap();
        assert_eq!(buf[(x, y)].fg, Color::Red);
    }

    #[test]
    fn test_title_shortens_with_width() {
        let (folders, statuses) = demo();
        assert_eq!(
            folder_list_title(&folders, &statuses, 56),
            " Folders (5 total, 2 synced, 1 syncing, 1 paused) "
        );
        assert_eq!(
            folder_list_title(&folders, &statuses, 36),
            " Folders (5 · 2 synced · 1 syncing) "
        );
        assert_eq!(folder_list_title(&folders, &statuses, 20), " Folders 5 ");
    }
}
