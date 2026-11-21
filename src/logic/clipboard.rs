//! Copying through the terminal with OSC 52, which works locally and over SSH

/// The OSC 52 sequence that copies `text` to the terminal's clipboard
pub fn osc52_sequence(text: &str) -> String {
    use crossterm::Command;
    let mut sequence = String::new();
    crossterm::clipboard::CopyToClipboard::to_clipboard_from(text.as_bytes())
        .write_ansi(&mut sequence)
        .expect("writing to a String cannot fail");
    sequence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osc52_sequence_bytes() {
        assert_eq!(osc52_sequence("hi"), "\x1b]52;c;aGk=\x1b\\");
    }

    #[test]
    fn test_osc52_sequence_non_ascii() {
        assert_eq!(osc52_sequence("café/ñ"), "\x1b]52;c;Y2Fmw6kvw7E=\x1b\\");
    }
}
