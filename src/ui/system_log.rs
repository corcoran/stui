//! Log viewer popup

use crate::api::LogLine;
use crate::logic::formatting::format_rfc3339_local;
use crate::logic::preview_rows::wrap_rows;
use crate::logic::system_log::{
    LogFilter, LogLevel, level, level_label, one_row, parse_message, summary, visible,
};
use crate::model::types::{LogDetail, SystemLogState};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};

const GRAY: Color = Color::Rgb(120, 120, 120);

/// Draw the log viewer. Clamps `offset` and `detail.scroll` to the content.
pub fn render_system_log(f: &mut Frame, area: Rect, state: &mut SystemLogState) {
    let width = (area.width as f32 * 0.9) as u16;
    let height = (area.height as f32 * 0.9) as u16;
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    if popup.is_empty() {
        return;
    }
    f.render_widget(Clear, popup);

    let error = state.error.as_deref();
    if let Some(detail) = &mut state.detail {
        let block = popup_block("Syncthing log", "Esc: back", error);
        render_detail(f, popup, block, detail);
        return;
    }

    let shown = visible(&state.lines, state.filter);
    let keys = if shown.is_empty() {
        "Esc: close"
    } else {
        "Enter: details · Esc: close"
    };
    let block = popup_block(&list_title(state, shown.len()), keys, error);
    let inner = block.inner(popup);
    let rows = inner.height as usize;
    if shown.is_empty() || rows == 0 {
        let text = match (state.loaded, error) {
            (false, Some(error)) => format!("Can't load the log: {}", one_row(error)),
            (false, None) => "Loading…".to_string(),
            (true, _) if state.lines.is_empty() => "The log is empty".to_string(),
            (true, _) => "No lines at this level".to_string(),
        };
        let text = Paragraph::new(text).style(Style::default().fg(GRAY));
        f.render_widget(text.block(block), popup);
        return;
    }

    let selected = state
        .selected
        .and_then(|sel| shown.iter().position(|i| *i == sel));
    if let Some(row) = selected {
        state.offset = state.offset.clamp((row + 1).saturating_sub(rows), row);
    }
    state.offset = state.offset.min(shown.len().saturating_sub(rows));

    let items = shown
        .iter()
        .skip(state.offset)
        .take(rows)
        .map(|i| ListItem::new(log_row(&state.lines[*i], state.selected == Some(*i))));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));
    let mut list_state = ListState::default();
    list_state.select(selected.map(|row| row - state.offset));
    f.render_stateful_widget(list, popup, &mut list_state);

    if shown.len() > rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        let mut scrollbar_state =
            ScrollbarState::new(shown.len()).position(selected.unwrap_or(state.offset));
        f.render_stateful_widget(
            scrollbar,
            popup.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut scrollbar_state,
        );
    }
}

fn popup_block(title: &str, keys: &str, error: Option<&str>) -> Block<'static> {
    let mut spans = vec![Span::raw(format!(" {} ", title))];
    if let Some(error) = error {
        spans.push(Span::styled(
            format!("· {} ", one_row(error)),
            Style::default().fg(Color::Red),
        ));
    }
    Block::default()
        .title(Line::from(spans))
        .title_bottom(format!(" {} ", keys))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
}

fn list_title(state: &SystemLogState, shown: usize) -> String {
    let total = state.lines.len();
    let filter = match state.filter {
        LogFilter::All => format!("all ({})", total),
        LogFilter::Warnings => format!("warnings+ ({} of {})", shown, total),
        LogFilter::Errors => format!("errors ({} of {})", shown, total),
    };
    format!("Syncthing log · {} · v: level", filter)
}

fn level_style(level: LogLevel) -> Style {
    match level {
        LogLevel::Warning => Style::default().fg(Color::Yellow),
        LogLevel::Error => Style::default().fg(Color::Red),
        LogLevel::Debug => Style::default().add_modifier(Modifier::DIM),
        LogLevel::Info | LogLevel::Other => Style::default(),
    }
}

/// The line's time, or the text Syncthing sent when that isn't a time
fn local_time(line: &LogLine, fmt: &str) -> String {
    format_rfc3339_local(&line.when, fmt).unwrap_or_else(|| one_row(&line.when))
}

fn log_row(line: &LogLine, selected: bool) -> Line<'static> {
    let parsed = parse_message(&line.message);
    let level = level(&line.level);
    // Gray text can't be read on the selection highlight
    let time_style = if selected {
        Style::default()
    } else {
        Style::default().fg(GRAY)
    };
    let mut spans = vec![
        Span::styled(local_time(line, "%m-%d %H:%M:%S"), time_style),
        Span::raw("  "),
        Span::styled(one_row(&level_label(&line.level)), level_style(level)),
        Span::raw("  "),
        Span::raw(one_row(&parsed.text)),
    ];
    let summary = one_row(&summary(&parsed));
    if !summary.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::raw(summary));
    }
    let row = Line::from(spans);
    match level {
        LogLevel::Debug => row.style(level_style(level)),
        _ => row,
    }
}

fn render_detail(f: &mut Frame, popup: Rect, block: Block, detail: &mut LogDetail) {
    let inner = block.inner(popup);
    let rows = wrap_rows(&detail_text(&detail.line), inner.width);
    let last = rows.len().saturating_sub(inner.height as usize);
    detail.scroll = detail.scroll.min(u16::try_from(last).unwrap_or(u16::MAX));
    let on_screen: Vec<Line> = rows
        .into_iter()
        .skip(detail.scroll as usize)
        .take(inner.height as usize)
        .collect();
    f.render_widget(Paragraph::new(on_screen).block(block), popup);
}

fn detail_text(line: &LogLine) -> Text<'static> {
    let parsed = parse_message(&line.message);
    let mut lines = vec![
        Line::from(vec![
            Span::raw(local_time(line, "%Y-%m-%d %H:%M:%S")),
            Span::raw("  "),
            Span::styled(
                one_row(&level_label(&line.level)),
                level_style(level(&line.level)),
            ),
        ]),
        Line::default(),
    ];
    lines.extend(parsed.text.lines().map(|l| Line::from(one_row(l))));
    if !parsed.fields.is_empty() {
        lines.push(Line::default());
    }
    for (key, value) in &parsed.fields {
        let mut value_lines = value.lines().map(one_row);
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", one_row(key)), Style::default().fg(GRAY)),
            Span::raw(value_lines.next().unwrap_or_default()),
        ]));
        lines.extend(value_lines.map(Line::from));
    }
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RawLevel;
    use crate::logic::system_log::{cycle_filter, merge, select_first, select_last};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    const WHEN: &str = "2026-01-01T10:00:00Z";
    const SYNCED: &str = "Synced file (folder.label=Sites file.name=a/b.txt log.pkg=model)";

    fn line(level: &str, message: &str) -> LogLine {
        LogLine {
            when: WHEN.to_string(),
            message: message.to_string(),
            level: RawLevel::Name(level.to_string()),
        }
    }

    fn state(lines: Vec<LogLine>) -> SystemLogState {
        let mut s = SystemLogState {
            generation: 1,
            lines: vec![],
            filter: Default::default(),
            selected: None,
            offset: 0,
            detail: None,
            error: None,
            in_flight: false,
            loaded: true,
            last_request: std::time::Instant::now(),
        };
        merge(&mut s, lines);
        s
    }

    fn draw_at(s: &mut SystemLogState, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| render_system_log(f, f.area(), s))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn draw(s: &mut SystemLogState) -> Buffer {
        draw_at(s, 120, 30)
    }

    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn row_of(buffer: &Buffer, text: &str) -> Option<usize> {
        rows(buffer).iter().position(|r| r.contains(text))
    }

    /// Cell where `text` (ASCII, on a row with single-column characters) starts
    fn cell_of(buffer: &Buffer, text: &str) -> ratatui::buffer::Cell {
        let y = row_of(buffer, text).unwrap();
        let row = &rows(buffer)[y];
        let x = row[..row.find(text).unwrap()].chars().count();
        buffer[(x as u16, y as u16)].clone()
    }

    // ========================================
    // ROWS
    // ========================================

    #[test]
    fn test_row_shows_time_level_text_summary() {
        let buffer = draw(&mut state(vec![line("INF", SYNCED)]));
        let time = format_rfc3339_local(WHEN, "%m-%d %H:%M:%S").unwrap();
        let row = row_of(&buffer, "INF  Synced file  Sites: a/b.txt").unwrap();
        assert!(rows(&buffer)[row].contains(&time));
        assert!(!rows(&buffer)[row].contains("log.pkg"));
    }

    #[test]
    fn test_level_colors() {
        let buffer = draw(&mut state(vec![
            line("WRN", "warned"),
            line("ERR", "failed"),
            line("DBG", "traced"),
            line("INF", "informed"),
        ]));
        assert_eq!(cell_of(&buffer, "WRN").fg, Color::Yellow);
        assert_eq!(cell_of(&buffer, "ERR").fg, Color::Red);
        for text in ["DBG", "traced"] {
            assert!(cell_of(&buffer, text).modifier.contains(Modifier::DIM));
        }
        assert!(!cell_of(&buffer, "INF").modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_unparsed_message_shown_unchanged() {
        let buffer = draw(&mut state(vec![line("INF", "odd (not fields)")]));
        assert!(row_of(&buffer, "INF  odd (not fields)").is_some());
    }

    #[test]
    fn test_unparsable_time_shown_as_sent() {
        let mut odd = line("INF", "text");
        odd.when = "sometime".to_string();
        assert!(row_of(&draw(&mut state(vec![odd])), "sometime  INF  text").is_some());
    }

    #[test]
    fn test_multiline_value_stays_one_row() {
        let buffer = draw(&mut state(vec![
            line("ERR", r#"Failed (error="one\ntwo")"#),
            line("INF", "next"),
        ]));
        let first = row_of(&buffer, "Failed  one two").unwrap();
        assert!(rows(&buffer)[first + 1].contains("next"));
    }

    // ========================================
    // TITLE AND MESSAGES
    // ========================================

    fn three() -> SystemLogState {
        state(vec![line("INF", "a"), line("WRN", "b"), line("INF", "c")])
    }

    #[test]
    fn test_title_counts() {
        let mut s = three();
        for title in [
            "Syncthing log · all (3) · v: level",
            "Syncthing log · warnings+ (1 of 3) · v: level",
            "Syncthing log · errors (0 of 3) · v: level",
        ] {
            assert!(row_of(&draw(&mut s), title).is_some(), "{title}");
            cycle_filter(&mut s);
        }
    }

    #[test]
    fn test_title_shows_error() {
        let mut s = three();
        s.error = Some("timeout".to_string());
        let shown = row_of(&draw(&mut s), "timeout");
        assert_eq!(shown, row_of(&draw(&mut s), "Syncthing log"));
        assert!(shown.is_some());
    }

    #[test]
    fn test_placeholder_text() {
        let mut s = state(vec![]);
        s.loaded = false;
        assert!(row_of(&draw(&mut s), "Loading…").is_some());
        s.loaded = true;
        assert!(row_of(&draw(&mut s), "The log is empty").is_some());
        let mut s = three();
        cycle_filter(&mut s);
        cycle_filter(&mut s);
        assert!(row_of(&draw(&mut s), "No lines at this level").is_some());
    }

    #[test]
    fn test_failed_first_load_shows_the_error() {
        let mut s = state(vec![]);
        s.loaded = false;
        s.error = Some("HTTP 403 Forbidden".to_string());
        let buffer = draw(&mut s);
        assert!(row_of(&buffer, "Loading…").is_none());
        assert!(row_of(&buffer, "Can't load the log: HTTP 403 Forbidden").is_some());
    }

    /// Bottom border row of the popup drawn by `draw`: 90% of 30 rows, centered
    const BOTTOM: usize = 27;

    #[test]
    fn test_list_keys_on_bottom_border() {
        let buffer = draw(&mut three());
        assert_eq!(row_of(&buffer, "Enter: details · Esc: close"), Some(BOTTOM));
        assert!(row_of(&buffer, "v: level").unwrap() < BOTTOM);
    }

    #[test]
    fn test_empty_list_offers_only_close() {
        let mut s = three();
        cycle_filter(&mut s);
        cycle_filter(&mut s);
        let buffer = draw(&mut s);
        assert_eq!(row_of(&buffer, "Esc: close"), Some(BOTTOM));
        assert!(row_of(&buffer, "Enter").is_none());
    }

    // ========================================
    // SCROLLING
    // ========================================

    fn many() -> SystemLogState {
        state((0..200).map(|n| line("INF", &format!("m{n:03}"))).collect())
    }

    #[test]
    fn test_selected_row_is_on_screen() {
        let mut s = many();
        assert!(row_of(&draw(&mut s), "m199").is_some());
        select_first(&mut s);
        assert!(row_of(&draw(&mut s), "m000").is_some());
        select_last(&mut s);
        assert!(row_of(&draw(&mut s), "m199").is_some());
    }

    #[test]
    fn test_moving_up_inside_the_view_does_not_scroll() {
        let mut s = many();
        draw(&mut s);
        crate::logic::system_log::move_selection(&mut s, -5);
        assert!(row_of(&draw(&mut s), "m199").is_some());
    }

    #[test]
    fn test_selected_time_stands_out_from_its_highlight() {
        let mut s = three();
        select_first(&mut s);
        let buffer = draw(&mut s);
        let time = format_rfc3339_local(WHEN, "%m-%d %H:%M:%S").unwrap();
        let first = cell_of(&buffer, &time);
        assert_eq!(first.bg, Color::DarkGray);
        assert_ne!(first.fg, GRAY);
        let y = row_of(&buffer, "WRN  b").unwrap() as u16;
        let x = (0..buffer.area.width)
            .find(|x| buffer[(*x, y)].symbol() == &time[..1] && *x > 6)
            .unwrap();
        assert_eq!(buffer[(x, y)].fg, GRAY);
    }

    // ========================================
    // DETAIL
    // ========================================

    fn detail(message: &str) -> SystemLogState {
        let mut s = three();
        s.detail = Some(LogDetail {
            line: line("ERR", message),
            scroll: 0,
        });
        s
    }

    #[test]
    fn test_detail_lists_fields() {
        let buffer = draw(&mut detail(SYNCED));
        let date = format_rfc3339_local(WHEN, "%Y-%m-%d %H:%M:%S").unwrap();
        for text in [
            date.as_str(),
            "ERR",
            "Synced file",
            "folder.label: Sites",
            "file.name: a/b.txt",
            "log.pkg: model",
        ] {
            assert!(row_of(&buffer, text).is_some(), "{text}");
        }
    }

    #[test]
    fn test_detail_keys_on_bottom_border() {
        let buffer = draw(&mut detail(SYNCED));
        assert_eq!(row_of(&buffer, "Esc: back"), Some(BOTTOM));
        assert!(row_of(&buffer, "Enter").is_none());
    }

    #[test]
    fn test_detail_breaks_multiline_values() {
        let buffer = draw(&mut detail(r#"Failed (error="one\ntwo\tthree")"#));
        let first = row_of(&buffer, "error: one").unwrap();
        assert!(rows(&buffer)[first + 1].contains("two three"));
    }

    #[test]
    fn test_detail_scroll_is_clamped() {
        let fields: Vec<String> = (0..60).map(|n| format!("k{n}=v")).collect();
        let mut s = detail(&format!("Text ({} last=end)", fields.join(" ")));
        s.detail.as_mut().unwrap().scroll = 9999;
        assert!(row_of(&draw(&mut s), "last: end").is_some());
        assert!(s.detail.as_ref().unwrap().scroll < 100);

        let mut s = detail("short");
        s.detail.as_mut().unwrap().scroll = 5;
        assert!(row_of(&draw(&mut s), "short").is_some());
        assert_eq!(s.detail.as_ref().unwrap().scroll, 0);
    }

    #[test]
    fn test_tiny_terminal_does_not_panic() {
        for (width, height) in [(10, 3), (3, 10), (2, 2), (1, 1)] {
            draw_at(&mut many(), width, height);
            draw_at(&mut detail(SYNCED), width, height);
            draw_at(&mut state(vec![]), width, height);
        }
    }
}
