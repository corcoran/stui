//! JSON re-indented with light colors for the file preview.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

enum Token<'a> {
    Str(&'a str),
    Punct(char),
    Lit(&'a str),
}

/// Re-indents `content` with two spaces and colors keys, strings, numbers and literals.
/// Invalid JSON gets a note on the first line followed by the raw content.
pub fn render(content: &str) -> Text<'static> {
    if content.trim().is_empty() {
        return Text::default();
    }
    if let Err(err) = serde_json::from_str::<serde::de::IgnoredAny>(content) {
        return invalid(content, &err);
    }
    layout(&tokenize(content))
}

fn invalid(content: &str, err: &serde_json::Error) -> Text<'static> {
    let suffix = format!(" at line {} column {}", err.line(), err.column());
    let full = err.to_string();
    let message = full.strip_suffix(&suffix).unwrap_or(&full);
    let note = format!(
        "Not valid JSON (line {}, column {}): {}",
        err.line(),
        err.column(),
        message
    );
    let mut lines = vec![Line::from(Span::styled(
        note,
        Style::default().fg(Color::Yellow),
    ))];
    lines.extend(content.lines().map(|l| Line::from(l.to_string())));
    Text::from(lines)
}

fn tokenize(content: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut chars = content.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        match c {
            '{' | '}' | '[' | ']' | ',' | ':' => tokens.push(Token::Punct(c)),
            '"' => {
                let mut end = content.len();
                let mut escaped = false;
                for (i, c) in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        end = i + 1;
                        break;
                    }
                }
                tokens.push(Token::Str(&content[start..end]));
            }
            _ => {
                let mut end = start + c.len_utf8();
                while let Some(&(i, n)) = chars.peek() {
                    if n.is_whitespace() || matches!(n, '{' | '}' | '[' | ']' | ',' | ':' | '"') {
                        break;
                    }
                    end = i + n.len_utf8();
                    chars.next();
                }
                tokens.push(Token::Lit(&content[start..end]));
            }
        }
    }
    tokens
}

fn layout(tokens: &[Token<'_>]) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        if spans.is_empty() {
            spans.push(Span::raw("  ".repeat(depth)));
        }
        match &tokens[i] {
            Token::Punct(open @ ('{' | '[')) => {
                let close = if *open == '{' { '}' } else { ']' };
                if matches!(tokens.get(i + 1), Some(Token::Punct(c)) if *c == close) {
                    spans.push(Span::raw(format!("{open}{close}")));
                    i += 1;
                } else {
                    spans.push(Span::raw(open.to_string()));
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    depth += 1;
                }
            }
            Token::Punct(close @ ('}' | ']')) => {
                depth = depth.saturating_sub(1);
                if spans.len() > 1 {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    spans.push(Span::raw("  ".repeat(depth)));
                } else {
                    spans[0] = Span::raw("  ".repeat(depth));
                }
                spans.push(Span::raw(close.to_string()));
            }
            Token::Punct(',') => {
                spans.push(Span::raw(","));
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            Token::Punct(_) => spans.push(Span::raw(": ")),
            Token::Str(s) => {
                let is_key = matches!(tokens.get(i + 1), Some(Token::Punct(':')));
                let color = if is_key { Color::Cyan } else { Color::Green };
                spans.push(Span::styled(s.to_string(), Style::default().fg(color)));
            }
            Token::Lit(s) => {
                let color = if matches!(*s, "true" | "false" | "null") {
                    Color::Magenta
                } else {
                    Color::Yellow
                };
                spans.push(Span::styled(s.to_string(), Style::default().fg(color)));
            }
        }
        i += 1;
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    Text::from(lines)
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
    fn test_json_nested_layout() {
        let t = render(r#"{"a":1,"b":[true,null],"c":{"d":"x"}}"#);
        assert_eq!(
            lines(&t),
            [
                "{",
                "  \"a\": 1,",
                "  \"b\": [",
                "    true,",
                "    null",
                "  ],",
                "  \"c\": {",
                "    \"d\": \"x\"",
                "  }",
                "}",
            ]
        );
    }

    #[test]
    fn test_json_empty_containers_stay_inline() {
        assert_eq!(
            lines(&render(r#"{"a":{},"b":[ ]}"#)),
            ["{", "  \"a\": {},", "  \"b\": []", "}"]
        );
    }

    #[test]
    fn test_json_keeps_key_order_duplicates_and_numbers() {
        assert_eq!(
            lines(&render(r#"{"z":1.10,"a":1e3,"z":-0}"#)),
            ["{", "  \"z\": 1.10,", "  \"a\": 1e3,", "  \"z\": -0", "}"]
        );
    }

    #[test]
    fn test_json_structural_chars_inside_strings() {
        assert_eq!(
            lines(&render(r#"["a,{b}:[c]\"d", "é"]"#)),
            ["[", r#"  "a,{b}:[c]\"d","#, r#"  "é""#, "]"]
        );
    }

    #[test]
    fn test_json_colors() {
        use ratatui::style::Color;
        let t = render(r#"{"k":"v","n":2,"t":true}"#);
        let style_of = |text: &str| {
            t.lines
                .iter()
                .flat_map(|l| &l.spans)
                .find(|s| s.content == text)
                .unwrap()
                .style
                .fg
        };
        assert_eq!(style_of("\"k\""), Some(Color::Cyan));
        assert_eq!(style_of("\"v\""), Some(Color::Green));
        assert_eq!(style_of("2"), Some(Color::Yellow));
        assert_eq!(style_of("true"), Some(Color::Magenta));
    }

    #[test]
    fn test_json_top_level_scalar() {
        assert_eq!(lines(&render("42")), ["42"]);
    }

    #[test]
    fn test_json_invalid_shows_note_and_raw() {
        let t = render("{\n  \"a\": 1,\n  oops\n}");
        let l = lines(&t);
        assert!(
            l[0].starts_with("Not valid JSON (line 3, column 3): "),
            "{:?}",
            l[0]
        );
        assert!(!l[0].contains(" at line "), "{:?}", l[0]);
        assert_eq!(
            t.lines[0].spans[0].style.fg,
            Some(ratatui::style::Color::Yellow)
        );
        assert_eq!(&l[1..], ["{", "  \"a\": 1,", "  oops", "}"]);
    }

    #[test]
    fn test_json_empty_file_is_blank() {
        assert!(render("").lines.is_empty());
        assert!(render("  \n ").lines.is_empty());
    }
}
