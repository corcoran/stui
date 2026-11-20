//! Markdown to styled, pre-wrapped lines for the file preview.

use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const BULLETS: [&str; 3] = ["•", "◦", "▪"];
const CODE_BG: Color = Color::Indexed(236);

/// Renders `text` as lines no wider than `width` columns.
pub fn render(text: &str, width: u16) -> Text<'static> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::new(width as usize);
    for event in Parser::new_ext(text, options) {
        r.event(event);
    }
    r.flush_inline();
    Text::from(r.lines)
}

enum Inline {
    Text(String, Style),
    Break,
}

enum Container {
    Quote,
    Margin,
    Item { marker: String, used: bool },
}

struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<(char, Style)>>>,
    row: Vec<Vec<(char, Style)>>,
    has_head: bool,
}

struct Renderer {
    width: usize,
    lines: Vec<Line<'static>>,
    inline: Vec<Inline>,
    styles: Vec<Style>,
    containers: Vec<Container>,
    lists: Vec<Option<u64>>,
    links: Vec<(String, usize, LinkType)>,
    code: Option<(String, String)>,
    in_html_block: bool,
    table: Option<Table>,
    blank_pending: bool,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Self {
            width,
            lines: Vec::new(),
            inline: Vec::new(),
            styles: Vec::new(),
            containers: Vec::new(),
            lists: Vec::new(),
            links: Vec::new(),
            code: None,
            in_html_block: false,
            table: None,
            blank_pending: false,
        }
    }

    fn style(&self) -> Style {
        self.styles
            .iter()
            .fold(Style::default(), |acc, s| acc.patch(*s))
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => self.inline.push(Inline::Text(t.to_string(), self.style())),
            },
            Event::Code(t) => {
                let style = self
                    .style()
                    .patch(Style::default().fg(Color::Yellow).bg(CODE_BG));
                self.inline.push(Inline::Text(t.to_string(), style));
            }
            Event::Html(t) if self.in_html_block => {
                let dim = self.style().add_modifier(Modifier::DIM);
                for line in t.lines() {
                    self.inline.push(Inline::Text(line.to_string(), dim));
                    self.inline.push(Inline::Break);
                }
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let dim = self.style().add_modifier(Modifier::DIM);
                self.inline.push(Inline::Text(t.to_string(), dim));
            }
            Event::SoftBreak => self
                .inline
                .push(Inline::Text(" ".to_string(), self.style())),
            Event::HardBreak => self.inline.push(Inline::Break),
            Event::Rule => {
                self.block_start();
                let prefix = self.prefix();
                let rule = "─".repeat(self.avail(&prefix));
                self.push_prefixed(prefix, vec![Span::styled(rule, dim())]);
                self.blank_pending = true;
            }
            Event::TaskListMarker(done) => {
                if let Some(Container::Item {
                    marker,
                    used: false,
                }) = self.containers.last_mut()
                {
                    *marker = if done { "☑ " } else { "☐ " }.to_string();
                }
            }
            Event::FootnoteReference(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                self.inline.push(Inline::Text(t.to_string(), self.style()));
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.block_start(),
            Tag::Heading { level, .. } => {
                self.block_start();
                let style = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => Style::default().fg(Color::Cyan),
                    HeadingLevel::H3 => Style::default().fg(Color::Yellow),
                    _ => Style::default(),
                };
                self.styles.push(style.add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.block_start();
                self.containers.push(Container::Quote);
                self.styles.push(
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Tag::CodeBlock(kind) => {
                self.block_start();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {
                self.block_start();
                self.in_html_block = true;
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.blank_if_pending();
                    self.containers.push(Container::Margin);
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_inline();
                let depth = self.lists.len().max(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{}. ", n);
                        *n += 1;
                        m
                    }
                    _ => format!("{} ", BULLETS[(depth - 1) % BULLETS.len()]),
                };
                self.containers.push(Container::Item {
                    marker,
                    used: false,
                });
            }
            Tag::Table(alignments) => {
                self.block_start();
                self.table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                    row: Vec::new(),
                    has_head: false,
                });
            }
            Tag::TableHead => {
                self.styles
                    .push(Style::default().add_modifier(Modifier::BOLD));
                if let Some(t) = &mut self.table {
                    t.has_head = true;
                }
            }
            Tag::TableRow | Tag::TableCell => {}
            Tag::Emphasis => self
                .styles
                .push(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self
                .styles
                .push(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                self.links
                    .push((dest_url.to_string(), self.inline.len(), link_type));
                self.styles.push(
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Tag::Image { .. } => self
                .inline
                .push(Inline::Text("🖼 ".to_string(), self.style())),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                self.blank_pending = true;
            }
            TagEnd::Heading(level) => {
                self.flush_inline();
                let style = self.style();
                self.styles.pop();
                let rule = match level {
                    HeadingLevel::H1 => Some("━"),
                    HeadingLevel::H2 => Some("─"),
                    _ => None,
                };
                if let Some(ch) = rule {
                    let prefix = self.prefix();
                    let line = ch.repeat(self.avail(&prefix));
                    let rule_style = Style::default().fg(style.fg.unwrap_or(Color::Reset));
                    self.push_prefixed(prefix, vec![Span::styled(line, rule_style)]);
                }
                self.blank_pending = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.containers.pop();
                self.styles.pop();
                self.blank_pending = true;
            }
            TagEnd::CodeBlock => {
                if let Some((lang, buf)) = self.code.take() {
                    self.emit_code(&lang, &buf);
                }
                self.blank_pending = true;
            }
            TagEnd::HtmlBlock => {
                self.flush_inline();
                self.in_html_block = false;
                self.blank_pending = true;
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.containers.pop();
                    self.blank_pending = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                self.containers.pop();
            }
            TagEnd::TableCell => {
                let cell = inline_chars(std::mem::take(&mut self.inline));
                if let Some(t) = &mut self.table {
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if tag == TagEnd::TableHead {
                    self.styles.pop();
                }
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit_table(t);
                }
                self.blank_pending = true;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                if let Some((url, start, link_type)) = self.links.pop() {
                    let text: String = self.inline[start.min(self.inline.len())..]
                        .iter()
                        .filter_map(|i| match i {
                            Inline::Text(t, _) => Some(t.as_str()),
                            Inline::Break => None,
                        })
                        .collect();
                    let repeats = url.is_empty()
                        || text == url
                        || format!("mailto:{}", text) == url
                        || matches!(link_type, LinkType::Email);
                    if !repeats {
                        let style = self.style().add_modifier(Modifier::DIM);
                        self.inline.push(Inline::Text(format!(" ({})", url), style));
                    }
                }
            }
            _ => {}
        }
    }

    fn block_start(&mut self) {
        self.flush_inline();
        self.blank_if_pending();
    }

    fn blank_if_pending(&mut self) {
        if self.blank_pending && !self.lines.is_empty() {
            let prefix = self.rest_prefix();
            let text: String = prefix.iter().map(|s| s.content.as_ref()).collect();
            let keep = text.trim_end().width();
            let mut spans = Vec::new();
            let mut w = 0;
            for span in prefix {
                if w >= keep {
                    break;
                }
                w += span.width();
                let content = span.content.trim_end().to_string();
                spans.push(Span::styled(content, span.style));
            }
            self.push_line(Line::from(spans));
        }
        self.blank_pending = false;
    }

    /// Prefix for the next line: unused list markers are shown once.
    fn prefix(&self) -> Vec<Span<'static>> {
        self.build_prefix(true)
    }

    fn rest_prefix(&self) -> Vec<Span<'static>> {
        self.build_prefix(false)
    }

    fn build_prefix(&self, first: bool) -> Vec<Span<'static>> {
        self.containers
            .iter()
            .map(|c| match c {
                Container::Quote => Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Container::Margin => Span::raw("  "),
                Container::Item { marker, used } if first && !used => Span::raw(marker.clone()),
                Container::Item { marker, .. } => Span::raw(" ".repeat(marker.width())),
            })
            .collect()
    }

    fn mark_markers_used(&mut self) {
        for c in &mut self.containers {
            if let Container::Item { used, .. } = c {
                *used = true;
            }
        }
    }

    fn avail(&self, prefix: &[Span]) -> usize {
        let w: usize = prefix.iter().map(|s| s.width()).sum();
        self.width.saturating_sub(w)
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.lines.push(clip(line, self.width));
    }

    fn push_prefixed(&mut self, mut prefix: Vec<Span<'static>>, content: Vec<Span<'static>>) {
        prefix.extend(content);
        self.push_line(Line::from(prefix));
        self.mark_markers_used();
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let items = std::mem::take(&mut self.inline);
        let lines = wrap(items, self.prefix(), self.rest_prefix(), self.width);
        if !lines.is_empty() {
            for line in lines {
                self.push_line(line);
            }
            self.mark_markers_used();
        }
    }

    fn emit_code(&mut self, lang: &str, buf: &str) {
        let bg = Style::default().bg(CODE_BG);
        if !lang.is_empty() {
            let prefix = self.prefix();
            let avail = self.avail(&prefix);
            let label = pad(format!("  {}", lang), avail);
            self.push_prefixed(
                prefix,
                vec![Span::styled(label, bg.add_modifier(Modifier::DIM))],
            );
        }
        for source in buf.trim_end_matches('\n').split('\n') {
            let source = source.replace('\t', "    ");
            let prefix = self.prefix();
            let avail = self.avail(&prefix);
            let room = avail.saturating_sub(2).max(1);
            for chunk in chunk_by_width(&source, room) {
                let prefix = self.prefix();
                let content = pad(format!("  {}", chunk), avail);
                self.push_prefixed(prefix, vec![Span::styled(content, bg)]);
            }
        }
    }

    fn emit_table(&mut self, t: Table) {
        let ncols = t
            .rows
            .iter()
            .map(|r| r.len())
            .max()
            .unwrap_or(0)
            .max(t.alignments.len());
        if ncols == 0 {
            return;
        }
        let mut widths = vec![1usize; ncols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(chars_width(cell));
            }
        }
        let prefix = self.prefix();
        let budget = self.avail(&prefix).saturating_sub(3 * ncols + 1);
        while widths.iter().sum::<usize>() > budget {
            let (i, w) = widths
                .iter()
                .enumerate()
                .max_by_key(|(_, w)| **w)
                .map(|(i, w)| (i, *w))
                .unwrap_or((0, 1));
            if w <= 1 {
                break;
            }
            widths[i] -= 1;
        }

        let border = |l: &str, m: &str, r: &str| -> Vec<Span<'static>> {
            let inner: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            vec![Span::styled(format!("{}{}{}", l, inner.join(m), r), dim())]
        };

        let top = border("┌", "┬", "┐");
        let mid = border("├", "┼", "┤");
        let bottom = border("└", "┴", "┘");
        self.push_prefixed(self.prefix(), top);
        for (ri, row) in t.rows.iter().enumerate() {
            let mut spans = vec![Span::styled("│", dim())];
            for (ci, w) in widths.iter().enumerate() {
                let cell = row.get(ci).cloned().unwrap_or_default();
                let cell = truncate(cell, *w);
                let gap = w - chars_width(&cell);
                let (left, right) = match t.alignments.get(ci) {
                    Some(Alignment::Right) => (gap, 0),
                    Some(Alignment::Center) => (gap / 2, gap - gap / 2),
                    _ => (0, gap),
                };
                spans.push(Span::raw(" ".repeat(left + 1)));
                spans.extend(group(&cell));
                spans.push(Span::raw(" ".repeat(right + 1)));
                spans.push(Span::styled("│", dim()));
            }
            self.push_prefixed(self.prefix(), spans);
            if ri == 0 && t.has_head {
                self.push_prefixed(self.prefix(), mid.clone());
            }
        }
        self.push_prefixed(self.prefix(), bottom);
    }
}

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

fn chars_width(chars: &[(char, Style)]) -> usize {
    chars.iter().map(|(c, _)| char_width(*c)).sum()
}

fn pad(s: String, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

fn chunk_by_width(s: &str, width: usize) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > width && w > 0 {
            chunks.push(String::new());
            w = 0;
        }
        chunks.last_mut().unwrap().push(c);
        w += cw;
    }
    chunks
}

fn inline_chars(items: Vec<Inline>) -> Vec<(char, Style)> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Inline::Text(t, style) => out.extend(t.chars().map(|c| (c, style))),
            Inline::Break => out.push((' ', Style::default())),
        }
    }
    out
}

/// Cuts `cell` to `width` columns, ending in `…` when anything was cut.
fn truncate(cell: Vec<(char, Style)>, width: usize) -> Vec<(char, Style)> {
    if chars_width(&cell) <= width {
        return cell;
    }
    let mut out = Vec::new();
    let mut w = 0;
    let style = cell.first().map(|(_, s)| *s).unwrap_or_default();
    for (c, s) in cell {
        let cw = char_width(c);
        if w + cw + 1 > width {
            break;
        }
        out.push((c, s));
        w += cw;
    }
    out.push(('…', style));
    out
}

/// Joins consecutive same-style characters into spans.
fn group(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<Style> = None;
    for (c, s) in chars {
        if cur != Some(*s) {
            if let Some(style) = cur {
                spans.push(Span::styled(std::mem::take(&mut buf), style));
            }
            cur = Some(*s);
        }
        buf.push(*c);
    }
    if let Some(style) = cur {
        spans.push(Span::styled(buf, style));
    }
    spans
}

/// Truncates a line to `width` columns.
fn clip(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let mut spans = Vec::new();
    let mut w = 0;
    for span in line.spans {
        let mut content = String::new();
        for c in span.content.chars() {
            let cw = char_width(c);
            if w + cw > width {
                break;
            }
            content.push(c);
            w += cw;
        }
        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }
        if w >= width {
            break;
        }
    }
    Line::from(spans)
}

enum Token {
    Word(Vec<(char, Style)>),
    Space(Style),
    Break,
}

fn tokenize(items: Vec<Inline>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<(char, Style)> = Vec::new();
    for item in items {
        match item {
            Inline::Break => {
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                tokens.push(Token::Break);
            }
            Inline::Text(t, style) => {
                for c in t.chars() {
                    if c.is_whitespace() {
                        if !word.is_empty() {
                            tokens.push(Token::Word(std::mem::take(&mut word)));
                        }
                        if !matches!(tokens.last(), Some(Token::Space(_))) {
                            tokens.push(Token::Space(style));
                        }
                    } else {
                        word.push((c, style));
                    }
                }
            }
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

/// Word-wraps inline content to `width`, starting each line with a prefix.
fn wrap(
    items: Vec<Inline>,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    let rest_w: usize = rest.iter().map(|s| s.width()).sum();
    let mut lines = Vec::new();
    let mut prefix = first;
    let mut prefix_w: usize = prefix.iter().map(|s| s.width()).sum();
    let mut content: Vec<(char, Style)> = Vec::new();
    let mut content_w = 0;
    let mut space: Option<Style> = None;

    let finish = |prefix: &mut Vec<Span<'static>>,
                  content: &mut Vec<(char, Style)>,
                  lines: &mut Vec<Line<'static>>| {
        let mut spans = std::mem::take(prefix);
        spans.extend(group(content));
        content.clear();
        lines.push(Line::from(spans));
    };

    for token in tokenize(items) {
        match token {
            Token::Space(style) => {
                if content_w > 0 {
                    space = Some(style);
                }
            }
            Token::Break => {
                finish(&mut prefix, &mut content, &mut lines);
                prefix = rest.clone();
                prefix_w = rest_w;
                content_w = 0;
                space = None;
            }
            Token::Word(word) => {
                let word_w = chars_width(&word);
                let space_w = usize::from(space.is_some() && content_w > 0);
                if content_w > 0 && prefix_w + content_w + space_w + word_w > width {
                    finish(&mut prefix, &mut content, &mut lines);
                    prefix = rest.clone();
                    prefix_w = rest_w;
                    content_w = 0;
                }
                if content_w > 0
                    && let Some(style) = space
                {
                    content.push((' ', style));
                    content_w += 1;
                }
                space = None;
                for (c, s) in word {
                    let cw = char_width(c);
                    let room = width.saturating_sub(prefix_w).max(1);
                    if content_w > 0 && content_w + cw > room {
                        finish(&mut prefix, &mut content, &mut lines);
                        prefix = rest.clone();
                        prefix_w = rest_w;
                        content_w = 0;
                    }
                    content.push((c, s));
                    content_w += cw;
                }
            }
        }
    }
    if content_w > 0 {
        finish(&mut prefix, &mut content, &mut lines);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use unicode_width::UnicodeWidthStr;

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn rows(md: &str, width: u16) -> Vec<String> {
        render(md, width)
            .lines
            .iter()
            .map(|l| line_text(l).trim_end().to_string())
            .collect()
    }

    fn span_with<'a>(text: &'a Text, content: &str) -> &'a Span<'a> {
        text.lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(content))
            .unwrap_or_else(|| panic!("no span containing {:?}", content))
    }

    fn has(style: Style, m: Modifier) -> bool {
        style.add_modifier.contains(m)
    }

    // ========================================
    // HEADINGS
    // ========================================

    #[test]
    fn test_h1_hides_marker_and_has_heavy_rule() {
        let r = rows("# Title", 20);
        assert_eq!(r[0], "Title");
        assert_eq!(r[1], "━".repeat(20));
    }

    #[test]
    fn test_h2_has_light_rule() {
        let r = rows("## Section", 20);
        assert_eq!(r[0], "Section");
        assert_eq!(r[1], "─".repeat(20));
    }

    #[test]
    fn test_h3_has_no_rule() {
        let r = rows("### Sub\n\nbody", 20);
        assert_eq!(r, vec!["Sub", "", "body"]);
    }

    #[test]
    fn test_heading_styles() {
        let t = render("# One\n\n### Three\n\n#### Four", 40);
        let one = span_with(&t, "One").style;
        assert!(has(one, Modifier::BOLD));
        assert_eq!(one.fg, Some(Color::Cyan));
        let three = span_with(&t, "Three").style;
        assert!(has(three, Modifier::BOLD));
        assert_eq!(three.fg, Some(Color::Yellow));
        assert!(has(span_with(&t, "Four").style, Modifier::BOLD));
    }

    // ========================================
    // INLINE STYLES
    // ========================================

    #[test]
    fn test_emphasis_markers_hidden_and_styled() {
        let t = render("*em* **strong** ~~del~~", 40);
        assert_eq!(line_text(&t.lines[0]), "em strong del");
        assert!(has(span_with(&t, "em").style, Modifier::ITALIC));
        assert!(has(span_with(&t, "strong").style, Modifier::BOLD));
        assert!(has(span_with(&t, "del").style, Modifier::CROSSED_OUT));
    }

    #[test]
    fn test_inline_code_hides_backticks_and_has_background() {
        let t = render("run `cargo test` now", 40);
        assert_eq!(line_text(&t.lines[0]), "run cargo test now");
        let code = span_with(&t, "cargo").style;
        assert_eq!(code.fg, Some(Color::Yellow));
        assert!(code.bg.is_some());
    }

    #[test]
    fn test_link_shows_text_then_dim_url() {
        let t = render("see [docs](https://x.io) here", 60);
        assert_eq!(line_text(&t.lines[0]), "see docs (https://x.io) here");
        assert!(has(span_with(&t, "docs").style, Modifier::UNDERLINED));
        assert!(has(span_with(&t, "https://x.io").style, Modifier::DIM));
    }

    #[test]
    fn test_autolink_url_not_repeated() {
        assert_eq!(rows("<https://x.io>", 60), vec!["https://x.io"]);
    }

    #[test]
    fn test_image_shows_alt_text() {
        assert_eq!(rows("![a cat](cat.png)", 40), vec!["🖼 a cat"]);
    }

    #[test]
    fn test_inline_html_is_dim_text() {
        let t = render("a <br> b", 40);
        assert_eq!(line_text(&t.lines[0]), "a <br> b");
        assert!(has(span_with(&t, "<br>").style, Modifier::DIM));
    }

    // ========================================
    // PARAGRAPHS AND WRAPPING
    // ========================================

    #[test]
    fn test_empty_input_renders_nothing() {
        assert!(render("", 40).lines.is_empty());
    }

    #[test]
    fn test_paragraphs_separated_by_one_blank_line() {
        assert_eq!(rows("one\n\n\n\ntwo", 40), vec!["one", "", "two"]);
    }

    #[test]
    fn test_soft_break_joins_with_space() {
        assert_eq!(rows("one\ntwo", 40), vec!["one two"]);
    }

    #[test]
    fn test_hard_break_starts_new_line() {
        assert_eq!(rows("one  \ntwo", 40), vec!["one", "two"]);
    }

    #[test]
    fn test_paragraph_wraps_on_word_boundaries() {
        let r = rows("alpha beta gamma delta epsilon", 12);
        assert_eq!(r, vec!["alpha beta", "gamma delta", "epsilon"]);
    }

    #[test]
    fn test_long_word_is_broken_to_fit() {
        assert_eq!(rows("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn test_wrapping_keeps_styles_across_lines() {
        let t = render("**alpha beta gamma**", 11);
        assert_eq!(t.lines.len(), 2);
        for line in &t.lines {
            for span in line.spans.iter().filter(|s| !s.content.trim().is_empty()) {
                assert!(has(span.style, Modifier::BOLD), "{:?}", span);
            }
        }
    }

    // ========================================
    // LISTS
    // ========================================

    #[test]
    fn test_bullet_list_uses_bullets_with_margin() {
        assert_eq!(rows("- a\n- b", 40), vec!["  • a", "  • b"]);
    }

    #[test]
    fn test_nested_bullets_change_by_depth() {
        let r = rows("- a\n  - b\n    - c\n      - d", 40);
        assert_eq!(r, vec!["  • a", "    ◦ b", "      ▪ c", "        • d"]);
    }

    #[test]
    fn test_wrapped_bullet_has_hanging_indent() {
        let r = rows("- alpha beta gamma delta", 16);
        assert_eq!(r, vec!["  • alpha beta", "    gamma delta"]);
    }

    #[test]
    fn test_wrapped_nested_bullet_has_hanging_indent() {
        let r = rows("- a\n  - alpha beta gamma", 16);
        assert_eq!(r, vec!["  • a", "    ◦ alpha beta", "      gamma"]);
    }

    #[test]
    fn test_list_after_paragraph_has_blank_line_before() {
        assert_eq!(rows("intro\n\n- a", 40), vec!["intro", "", "  • a"]);
    }

    #[test]
    fn test_loose_list_items_separated_by_blank_line() {
        assert_eq!(rows("- a\n\n- b", 40), vec!["  • a", "", "  • b"]);
    }

    #[test]
    fn test_ordered_list_numbers_from_start() {
        let r = rows("3. alpha beta gamma\n4. b", 15);
        assert_eq!(r, vec!["  3. alpha beta", "     gamma", "  4. b"]);
    }

    #[test]
    fn test_task_list_uses_checkboxes() {
        assert_eq!(
            rows("- [ ] todo\n- [x] done", 40),
            vec!["  ☐ todo", "  ☑ done"]
        );
    }

    // ========================================
    // BLOCK QUOTES
    // ========================================

    #[test]
    fn test_block_quote_bar_on_every_wrapped_line() {
        let r = rows("> alpha beta gamma", 12);
        assert_eq!(r, vec!["│ alpha beta", "│ gamma"]);
    }

    #[test]
    fn test_block_quote_text_is_italic() {
        let t = render("> quoted", 40);
        assert!(has(span_with(&t, "quoted").style, Modifier::ITALIC));
    }

    #[test]
    fn test_block_quote_paragraphs_keep_bar_on_blank_line() {
        assert_eq!(rows("> one\n>\n> two", 40), vec!["│ one", "│", "│ two"]);
    }

    // ========================================
    // CODE BLOCKS
    // ========================================

    #[test]
    fn test_code_block_hides_fences_and_indents() {
        let r = rows("```\nlet x = 1;\n  y\n```", 20);
        assert_eq!(r, vec!["  let x = 1;", "    y"]);
    }

    #[test]
    fn test_code_block_lines_shaded_to_full_width() {
        let t = render("```\nx\n```", 20);
        let line = &t.lines[0];
        assert_eq!(line.width(), 20);
        assert!(line.spans.iter().all(|s| s.style.bg.is_some()));
    }

    #[test]
    fn test_code_block_shows_dim_language_label() {
        let t = render("```rust\nfn main() {}\n```", 30);
        assert_eq!(line_text(&t.lines[0]).trim(), "rust");
        assert!(has(span_with(&t, "rust").style, Modifier::DIM));
        assert_eq!(line_text(&t.lines[1]).trim_end(), "  fn main() {}");
    }

    #[test]
    fn test_code_block_long_line_is_hard_wrapped() {
        let r = rows("```\nabcdefghij\n```", 8);
        assert_eq!(r, vec!["  abcdef", "  ghij"]);
    }

    #[test]
    fn test_code_block_keeps_words_unwrapped_and_spacing() {
        let r = rows("```\na    b\n```", 20);
        assert_eq!(r, vec!["  a    b"]);
    }

    // ========================================
    // RULES, TABLES, HTML
    // ========================================

    #[test]
    fn test_horizontal_rule_full_width() {
        let r = rows("a\n\n---\n\nb", 10);
        let rule = "─".repeat(10);
        assert_eq!(r, vec!["a", "", rule.as_str(), "", "b"]);
    }

    #[test]
    fn test_table_is_box_drawn() {
        let r = rows("| a | bb |\n|---|---|\n| 1 | 2 |", 40);
        assert_eq!(
            r,
            vec![
                "┌───┬────┐",
                "│ a │ bb │",
                "├───┼────┤",
                "│ 1 │ 2  │",
                "└───┴────┘",
            ]
        );
    }

    #[test]
    fn test_table_header_is_bold() {
        let t = render("| head |\n|---|\n| x |", 40);
        assert!(has(span_with(&t, "head").style, Modifier::BOLD));
    }

    #[test]
    fn test_table_respects_alignment() {
        let r = rows("| left | right |\n|:---|---:|\n| a | b |", 40);
        assert_eq!(r[3], "│ a    │     b │");
    }

    #[test]
    fn test_table_shrinks_wide_columns_with_ellipsis() {
        let md = "| id | description |\n|---|---|\n| 1 | a very long description here |";
        let r = rows(md, 24);
        for row in &r {
            assert!(row.width() <= 24, "too wide: {:?}", row);
        }
        assert!(r[3].contains('…'), "{:?}", r[3]);
        assert!(r[3].starts_with("│ 1  │ a very"), "{:?}", r[3]);
    }

    #[test]
    fn test_html_block_is_dim_text() {
        let t = render("<div>\nhi\n</div>", 40);
        assert_eq!(line_text(&t.lines[0]), "<div>");
        assert!(has(span_with(&t, "<div>").style, Modifier::DIM));
    }

    // ========================================
    // WIDTH INVARIANT
    // ========================================

    const KITCHEN_SINK: &str = "# Heading one\n\n## Heading two\n\n\
        Paragraph with *em*, **strong**, `code`, a [link](https://example.com/a/long/path) \
        and an extraordinarilylongunbreakablewordthatgoesonandon.\n\n\
        - item one with enough words to wrap around\n  - nested 📄✅ emoji item\n\
        - [x] task\n\n1. first\n2. second\n\n> quote with words that wrap\n> > nested quote\n\n\
        ```rust\nfn main() { println!(\"a long line of code that must be hard wrapped\"); }\n```\n\n\
        | col | another column | third |\n|:--|:-:|--:|\n| a | b | c |\n\n---\n\n<p>html</p>\n";

    #[test]
    fn test_no_line_exceeds_width() {
        for width in [1u16, 2, 5, 8, 12, 20, 40, 80, 120] {
            for line in render(KITCHEN_SINK, width).lines {
                assert!(
                    line.width() <= width as usize,
                    "width {}: {:?} is {} wide",
                    width,
                    line_text(&line),
                    line.width()
                );
            }
        }
    }

    #[test]
    fn test_zero_width_does_not_panic() {
        render(KITCHEN_SINK, 0);
    }
}
