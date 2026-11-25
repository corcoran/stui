//! Syncthing log: parsing, filtering and selection
//!
//! Syncthing 2.x log messages read `Text (key=value key="quoted value" log.pkg=model)`.
//! Syncthing 1.x has no such fields, numeric levels and no error level, so the viewer
//! refuses it.

use std::collections::HashSet;

use crate::api::{LogLine, RawLevel};
use crate::model::types::SystemLogState;

/// Lines kept, as many as Syncthing itself keeps
pub const MAX_LINES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    /// A level stui doesn't know, shown as sent and only under `LogFilter::All`
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFilter {
    #[default]
    All,
    Warnings,
    Errors,
}

impl LogFilter {
    pub fn next(self) -> Self {
        match self {
            LogFilter::All => LogFilter::Warnings,
            LogFilter::Warnings => LogFilter::Errors,
            LogFilter::Errors => LogFilter::All,
        }
    }

    pub fn matches(self, level: LogLevel) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::Warnings => matches!(level, LogLevel::Warning | LogLevel::Error),
            LogFilter::Errors => level == LogLevel::Error,
        }
    }
}

/// A message split into its text and fields
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    pub text: String,
    pub fields: Vec<(String, String)>,
}

/// Syncthing writes in-between levels with a signed suffix (`INF+1`), which is ignored.
pub fn level(raw: &RawLevel) -> LogLevel {
    let RawLevel::Name(name) = raw else {
        return LogLevel::Other;
    };
    match name.split(['+', '-']).next().unwrap_or("") {
        "DBG" => LogLevel::Debug,
        "INF" => LogLevel::Info,
        "WRN" => LogLevel::Warning,
        "ERR" => LogLevel::Error,
        _ => LogLevel::Other,
    }
}

/// The level as Syncthing sent it
pub fn level_label(raw: &RawLevel) -> String {
    match raw {
        RawLevel::Name(name) => name.clone(),
        RawLevel::Number(n) => n.to_string(),
    }
}

/// Split a message into text and fields. The text can hold parentheses itself, so the field
/// group is the first ` (` whose rest parses entirely as fields. A message without one is
/// all text.
pub fn parse_message(message: &str) -> ParsedMessage {
    message
        .match_indices(" (")
        .find_map(|(start, opening)| {
            let group = message[start + opening.len()..].strip_suffix(')')?;
            Some(ParsedMessage {
                text: message[..start].to_string(),
                fields: parse_fields(group)?,
            })
        })
        .unwrap_or_else(|| ParsedMessage {
            text: message.to_string(),
            fields: vec![],
        })
}

/// Space-separated `key=value` pairs, or `None` when anything else is in the way
fn parse_fields(group: &str) -> Option<Vec<(String, String)>> {
    let mut fields = Vec::new();
    let mut rest = group;
    loop {
        let (key, after) = rest.split_once('=')?;
        let bad_key = |c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"');
        if key.is_empty() || key.contains(bad_key) {
            return None;
        }
        let (value, after) = match after.strip_prefix('"') {
            Some(quoted) => parse_quoted(quoted)?,
            None => {
                let end = after.find(' ').unwrap_or(after.len());
                (after[..end].to_string(), &after[end..])
            }
        };
        fields.push((key.to_string(), value));
        if after.is_empty() {
            return Some(fields);
        }
        rest = after.strip_prefix(' ')?;
    }
}

/// The value up to its closing quote, and what follows it. `\x1b`-style escapes stay as
/// written so that no control character comes out of them.
fn parse_quoted(quoted: &str) -> Option<(String, &str)> {
    let mut value = String::new();
    let mut chars = quoted.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((value, &quoted[at + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                c @ ('"' | '\\') => value.push(c),
                c => {
                    value.push('\\');
                    value.push(c);
                }
            },
            c => value.push(c),
        }
    }
    None
}

/// What the line is about: folder or device, file, error. Without any of these, the fields.
pub fn summary(parsed: &ParsedMessage) -> String {
    let field = |key: &str| {
        parsed
            .fields
            .iter()
            .find(|(k, v)| k == key && !v.is_empty())
            .map(|(_, v)| v.as_str())
    };
    let subject = field("folder.label")
        .or_else(|| field("folder.id"))
        .or_else(|| field("device"));
    let parts: Vec<&str> = [subject, field("file.name"), field("error")]
        .into_iter()
        .flatten()
        .collect();
    if !parts.is_empty() {
        return parts.join(": ");
    }
    parsed
        .fields
        .iter()
        .filter(|(k, _)| k != "log.pkg")
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Text as one screen row: control characters become spaces
pub fn one_row(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Indices of the lines the filter shows
pub fn visible(lines: &[LogLine], filter: LogFilter) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| filter.matches(level(&line.level)))
        .map(|(i, _)| i)
        .collect()
}

/// The latest time held. Lines can be out of time order, so it isn't always the last line's.
pub fn newest_when(lines: &[LogLine]) -> Option<&str> {
    lines
        .iter()
        .filter_map(|line| {
            let time = chrono::DateTime::parse_from_rfc3339(&line.when).ok()?;
            Some((time, line.when.as_str()))
        })
        .max_by_key(|(time, _)| *time)
        .map(|(_, when)| when)
}

/// False only for a version known to be older than 2.0
pub fn supports_log_viewer(version: Option<&str>) -> bool {
    version
        .and_then(|v| v.trim_start_matches('v').split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_none_or(|major| major >= 2)
}

pub fn unsupported_message(version: Option<&str>) -> String {
    match version {
        Some(v) => format!(
            "The log viewer needs Syncthing 2.0 or newer (this device runs {})",
            v
        ),
        None => "The log viewer needs Syncthing 2.0 or newer".to_string(),
    }
}

/// Only Syncthing 1.x sends numeric levels
pub fn is_unsupported_reply(lines: &[LogLine]) -> bool {
    lines
        .iter()
        .any(|line| matches!(line.level, RawLevel::Number(_)))
}

/// Add a reply's new lines. The selection follows the newest visible line while it is on it,
/// and otherwise stays on its line.
pub fn merge(state: &mut SystemLogState, reply: Vec<LogLine>) {
    let follow =
        state.selected.is_none() || state.selected == visible(&state.lines, state.filter).pop();

    let fresh: Vec<LogLine> = {
        let held: HashSet<&LogLine> = state.lines.iter().collect();
        reply
            .into_iter()
            .filter(|line| !held.contains(line))
            .collect()
    };
    state.lines.extend(fresh);
    let dropped = state.lines.len().saturating_sub(MAX_LINES);
    state.lines.drain(..dropped);

    let shown = visible(&state.lines, state.filter);
    state.selected = match state.selected.and_then(|i| i.checked_sub(dropped)) {
        _ if follow => shown.last().copied(),
        Some(kept) => Some(kept),
        None => shown.first().copied(),
    };
}

/// Show the next level filter, keeping the selection on its line or the nearest visible one
pub fn cycle_filter(state: &mut SystemLogState) {
    state.filter = state.filter.next();
    let shown = visible(&state.lines, state.filter);
    state.selected = match state.selected {
        // Of two equally near lines, the later one
        Some(old) => shown.into_iter().rev().min_by_key(|i| i.abs_diff(old)),
        None => shown.last().copied(),
    };
}

/// Move over the visible lines, stopping at either end
pub fn move_selection(state: &mut SystemLogState, delta: isize) {
    let shown = visible(&state.lines, state.filter);
    let Some(row) = state
        .selected
        .and_then(|sel| shown.iter().position(|i| *i == sel))
    else {
        return;
    };
    let row = row.saturating_add_signed(delta).min(shown.len() - 1);
    state.selected = Some(shown[row]);
}

pub fn select_first(state: &mut SystemLogState) {
    state.selected = visible(&state.lines, state.filter).first().copied();
}

pub fn select_last(state: &mut SystemLogState) {
    state.selected = visible(&state.lines, state.filter).last().copied();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn line_at(when: &str, level: &str, message: &str) -> LogLine {
        LogLine {
            when: when.to_string(),
            message: message.to_string(),
            level: RawLevel::Name(level.to_string()),
        }
    }

    fn line(level: &str, message: &str) -> LogLine {
        line_at("2026-01-01T10:00:00Z", level, message)
    }

    fn state() -> SystemLogState {
        SystemLogState {
            generation: 1,
            lines: vec![],
            filter: LogFilter::All,
            selected: None,
            detail: None,
            error: None,
            in_flight: false,
            loaded: false,
            last_request: Instant::now(),
        }
    }

    fn state_with(lines: &[(&str, &str)]) -> SystemLogState {
        let mut s = state();
        merge(&mut s, lines.iter().map(|(l, m)| line(l, m)).collect());
        s
    }

    fn selected_message(s: &SystemLogState) -> Option<&str> {
        s.selected.map(|i| s.lines[i].message.as_str())
    }

    fn numbered(range: std::ops::Range<usize>) -> Vec<LogLine> {
        range.map(|n| line("INF", &format!("m{n}"))).collect()
    }

    fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn summary_of(pairs: &[(&str, &str)]) -> String {
        summary(&ParsedMessage {
            text: "Text".to_string(),
            fields: fields(pairs),
        })
    }

    // ========================================
    // LEVELS
    // ========================================

    #[test]
    fn test_level_names() {
        let named = |s: &str| level(&RawLevel::Name(s.to_string()));
        assert_eq!(named("DBG"), LogLevel::Debug);
        assert_eq!(named("INF"), LogLevel::Info);
        assert_eq!(named("WRN"), LogLevel::Warning);
        assert_eq!(named("ERR"), LogLevel::Error);
        assert_eq!(named("INF+1"), LogLevel::Info);
        assert_eq!(named("WRN-2"), LogLevel::Warning);
        assert_eq!(named("XYZ"), LogLevel::Other);
        assert_eq!(level(&RawLevel::Number(3)), LogLevel::Other);
    }

    #[test]
    fn test_level_label_keeps_suffix() {
        assert_eq!(level_label(&RawLevel::Name("INF+1".to_string())), "INF+1");
        assert_eq!(level_label(&RawLevel::Number(3)), "3");
    }

    // ========================================
    // PARSING
    // ========================================

    #[test]
    fn test_parses_fields() {
        let p = parse_message("Synced file (folder.label=Sites file.name=a/b.txt log.pkg=model)");
        assert_eq!(p.text, "Synced file");
        assert_eq!(
            p.fields,
            fields(&[
                ("folder.label", "Sites"),
                ("file.name", "a/b.txt"),
                ("log.pkg", "model")
            ])
        );
    }

    #[test]
    fn test_parses_escaped_quotes() {
        let p = parse_message(
            r#"Service failed (error="Get \"https://x\": dial tcp" log.pkg=svcutil)"#,
        );
        assert_eq!(p.text, "Service failed");
        assert_eq!(
            p.fields,
            fields(&[
                ("error", r#"Get "https://x": dial tcp"#),
                ("log.pkg", "svcutil")
            ])
        );
    }

    #[test]
    fn test_decodes_go_escapes() {
        let p = parse_message(r#"E (error="a\nb\tc\rd\\e\x1bf")"#);
        assert_eq!(p.fields, fields(&[("error", "a\nb\tc\rd\\e\\x1bf")]));
    }

    #[test]
    fn test_text_may_hold_parentheses() {
        let p = parse_message("Sync (retry) failed (error=EOF)");
        assert_eq!(p.text, "Sync (retry) failed");
        assert_eq!(p.fields, fields(&[("error", "EOF")]));

        let p = parse_message("Set (a=b) up (error=EOF)");
        assert_eq!(p.text, "Set (a=b) up");
    }

    #[test]
    fn test_quoted_value_may_hold_a_group() {
        let p = parse_message(r#"Failed (file.name="a (b=c)")"#);
        assert_eq!(p.text, "Failed");
        assert_eq!(p.fields, fields(&[("file.name", "a (b=c)")]));
    }

    #[test]
    fn test_value_may_hold_parenthesis_and_equals() {
        let p = parse_message("Synced file (file.name=a(1)=b.txt log.pkg=model)");
        assert_eq!(
            p.fields,
            fields(&[("file.name", "a(1)=b.txt"), ("log.pkg", "model")])
        );
        let p = parse_message("Synced file (file.name=a)");
        assert_eq!(p.fields, fields(&[("file.name", "a")]));
        let p = parse_message("Synced file (file.name=a))");
        assert_eq!(p.fields, fields(&[("file.name", "a)")]));
    }

    #[test]
    fn test_unparsable_is_unchanged() {
        for message in [
            "",
            "plain",
            "odd (not fields)",
            "empty ()",
            "cut (error=\"open",
            "cut (error=\"open)",
            "junk (error=\"a\"b)",
            "spaces (a=b  c=d)",
        ] {
            let p = parse_message(message);
            assert_eq!(p.text, message);
            assert!(p.fields.is_empty(), "{message}");
        }
    }

    #[test]
    fn test_one_row_strips_controls() {
        let row = one_row("a\nb\x1b[31mc\t");
        assert_eq!(row, "a b [31mc ");
    }

    // ========================================
    // SUMMARY
    // ========================================

    #[test]
    fn test_summary_label_file_error() {
        assert_eq!(
            summary_of(&[
                ("error", "EOF"),
                ("file.name", "a.txt"),
                ("folder.id", "x-1"),
                ("folder.label", "Sites"),
            ]),
            "Sites: a.txt: EOF"
        );
    }

    #[test]
    fn test_summary_folder_id_fallback() {
        assert_eq!(
            summary_of(&[("folder.id", "x-1"), ("file.name", "a.txt")]),
            "x-1: a.txt"
        );
        assert_eq!(
            summary_of(&[("folder.label", ""), ("folder.id", "x-1")]),
            "x-1"
        );
    }

    #[test]
    fn test_summary_device_error() {
        assert_eq!(
            summary_of(&[("device", "CX"), ("address", "1.2.3.4:5"), ("error", "EOF")]),
            "CX: EOF"
        );
    }

    #[test]
    fn test_summary_error_only() {
        assert_eq!(summary_of(&[("error", "EOF"), ("supervisor", "s")]), "EOF");
    }

    #[test]
    fn test_summary_other_fields() {
        assert_eq!(
            summary_of(&[("uri", "u"), ("n", "2"), ("log.pkg", "relay")]),
            "uri=u n=2"
        );
        assert_eq!(summary_of(&[("log.pkg", "relay")]), "");
        assert_eq!(summary_of(&[]), "");
    }

    // ========================================
    // FILTER
    // ========================================

    #[test]
    fn test_filter_cycle() {
        assert_eq!(LogFilter::default(), LogFilter::All);
        assert_eq!(LogFilter::All.next(), LogFilter::Warnings);
        assert_eq!(LogFilter::Warnings.next(), LogFilter::Errors);
        assert_eq!(LogFilter::Errors.next(), LogFilter::All);
    }

    #[test]
    fn test_filter_matches() {
        use LogLevel::*;
        for (level, warnings, errors) in [
            (Debug, false, false),
            (Info, false, false),
            (Warning, true, false),
            (Error, true, true),
            (Other, false, false),
        ] {
            assert!(LogFilter::All.matches(level));
            assert_eq!(LogFilter::Warnings.matches(level), warnings);
            assert_eq!(LogFilter::Errors.matches(level), errors);
        }
    }

    // ========================================
    // MERGE
    // ========================================

    #[test]
    fn test_first_merge_selects_newest() {
        assert_eq!(
            selected_message(&state_with(&[("INF", "a"), ("INF", "b")])),
            Some("b")
        );
    }

    #[test]
    fn test_merge_appends_in_reply_order() {
        let mut s = state();
        merge(
            &mut s,
            vec![
                line_at("2026-01-01T12:00:00Z", "INF", "late"),
                line_at("2026-01-01T11:00:00Z", "INF", "early"),
            ],
        );
        let order: Vec<_> = s.lines.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(order, ["late", "early"]);
    }

    #[test]
    fn test_merge_skips_held_lines() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b")]);
        merge(&mut s, vec![line("INF", "b"), line("INF", "c")]);
        assert_eq!(s.lines.len(), 3);
    }

    #[test]
    fn test_merge_keeps_equal_text_with_different_when() {
        let mut s = state_with(&[("INF", "a")]);
        merge(&mut s, vec![line_at("2026-01-01T10:00:01Z", "INF", "a")]);
        assert_eq!(s.lines.len(), 2);
    }

    #[test]
    fn test_merge_caps_at_1000() {
        let mut s = state();
        merge(&mut s, numbered(0..1000));
        merge(&mut s, numbered(1000..1005));
        assert_eq!(s.lines.len(), 1000);
        assert_eq!(s.lines[0].message, "m5");
    }

    #[test]
    fn test_newest_when_ignores_order() {
        let lines: Vec<_> = ["10", "12", "11"]
            .iter()
            .map(|h| line_at(&format!("2026-01-01T{h}:00:00Z"), "INF", "m"))
            .collect();
        assert_eq!(newest_when(&lines), Some("2026-01-01T12:00:00Z"));
        assert_eq!(newest_when(&[]), None);
        assert_eq!(newest_when(&[line_at("junk", "INF", "m")]), None);
    }

    #[test]
    fn test_newest_when_compares_offsets() {
        let lines = [
            line_at("2026-01-01T10:00:00+02:00", "INF", "m"),
            line_at("2026-01-01T09:00:00Z", "INF", "m"),
        ];
        assert_eq!(newest_when(&lines), Some("2026-01-01T09:00:00Z"));
    }

    // ========================================
    // SELECTION
    // ========================================

    #[test]
    fn test_follows_at_bottom() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b")]);
        merge(&mut s, vec![line("INF", "c")]);
        assert_eq!(selected_message(&s), Some("c"));
    }

    #[test]
    fn test_stays_when_scrolled_up() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b")]);
        move_selection(&mut s, -1);
        merge(&mut s, vec![line("INF", "c")]);
        assert_eq!(selected_message(&s), Some("a"));
    }

    #[test]
    fn test_shifts_when_old_lines_drop() {
        let mut s = state();
        merge(&mut s, numbered(0..1000));
        s.selected = Some(500);
        merge(&mut s, numbered(1000..1005));
        assert_eq!(selected_message(&s), Some("m500"));
    }

    #[test]
    fn test_dropped_selection_moves_to_first_visible() {
        let mut s = state();
        merge(&mut s, numbered(0..1000));
        s.selected = Some(2);
        merge(&mut s, numbered(1000..1005));
        assert_eq!(s.selected, Some(0));
    }

    #[test]
    fn test_follows_last_visible_under_filter() {
        let mut s = state_with(&[("WRN", "w1"), ("INF", "i1")]);
        cycle_filter(&mut s);
        assert_eq!(selected_message(&s), Some("w1"));
        merge(&mut s, vec![line("INF", "i2"), line("WRN", "w2")]);
        assert_eq!(selected_message(&s), Some("w2"));
    }

    #[test]
    fn test_last_visible_drops_with_no_other_match() {
        let mut s = state_with(&[("WRN", "w1")]);
        cycle_filter(&mut s);
        merge(&mut s, numbered(0..1000));
        assert_eq!(s.selected, None);
    }

    #[test]
    fn test_no_match_then_match() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b")]);
        s.filter = LogFilter::Warnings;
        cycle_filter(&mut s);
        assert_eq!(s.filter, LogFilter::Errors);
        assert_eq!(s.selected, None);
        move_selection(&mut s, 1);
        move_selection(&mut s, -10);
        select_first(&mut s);
        select_last(&mut s);
        assert_eq!(s.selected, None);
        merge(&mut s, vec![line("ERR", "e")]);
        assert_eq!(selected_message(&s), Some("e"));
    }

    #[test]
    fn test_cycle_keeps_visible_line() {
        let mut s = state_with(&[("WRN", "w1"), ("INF", "i"), ("WRN", "w2")]);
        select_first(&mut s);
        cycle_filter(&mut s);
        assert_eq!(selected_message(&s), Some("w1"));
    }

    #[test]
    fn test_cycle_moves_to_nearest() {
        let mut s = state_with(&[("WRN", "w1"), ("INF", "i1"), ("INF", "i2"), ("WRN", "w2")]);
        s.selected = Some(1);
        cycle_filter(&mut s);
        assert_eq!(selected_message(&s), Some("w1"));

        let mut s = state_with(&[("WRN", "w1"), ("INF", "i"), ("WRN", "w2")]);
        s.selected = Some(1);
        cycle_filter(&mut s);
        assert_eq!(selected_message(&s), Some("w2"));
    }

    #[test]
    fn test_cycle_from_empty_selects_last() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b")]);
        cycle_filter(&mut s);
        cycle_filter(&mut s);
        assert_eq!(s.selected, None);
        cycle_filter(&mut s);
        assert_eq!(selected_message(&s), Some("b"));
    }

    #[test]
    fn test_move_clamps_at_ends() {
        let mut s = state_with(&[("INF", "a"), ("INF", "b"), ("INF", "c")]);
        move_selection(&mut s, 10);
        assert_eq!(s.selected, Some(2));
        move_selection(&mut s, -10);
        assert_eq!(s.selected, Some(0));
        select_last(&mut s);
        assert_eq!(s.selected, Some(2));
        select_first(&mut s);
        assert_eq!(s.selected, Some(0));
    }

    #[test]
    fn test_move_skips_hidden_lines() {
        let mut s = state_with(&[("WRN", "w1"), ("INF", "i"), ("WRN", "w2")]);
        cycle_filter(&mut s);
        move_selection(&mut s, -1);
        assert_eq!(selected_message(&s), Some("w1"));
    }

    // ========================================
    // VERSION
    // ========================================

    #[test]
    fn test_supports_log_viewer() {
        for version in [
            None,
            Some("v2.0.0"),
            Some("v2.1.7"),
            Some("v10.0.0"),
            Some("weird"),
        ] {
            assert!(supports_log_viewer(version), "{version:?}");
        }
        assert!(!supports_log_viewer(Some("v1.30.0")));
    }

    #[test]
    fn test_unsupported_message() {
        assert_eq!(
            unsupported_message(Some("v1.30.0")),
            "The log viewer needs Syncthing 2.0 or newer (this device runs v1.30.0)"
        );
        assert_eq!(
            unsupported_message(None),
            "The log viewer needs Syncthing 2.0 or newer"
        );
    }

    #[test]
    fn test_unsupported_reply() {
        let numeric = LogLine {
            level: RawLevel::Number(2),
            ..line("INF", "m")
        };
        assert!(is_unsupported_reply(&[numeric]));
        assert!(!is_unsupported_reply(&[line("INF", "m")]));
        assert!(!is_unsupported_reply(&[]));
    }
}
