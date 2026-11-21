//! Configured commands (`clipboard_command`, `open_command`, ...) as a program and its
//! arguments.

/// Splits a configured command into its program and arguments, with shell quoting rules
/// but no shell. `Ok(None)` for a blank command; `Err` for unbalanced quotes.
pub fn parse_command(command: &str) -> Result<Option<(String, Vec<String>)>, String> {
    let mut words = shell_words::split(command).map_err(|e| e.to_string())?;
    if words.is_empty() {
        return Ok(None);
    }
    let program = words.remove(0);
    Ok(Some((program, words)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain() {
        assert_eq!(
            parse_command("wl-copy"),
            Ok(Some(("wl-copy".to_string(), vec![])))
        );
    }

    #[test]
    fn test_parse_arguments() {
        assert_eq!(
            parse_command("xclip -selection clipboard"),
            Ok(Some((
                "xclip".to_string(),
                vec!["-selection".to_string(), "clipboard".to_string()]
            )))
        );
    }

    #[test]
    fn test_parse_quoted() {
        assert_eq!(
            parse_command("\"/opt/My Apps/copy\" --x"),
            Ok(Some((
                "/opt/My Apps/copy".to_string(),
                vec!["--x".to_string()]
            )))
        );
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_command("  "), Ok(None));
    }

    #[test]
    fn test_parse_unbalanced_quote_is_error() {
        assert!(parse_command("copy \"oops").is_err());
    }
}
