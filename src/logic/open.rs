//! Whether `o` opens a file with `open_command` (desktop apps, in the background) or with the
//! editor inside stui's window

/// What the environment says about where a file can be opened
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenEnv {
    /// SSH_CONNECTION or SSH_TTY is set
    pub ssh: bool,
    /// WAYLAND_DISPLAY or DISPLAY is set
    pub desktop_session: bool,
    /// Linux/BSD, where a desktop session needs one of those variables. False on macOS and
    /// Windows, which always have a desktop locally.
    pub display_server_os: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWith {
    /// `open_command`, started in the background
    Desktop,
    /// The editor, run inside stui's window while stui waits
    Editor,
    /// A binary file with no desktop to open it on
    Blocked,
}

/// What `o` is opening, as far as choosing a program goes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Text,
    Binary,
    Directory,
}

/// Whether desktop apps can open windows here: not over SSH, and on Linux/BSD only with a
/// Wayland or X session
pub fn has_desktop(env: &OpenEnv) -> bool {
    !env.ssh && (!env.display_server_os || env.desktop_session)
}

/// Where `o` opens a file. Without a desktop, text files and directories go to the editor
/// and binary files can't open; on the desktop, text files go to the editor when
/// `open_text_in_editor` is set and everything else to `open_command`.
pub fn open_with(env: &OpenEnv, open_text_in_editor: bool, kind: FileKind) -> OpenWith {
    match (has_desktop(env), kind) {
        (false, FileKind::Binary) => OpenWith::Blocked,
        (false, _) => OpenWith::Editor,
        (true, FileKind::Text) if open_text_in_editor => OpenWith::Editor,
        (true, _) => OpenWith::Desktop,
    }
}

/// The editor: the `editor` setting, else `$VISUAL`, else `$EDITOR`. None when none is set;
/// blank values count as unset.
pub fn editor_command(
    configured: Option<&str>,
    visual: Option<&str>,
    editor: Option<&str>,
) -> Option<String> {
    [configured, visual, editor]
        .into_iter()
        .flatten()
        .find(|command| !command.trim().is_empty())
        .map(str::to_string)
}

/// `OpenEnv` from environment variables read through `var`; empty values count as unset
pub fn open_env(var: impl Fn(&str) -> Option<String>, display_server_os: bool) -> OpenEnv {
    let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    OpenEnv {
        ssh: set("SSH_CONNECTION") || set("SSH_TTY"),
        desktop_session: set("WAYLAND_DISPLAY") || set("DISPLAY"),
        display_server_os,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(ssh: bool, desktop_session: bool, display_server_os: bool) -> OpenEnv {
        OpenEnv {
            ssh,
            desktop_session,
            display_server_os,
        }
    }

    // ========================================
    // DESKTOP DETECTION
    // ========================================

    #[test]
    fn test_ssh_session_has_no_desktop() {
        assert!(!has_desktop(&env(true, true, true)));
    }

    #[test]
    fn test_display_server_os_without_session_has_no_desktop() {
        assert!(!has_desktop(&env(false, false, true)));
    }

    #[test]
    fn test_local_desktop_session_has_desktop() {
        assert!(has_desktop(&env(false, true, true)));
        // macOS and Windows have a desktop without WAYLAND_DISPLAY or DISPLAY
        assert!(has_desktop(&env(false, false, false)));
    }

    // ========================================
    // OPEN WITH
    // ========================================

    #[test]
    fn test_without_desktop_text_and_directories_open_in_editor() {
        let ssh = env(true, true, true);
        assert_eq!(open_with(&ssh, false, FileKind::Text), OpenWith::Editor);
        assert_eq!(
            open_with(&ssh, false, FileKind::Directory),
            OpenWith::Editor
        );
    }

    #[test]
    fn test_without_desktop_binary_files_are_blocked() {
        let ssh = env(true, true, true);
        assert_eq!(open_with(&ssh, false, FileKind::Binary), OpenWith::Blocked);
        assert_eq!(open_with(&ssh, true, FileKind::Binary), OpenWith::Blocked);
    }

    #[test]
    fn test_desktop_opens_every_file_with_open_command_by_default() {
        let desktop = env(false, true, true);
        for kind in [FileKind::Text, FileKind::Binary, FileKind::Directory] {
            assert_eq!(
                open_with(&desktop, false, kind),
                OpenWith::Desktop,
                "{:?}",
                kind
            );
        }
    }

    #[test]
    fn test_open_text_in_editor_sends_only_text_to_editor() {
        let desktop = env(false, true, true);
        assert_eq!(open_with(&desktop, true, FileKind::Text), OpenWith::Editor);
        assert_eq!(
            open_with(&desktop, true, FileKind::Binary),
            OpenWith::Desktop
        );
        assert_eq!(
            open_with(&desktop, true, FileKind::Directory),
            OpenWith::Desktop
        );
    }

    // ========================================
    // EDITOR COMMAND
    // ========================================

    #[test]
    fn test_editor_command_precedence() {
        assert_eq!(
            editor_command(Some("hx"), Some("nvim"), Some("vi")),
            Some("hx".to_string())
        );
        assert_eq!(
            editor_command(None, Some("nvim"), Some("vi")),
            Some("nvim".to_string())
        );
        assert_eq!(
            editor_command(None, None, Some("vi")),
            Some("vi".to_string())
        );
    }

    #[test]
    fn test_editor_command_none_without_an_editor() {
        assert_eq!(editor_command(None, None, None), None);
    }

    #[test]
    fn test_empty_editor_vars_count_as_unset() {
        assert_eq!(
            editor_command(Some(" "), Some(""), Some("vi")),
            Some("vi".to_string())
        );
        assert_eq!(editor_command(None, Some(""), Some("")), None);
    }

    #[test]
    fn test_open_env_empty_vars_unset() {
        let vars = |name: &str| match name {
            "SSH_CONNECTION" | "SSH_TTY" => Some(String::new()),
            "WAYLAND_DISPLAY" => Some("wayland-0".to_string()),
            _ => None,
        };
        let env = open_env(vars, true);
        assert!(!env.ssh);
        assert!(env.desktop_session);
        assert!(env.display_server_os);
    }
}
