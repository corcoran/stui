//! Dispatch from a [`RenderedView`] to its renderer.

use crate::logic::file::{DEFAULT_ART_WIDTH, RenderedView, parse_ansi_to_text};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

/// JSON, CSV and TSV above this size show as the raw file.
pub const MAX_FORMATTED_BYTES: usize = 2 * 1024 * 1024;

/// Render `content` for `view` at `width` columns.
pub fn render(view: RenderedView, content: &str, width: u16) -> Text<'static> {
    if view == RenderedView::Markdown {
        return crate::logic::markdown::render(content, width);
    }
    if content.len() > MAX_FORMATTED_BYTES {
        let notice = format!(
            "Too large to format ({}); showing the file as is",
            crate::utils::format_bytes(content.len() as u64)
        );
        let mut lines = vec![Line::from(Span::styled(
            notice,
            Style::default().fg(Color::Yellow),
        ))];
        lines.extend(parse_ansi_to_text(content, DEFAULT_ART_WIDTH).lines);
        return Text::from(lines);
    }
    match view {
        RenderedView::Json => crate::logic::json::render(content),
        RenderedView::Csv => crate::logic::csv::render(content, None, width),
        RenderedView::Tsv => crate::logic::csv::render(content, Some('\t'), width),
        RenderedView::Markdown => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_large_json_is_not_formatted() {
        let big = format!("[{}1]", "1,".repeat(MAX_FORMATTED_BYTES / 2));
        let t = render(RenderedView::Json, &big, 80);
        let first: String = t.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(first.starts_with("Too large to format ("), "{}", first);
        assert!(first.ends_with("); showing the file as is"), "{}", first);
    }

    #[test]
    fn test_small_json_is_formatted() {
        let t = render(RenderedView::Json, r#"{"a":1}"#, 80);
        assert_eq!(t.lines.len(), 3);
    }

    #[test]
    fn test_tsv_and_csv_dispatch() {
        let first = |v, s| -> String {
            render(v, s, 80).lines[0]
                .spans
                .iter()
                .map(|x| x.content.as_ref())
                .collect()
        };
        assert_eq!(first(RenderedView::Tsv, "a,b\tc\n"), "a,b │ c");
        assert_eq!(first(RenderedView::Csv, "a;b\n"), "a │ b");
    }
}
