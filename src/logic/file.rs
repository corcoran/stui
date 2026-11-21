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

/// Check if file content is binary based on presence of null bytes
///
/// A file is considered binary if it contains any null bytes (0x00) in the
/// first portion of the content. This is a heuristic similar to how Unix
/// tools like `file` detect binary content.
///
/// # Arguments
/// * `bytes` - File content to check (typically first 8KB is sufficient)
///
/// # Returns
/// `true` if the content contains null bytes (binary), `false` otherwise
///
/// # Examples
/// ```
/// use stui::logic::file::is_binary_content;
///
/// // Text content - no null bytes
/// assert!(!is_binary_content(b"Hello, world!"));
/// assert!(!is_binary_content(b"UTF-8: \xE2\x9C\x93"));
///
/// // Binary content - contains null bytes
/// assert!(is_binary_content(b"Hello\x00World"));
/// assert!(is_binary_content(b"\x00\x01\x02\x03"));
/// ```
pub fn is_markdown_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".md", ".markdown", ".mdown"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Whether the preview renders this file's content and `r` can switch to the raw source.
pub fn has_rendered_view(path: &str) -> bool {
    is_markdown_path(path)
}

pub fn is_binary_content(bytes: &[u8]) -> bool {
    bytes.contains(&0)
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
            22 => self.bold = false,
            23 => self.italic = false,
            24 => self.underline = false,
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
    /// grey, and does not change the font.
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
/// Draws the text on an 80-column grid of rows, like the DOS screen ANSI art was made
/// on, and returns the rows. Nothing reaches the real terminal. Relative cursor moves
/// (CSI A/B/C/D) move within the grid; screen clearing, absolute positioning and other
/// codes are ignored.
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
///
/// # Returns
/// Ratatui `Text` with styling applied, all escape codes stripped
pub fn parse_ansi_to_text(text: &str) -> ratatui::text::Text<'static> {
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

    // ANSI art is drawn on an 80-column screen; cursor codes move around a grid of rows
    const LINE_WIDTH: usize = 80;
    let blank_row = vec![(' ', Style::default()); LINE_WIDTH];
    let mut rows: Vec<Vec<(char, Style)>> = vec![blank_row.clone()];
    let mut row: usize = 0;
    let mut col: usize = 0;
    // Writing column 80 wraps to the next row at once, as on DOS. A newline right after
    // that wrap ends the same line, so it doesn't add a blank row.
    let mut just_wrapped = false;

    // SGR state carries across line breaks until the file changes it
    let mut sgr = SgrState::default();
    let mut current_style = sgr.style();

    let mut chars = normalized_text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['

            let mut params = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_ascii_digit() || next_ch == ';' {
                    params.push(next_ch);
                    chars.next();
                } else {
                    break;
                }
            }

            let count = || params.parse::<usize>().unwrap_or(1).max(1);
            match chars.next() {
                Some('A') => {
                    row = row.saturating_sub(count());
                    just_wrapped = false;
                }
                Some('B') => {
                    row += count();
                    just_wrapped = false;
                }
                Some('C') => col = (col + count()).min(LINE_WIDTH - 1),
                Some('D') => col = col.saturating_sub(count()),
                Some('m') => {
                    // Empty params or an empty parameter mean reset (0)
                    let codes: Vec<u8> = if params.is_empty() {
                        vec![0]
                    } else {
                        params
                            .split(';')
                            .filter_map(|s| {
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    Some(0)
                                } else {
                                    trimmed.parse().ok()
                                }
                            })
                            .collect()
                    };
                    for code in codes {
                        sgr.apply(code);
                    }
                    current_style = sgr.style();
                }
                // Screen clearing, absolute positioning and the rest are ignored
                _ => {}
            }
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
            just_wrapped = col == LINE_WIDTH;
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
}

/// The parts of a SAUCE record the preview uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sauce {
    pub width: Option<u16>,
}

/// Result of decoding a file's bytes for the preview.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedText {
    pub text: String,
    pub is_binary: bool,
    pub art_width: Option<u16>,
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
    let character_art = rec[94] == 1 && rec[95] <= 2;
    Some(Sauce {
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

fn has_art_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".ans", ".asc", ".nfo", ".diz"]
        .iter()
        .any(|ext| lower.ends_with(ext))
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
    for pair in window.chunks_exact(2) {
        even_nul += usize::from(pair[0] == 0);
        odd_nul += usize::from(pair[1] == 0);
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
/// 3. Extension `.ans`/`.asc`/`.nfo`/`.diz` or a SAUCE record: CP437, even with NUL bytes
/// 4. A NUL byte in the first 8 KB: binary
/// 5. Escape codes, or invalid UTF-8 with a CP437 art run: CP437
/// 6. Otherwise UTF-8
pub fn detect_encoding(path: &str, bytes: &[u8]) -> Option<TextEncoding> {
    match bytes {
        [0xFF, 0xFE, ..] => return Some(TextEncoding::Utf16Le),
        [0xFE, 0xFF, ..] => return Some(TextEncoding::Utf16Be),
        _ => {}
    }
    if let Some(enc) = sniff_utf16(bytes) {
        return Some(enc);
    }
    if has_art_extension(path) || sauce(bytes).is_some() {
        return Some(TextEncoding::Cp437);
    }
    if bytes[..bytes.len().min(PREVIEW_SNIFF_BYTES)].contains(&0) {
        return None;
    }
    if contains_ansi_codes(bytes)
        || (std::str::from_utf8(bytes).is_err() && has_cp437_art_run(bytes))
    {
        return Some(TextEncoding::Cp437);
    }
    Some(TextEncoding::Utf8)
}

/// Decodes a file's bytes for the preview according to `detect_encoding`.
///
/// Binary files yield their extracted strings. Invalid UTF-8 is decoded lossily
/// and flagged binary. `art_width` is set only for CP437 art.
pub fn decode_preview_bytes(path: &str, bytes: &[u8]) -> DecodedText {
    let text_only = |text: String| DecodedText {
        text,
        is_binary: false,
        art_width: None,
    };
    let utf16 = |units: &[u8], le: bool| {
        let units: Vec<u16> = units
            .chunks_exact(2)
            .map(|p| {
                if le {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        text_only(String::from_utf16_lossy(&units))
    };
    match detect_encoding(path, bytes) {
        None => DecodedText {
            text: extract_text_from_binary(bytes),
            is_binary: true,
            art_width: None,
        },
        Some(TextEncoding::Utf8) => match std::str::from_utf8(bytes) {
            Ok(s) => text_only(s.strip_prefix('\u{feff}').unwrap_or(s).to_string()),
            Err(_) => DecodedText {
                text: String::from_utf8_lossy(bytes).into_owned(),
                is_binary: true,
                art_width: None,
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
            text: String::borrow_from_cp437(bytes, &CP437_CONTROL),
            is_binary: false,
            art_width: Some(
                sauce(bytes)
                    .and_then(|s| s.width)
                    .unwrap_or(DEFAULT_ART_WIDTH),
            ),
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

    // ========================================
    // BINARY CONTENT DETECTION
    // ========================================

    #[test]
    fn test_is_binary_content_text() {
        // Plain ASCII text
        assert!(!is_binary_content(b"Hello, world!"));
        assert!(!is_binary_content(b"Line 1\nLine 2\n"));
        assert!(!is_binary_content(b"")); // Empty file is text
    }

    #[test]
    fn test_is_binary_content_utf8() {
        // UTF-8 encoded text (no null bytes)
        assert!(!is_binary_content("UTF-8: ✓".as_bytes()));
        assert!(!is_binary_content("日本語".as_bytes()));
        assert!(!is_binary_content("Emoji: 🦀".as_bytes()));
    }

    #[test]
    fn test_is_binary_content_binary() {
        // Content with null bytes
        assert!(is_binary_content(b"Hello\x00World"));
        assert!(is_binary_content(b"\x00\x01\x02\x03"));
        assert!(is_binary_content(b"\x00")); // Just a null byte
    }

    #[test]
    fn test_is_binary_content_null_at_start() {
        // Null byte at beginning
        assert!(is_binary_content(b"\x00text after null"));
    }

    #[test]
    fn test_is_binary_content_null_at_end() {
        // Null byte at end
        assert!(is_binary_content(b"text before null\x00"));
    }

    // ========================================
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
        let text = parse_ansi_to_text("Hello, world!");
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans.len(), 1);
        assert_eq!(text.lines[0].spans[0].content, "Hello, world!");
    }

    #[test]
    fn test_parse_ansi_to_text_colors() {
        // Red foreground
        let text = parse_ansi_to_text("\x1b[31mRed\x1b[0m");
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans[0].style.fg, Some(cga_color(1)));
    }

    fn first_style(input: &str) -> ratatui::style::Style {
        parse_ansi_to_text(input).lines[0].spans[0].style
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
        let text = parse_ansi_to_text("\x1b[1;31mRed\x1b[0mPlain");
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
        let text = parse_ansi_to_text("A\x1b[5CB");
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
        let text = parse_ansi_to_text(&long_line);
        assert_eq!(text.lines.len(), 1);

        // Create a line with 81 characters - should wrap
        let longer_line = "x".repeat(81);
        let text = parse_ansi_to_text(&longer_line);
        assert_eq!(text.lines.len(), 2);
        assert_eq!(text.lines[0].spans[0].content, "x".repeat(80));
        assert_eq!(text.lines[1].spans[0].content, "x");
    }

    fn line_texts(input: &str) -> Vec<String> {
        parse_ansi_to_text(input)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
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
        let text = parse_ansi_to_text("\x1b[1;31;44mA\nB");
        let style = text.lines[1].spans[0].style;
        assert_eq!(style.fg, Some(cga_color(9)));
        assert_eq!(style.bg, Some(cga_color(4)));
    }

    #[test]
    fn test_reset_carries_over_to_the_next_line() {
        let text = parse_ansi_to_text("\x1b[31mA\x1b[0m\nB");
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
        let text = parse_ansi_to_text(text_with_sauce);
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
        let text = parse_ansi_to_text("Line1\r\nLine2\r\nLine3");
        assert_eq!(text.lines.len(), 3);

        // Test \r line endings
        let text = parse_ansi_to_text("Line1\rLine2\rLine3");
        assert_eq!(text.lines.len(), 3);

        // Test \n line endings
        let text = parse_ansi_to_text("Line1\nLine2\nLine3");
        assert_eq!(text.lines.len(), 3);
    }

    #[test]
    fn test_parse_ansi_to_text_background_stripped_from_spaces() {
        // Set background color, write text, then spaces via cursor forward
        let text = parse_ansi_to_text("\x1b[41mText\x1b[5C");
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
        let text = parse_ansi_to_text("\x1b[1;31;40;4mStyled\x1b[0m");
        let span = &text.lines[0].spans[0];

        assert_eq!(span.content, "Styled");
        assert_eq!(span.style.fg, Some(cga_color(9)));
        assert_eq!(span.style.bg, Some(cga_color(0)));
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_parse_ansi_to_text_empty_sgr_params() {
        // Empty SGR parameters should reset (like \x1b[m)
        let text = parse_ansi_to_text("\x1b[31mRed\x1b[mNormal");
        assert_eq!(text.lines.len(), 1);
        // Should have two spans: "Red" (colored) and "Normal" (reset)
        assert!(text.lines[0].spans.len() >= 2);
    }

    #[test]
    fn test_parse_ansi_to_text_line_buffer_positioning() {
        // Test that cursor positioning works correctly within 80-column buffer
        // Write at column 0, jump to column 40, write again
        let text = parse_ansi_to_text("Start\x1b[35CHere");
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
    fn test_has_rendered_view_only_for_markdown() {
        assert!(has_rendered_view("README.md"));
        assert!(!has_rendered_view("notes.txt"));
        assert!(!has_rendered_view("art.ans"));
        assert!(!has_rendered_view("photo.png"));
    }

    // ========================================
    // ENCODING DETECTION
    // ========================================

    fn with_sauce(body: &[u8], data_type: u8, file_type: u8, width: u16) -> Vec<u8> {
        let mut v = body.to_vec();
        v.push(0x1A);
        let mut rec = vec![b' '; 128];
        rec[..7].copy_from_slice(b"SAUCE00");
        rec[94] = data_type;
        rec[95] = file_type;
        rec[96..98].copy_from_slice(&width.to_le_bytes());
        v.extend(rec);
        v
    }

    #[test]
    fn test_sauce_reads_width() {
        assert_eq!(
            sauce(&with_sauce(b"art", 1, 1, 132)),
            Some(Sauce { width: Some(132) })
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
            Some(Sauce { width: None })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 3, 132)),
            Some(Sauce { width: None })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 1, 0)),
            Some(Sauce { width: None })
        );
        assert_eq!(
            sauce(&with_sauce(b"x", 1, 1, 1001)),
            Some(Sauce { width: None })
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
                art_width: None
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
                art_width: None
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
    fn test_decode_cp437_art_width() {
        let d = decode_preview_bytes("a.nfo", &[0xDB, b'A']);
        assert_eq!(
            d,
            DecodedText {
                text: "█A".into(),
                is_binary: false,
                art_width: Some(80)
            }
        );
        let wide = decode_preview_bytes("a.ans", &with_sauce(b"x", 1, 1, 132));
        assert_eq!(wide.art_width, Some(132));
    }

    #[test]
    fn test_decode_binary_extracts_strings() {
        let d = decode_preview_bytes("a.bin", b"\0\0Hello binary world\0\0");
        assert!(d.is_binary);
        assert_eq!(d.art_width, None);
        assert!(d.text.contains("Hello binary world"));
    }
}
