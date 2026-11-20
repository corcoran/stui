//! Terminal detection that ratatui-image does not cover.

use ratatui_image::picker::ProtocolType;

/// Graphics protocol to use when ratatui-image's `auto` detection returned
/// half-blocks but the terminal supports a graphics protocol.
///
/// ratatui-image skips its graphics queries whenever `KONSOLE_VERSION` is set,
/// which also hits kitty started from a Konsole shell (the variable is inherited).
/// Konsole gets iTerm2: it shows ratatui-image's Kitty unicode placeholders as
/// stray characters. Konsole sets `TERM=xterm-256color`; other terminals launched
/// from it set their own `TERM`, so they keep half-blocks.
pub fn protocol_override(
    detected: ProtocolType,
    env: impl Fn(&str) -> Option<String>,
) -> Option<ProtocolType> {
    if detected != ProtocolType::Halfblocks {
        return None;
    }
    let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    let term = env("TERM").unwrap_or_default();

    if set("TMUX") {
        return None;
    }
    if set("KITTY_WINDOW_ID") || term == "xterm-kitty" {
        return Some(ProtocolType::Kitty);
    }
    if set("KONSOLE_VERSION") && (term == "xterm-256color" || term.starts_with("konsole")) {
        return Some(ProtocolType::Iterm2);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    // ========================================
    // KITTY OVERRIDE
    // ========================================

    #[test]
    fn test_konsole_gets_iterm2() {
        let env = env_of(&[("KONSOLE_VERSION", "260803"), ("TERM", "xterm-256color")]);
        assert_eq!(
            protocol_override(ProtocolType::Halfblocks, env),
            Some(ProtocolType::Iterm2)
        );
    }

    #[test]
    fn test_kitty_started_from_konsole_gets_kitty() {
        let env = env_of(&[
            ("KONSOLE_VERSION", "260803"),
            ("KITTY_WINDOW_ID", "1"),
            ("TERM", "xterm-kitty"),
        ]);
        assert_eq!(
            protocol_override(ProtocolType::Halfblocks, env),
            Some(ProtocolType::Kitty)
        );
    }

    #[test]
    fn test_other_terminal_started_from_konsole_keeps_halfblocks() {
        for term in ["alacritty", "foot", "xterm-ghostty", "linux"] {
            let env = env_of(&[("KONSOLE_VERSION", "260803"), ("TERM", term)]);
            assert_eq!(
                protocol_override(ProtocolType::Halfblocks, env),
                None,
                "TERM={term}"
            );
        }
    }

    #[test]
    fn test_tmux_keeps_library_result() {
        let env = env_of(&[
            ("KONSOLE_VERSION", "260803"),
            ("TERM", "xterm-256color"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
        ]);
        assert_eq!(protocol_override(ProtocolType::Halfblocks, env), None);
    }

    #[test]
    fn test_detected_protocol_is_kept() {
        let env = env_of(&[("KONSOLE_VERSION", "260803"), ("TERM", "xterm-256color")]);
        for detected in [
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            assert_eq!(protocol_override(detected, &env), None);
        }
    }

    #[test]
    fn test_plain_terminal_keeps_halfblocks() {
        let env = env_of(&[("TERM", "xterm-256color")]);
        assert_eq!(protocol_override(ProtocolType::Halfblocks, env), None);
        let empty = env_of(&[("KONSOLE_VERSION", ""), ("TERM", "xterm-256color")]);
        assert_eq!(protocol_override(ProtocolType::Halfblocks, empty), None);
    }
}
