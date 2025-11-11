//! File type detection and utilities
//!
//! Pure functions for identifying file types and properties.

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
            while j < bytes.len() && j < i + 20 { // Check next 20 bytes max
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

/// Parse ANSI codes and convert to Ratatui Text with styling
///
/// This is a safe, custom ANSI parser that ONLY handles SGR (color/style) codes.
/// It ignores all cursor positioning, screen clearing, and other potentially
/// dangerous ANSI codes that could corrupt the TUI.
///
/// Supported SGR codes:
/// - 0: Reset
/// - 1: Bold
/// - 3: Italic
/// - 4: Underline
/// - 30-37: Foreground colors (8 colors)
/// - 40-47: Background colors (8 colors)
/// - 90-97: Bright foreground colors
/// - 100-107: Bright background colors
///
/// # Arguments
/// * `text` - Text with ANSI escape codes
///
/// # Returns
/// Ratatui `Text` with styling applied, all non-SGR codes stripped
pub fn parse_ansi_to_text(text: &str) -> ratatui::text::Text<'static> {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span, Text};

    let mut lines = Vec::new();

    // Strip SAUCE metadata if present (common in ANSI art files)
    // SAUCE starts with Ctrl-Z (0x1A) followed by "SAUCE00"
    let text_without_sauce = if let Some(pos) = text.find('\x1A') {
        &text[..pos]
    } else {
        text
    };

    // Handle different line endings: \n, \r\n, or \r
    // Replace \r\n with \n first, then split on both \n and \r
    let normalized_text = text_without_sauce.replace("\r\n", "\n").replace('\r', "\n");

    // Use fixed-width line buffer for proper ANSI art rendering
    // ANSI art uses cursor positioning to create multi-column layouts
    // Standard ANSI art wraps at 80 columns
    const LINE_WIDTH: usize = 80;

    for line_text in normalized_text.lines() {
        // Create a line buffer with (char, style) pairs
        let mut line_buffer: Vec<(char, Style)> = vec![(' ', Style::default()); LINE_WIDTH];
        let mut current_column = 0;
        let mut current_style = Style::default();
        let mut chars = line_text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                chars.next(); // consume '['

                // Parse the parameter bytes
                let mut params = String::new();
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_digit() || next_ch == ';' {
                        params.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Get the final byte (command)
                let command = chars.next();

                // Handle cursor forward (CUF) - move cursor n columns forward
                if command == Some('C') {
                    let move_amount = params.parse::<usize>().unwrap_or(1);
                    current_column = (current_column + move_amount).min(LINE_WIDTH - 1);
                }
                // Only process SGR codes (ending in 'm')
                else if command == Some('m') {
                    // Parse SGR parameters
                    // Empty params = reset (0)
                    let codes: Vec<u8> = if params.is_empty() {
                        vec![0]
                    } else {
                        params
                            .split(';')
                            .filter_map(|s| {
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    Some(0) // Empty parameter = reset
                                } else {
                                    trimmed.parse().ok()
                                }
                            })
                            .collect()
                    };

                    for code in codes {
                        match code {
                            0 => current_style = Style::default(), // Reset
                            1 => current_style = current_style.add_modifier(Modifier::BOLD),
                            3 => current_style = current_style.add_modifier(Modifier::ITALIC),
                            4 => current_style = current_style.add_modifier(Modifier::UNDERLINED),
                            // Foreground colors (30-37)
                            30 => current_style = current_style.fg(Color::Black),
                            31 => current_style = current_style.fg(Color::Red),
                            32 => current_style = current_style.fg(Color::Green),
                            33 => current_style = current_style.fg(Color::Yellow),
                            34 => current_style = current_style.fg(Color::Blue),
                            35 => current_style = current_style.fg(Color::Magenta),
                            36 => current_style = current_style.fg(Color::Cyan),
                            37 => current_style = current_style.fg(Color::White),
                            // Background colors (40-47)
                            40 => current_style = current_style.bg(Color::Black),
                            41 => current_style = current_style.bg(Color::Red),
                            42 => current_style = current_style.bg(Color::Green),
                            43 => current_style = current_style.bg(Color::Yellow),
                            44 => current_style = current_style.bg(Color::Blue),
                            45 => current_style = current_style.bg(Color::Magenta),
                            46 => current_style = current_style.bg(Color::Cyan),
                            47 => current_style = current_style.bg(Color::White),
                            // Bright foreground colors (90-97)
                            90 => current_style = current_style.fg(Color::DarkGray),
                            91 => current_style = current_style.fg(Color::LightRed),
                            92 => current_style = current_style.fg(Color::LightGreen),
                            93 => current_style = current_style.fg(Color::LightYellow),
                            94 => current_style = current_style.fg(Color::LightBlue),
                            95 => current_style = current_style.fg(Color::LightMagenta),
                            96 => current_style = current_style.fg(Color::LightCyan),
                            97 => current_style = current_style.fg(Color::Gray),
                            // Bright background colors (100-107)
                            100 => current_style = current_style.bg(Color::DarkGray),
                            101 => current_style = current_style.bg(Color::LightRed),
                            102 => current_style = current_style.bg(Color::LightGreen),
                            103 => current_style = current_style.bg(Color::LightYellow),
                            104 => current_style = current_style.bg(Color::LightBlue),
                            105 => current_style = current_style.bg(Color::LightMagenta),
                            106 => current_style = current_style.bg(Color::LightCyan),
                            107 => current_style = current_style.bg(Color::Gray),
                            _ => {} // Ignore unknown SGR codes
                        }
                    }
                }
                // All non-SGR codes are silently ignored (cursor movement, etc.)
            } else {
                // Write character to buffer at current position
                if current_column < LINE_WIDTH {
                    line_buffer[current_column] = (ch, current_style);
                    current_column += 1;

                    // If we hit column 80, flush the buffer as a line and start new one
                    if current_column >= LINE_WIDTH {
                        // Build spans from current line buffer
                        let spans = build_spans_from_buffer(&line_buffer);
                        lines.push(Line::from(spans));

                        // Reset buffer for next line
                        line_buffer = vec![(' ', Style::default()); LINE_WIDTH];
                        current_column = 0;
                    }
                }
            }
        }

        // Convert remaining line buffer to spans, trimming trailing spaces
        let mut max_col = 0;
        for (i, (ch, _)) in line_buffer.iter().enumerate() {
            if *ch != ' ' {
                max_col = i + 1;
            }
        }

        // Build spans from line buffer only if there's content
        if max_col > 0 {
            let spans = build_spans_from_buffer_upto(&line_buffer, max_col);
            lines.push(Line::from(spans));
        } else if lines.is_empty() {
            // Empty line at start - add it
            lines.push(Line::from(vec![Span::raw("")]));
        }
    }

    Text::from(lines)
}

/// Helper function to build spans from entire line buffer
fn build_spans_from_buffer(line_buffer: &[(char, ratatui::style::Style)]) -> Vec<ratatui::text::Span<'static>> {
    build_spans_from_buffer_upto(line_buffer, line_buffer.len())
}

/// Helper function to build spans from line buffer up to a certain column
fn build_spans_from_buffer_upto(line_buffer: &[(char, ratatui::style::Style)], max_col: usize) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;
    use ratatui::style::Style;

    let mut spans = Vec::new();
    if max_col == 0 {
        spans.push(Span::raw(""));
        return spans;
    }

    let mut current_span_text = String::new();
    let mut current_span_style = line_buffer[0].1;

    for i in 0..max_col {
        let (ch, mut style) = line_buffer[i];

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
        assert!(is_image_file(".png"));  // Just extension - technically valid image file
        assert!(is_image_file("a.png"));  // Single character filename
        assert!(!is_image_file("png"));  // No extension dot
    }

    // ========================================
    // BINARY CONTENT DETECTION
    // ========================================

    #[test]
    fn test_is_binary_content_text() {
        // Plain ASCII text
        assert!(!is_binary_content(b"Hello, world!"));
        assert!(!is_binary_content(b"Line 1\nLine 2\n"));
        assert!(!is_binary_content(b""));  // Empty file is text
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
        assert!(is_binary_content(b"\x00"));  // Just a null byte
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
        assert!(contains_ansi_codes(b"\x1b[31mRed\x1b[0m \x1b[32mGreen\x1b[0m"));
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
        use ratatui::style::Color;

        // Red foreground
        let text = parse_ansi_to_text("\x1b[31mRed\x1b[0m");
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn test_parse_ansi_to_text_cursor_forward() {
        // Cursor forward should add spaces
        let text = parse_ansi_to_text("A\x1b[5CB");
        assert_eq!(text.lines.len(), 1);
        // Should have "A" + 5 spaces + "B"
        let full_text: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
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

    #[test]
    fn test_parse_ansi_to_text_sauce_stripping() {
        // SAUCE metadata starts with Ctrl-Z
        let text_with_sauce = "Content\x1ASAUCE00metadata";
        let text = parse_ansi_to_text(text_with_sauce);
        let full_text: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
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
        use ratatui::style::Color;

        // Set background color, write text, then spaces via cursor forward
        let text = parse_ansi_to_text("\x1b[41mText\x1b[5C");
        let line = &text.lines[0];

        // First span should have red background (the "Text")
        assert_eq!(line.spans[0].content, "Text");
        assert_eq!(line.spans[0].style.bg, Some(Color::Red));

        // Spaces from cursor forward should NOT have background
        if line.spans.len() > 1 {
            assert_eq!(line.spans[1].style.bg, None);
        }
    }

    #[test]
    fn test_parse_ansi_to_text_multiple_styles() {
        use ratatui::style::{Color, Modifier};

        // Bold + Red foreground + Black background
        let text = parse_ansi_to_text("\x1b[1;31;40mStyled\x1b[0m");
        let span = &text.lines[0].spans[0];

        assert_eq!(span.content, "Styled");
        assert_eq!(span.style.fg, Some(Color::Red));
        assert_eq!(span.style.bg, Some(Color::Black));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
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

        assert!(result.contains("Binary file - extracted text"),
            "Should indicate it's a binary file");
        assert!(result.contains("Hello"), "Should extract 'Hello' (5 chars)");
        assert!(result.contains("World"), "Should extract 'World' (5 chars)");
        assert!(result.contains("Test String"), "Should extract 'Test String' (11 chars)");
    }

    #[test]
    fn test_extract_text_from_binary_pure_binary() {
        // Pure binary with no readable strings >= 4 chars
        let binary = b"\x00\x01\x02\x03\xFF\xFE\xFD\xFC";
        let result = extract_text_from_binary(binary);

        assert_eq!(result, "[Binary file - no readable text found]",
            "Pure binary should show 'no readable text found' message");
    }

    #[test]
    fn test_extract_text_from_binary_mixed_content() {
        // Mix of binary and text with various string lengths
        let binary = b"\x00\x00Data\x00\x01Short\x00\x02AB\x00LongerString\x00\xFF";
        let result = extract_text_from_binary(binary);

        assert!(result.contains("Data"), "Should extract 'Data' (4 chars - at threshold)");
        assert!(result.contains("Short"), "Should extract 'Short' (5 chars)");
        assert!(!result.contains("AB"), "Should NOT extract 'AB' (2 chars - below threshold)");
        assert!(result.contains("LongerString"), "Should extract 'LongerString' (12 chars)");
    }

    #[test]
    fn test_extract_text_from_binary_min_length_threshold() {
        // Test the MIN_STRING_LENGTH threshold (4 chars)
        let binary = b"A\x00AB\x00ABC\x00ABCD\x00ABCDE\x00";
        let result = extract_text_from_binary(binary);

        assert!(!result.contains("A\n"), "Should NOT extract 1-char strings");
        assert!(!result.contains("AB\n"), "Should NOT extract 2-char strings");
        assert!(!result.contains("ABC\n"), "Should NOT extract 3-char strings");
        assert!(result.contains("ABCD"), "Should extract 4-char strings (at threshold)");
        assert!(result.contains("ABCDE"), "Should extract 5-char strings");
    }

    #[test]
    fn test_extract_text_from_binary_special_chars() {
        // Test handling of printable ASCII + special characters (newline, tab)
        let binary = b"Line1\nLine2\x00Tab\tSeparated\x00Normal Text\x00\xFF";
        let result = extract_text_from_binary(binary);

        // Newlines and tabs should be preserved as part of strings
        assert!(result.contains("Line1\nLine2"),
            "Should preserve newlines within strings");
        assert!(result.contains("Tab\tSeparated"),
            "Should preserve tabs within strings");
        assert!(result.contains("Normal Text"),
            "Should extract normal printable ASCII");
    }
}
