//! CSV and TSV drawn as a table that fits the preview pane.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_COLUMN: usize = 40;
const MIN_COLUMN: usize = 3;
const SEPARATOR: &str = " │ ";

/// Draws `content` as a table fitted to `width` columns: bold header row, a rule, then the rows.
/// `delimiter: None` picks the most frequent of `,` `;` and tab in the first record (`,` on ties).
/// The widest column shrinks first and cut cells end in `…`.
#[allow(dead_code)]
pub fn render(content: &str, delimiter: Option<char>, width: u16) -> Text<'static> {
    let delimiter = delimiter.unwrap_or_else(|| sniff(content));
    let mut rows = parse(content, delimiter);
    if rows.is_empty() {
        return Text::default();
    }
    for row in &mut rows {
        for cell in row.iter_mut() {
            *cell = cell.replace("\r\n", " ").replace(['\n', '\r'], " ");
        }
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(columns, String::new());
    }
    let widths = column_widths(&rows, columns, width as usize);

    let mut lines = Vec::with_capacity(rows.len() + 1);
    for (i, row) in rows.iter().enumerate() {
        lines.push(row_line(row, &widths, i == 0));
        if i == 0 {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            lines.push(Line::from(rule.join("─┼─")));
        }
    }
    Text::from(lines)
}

fn column_widths(rows: &[Vec<String>], columns: usize, width: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = (0..columns)
        .map(|c| {
            rows.iter()
                .map(|r| UnicodeWidthStr::width(r[c].as_str()))
                .max()
                .unwrap_or(0)
                .min(MAX_COLUMN)
        })
        .collect();
    let separators = 3 * columns.saturating_sub(1);
    while widths.iter().sum::<usize>() + separators > width {
        let mut widest = 0;
        for (i, w) in widths.iter().enumerate() {
            if *w > widths[widest] {
                widest = i;
            }
        }
        if widths[widest] <= MIN_COLUMN {
            break;
        }
        widths[widest] -= 1;
    }
    widths
}

fn row_line(row: &[String], widths: &[usize], header: bool) -> Line<'static> {
    let style = if header {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let last = row.len().saturating_sub(1);
    let mut spans = Vec::with_capacity(row.len() * 2);
    for (i, cell) in row.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(SEPARATOR));
        }
        let mut text = fit(cell, widths[i]);
        if i != last {
            let pad = widths[i].saturating_sub(UnicodeWidthStr::width(text.as_str()));
            text.push_str(&" ".repeat(pad));
        }
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Cuts `cell` to `width` display columns, ending in `…` when anything was removed.
fn fit(cell: &str, width: usize) -> String {
    if UnicodeWidthStr::width(cell) <= width {
        return cell.to_string();
    }
    let budget = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for ch in cell.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn sniff(content: &str) -> char {
    let mut counts = [(',', 0usize), (';', 0), ('\t', 0)];
    let mut in_quotes = false;
    for ch in content.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if ch == '\n' {
                break;
            }
            if let Some(entry) = counts.iter_mut().find(|(d, _)| *d == ch) {
                entry.1 += 1;
            }
        }
    }
    let mut best = counts[0];
    for entry in &counts[1..] {
        if entry.1 > best.1 {
            best = *entry;
        }
    }
    best.0
}

fn parse(content: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_start = true;
    let mut pending = false;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        pending = true;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        if ch == '"' && field_start {
            in_quotes = true;
            field_start = false;
        } else if ch == delimiter {
            row.push(std::mem::take(&mut field));
            field_start = true;
        } else if ch == '\n' || (ch == '\r' && chars.peek() == Some(&'\n')) {
            if ch == '\r' {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            field_start = true;
            pending = false;
        } else {
            field.push(ch);
            field_start = false;
        }
    }
    if pending {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(t: &Text) -> Vec<String> {
        t.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn test_csv_basic_table() {
        let t = render("name,age\nAda,36\nAlan,41\n", None, 80);
        assert_eq!(
            lines(&t),
            ["name │ age", "─────┼────", "Ada  │ 36", "Alan │ 41"]
        );
        assert!(
            t.lines[0]
                .spans
                .iter()
                .filter(|s| !s.content.trim().is_empty() && s.content != " │ ")
                .all(|s| s
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD))
        );
    }
    #[test]
    fn test_csv_quoting() {
        let t = render(
            "a,b\n\"x, y\",\"say \"\"hi\"\"\"\n\"two\nlines\",z\n",
            None,
            80,
        );
        assert_eq!(&lines(&t)[2..], ["x, y      │ say \"hi\"", "two lines │ z"]);
    }
    #[test]
    fn test_csv_unterminated_quote() {
        assert_eq!(
            &lines(&render("a,b\n1,\"open\n2,3", None, 80))[2..],
            ["1 │ open 2,3"]
        );
    }
    #[test]
    fn test_csv_sniffs_delimiter() {
        assert_eq!(lines(&render("a;b\n1;2", None, 80))[0], "a │ b");
        assert_eq!(lines(&render("a\tb\n1\t2", None, 80))[0], "a │ b");
        assert_eq!(lines(&render("a;b,c\n", None, 80))[0], "a;b │ c");
    }
    #[test]
    fn test_tsv_uses_tab() {
        assert_eq!(lines(&render("a,b\tc\n", Some('\t'), 80))[0], "a,b │ c");
    }
    #[test]
    fn test_csv_crlf_records() {
        assert_eq!(
            lines(&render("a,b\r\n1,2\r\n", None, 80)),
            ["a │ b", "──┼──", "1 │ 2"]
        );
    }
    #[test]
    fn test_csv_ragged_rows_padded() {
        assert_eq!(lines(&render("a,b,c\n1\n", None, 80))[2], "1 │   │ ");
    }
    #[test]
    fn test_csv_shrinks_widest_column_first() {
        let t = render("aaaaaaaaaa,bbbb\nx,y\n", None, 12);
        assert_eq!(lines(&t)[0], "aaaa… │ bbbb");
    }
    #[test]
    fn test_csv_column_cap_40() {
        let long = "x".repeat(60);
        let first = lines(&render(&format!("{},b\n", long), None, 200))[0].clone();
        assert!(
            first.starts_with(&format!("{}… │", "x".repeat(39))),
            "{}",
            first
        );
    }
    #[test]
    fn test_csv_many_columns_narrow_pane() {
        let header = (0..30)
            .map(|i| format!("col{}", i))
            .collect::<Vec<_>>()
            .join(",");
        let t = render(&header, None, 40);
        assert!(lines(&t)[0].starts_with("co… │ co…"));
        let _ = render(&header, None, 0);
    }
    #[test]
    fn test_csv_wide_characters() {
        let t = render("名前,x\n東京都庁舎,y\n", None, 8);
        for line in lines(&t) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= 8 + 1,
                "{}",
                line
            );
        }
    }
    #[test]
    fn test_csv_empty_and_header_only() {
        assert!(render("", None, 80).lines.is_empty());
        assert_eq!(lines(&render("a,b\n", None, 80)), ["a │ b", "──┼──"]);
    }
}
