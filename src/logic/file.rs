//! File type detection and utilities
//!
//! Pure functions for identifying file types and properties.

use codepage_437::{BorrowFromCp437, CP437_CONTROL};

/// Check if a file path represents an image file based on extension
///
/// Supported formats: PNG, JPG/JPEG, GIF, BMP, WEBP, TIFF/TIF
///
/// # Arguments
/// * `path` - File path or name to check
///
/// # Returns
/// `true` if the file has an image extension, `false` otherwise
///
/// # Examples
/// ```
/// use stui::logic::file::is_image_file;
///
/// assert!(is_image_file("photo.jpg"));
/// assert!(is_image_file("image.PNG"));  // Case insensitive
/// assert!(is_image_file("/path/to/pic.webp"));
/// assert!(!is_image_file("document.pdf"));
/// assert!(!is_image_file("file.txt"));
/// ```
pub fn is_image_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    path_lower.ends_with(".png")
        || path_lower.ends_with(".jpg")
        || path_lower.ends_with(".jpeg")
        || path_lower.ends_with(".gif")
        || path_lower.ends_with(".bmp")
        || path_lower.ends_with(".webp")
        || path_lower.ends_with(".tiff")
        || path_lower.ends_with(".tif")
}

/// Whether the path has a Markdown extension.
pub fn is_markdown_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".md", ".markdown", ".mdown"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// A file type the preview draws formatted; `r` switches to the raw source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedView {
    Markdown,
    Json,
    Csv,
    Tsv,
}

impl RenderedView {
    /// Name shown in the preview title.
    pub fn label(self) -> &'static str {
        match self {
            RenderedView::Markdown => "Markdown",
            RenderedView::Json => "JSON",
            RenderedView::Csv => "CSV",
            RenderedView::Tsv => "TSV",
        }
    }

    /// Word for the formatted view in the raw view's title.
    pub fn rendered_word(self) -> &'static str {
        match self {
            RenderedView::Markdown => "rendered",
            _ => "formatted",
        }
    }
}

/// The formatted view for a path, by extension (case-insensitive).
pub fn rendered_view(path: &str) -> Option<RenderedView> {
    let lower = path.to_lowercase();
    if is_markdown_path(path) {
        Some(RenderedView::Markdown)
    } else if lower.ends_with(".json") {
        Some(RenderedView::Json)
    } else if lower.ends_with(".csv") {
        Some(RenderedView::Csv)
    } else if lower.ends_with(".tsv") {
        Some(RenderedView::Tsv)
    } else {
        None
    }
}

/// Whether the preview has a formatted view for this file.
pub fn has_rendered_view(path: &str) -> bool {
    rendered_view(path).is_some()
}

/// Detect if file content contains ANSI escape codes
///
/// Checks for the presence of ANSI escape sequences (ESC[...m for colors/styles,
/// ESC[...C for cursor positioning) which are commonly found in ANSI art files.
/// This allows auto-detection of ANSI content even if the file doesn't have
/// .ans or .asc extension.
///
/// # Arguments
/// * `bytes` - File content to check (first few KB is usually sufficient)
///
/// # Returns
/// `true` if ANSI escape sequences are detected, `false` otherwise
///
/// # Examples
/// ```
/// use stui::logic::file::contains_ansi_codes;
///
/// // Text with ANSI color codes
/// assert!(contains_ansi_codes(b"\x1b[31mRed text\x1b[0m"));
/// assert!(contains_ansi_codes(b"Normal \x1b[1;32mBold green\x1b[0m"));
///
/// // Text with cursor positioning
/// assert!(contains_ansi_codes(b"Text\x1b[5Chere"));
///
/// // Plain text without ANSI codes
/// assert!(!contains_ansi_codes(b"Just plain text"));
/// assert!(!contains_ansi_codes(b"No escape codes here"));
/// ```
pub fn contains_ansi_codes(bytes: &[u8]) -> bool {
    // Look for ESC[ sequences which are the start of ANSI codes
    // Common patterns:
    // - ESC[...m (SGR - colors/styles)
    // - ESC[...C (cursor forward)
    // - ESC[...H (cursor position)
    // We just check for ESC[ followed by any sequence ending in a letter

    let mut i = 0;
    while i < bytes.len().saturating_sub(2) {
        // Check for ESC character (0x1B)
        if bytes[i] == 0x1B && bytes[i + 1] == b'[' {
            // Found ESC[, this is likely an ANSI code
            // Verify it's followed by valid ANSI sequence (digits/semicolons then a letter)
            let mut j = i + 2;
            while j < bytes.len() && j < i + 20 {
                // Check next 20 bytes max
                let ch = bytes[j];
                if ch.is_ascii_alphabetic() {
                    // Valid ANSI sequence found (ends with letter)
                    return true;
                } else if ch.is_ascii_digit() || ch == b';' {
                    // Still in the parameter part, continue
                    j += 1;
                } else {
                    // Invalid character, not a valid ANSI sequence
                    break;
                }
            }
        }
        i += 1;
    }

    false
}

/// The 16 CGA text colors ANSI art was drawn for, indexed like SGR colors 0-15
const CGA_PALETTE: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xAA, 0x00, 0x00),
    (0x00, 0xAA, 0x00),
    (0xAA, 0x55, 0x00),
    (0x00, 0x00, 0xAA),
    (0xAA, 0x00, 0xAA),
    (0x00, 0xAA, 0xAA),
    (0xAA, 0xAA, 0xAA),
    (0x55, 0x55, 0x55),
    (0xFF, 0x55, 0x55),
    (0x55, 0xFF, 0x55),
    (0xFF, 0xFF, 0x55),
    (0x55, 0x55, 0xFF),
    (0xFF, 0x55, 0xFF),
    (0x55, 0xFF, 0xFF),
    (0xFF, 0xFF, 0xFF),
];

/// Text color of ANSI art where the file sets none (CGA light grey)
pub const ANSI_ART_FG: ratatui::style::Color = ratatui::style::Color::Rgb(0xAA, 0xAA, 0xAA);
/// Background of ANSI art where the file sets none (CGA black)
pub const ANSI_ART_BG: ratatui::style::Color = ratatui::style::Color::Rgb(0x00, 0x00, 0x00);

/// DOS glyphs for CP437 bytes 0x00-0x1F, which art uses as picture characters
const CP437_CONTROL_GLYPHS: [char; 32] = [
    ' ', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄', '↕',
    '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
];

/// The character DOS showed for a CP437-decoded control character. Tab, LF, CR, the
/// EOF byte before a SAUCE record (0x1A) and ESC keep their control meaning.
fn cp437_screen_char(ch: char) -> char {
    match ch {
        '\t' | '\n' | '\r' | '\x1a' | '\x1b' => ch,
        '\x7f' => '⌂',
        c if (c as u32) < 0x20 => CP437_CONTROL_GLYPHS[c as usize],
        c => c,
    }
}

/// Exact RGB for CGA color `index` (0-15), so the terminal theme can't remap it
pub fn cga_color(index: u8) -> ratatui::style::Color {
    let (r, g, b) = CGA_PALETTE[(index & 0x0F) as usize];
    ratatui::style::Color::Rgb(r, g, b)
}

/// SGR state of the ANSI parser. Colors are CGA indexes; None is the pane default.
#[derive(Clone, Copy, Default)]
struct SgrState {
    fg: Option<u8>,
    bg: Option<u8>,
    bold: bool,
    blink: bool,
    italic: bool,
    underline: bool,
}

impl SgrState {
    fn apply(&mut self, code: u8) {
        match code {
            0 => *self = SgrState::default(),
            1 => self.bold = true,
            3 => self.italic = true,
            4 => self.underline = true,
            5 | 6 => self.blink = true,
            22 => self.bold = false,
            23 => self.italic = false,
            24 => self.underline = false,
            25 => self.blink = false,
            30..=37 => self.fg = Some(code - 30),
            39 => self.fg = None,
            40..=47 => self.bg = Some(code - 40),
            49 => self.bg = None,
            90..=97 => self.fg = Some(code - 90 + 8),
            100..=107 => self.bg = Some(code - 100 + 8),
            _ => {}
        }
    }

    /// Bold means high intensity, as on DOS: it brightens colors 0-7 and the default
    /// grey, and does not change the font. Blink draws blinking text: SAUCE iCE flags
    /// are unreliable, and blinking is the more common intent.
    fn style(&self) -> ratatui::style::Style {
        use ratatui::style::{Modifier, Style};
        let fg = match (self.fg, self.bold) {
            (Some(i), true) if i < 8 => Some(i + 8),
            (None, true) => Some(15),
            (fg, _) => fg,
        };
        let mut style = Style::default();
        if let Some(i) = fg {
            style = style.fg(cga_color(i));
        }
        if let Some(i) = self.bg {
            style = style.bg(cga_color(i));
        }
        if self.blink {
            style = style.add_modifier(Modifier::SLOW_BLINK);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }
}

/// Parse ANSI codes and convert to Ratatui Text with styling
///
/// Draws the text on a grid of rows `width` columns wide, like the DOS screen ANSI art
/// was made on, and returns the rows. Nothing reaches the real terminal. Cursor moves
/// (CSI A/B/C/D, H/f) move within the grid, CSI K erases in the row, tab moves to the
/// next 8-column stop. Screen clearing, private (`ESC[?7h`) and other codes are skipped.
///
/// Supported SGR codes:
/// - 0: Reset
/// - 1 / 22: Bright (high intensity) on / off
/// - 3 / 23: Italic on / off
/// - 4 / 24: Underline on / off
/// - 30-37, 90-97: Foreground colors; 39: default
/// - 40-47, 100-107: Background colors; 49: default
///
/// # Arguments
/// * `text` - Text with ANSI escape codes
/// * `width` - canvas width in columns; rows wrap at it; at least 1
///
/// # Returns
/// Ratatui `Text` with styling applied, all escape codes stripped
pub fn parse_ansi_to_text(text: &str, width: u16) -> ratatui::text::Text<'static> {
    use ratatui::style::Style;
    use ratatui::text::{Line, Span, Text};

    // Strip SAUCE metadata if present (common in ANSI art files)
    // SAUCE starts with Ctrl-Z (0x1A) followed by "SAUCE00"
    let text_without_sauce = if let Some(pos) = text.find('\x1A') {
        &text[..pos]
    } else {
        text
    };

    // Handle different line endings: \n, \r\n, or \r
    let normalized_text = text_without_sauce.replace("\r\n", "\n").replace('\r', "\n");

    // Cursor codes move around a grid of rows `width` columns wide
    let line_width: usize = usize::from(width.max(1));
    let blank_row = vec![(' ', Style::default()); line_width];
    let mut rows: Vec<Vec<(char, Style)>> = vec![blank_row.clone()];
    let mut row: usize = 0;
    let mut col: usize = 0;
    // Writing the last column wraps to the next row at once, as on DOS. A newline right after
    // that wrap ends the same line, so it doesn't add a blank row.
    let mut just_wrapped = false;

    // SGR state carries across line breaks until the file changes it
    let mut sgr = SgrState::default();
    let mut current_style = sgr.style();

    let mut chars = normalized_text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['

            // CSI: parameter bytes 0x30-0x3F, intermediate bytes 0x20-0x2F, final 0x40-0x7E
            let mut params = String::new();
            while let Some(&c) = chars.peek().filter(|c| ('\x30'..='\x3f').contains(*c)) {
                params.push(c);
                chars.next();
            }
            let mut has_intermediate = false;
            while chars.peek().is_some_and(|c| ('\x20'..='\x2f').contains(c)) {
                has_intermediate = true;
                chars.next();
            }
            let Some(command) = chars.next() else { break };
            let private = params.starts_with(['?', '<', '=', '>']);
            if private || has_intermediate {
                continue;
            }

            let numbers: Vec<usize> = params
                .split(';')
                .map(|p| p.trim().parse().unwrap_or(0))
                .collect();
            let count = || numbers[0].max(1);
            match command {
                'A' => {
                    row = row.saturating_sub(count());
                    just_wrapped = false;
                }
                'B' => {
                    row += count();
                    just_wrapped = false;
                }
                'C' => col = (col + count()).min(line_width - 1),
                'D' => col = col.saturating_sub(count()),
                'H' | 'f' => {
                    row = numbers[0].max(1) - 1;
                    col = (numbers.get(1).copied().unwrap_or(1).max(1) - 1).min(line_width - 1);
                    just_wrapped = false;
                }
                'K' => {
                    if rows.len() <= row {
                        rows.resize(row + 1, blank_row.clone());
                    }
                    let cells = &mut rows[row];
                    let range = match numbers[0] {
                        1 => 0..(col + 1).min(line_width),
                        2 => 0..line_width,
                        _ => col.min(line_width)..line_width,
                    };
                    for cell in &mut cells[range] {
                        *cell = (' ', Style::default());
                    }
                }
                'm' => {
                    // Empty params or an empty parameter mean reset (0)
                    for code in &numbers {
                        sgr.apply(u8::try_from(*code).unwrap_or(u8::MAX));
                    }
                    current_style = sgr.style();
                }
                // Screen clearing and the rest are ignored
                _ => {}
            }
        } else if ch == '\x1b' {
            // Other escape sequences: ESC plus one byte, or a charset choice like ESC ( B
            if let Some(next) = chars.next()
                && matches!(next, '(' | ')' | '*' | '+')
            {
                chars.next();
            }
        } else if ch == '\t' {
            col = ((col / 8 + 1) * 8).min(line_width - 1);
            just_wrapped = false;
        } else if ch == '\n' {
            if !just_wrapped {
                row += 1;
            }
            col = 0;
            just_wrapped = false;
        } else {
            if rows.len() <= row {
                rows.resize(row + 1, blank_row.clone());
            }
            rows[row][col] = (ch, current_style);
            col += 1;
            just_wrapped = col == line_width;
            if just_wrapped {
                row += 1;
                col = 0;
            }
        }
    }

    // Rows between written rows stay blank: blank lines position the art vertically
    let mut lines: Vec<Line<'static>> = rows
        .iter()
        .map(|cells| {
            let width = cells
                .iter()
                .rposition(|(ch, _)| *ch != ' ')
                .map_or(0, |i| i + 1);
            if width == 0 {
                Line::from(vec![Span::raw("")])
            } else {
                Line::from(build_spans_from_buffer_upto(cells, width))
            }
        })
        .collect();

    while lines.len() > 1 && lines.last().is_some_and(|l| l.width() == 0) {
        lines.pop();
    }

    Text::from(lines)
}

/// Helper function to build spans from line buffer up to a certain column
fn build_spans_from_buffer_upto(
    line_buffer: &[(char, ratatui::style::Style)],
    max_col: usize,
) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::style::Style;
    use ratatui::text::Span;

    let mut spans = Vec::new();
    if max_col == 0 {
        spans.push(Span::raw(""));
        return spans;
    }

    let mut current_span_text = String::new();
    let mut current_span_style = line_buffer[0].1;

    for (ch, mut style) in line_buffer.iter().take(max_col).copied() {
        // Strip background color from spaces to prevent unwanted background bleeding
        if ch == ' ' && style.bg.is_some() {
            style = Style {
                fg: style.fg,
                bg: None,
                add_modifier: style.add_modifier,
                sub_modifier: style.sub_modifier,
                underline_color: style.underline_color,
            };
        }

        if style == current_span_style {
            current_span_text.push(ch);
        } else {
            // Style changed, save current span and start new one
            if !current_span_text.is_empty() {
                spans.push(Span::styled(current_span_text.clone(), current_span_style));
                current_span_text.clear();
            }
            current_span_text.push(ch);
            current_span_style = style;
        }
    }

    // Add final span
    if !current_span_text.is_empty() {
        spans.push(Span::styled(current_span_text, current_span_style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(""));
    }

    spans
}

/// Extract printable ASCII strings from binary data
///
/// This function scans binary data and extracts readable text strings,
/// similar to the Unix `strings` command. Only strings of at least 4
/// characters are extracted.
///
/// # Arguments
/// * `bytes` - Binary data to scan for text
///
/// # Returns
/// A formatted string containing:
/// - All extracted text strings (>= 4 chars) separated by newlines
/// - A header indicating it's a binary file
/// - If no text found: "[Binary file - no readable text found]"
///
/// # Printable Characters
/// Considers printable ASCII (32-126), newlines (`\n`), and tabs (`\t`) as valid text.
///
/// # Example
/// ```
/// use stui::logic::file::extract_text_from_binary;
///
/// let binary = b"Hello\x00World\x00\x01\x02Test\x00";
/// let result = extract_text_from_binary(binary);
/// assert!(result.contains("Hello"));
/// assert!(result.contains("World"));
/// assert!(result.contains("Test"));
/// ```
pub fn extract_text_from_binary(bytes: &[u8]) -> String {
    // Extract printable ASCII strings (similar to 'strings' command)
    let mut result = String::new();
    let mut current_string = String::new();
    const MIN_STRING_LENGTH: usize = 4;

    for &byte in bytes {
        if (32..=126).contains(&byte) || byte == b'\n' || byte == b'\t' {
            current_string.push(byte as char);
        } else {
            if current_string.len() >= MIN_STRING_LENGTH {
                result.push_str(&current_string);
                result.push('\n');
            }
            current_string.clear();
        }
    }

    if current_string.len() >= MIN_STRING_LENGTH {
        result.push_str(&current_string);
    }

    if result.is_empty() {
        result = "[Binary file - no readable text found]".to_string();
    } else {
        result = format!("[Binary file - extracted text]\n\n{}", result);
    }

    result
}

/// Bytes inspected for NUL bytes and the BOM-less UTF-16 heuristic.
const PREVIEW_SNIFF_BYTES: usize = 8192;

/// Canvas width for CP437 art without a SAUCE width.
pub const DEFAULT_ART_WIDTH: u16 = 80;

/// How a file's bytes are decoded for the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Cp437,
    /// ISO-8859-1, the character set of Amiga ASCII art
    Latin1,
}

/// How art is drawn: its canvas width in columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtStyle {
    pub width: u16,
}

/// The parts of a SAUCE record the preview uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sauce {
    pub width: Option<u16>,
    /// DataType 1 (Character): the file is text art
    pub character: bool,
    /// The font name (TInfoS) names an Amiga font, so the text is ISO-8859-1
    pub amiga_font: bool,
}

/// Result of decoding a file's bytes for the preview.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedText {
    pub text: String,
    pub is_binary: bool,
    pub art: Option<ArtStyle>,
}

/// Reads the SAUCE record in the last 128 bytes, if there is one.
///
/// `width` is the character width (`TInfo1`) and is set only for character art
/// (DataType 1, FileType 0, 1 or 2) with a width from 1 to 1000.
pub fn sauce(bytes: &[u8]) -> Option<Sauce> {
    let rec = bytes.len().checked_sub(128).map(|start| &bytes[start..])?;
    if !rec.starts_with(b"SAUCE00") {
        return None;
    }
    let width = u16::from_le_bytes([rec[96], rec[97]]);
    let character = rec[94] == 1;
    let character_art = character && rec[95] <= 2;
    let font = String::from_utf8_lossy(&rec[106..128]).to_ascii_lowercase();
    let amiga_font = ["amiga", "topaz", "p0t-noodle", "microknight", "mo'soul"]
        .iter()
        .any(|name| font.contains(name));
    Some(Sauce {
        character,
        amiga_font,
        width: (character_art && (1..=1000).contains(&width)).then_some(width),
    })
}

/// True when 4 or more identical consecutive bytes are all block/shade characters
/// (B0 B1 B2 DB DC DD DE DF) or all line-drawing characters (C4 CD B3 BA) in CP437.
pub fn has_cp437_art_run(bytes: &[u8]) -> bool {
    const BLOCKS: [u8; 8] = [0xB0, 0xB1, 0xB2, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF];
    const LINES: [u8; 4] = [0xC4, 0xCD, 0xB3, 0xBA];
    bytes
        .windows(4)
        .any(|w| w.iter().all(|&b| b == w[0]) && (BLOCKS.contains(&w[0]) || LINES.contains(&w[0])))
}

/// True when the high bytes look like Amiga ASCII (ISO-8859-1) rather than CP437:
/// Amiga symbols (`¡`-`¯`, `µ¶·¸½¾×Ø`) outnumber both CP437 drawing bytes (shades,
/// blocks, box lines, dots) and accented letters (`À`-`ÿ`). The letter guard keeps
/// Latin-1 prose and PC art that uses `⌐ ¬ ²` as strokes out.
fn is_amiga_art(bytes: &[u8]) -> bool {
    const DOS: [u8; 32] = [
        0xB0, 0xB1, 0xB2, 0xB3, 0xB9, 0xBA, 0xBB, 0xBC, 0xBF, 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5,
        0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD, 0xCE, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xF9,
        0xFA, 0xFE,
    ];
    let is_amiga = |b: u8| matches!(b, 0xA0..=0xAF | 0xB5..=0xB8 | 0xBD | 0xBE | 0xD7 | 0xD8);
    let is_letter = |b: u8| b >= 0xC0 && b != 0xD7 && b != 0xF7;
    let (mut amiga, mut dos, mut letters) = (0usize, 0usize, 0usize);
    for &b in bytes {
        amiga += usize::from(is_amiga(b));
        dos += usize::from(DOS.contains(&b));
        letters += usize::from(is_letter(b));
    }
    amiga > 0 && amiga > dos && amiga >= letters
}

/// CP437 or ISO-8859-1 for a file already known to be art: an Amiga SAUCE font means
/// ISO-8859-1, escape codes mean CP437 (DOS ANSI), otherwise the byte mix decides.
fn art_encoding(bytes: &[u8]) -> TextEncoding {
    if sauce(bytes).is_some_and(|s| s.amiga_font) {
        TextEncoding::Latin1
    } else if contains_ansi_codes(bytes) || !is_amiga_art(bytes) {
        TextEncoding::Cp437
    } else {
        TextEncoding::Latin1
    }
}

fn has_art_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".ans", ".asc", ".nfo", ".diz"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// `.nfo` and `.diz` files hold either scene CP437 art or modern text (Kodi/Jellyfin XML,
/// UTF-8 notes). They are modern text when valid UTF-8 with a multi-byte character, or
/// when the first non-whitespace byte after an optional BOM is `<`.
fn is_modern_info_text(path: &str, bytes: &[u8]) -> bool {
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".nfo") || lower.ends_with(".diz")) {
        return false;
    }
    if std::str::from_utf8(bytes).is_ok() && bytes.iter().any(|&b| b >= 0x80) {
        return true;
    }
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
    body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

/// UTF-16 without a BOM: even length of at least 4, and in the first 8 KB at least
/// 90% of one byte lane is NUL while the other lane has no NUL. The NUL lane is the
/// high byte (odd indexes) for LE and the low byte (even indexes) for BE.
fn sniff_utf16(bytes: &[u8]) -> Option<TextEncoding> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let window = &bytes[..bytes.len().min(PREVIEW_SNIFF_BYTES) & !1];
    let (mut even_nul, mut odd_nul) = (0usize, 0usize);
    for [low, high] in window.as_chunks::<2>().0 {
        even_nul += usize::from(*low == 0);
        odd_nul += usize::from(*high == 0);
    }
    let units = window.len() / 2;
    if even_nul == 0 && odd_nul * 10 >= units * 9 {
        Some(TextEncoding::Utf16Le)
    } else if odd_nul == 0 && even_nul * 10 >= units * 9 {
        Some(TextEncoding::Utf16Be)
    } else {
        None
    }
}

/// Decides how a file's bytes are decoded for the preview; `None` means binary.
///
/// First match wins:
/// 1. UTF-16 BOM (`FF FE` LE, `FE FF` BE)
/// 2. UTF-16 without a BOM (see `sniff_utf16`)
/// 3. `.nfo`/`.diz` that is modern text (see `is_modern_info_text`): UTF-8
/// 4. Extension `.ans`/`.asc`/`.nfo`/`.diz` or a character-art SAUCE record (DataType 1):
///    art even with NUL bytes, CP437 or ISO-8859-1 by `art_encoding`
/// 5. A NUL byte in the first 8 KB: binary
/// 6. Escape codes: CP437
/// 7. Invalid UTF-8 that looks like Amiga art (`is_amiga_art`): ISO-8859-1; with a CP437
///    art run: CP437
/// 8. Otherwise UTF-8
pub fn detect_encoding(path: &str, bytes: &[u8]) -> Option<TextEncoding> {
    match bytes {
        [0xFF, 0xFE, ..] => return Some(TextEncoding::Utf16Le),
        [0xFE, 0xFF, ..] => return Some(TextEncoding::Utf16Be),
        _ => {}
    }
    if let Some(enc) = sniff_utf16(bytes) {
        return Some(enc);
    }
    if is_modern_info_text(path, bytes) {
        return Some(TextEncoding::Utf8);
    }
    if has_art_extension(path) || sauce(bytes).is_some_and(|s| s.character) {
        return Some(art_encoding(bytes));
    }
    if bytes[..bytes.len().min(PREVIEW_SNIFF_BYTES)].contains(&0) {
        return None;
    }
    if contains_ansi_codes(bytes) {
        return Some(TextEncoding::Cp437);
    }
    if std::str::from_utf8(bytes).is_err() {
        if is_amiga_art(bytes) {
            return Some(TextEncoding::Latin1);
        }
        if has_cp437_art_run(bytes) {
            return Some(TextEncoding::Cp437);
        }
    }
    Some(TextEncoding::Utf8)
}

/// Canvas width: the SAUCE width, else `DEFAULT_ART_WIDTH`.
fn art_style(bytes: &[u8]) -> ArtStyle {
    ArtStyle {
        width: sauce(bytes)
            .and_then(|s| s.width)
            .unwrap_or(DEFAULT_ART_WIDTH),
    }
}

/// ISO-8859-1 byte as a character. Control bytes other than tab, LF, CR, the EOF byte
/// before a SAUCE record and ESC become spaces, so C1 bytes never reach the terminal.
fn latin1_screen_char(b: u8) -> char {
    match b {
        b'\t' | b'\n' | b'\r' | 0x1A | 0x1B => b as char,
        0x00..=0x1F | 0x7F..=0x9F => ' ',
        _ => b as char,
    }
}

/// Decodes a file's bytes for the preview according to `detect_encoding`.
///
/// Binary files yield their extracted strings. Invalid UTF-8 is decoded lossily
/// and flagged binary. `art` is set only for art.
pub fn decode_preview_bytes(path: &str, bytes: &[u8]) -> DecodedText {
    let text_only = |text: String| DecodedText {
        text,
        is_binary: false,
        art: None,
    };
    let utf16 = |units: &[u8], le: bool| {
        let units: Vec<u16> = units
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if le {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        text_only(String::from_utf16_lossy(&units))
    };
    match detect_encoding(path, bytes) {
        None => DecodedText {
            text: extract_text_from_binary(bytes),
            is_binary: true,
            art: None,
        },
        Some(TextEncoding::Utf8) => match std::str::from_utf8(bytes) {
            Ok(s) => text_only(s.strip_prefix('\u{feff}').unwrap_or(s).to_string()),
            Err(_) => DecodedText {
                text: String::from_utf8_lossy(bytes).into_owned(),
                is_binary: true,
                art: None,
            },
        },
        Some(enc @ (TextEncoding::Utf16Le | TextEncoding::Utf16Be)) => {
            let body = match bytes {
                [0xFF, 0xFE, rest @ ..] | [0xFE, 0xFF, rest @ ..] => rest,
                _ => bytes,
            };
            utf16(body, enc == TextEncoding::Utf16Le)
        }
        Some(TextEncoding::Cp437) => DecodedText {
            text: String::borrow_from_cp437(bytes, &CP437_CONTROL)
                .chars()
                .map(cp437_screen_char)
                .collect(),
            is_binary: false,
            art: Some(art_style(bytes)),
        },
        Some(TextEncoding::Latin1) => DecodedText {
            text: bytes.iter().map(|&b| latin1_screen_char(b)).collect(),
            is_binary: false,
            art: Some(art_style(bytes)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // IMAGE FILE DETECTION
    // ========================================

    #[test]
    fn test_is_image_file_png() {
        assert!(is_image_file("photo.png"));
        assert!(is_image_file("photo.PNG"));
        assert!(is_image_file("/path/to/photo.png"));
    }

    #[test]
    fn test_is_image_file_jpg() {
        assert!(is_image_file("photo.jpg"));
        assert!(is_image_file("photo.JPG"));
        assert!(is_image_file("photo.jpeg"));
        assert!(is_image_file("photo.JPEG"));
    }

    #[test]
    fn test_is_image_file_other_formats() {
        assert!(is_image_file("image.gif"));
        assert!(is_image_file("image.bmp"));
        assert!(is_image_file("image.webp"));
        assert!(is_image_file("image.tiff"));
        assert!(is_image_file("image.tif"));
    }

    #[test]
    fn test_is_image_file_case_insensitive() {
        assert!(is_image_file("PHOTO.PNG"));
        assert!(is_image_file("Photo.Jpg"));
        assert!(is_image_file("IMAGE.WebP"));
    }

    #[test]
    fn test_is_image_file_non_images() {
        assert!(!is_image_file("document.pdf"));
        assert!(!is_image_file("file.txt"));
        assert!(!is_image_file("video.mp4"));
        assert!(!is_image_file("archive.zip"));
        assert!(!is_image_file("no_extension"));
    }

    #[test]
    fn test_is_image_file_edge_cases() {
        assert!(!is_image_file(""));
        assert!(is_image_file(".png")); // Just extension - technically valid image file
        assert!(is_image_file("a.png")); // Single character filename
        assert!(!is_image_file("png")); // No extension dot
    }

    // ANSI CODE DETECTION
    // ========================================

    #[test]
    fn test_contains_ansi_codes_with_color() {
        // Red foreground color
        assert!(contains_ansi_codes(b"\x1b[31mRed text\x1b[0m"));
        // Green background
        assert!(contains_ansi_codes(b"\x1b[42mGreen background\x1b[0m"));
    }

    #[test]
    fn test_contains_ansi_codes_with_multiple_params() {
        // Bold + red foreground + black background
        assert!(contains_ansi_codes(b"\x1b[1;31;40mStyled text\x1b[0m"));
        // Multiple semicolons
        assert!(contains_ansi_codes(b"Normal \x1b[1;32mBold green\x1b[0m"));
    }

    #[test]
    fn test_contains_ansi_codes_with_cursor() {
        // Cursor forward
        assert!(contains_ansi_codes(b"Text\x1b[5Chere"));
        // Cursor position
        assert!(contains_ansi_codes(b"\x1b[10;20HPositioned"));
    }

    #[test]
    fn test_contains_ansi_codes_plain_text() {
        // No ANSI codes
        assert!(!contains_ansi_codes(b"Just plain text"));
        assert!(!contains_ansi_codes(b"No escape codes here"));
        assert!(!contains_ansi_codes(b""));
    }

    #[test]
    fn test_contains_ansi_codes_partial_sequences() {
        // Incomplete ESC sequence (no '[')
        assert!(!contains_ansi_codes(b"\x1bText"));
        // ESC[ but invalid character breaks sequence (newline before terminator)
        assert!(!contains_ansi_codes(b"\x1b[\n"));
        // ESC[ with special char that's not digit/semicolon/letter
        assert!(!contains_ansi_codes(b"\x1b[@#$"));
    }

    #[test]
    fn test_contains_ansi_codes_mixed_content() {
        // ANSI codes in middle of text
        assert!(contains_ansi_codes(b"Start \x1b[31mred\x1b[0m end"));
        // Multiple ANSI sequences
        assert!(contains_ansi_codes(
            b"\x1b[31mRed\x1b[0m \x1b[32mGreen\x1b[0m"
        ));
    }

    #[test]
    fn test_contains_ansi_codes_reset_code() {
        // Reset code (ESC[0m)
        assert!(contains_ansi_codes(b"\x1b[0m"));
        // Empty SGR params (ESC[m) - should be valid
        assert!(contains_ansi_codes(b"\x1b[m"));
    }

    #[test]
    fn test_contains_ansi_codes_bright_colors() {
        // Bright foreground
        assert!(contains_ansi_codes(b"\x1b[91mBright red\x1b[0m"));
        // Bright background
        assert!(contains_ansi_codes(b"\x1b[104mBright blue bg\x1b[0m"));
    }

    #[test]
    fn test_contains_ansi_codes_edge_cases() {
        // ESC at end of buffer (too short for valid sequence)
        assert!(!contains_ansi_codes(b"Text\x1b"));
        // ESC[ at end (no terminator)
        assert!(!contains_ansi_codes(b"Text\x1b["));
        // Very long parameter string (should still detect within 20 byte limit)
        assert!(contains_ansi_codes(b"\x1b[1;2;3;4;5mText"));
    }

    // ========================================
    // ANSI TEXT PARSING
    // ========================================

    #[test]
    fn test_parse_ansi_to_text_plain() {
        let text = parse_ansi_to_text("Hello, world!", 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans.len(), 1);
        assert_eq!(text.lines[0].spans[0].content, "Hello, world!");
    }

    #[test]
    fn test_parse_ansi_to_text_colors() {
        // Red foreground
        let text = parse_ansi_to_text("\x1b[31mRed\x1b[0m", 80);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans[0].style.fg, Some(cga_color(1)));
    }

    fn first_style(input: &str) -> ratatui::style::Style {
        parse_ansi_to_text(input, 80).lines[0].spans[0].style
    }

    #[test]
    fn test_cga_palette_values() {
        use ratatui::style::Color::Rgb;
        let expected = [
            Rgb(0x00, 0x00, 0x00),
            Rgb(0xAA, 0x00, 0x00),
            Rgb(0x00, 0xAA, 0x00),
            Rgb(0xAA, 0x55, 0x00),
            Rgb(0x00, 0x00, 0xAA),
            Rgb(0xAA, 0x00, 0xAA),
            Rgb(0x00, 0xAA, 0xAA),
            Rgb(0xAA, 0xAA, 0xAA),
            Rgb(0x55, 0x55, 0x55),
            Rgb(0xFF, 0x55, 0x55),
            Rgb(0x55, 0xFF, 0x55),
            Rgb(0xFF, 0xFF, 0x55),
            Rgb(0x55, 0x55, 0xFF),
            Rgb(0xFF, 0x55, 0xFF),
            Rgb(0x55, 0xFF, 0xFF),
            Rgb(0xFF, 0xFF, 0xFF),
        ];
        for (i, color) in expected.iter().enumerate() {
            assert_eq!(cga_color(i as u8), *color, "index {}", i);
        }
        assert_eq!(ANSI_ART_FG, cga_color(7));
        assert_eq!(ANSI_ART_BG, cga_color(0));
    }

    #[test]
    fn test_sgr_foreground_codes_use_cga_palette() {
        for code in 30..=37u8 {
            let style = first_style(&format!("\x1b[{}mX", code));
            assert_eq!(style.fg, Some(cga_color(code - 30)), "SGR {}", code);
        }
        for code in 90..=97u8 {
            let style = first_style(&format!("\x1b[{}mX", code));
            assert_eq!(style.fg, Some(cga_color(code - 90 + 8)), "SGR {}", code);
        }
    }

    #[test]
    fn test_sgr_background_codes_use_cga_palette() {
        for code in 40..=47u8 {
            let style = first_style(&format!("\x1b[{}mX", code));
            assert_eq!(style.bg, Some(cga_color(code - 40)), "SGR {}", code);
        }
        for code in 100..=107u8 {
            let style = first_style(&format!("\x1b[{}mX", code));
            assert_eq!(style.bg, Some(cga_color(code - 100 + 8)), "SGR {}", code);
        }
    }

    #[test]
    fn test_white_is_light_grey_and_bright_white_is_white() {
        use ratatui::style::Color::Rgb;
        assert_eq!(first_style("\x1b[37mX").fg, Some(Rgb(0xAA, 0xAA, 0xAA)));
        assert_eq!(first_style("\x1b[97mX").fg, Some(Rgb(0xFF, 0xFF, 0xFF)));
    }

    #[test]
    fn test_uncolored_text_has_no_color_so_the_pane_default_applies() {
        let style = first_style("plain");
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
    }

    #[test]
    fn test_bold_brightens_base_colors() {
        assert_eq!(first_style("\x1b[1;31mX").fg, Some(cga_color(9)));
        assert_eq!(first_style("\x1b[31;1mX").fg, Some(cga_color(9)));
        assert_eq!(first_style("\x1b[1;37mX").fg, Some(cga_color(15)));
    }

    #[test]
    fn test_bold_default_text_is_bright_white() {
        assert_eq!(first_style("\x1b[1mX").fg, Some(cga_color(15)));
    }

    #[test]
    fn test_bold_leaves_bright_colors_and_backgrounds_alone() {
        assert_eq!(first_style("\x1b[1;91mX").fg, Some(cga_color(9)));
        assert_eq!(first_style("\x1b[1;41mX").bg, Some(cga_color(1)));
    }

    #[test]
    fn test_bold_does_not_change_the_font() {
        use ratatui::style::Modifier;
        assert!(
            !first_style("\x1b[1;31mX")
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn test_sgr_22_ends_bold() {
        assert_eq!(first_style("\x1b[1;31;22mX").fg, Some(cga_color(1)));
    }

    #[test]
    fn test_sgr_39_and_49_restore_default_colors() {
        let style = first_style("\x1b[31;44;39;49mX");
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
    }

    #[test]
    fn test_reset_after_color_returns_to_default() {
        let text = parse_ansi_to_text("\x1b[1;31mRed\x1b[0mPlain", 80);
        let plain = text.lines[0]
            .spans
            .iter()
            .find(|s| s.content == "Plain")
            .expect("plain span");
        assert_eq!(plain.style.fg, None);
        assert_eq!(plain.style.bg, None);
    }

    #[test]
    fn test_parse_ansi_to_text_cursor_forward() {
        // Cursor forward should add spaces
        let text = parse_ansi_to_text("A\x1b[5CB", 80);
        assert_eq!(text.lines.len(), 1);
        // Should have "A" + 5 spaces + "B"
        let full_text: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(full_text, "A     B");
    }

    #[test]
    fn test_parse_ansi_to_text_80_column_wrap() {
        // Create a line with exactly 80 characters
        let long_line = "x".repeat(80);
        let text = parse_ansi_to_text(&long_line, 80);
        assert_eq!(text.lines.len(), 1);

        // Create a line with 81 characters - should wrap
        let longer_line = "x".repeat(81);
        let text = parse_ansi_to_text(&longer_line, 80);
        assert_eq!(text.lines.len(), 2);
        assert_eq!(text.lines[0].spans[0].content, "x".repeat(80));
        assert_eq!(text.lines[1].spans[0].content, "x");
    }

    fn line_texts_w(input: &str, width: u16) -> Vec<String> {
        parse_ansi_to_text(input, width)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn line_texts(input: &str) -> Vec<String> {
        line_texts_w(input, 80)
    }

    #[test]
    fn test_blink_draws_blinking_text() {
        use ratatui::style::Modifier;
        let style = first_style("\x1b[5;41mX");
        assert!(style.add_modifier.contains(Modifier::SLOW_BLINK));
        assert_eq!(style.bg, Some(cga_color(1)));
        assert!(
            first_style("\x1b[6mX")
                .add_modifier
                .contains(Modifier::SLOW_BLINK)
        );
        assert!(
            !first_style("\x1b[5;25mX")
                .add_modifier
                .contains(Modifier::SLOW_BLINK)
        );
    }

    #[test]
    fn test_private_and_intermediate_codes_are_skipped() {
        assert_eq!(line_texts("\x1b[?7hAB"), ["AB"]);
        assert_eq!(line_texts("\x1b[=3hX"), ["X"]);
        assert_eq!(line_texts("\x1b[0 qX"), ["X"]);
    }

    #[test]
    fn test_cursor_position() {
        assert_eq!(line_texts("abc\x1b[2;3HX"), ["abc", "  X"]);
        assert_eq!(line_texts("abc\x1b[HX"), ["Xbc"]);
        assert_eq!(line_texts("\x1b[2;1fY"), ["", "Y"]);
        assert_eq!(line_texts_w("\x1b[1;200HX", 10), ["         X"]);
    }

    #[test]
    fn test_erase_in_line() {
        assert_eq!(line_texts("abcdef\x1b[3D\x1b[KX"), ["abcX"]);
        assert_eq!(line_texts("abcdef\x1b[3D\x1b[1KX"), ["   Xef"]);
        assert_eq!(line_texts("abcdef\x1b[2KX"), ["      X"]);
    }

    #[test]
    fn test_tab_moves_to_next_stop() {
        assert_eq!(line_texts("a\tb"), ["a       b"]);
        assert_eq!(line_texts_w("12345678\tX", 10), ["12345678 X"]);
    }

    #[test]
    fn test_other_escape_sequences_are_skipped() {
        assert_eq!(line_texts("A\x1b(BB"), ["AB"]);
        assert_eq!(line_texts("A\x1bcB"), ["AB"]);
    }

    #[test]
    fn test_canvas_width_from_caller() {
        let line = "x".repeat(100);
        assert_eq!(line_texts_w(&line, 132), [line.as_str()]);
        assert_eq!(line_texts_w(&line, 80).len(), 2);
    }

    #[test]
    fn test_blank_lines_between_content_are_kept() {
        assert_eq!(line_texts("A\n\n\nB"), ["A", "", "", "B"]);
    }

    #[test]
    fn test_leading_blank_lines_are_kept() {
        assert_eq!(line_texts("\r\n\r\n\r\nA"), ["", "", "", "A"]);
    }

    #[test]
    fn test_lines_of_only_spaces_or_codes_count_as_blank() {
        assert_eq!(line_texts("A\n   \n\x1b[0m\nB"), ["A", "", "", "B"]);
    }

    #[test]
    fn test_full_80_column_line_adds_no_blank_line() {
        let full = "x".repeat(80);
        assert_eq!(line_texts(&format!("{}\nB", full)), [full.as_str(), "B"]);
    }

    #[test]
    fn test_blank_line_after_full_80_column_line_is_kept() {
        let full = "x".repeat(80);
        assert_eq!(
            line_texts(&format!("{}\n\nB", full)),
            [full.as_str(), "", "B"]
        );
    }

    #[test]
    fn test_cursor_up_and_forward_finish_the_previous_row() {
        // 79 columns per line, then up one row and forward 79 to write the 80th
        let input = format!("{}\r\n\x1b[A\x1b[79Cy{}", "x".repeat(79), "z".repeat(3));
        let first = format!("{}y", "x".repeat(79));
        assert_eq!(line_texts(&input), [first.as_str(), "zzz"]);
    }

    #[test]
    fn test_cursor_forward_after_a_full_row_moves_in_the_next_row() {
        let input = format!("{}\x1b[3CB", "x".repeat(80));
        assert_eq!(line_texts(&input), ["x".repeat(80).as_str(), "   B"]);
    }

    #[test]
    fn test_cursor_up_overwrites_without_erasing() {
        assert_eq!(line_texts("abcdef\n\x1b[A\x1b[2CXY"), ["abXYef"]);
    }

    #[test]
    fn test_cursor_up_stops_at_the_first_row() {
        assert_eq!(line_texts("\x1b[5Aab"), ["ab"]);
    }

    #[test]
    fn test_cursor_down_keeps_the_column() {
        assert_eq!(line_texts("a\x1b[2Bb"), ["a", "", " b"]);
    }

    #[test]
    fn test_cursor_back_moves_left() {
        assert_eq!(line_texts("abc\x1b[2DX"), ["aXc"]);
        assert_eq!(line_texts("ab\x1b[9DX"), ["Xb"]);
    }

    #[test]
    fn test_colors_carry_over_to_the_next_line() {
        let text = parse_ansi_to_text("\x1b[1;31;44mA\nB", 80);
        let style = text.lines[1].spans[0].style;
        assert_eq!(style.fg, Some(cga_color(9)));
        assert_eq!(style.bg, Some(cga_color(4)));
    }

    #[test]
    fn test_reset_carries_over_to_the_next_line() {
        let text = parse_ansi_to_text("\x1b[31mA\x1b[0m\nB", 80);
        assert_eq!(text.lines[1].spans[0].style.fg, None);
    }

    #[test]
    fn test_trailing_blank_lines_are_dropped() {
        assert_eq!(line_texts("A\n\n\n"), ["A"]);
    }

    #[test]
    fn test_parse_ansi_to_text_sauce_stripping() {
        // SAUCE metadata starts with Ctrl-Z
        let text_with_sauce = "Content\x1ASAUCE00metadata";
        let text = parse_ansi_to_text(text_with_sauce, 80);
        let full_text: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(full_text, "Content");
    }

    #[test]
    fn test_parse_ansi_to_text_line_endings() {
        // Test \r\n line endings
        let text = parse_ansi_to_text("Line1\r\nLine2\r\nLine3", 80);
        assert_eq!(text.lines.len(), 3);

        // Test \r line endings
        let text = parse_ansi_to_text("Line1\rLine2\rLine3", 80);
        assert_eq!(text.lines.len(), 3);

        // Test \n line endings
        let text = parse_ansi_to_text("Line1\nLine2\nLine3", 80);
        assert_eq!(text.lines.len(), 3);
    }

    #[test]
    fn test_parse_ansi_to_text_background_stripped_from_spaces() {
        // Set background color, write text, then spaces via cursor forward
        let text = parse_ansi_to_text("\x1b[41mText\x1b[5C", 80);
        let line = &text.lines[0];

        // First span should have red background (the "Text")
        assert_eq!(line.spans[0].content, "Text");
        assert_eq!(line.spans[0].style.bg, Some(cga_color(1)));

        // Spaces from cursor forward should NOT have background
        if line.spans.len() > 1 {
            assert_eq!(line.spans[1].style.bg, None);
        }
    }

    #[test]
    fn test_parse_ansi_to_text_multiple_styles() {
        use ratatui::style::Modifier;

        // Bold + Red foreground + Black background + Underline
        let text = parse_ansi_to_text("\x1b[1;31;40;4mStyled\x1b[0m", 80);
        let span = &text.lines[0].spans[0];

        assert_eq!(span.content, "Styled");
        assert_eq!(span.style.fg, Some(cga_color(9)));
        assert_eq!(span.style.bg, Some(cga_color(0)));
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_parse_ansi_to_text_empty_sgr_params() {
        // Empty SGR parameters should reset (like \x1b[m)
        let text = parse_ansi_to_text("\x1b[31mRed\x1b[mNormal", 80);
        assert_eq!(text.lines.len(), 1);
        // Should have two spans: "Red" (colored) and "Normal" (reset)
        assert!(text.lines[0].spans.len() >= 2);
    }

    #[test]
    fn test_parse_ansi_to_text_line_buffer_positioning() {
        // Test that cursor positioning works correctly within 80-column buffer
        // Write at column 0, jump to column 40, write again
        let text = parse_ansi_to_text("Start\x1b[35CHere", 80);
        let line = &text.lines[0];
        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        // Should have "Start" at position 0, spaces, then "Here" at position 40
        assert!(full_text.starts_with("Start"));
        assert!(full_text.contains("Here"));
        // The "Here" should be at approximately position 40
        assert!(full_text.len() <= 80);
    }

    // ========================================
    // BINARY TEXT EXTRACTION
    // ========================================

    #[test]
    fn test_extract_text_from_binary_with_readable_strings() {
        // Binary data with some readable strings (>= 4 chars)
        let binary = b"Hello\x00World\x00\x01\x02Test String\x00End";
        let result = extract_text_from_binary(binary);

        assert!(
            result.contains("Binary file - extracted text"),
            "Should indicate it's a binary file"
        );
        assert!(result.contains("Hello"), "Should extract 'Hello' (5 chars)");
        assert!(result.contains("World"), "Should extract 'World' (5 chars)");
        assert!(
            result.contains("Test String"),
            "Should extract 'Test String' (11 chars)"
        );
    }

    #[test]
    fn test_extract_text_from_binary_pure_binary() {
        // Pure binary with no readable strings >= 4 chars
        let binary = b"\x00\x01\x02\x03\xFF\xFE\xFD\xFC";
        let result = extract_text_from_binary(binary);

        assert_eq!(
            result, "[Binary file - no readable text found]",
            "Pure binary should show 'no readable text found' message"
        );
    }

    #[test]
    fn test_extract_text_from_binary_mixed_content() {
        // Mix of binary and text with various string lengths
        let binary = b"\x00\x00Data\x00\x01Short\x00\x02AB\x00LongerString\x00\xFF";
        let result = extract_text_from_binary(binary);

        assert!(
            result.contains("Data"),
            "Should extract 'Data' (4 chars - at threshold)"
        );
        assert!(result.contains("Short"), "Should extract 'Short' (5 chars)");
        assert!(
            !result.contains("AB"),
            "Should NOT extract 'AB' (2 chars - below threshold)"
        );
        assert!(
            result.contains("LongerString"),
            "Should extract 'LongerString' (12 chars)"
        );
    }

    #[test]
    fn test_extract_text_from_binary_min_length_threshold() {
        // Test the MIN_STRING_LENGTH threshold (4 chars)
        let binary = b"A\x00AB\x00ABC\x00ABCD\x00ABCDE\x00";
        let result = extract_text_from_binary(binary);

        assert!(!result.contains("A\n"), "Should NOT extract 1-char strings");
        assert!(
            !result.contains("AB\n"),
            "Should NOT extract 2-char strings"
        );
        assert!(
            !result.contains("ABC\n"),
            "Should NOT extract 3-char strings"
        );
        assert!(
            result.contains("ABCD"),
            "Should extract 4-char strings (at threshold)"
        );
        assert!(result.contains("ABCDE"), "Should extract 5-char strings");
    }

    #[test]
    fn test_extract_text_from_binary_special_chars() {
        // Test handling of printable ASCII + special characters (newline, tab)
        let binary = b"Line1\nLine2\x00Tab\tSeparated\x00Normal Text\x00\xFF";
        let result = extract_text_from_binary(binary);

        // Newlines and tabs should be preserved as part of strings
        assert!(
            result.contains("Line1\nLine2"),
            "Should preserve newlines within strings"
        );
        assert!(
            result.contains("Tab\tSeparated"),
            "Should preserve tabs within strings"
        );
        assert!(
            result.contains("Normal Text"),
            "Should extract normal printable ASCII"
        );
    }

    // ========================================
    // RENDERED VIEW DETECTION
    // ========================================

    #[test]
    fn test_is_markdown_path_extensions() {
        assert!(is_markdown_path("README.md"));
        assert!(is_markdown_path("docs/guide.markdown"));
        assert!(is_markdown_path("notes.mdown"));
    }

    #[test]
    fn test_is_markdown_path_case_insensitive() {
        assert!(is_markdown_path("README.MD"));
        assert!(is_markdown_path("Guide.Markdown"));
    }

    #[test]
    fn test_is_markdown_path_non_markdown() {
        assert!(!is_markdown_path("notes.mdx"));
        assert!(!is_markdown_path("md"));
        assert!(!is_markdown_path("file.txt"));
        assert!(!is_markdown_path("dir.md/file.txt"));
        assert!(!is_markdown_path(""));
    }

    #[test]
    fn test_rendered_view_by_extension() {
        assert_eq!(rendered_view("README.md"), Some(RenderedView::Markdown));
        assert_eq!(rendered_view("data/Config.JSON"), Some(RenderedView::Json));
        assert_eq!(rendered_view("people.csv"), Some(RenderedView::Csv));
        assert_eq!(rendered_view("people.TSV"), Some(RenderedView::Tsv));
        assert_eq!(rendered_view("notes.txt"), None);
        assert!(
            has_rendered_view("a.json")
                && has_rendered_view("a.csv")
                && !has_rendered_view("a.ans")
        );
    }

    #[test]
    fn test_rendered_view_labels() {
        assert_eq!(RenderedView::Json.label(), "JSON");
        assert_eq!(RenderedView::Tsv.label(), "TSV");
        assert_eq!(RenderedView::Markdown.rendered_word(), "rendered");
        assert_eq!(RenderedView::Csv.rendered_word(), "formatted");
    }

    #[test]
    fn test_has_rendered_view_only_for_formatted_types() {
        assert!(has_rendered_view("README.md"));
        assert!(!has_rendered_view("notes.txt"));
        assert!(!has_rendered_view("art.ans"));
        assert!(!has_rendered_view("photo.png"));
    }

    // ========================================
    // ENCODING DETECTION
    // ========================================

    fn with_sauce(body: &[u8], data_type: u8, file_type: u8, width: u16) -> Vec<u8> {
        with_sauce_full(body, data_type, file_type, width, 0, "")
    }

    fn with_sauce_full(
        body: &[u8],
        data_type: u8,
        file_type: u8,
        width: u16,
        flags: u8,
        font: &str,
    ) -> Vec<u8> {
        let mut v = body.to_vec();
        v.push(0x1A);
        let mut rec = vec![b' '; 128];
        rec[..7].copy_from_slice(b"SAUCE00");
        rec[94] = data_type;
        rec[95] = file_type;
        rec[96..98].copy_from_slice(&width.to_le_bytes());
        rec[105] = flags;
        rec[106..128].fill(0);
        rec[106..106 + font.len()].copy_from_slice(font.as_bytes());
        v.extend(rec);
        v
    }

    // ========================================
    // AMIGA (LATIN-1) ART AND ICE COLORS
    // ========================================

    #[test]
    fn test_amiga_asc_is_latin1() {
        // · ¥ ¯ dominate: Amiga ASCII
        let bytes = b"$$\xb7\xb7\xa5\xa5\xa5\xaf\xaf $$";
        assert_eq!(
            detect_encoding("logo.asc", bytes),
            Some(TextEncoding::Latin1)
        );
    }

    #[test]
    fn test_dos_asc_stays_cp437() {
        assert_eq!(
            detect_encoding("logo.asc", &[0xDB, 0xDB, 0xB1, 0xB0, b' ']),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_pc_strokes_with_letters_stay_cp437() {
        // ⌐ ¬ ² strokes in PC art: 0xA9/0xAA count as Amiga bytes, 0xFD as a letter
        let mut bytes = Vec::new();
        for _ in 0..10 {
            bytes.extend_from_slice(b"$\xa9\xaa\xfd\xfd\xfd ");
        }
        assert_eq!(detect_encoding("ko.asc", &bytes), Some(TextEncoding::Cp437));
    }

    #[test]
    fn test_escape_codes_keep_cp437_despite_amiga_bytes() {
        let bytes = b"\x1b[31m\xb7\xb7\xb7\xaf";
        assert_eq!(detect_encoding("a.asc", bytes), Some(TextEncoding::Cp437));
        assert_eq!(detect_encoding("a.txt", bytes), Some(TextEncoding::Cp437));
    }

    #[test]
    fn test_amiga_sauce_font_is_latin1() {
        let bytes = with_sauce_full(b"\x1b[31m\xdb\xdb", 1, 1, 80, 0, "Amiga Topaz 2+");
        assert_eq!(detect_encoding("a.ans", &bytes), Some(TextEncoding::Latin1));
        let pot = with_sauce_full(b"x", 1, 0, 80, 0, "P0T-NOoDLE");
        assert_eq!(detect_encoding("a.ans", &pot), Some(TextEncoding::Latin1));
    }

    #[test]
    fn test_amiga_txt_is_latin1() {
        let bytes = b"  \xb7\xb7\xb7 \xaf\xaf\xd6 kayozz";
        assert_eq!(
            detect_encoding("art.txt", bytes),
            Some(TextEncoding::Latin1)
        );
    }

    #[test]
    fn test_latin1_prose_is_not_amiga_art() {
        let french: Vec<u8> = "Le café était fermé, voilà © 2020, très élégant"
            .chars()
            .map(|c| c as u32 as u8)
            .collect();
        assert_eq!(
            detect_encoding("notes.txt", &french),
            Some(TextEncoding::Utf8)
        );
        let german: Vec<u8> = "Größe Straße Übung © Äpfel"
            .chars()
            .map(|c| c as u32 as u8)
            .collect();
        assert_eq!(
            detect_encoding("notes.txt", &german),
            Some(TextEncoding::Utf8)
        );
    }

    #[test]
    fn test_decode_latin1_art() {
        let d = decode_preview_bytes("a.asc", b"\xb7\xa5\xaf\x01\x9bx");
        assert_eq!(d.text, "·¥¯  x");
        assert!(!d.is_binary);
        assert_eq!(d.art, Some(ArtStyle { width: 80 }));
    }

    #[test]
    fn test_sauce_ice_flag_does_not_change_art() {
        let bytes = with_sauce_full(b"\x1b[5;41mX", 1, 1, 80, 1, "IBM VGA");
        assert_eq!(
            decode_preview_bytes("a.ans", &bytes).art,
            Some(ArtStyle { width: 80 })
        );
    }

    #[test]
    fn test_sauce_reads_width() {
        assert_eq!(
            sauce(&with_sauce(b"art", 1, 1, 132)),
            Some(Sauce {
                width: Some(132),
                character: true,
                amiga_font: false
            })
        );
    }

    #[test]
    fn test_sauce_missing_or_short() {
        assert_eq!(sauce(b"no record here"), None);
        assert_eq!(sauce(b"SAUCE00"), None);
    }

    #[test]
    fn test_sauce_width_only_for_character_art() {
        assert_eq!(
            sauce(&with_sauce(b"x", 5, 1, 132)),
            Some(Sauce {
                width: None,
                character: false,
                amiga_font: false
            })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 3, 132)),
            Some(Sauce {
                width: None,
                character: true,
                amiga_font: false
            })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 1, 0)),
            Some(Sauce {
                width: None,
                character: true,
                amiga_font: false
            })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 1, 1001)),
            Some(Sauce {
                width: None,
                character: true,
                amiga_font: false
            })
        );
    }

    #[test]
    fn test_art_run_needs_four_in_a_row() {
        assert!(!has_cp437_art_run(&[0xDB, 0xDB, 0xDB, b'a']));
        assert!(has_cp437_art_run(&[b'a', 0xDB, 0xDB, 0xDB, 0xDB]));
        assert!(has_cp437_art_run(&[0xCD; 4]));
        assert!(!has_cp437_art_run(&[0xDB, 0xB0, 0xDB, 0xB0])); // mixed bytes are not a run
        assert!(!has_cp437_art_run(&[0xDF, b' ', 0xDF, b' ', 0xDF])); // scattered ß
    }

    #[test]
    fn test_detect_utf16_boms() {
        assert_eq!(
            detect_encoding("a.txt", &[0xFF, 0xFE, b'h', 0]),
            Some(TextEncoding::Utf16Le)
        );
        assert_eq!(
            detect_encoding("a.txt", &[0xFE, 0xFF, 0, b'h']),
            Some(TextEncoding::Utf16Be)
        );
    }

    #[test]
    fn test_detect_utf16_without_bom() {
        let le: Vec<u8> = "hello world"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let be: Vec<u8> = "hello world"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert_eq!(detect_encoding("a.txt", &le), Some(TextEncoding::Utf16Le));
        assert_eq!(detect_encoding("a.txt", &be), Some(TextEncoding::Utf16Be));
    }

    #[test]
    fn test_mostly_nul_binary_is_not_utf16() {
        // Odd bytes all NUL, but an even byte is NUL too
        assert_eq!(
            detect_encoding("a.bin", &[b'a', 0, 0, 0, b'b', 0, b'c', 0]),
            None
        );
    }

    #[test]
    fn test_detect_art_by_extension_even_with_nul() {
        for name in ["a.ans", "A.ASC", "release.nfo", "file_id.DIZ"] {
            assert_eq!(
                detect_encoding(name, b"x\0y"),
                Some(TextEncoding::Cp437),
                "{}",
                name
            );
        }
    }

    #[test]
    fn test_non_character_sauce_is_not_art() {
        assert_eq!(
            detect_encoding("song.xm", &with_sauce(&[0, 1, 2, 0], 4, 0, 0)),
            None
        );
    }

    #[test]
    fn test_escape_codes_make_art() {
        assert_eq!(
            detect_encoding("a.txt", b"\x1b[31mred"),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_kodi_nfo_xml_is_text() {
        let xml = "<movie><plot>It\u{2019}s caf\u{e9}</plot></movie>".as_bytes();
        assert_eq!(detect_encoding("movie.nfo", xml), Some(TextEncoding::Utf8));
        assert_eq!(decode_preview_bytes("movie.nfo", xml).art, None);
    }

    #[test]
    fn test_ascii_xml_nfo_is_text() {
        assert_eq!(
            detect_encoding("tvshow.nfo", b"  <?xml version=\"1.0\"?><tvshow/>"),
            Some(TextEncoding::Utf8)
        );
        assert_eq!(
            detect_encoding("tvshow.nfo", b"\xEF\xBB\xBF\n<tvshow/>"),
            Some(TextEncoding::Utf8)
        );
    }

    #[test]
    fn test_ascii_scene_nfo_is_art() {
        assert_eq!(
            detect_encoding("release.nfo", b"  RELEASE INFO\n  ====="),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_cp437_nfo_is_art() {
        assert_eq!(
            detect_encoding("release.nfo", &[0xDB, 0xDB, b'A']),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_detect_art_by_sauce() {
        assert_eq!(
            detect_encoding("art.txt", &with_sauce(b"plain", 1, 1, 80)),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_detect_art_by_run_in_invalid_utf8() {
        assert_eq!(
            detect_encoding("logo.txt", &[b' ', 0xDB, 0xDB, 0xDB, 0xDB, b'\n']),
            Some(TextEncoding::Cp437)
        );
    }

    #[test]
    fn test_latin1_text_is_not_art() {
        // "Straße Größe" in Latin-1: invalid UTF-8, no runs
        let latin1 = b"Stra\xDFe Gr\xF6\xDFe";
        assert_eq!(detect_encoding("de.txt", latin1), Some(TextEncoding::Utf8));
    }

    #[test]
    fn test_binary_with_escape_or_run_stays_binary() {
        assert_eq!(detect_encoding("a.bin", b"\0\x1b[31mred"), None);
        assert_eq!(detect_encoding("a.bin", &[0, 0xDB, 0xDB, 0xDB, 0xDB]), None);
    }

    #[test]
    fn test_detect_empty_and_tiny_files() {
        assert_eq!(detect_encoding("a.txt", b""), Some(TextEncoding::Utf8));
        assert_eq!(detect_encoding("a.txt", b"hi"), Some(TextEncoding::Utf8));
        assert_eq!(decode_preview_bytes("a.txt", b"").text, "");
    }

    #[test]
    fn test_decode_utf8_bom_dropped() {
        let d = decode_preview_bytes("a.txt", b"\xEF\xBB\xBFhello");
        assert_eq!(
            d,
            DecodedText {
                text: "hello".into(),
                is_binary: false,
                art: None
            }
        );
    }

    #[test]
    fn test_decode_invalid_utf8_is_lossy_and_binary() {
        let d = decode_preview_bytes("de.txt", b"Stra\xDFe");
        assert!(d.is_binary);
        assert!(d.text.starts_with("Stra"));
    }

    #[test]
    fn test_decode_utf16_le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("héllo\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()));
        let d = decode_preview_bytes("a.txt", &bytes);
        assert_eq!(
            d,
            DecodedText {
                text: "héllo\r\n".into(),
                is_binary: false,
                art: None
            }
        );
    }

    #[test]
    fn test_decode_utf16_be_drops_trailing_odd_byte() {
        let mut bytes: Vec<u8> = "ok".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        bytes.insert(0, 0xFF);
        bytes.insert(0, 0xFE);
        bytes.push(b'x');
        assert_eq!(decode_preview_bytes("a.txt", &bytes).text, "ok");
    }

    #[test]
    fn test_cp437_control_bytes_show_their_dos_glyphs() {
        let bytes: Vec<u8> = (0x01..=0x1Fu8)
            .filter(|b| ![0x09, 0x0A, 0x0D, 0x1A, 0x1B].contains(b))
            .chain([0x7F])
            .collect();
        assert_eq!(
            decode_preview_bytes("a.ans", &bytes).text,
            "☺☻♥♦♣♠•◘♂♀♫☼►◄↕‼¶§▬↨↑↓∟↔▲▼⌂"
        );
    }

    #[test]
    fn test_cp437_keeps_working_control_bytes() {
        let text = decode_preview_bytes("a.ans", b"\x1b[31mA\tB\r\nC\x1aSAUCE00").text;
        assert_eq!(text, "\x1b[31mA\tB\r\nC\x1aSAUCE00");
    }

    #[test]
    fn test_cp437_nul_is_blank() {
        assert_eq!(decode_preview_bytes("a.ans", b"A\0B").text, "A B");
    }

    #[test]
    fn test_utf8_text_keeps_control_bytes() {
        assert_eq!(decode_preview_bytes("a.txt", b"A\x01B").text, "A\x01B");
    }

    #[test]
    fn test_decode_cp437_art_style() {
        let d = decode_preview_bytes("a.nfo", &[0xDB, b'A']);
        assert_eq!(
            d,
            DecodedText {
                text: "█A".into(),
                is_binary: false,
                art: Some(ArtStyle { width: 80 })
            }
        );
        let wide = decode_preview_bytes("a.ans", &with_sauce(b"x", 1, 1, 132));
        assert_eq!(wide.art.map(|a| a.width), Some(132));
    }

    #[test]
    fn test_decode_binary_extracts_strings() {
        let d = decode_preview_bytes("a.bin", b"\0\0Hello binary world\0\0");
        assert!(d.is_binary);
        assert_eq!(d.art, None);
        assert!(d.text.contains("Hello binary world"));
    }
}
