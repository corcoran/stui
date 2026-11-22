//! Preview text wrapped once into screen rows, so a frame draws only the rows on screen.
use ratatui::buffer::CellWidth;
use ratatui::layout::Alignment;
use ratatui::style::Style;
use ratatui::text::{Line, Span, StyledGrapheme, Text};
use std::collections::VecDeque;

/// `text` word-wrapped to `width` columns as ratatui's `Paragraph` wraps it with
/// `Wrap { trim: false }`, one `Line` per screen row. Must stay in step with ratatui's
/// `WordWrapper`, or rows drift from what wrapping the whole text would draw.
pub fn wrap_rows(text: &Text, width: u16) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if width == 0 {
        return rows;
    }
    for line in &text.lines {
        let alignment = line.alignment;
        for row in wrap_line(line.styled_graphemes(text.style), width) {
            rows.push(to_line(&row, alignment));
        }
    }
    rows
}

/// Port of ratatui's `WordWrapper::process_input` with `trim` false, except that a
/// double-width symbol that doesn't fit starts the next row.
fn wrap_line<'a>(
    graphemes: impl Iterator<Item = StyledGrapheme<'a>>,
    max_width: u16,
) -> Vec<Vec<StyledGrapheme<'a>>> {
    let mut rows = Vec::new();
    let mut pending_line: Vec<StyledGrapheme<'a>> = Vec::new();
    let mut pending_word: Vec<StyledGrapheme<'a>> = Vec::new();
    let mut pending_whitespace: VecDeque<StyledGrapheme<'a>> = VecDeque::new();
    let mut line_width: u16 = 0;
    let mut word_width: u16 = 0;
    let mut whitespace_width: u16 = 0;
    let mut non_whitespace_previous = false;

    for grapheme in graphemes {
        let is_whitespace = is_whitespace(&grapheme);
        let symbol_width = grapheme.symbol.cell_width();
        if symbol_width > max_width {
            continue;
        }

        let word_found = non_whitespace_previous && is_whitespace;
        let untrimmed_overflow =
            pending_line.is_empty() && word_width + whitespace_width + symbol_width > max_width;
        if word_found || untrimmed_overflow {
            pending_line.extend(pending_whitespace.drain(..));
            line_width += whitespace_width;
            pending_line.append(&mut pending_word);
            line_width += word_width;
            whitespace_width = 0;
            word_width = 0;
        }

        let line_full = line_width >= max_width;
        // Ratatui tests `>= max_width` before adding the symbol, which lets a double-width
        // symbol overhang; counting the symbol is the same test for width 1.
        let pending_word_overflow = symbol_width > 0
            && line_width + whitespace_width + word_width + symbol_width > max_width;
        if line_full || pending_word_overflow {
            let mut remaining_width = max_width.saturating_sub(line_width);
            rows.push(std::mem::take(&mut pending_line));
            line_width = 0;
            while let Some(front) = pending_whitespace.front() {
                let width = front.symbol.cell_width();
                if width > remaining_width {
                    break;
                }
                whitespace_width -= width;
                remaining_width -= width;
                pending_whitespace.pop_front();
            }
            if is_whitespace && pending_whitespace.is_empty() {
                continue;
            }
        }

        if is_whitespace {
            whitespace_width += symbol_width;
            pending_whitespace.push_back(grapheme);
        } else {
            word_width += symbol_width;
            pending_word.push(grapheme);
        }
        non_whitespace_previous = !is_whitespace;
    }

    pending_line.extend(pending_whitespace.drain(..));
    pending_line.append(&mut pending_word);
    if !pending_line.is_empty() {
        rows.push(pending_line);
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

/// Same rule as ratatui's `StyledGrapheme::is_whitespace`, which is not public.
fn is_whitespace(grapheme: &StyledGrapheme) -> bool {
    const ZWSP: &str = "\u{200b}";
    const NBSP: &str = "\u{00a0}";
    let symbol = grapheme.symbol;
    symbol == ZWSP || symbol.chars().all(char::is_whitespace) && symbol != NBSP
}

/// One row as a line of owned spans, merging neighbours that share a style.
fn to_line(row: &[StyledGrapheme], alignment: Option<Alignment>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_style = Style::default();
    for grapheme in row {
        if grapheme.style != current_style && !current.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut current), current_style));
        }
        current_style = grapheme.style;
        current.push_str(grapheme.symbol);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, current_style));
    }
    let line = Line::from(spans);
    match alignment {
        Some(a) => line.alignment(a),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;
    use ratatui::widgets::{Paragraph, Widget, Wrap};

    fn draw(paragraph: Paragraph, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        paragraph.render(area, &mut buf);
        buf
    }

    /// Drawing `wrap_rows` without wrapping matches ratatui wrapping the whole text.
    fn assert_matches_ratatui(text: Text<'static>, width: u16) {
        let text_dbg = text.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let expected_rows = Paragraph::new(text.clone())
            .wrap(Wrap { trim: false })
            .line_count(width);
        let rows = wrap_rows(&text, width);
        assert_eq!(rows.len(), expected_rows, "row count at width {width}");
        let height = expected_rows as u16 + 1;
        let expected = draw(
            Paragraph::new(text).wrap(Wrap { trim: false }),
            width,
            height,
        );
        let actual = draw(Paragraph::new(Text::from(rows)), width, height);
        assert_eq!(
            actual, expected,
            "cells at width {width} for {:?}",
            text_dbg
        );
    }

    fn corpus() -> Vec<Text<'static>> {
        vec![
            Text::from("short"),
            Text::from(""),
            Text::from("one two three four five six seven eight nine ten eleven twelve"),
            Text::from("a\n\nb\n"),
            Text::from("   leading spaces and  double  gaps   trailing   "),
            Text::from("averyveryveryveryveryveryverylongwordwithoutanybreaksatallhere"),
            Text::from("mixed averyveryveryveryverylongword tail and more words after it"),
            Text::from(
                r#"{"id":1,"name":"user 1","tags":["a","b"],"ok":true},{"id":2,"name":"user 2"}"#,
            ),
            Text::from(vec![
                Line::from(vec![
                    Span::styled("red words ", Style::default().fg(Color::Red)),
                    Span::styled(
                        "bold continued text",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" plain tail that wraps around"),
                ]),
                Line::from(Span::styled(
                    "x".repeat(50),
                    Style::default().bg(Color::Blue),
                )),
            ]),
            Text::from(
                Line::from("centered line that is long enough to wrap twice or so").centered(),
            ),
            Text::from("tab\tseparated\tvalues\there"),
        ]
    }

    #[test]
    fn test_rows_match_ratatui_wrapping() {
        for text in corpus() {
            for width in [1, 2, 5, 10, 17, 40, 80] {
                assert_matches_ratatui(text.clone(), width);
            }
        }
    }

    #[test]
    fn test_rows_keep_text_style() {
        let text = Text::from("styled text that wraps over lines")
            .style(Style::default().fg(Color::Green));
        for width in [6, 12] {
            assert_matches_ratatui(text.clone(), width);
        }
    }

    /// Ratatui lets a double-width character overhang the row and draws it half outside the
    /// pane; these rows break before it instead, so every character shows.
    #[test]
    fn test_wide_characters_never_overhang_a_row() {
        let text =
            Text::from("wide 漢字のテキストがここにあります and ascii 漢字漢字漢字漢字漢字漢字");
        let visible = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        for width in [2, 3, 5, 7, 10, 17] {
            let rows = wrap_rows(&text, width);
            for row in &rows {
                assert!(
                    row.width() <= width as usize,
                    "{:?} wider than {width}",
                    row.to_string()
                );
            }
            let joined: String = rows.iter().map(|r| r.to_string()).collect();
            assert_eq!(
                visible(&joined),
                visible(&text.lines[0].to_string()),
                "width {width}"
            );
        }
    }

    #[test]
    fn test_zero_width_gives_no_rows() {
        assert!(wrap_rows(&Text::from("abc"), 0).is_empty());
    }
}
