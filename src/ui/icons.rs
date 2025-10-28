use ratatui::{
    style::{Color, Style},
    text::Span,
};

use crate::api::SyncState;

/// Icon display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconMode {
    Emoji,    // Standard emoji icons (📁, 📄, etc.)
    NerdFont, // Nerd Fonts icons (U+E5FF, etc.)
}

/// Folder states for rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    Loading,
    Paused,
    Syncing,
    OutOfSync,
    Synced,
    Scanning,
    Unknown,
    Error,
}

/// Icon theme with pastel colors
#[derive(Debug, Clone)]
pub struct IconTheme {
    // File type colors
    pub folder_color: Color,
    pub file_color: Color,

    // Status colors
    pub synced_color: Color,
    pub out_of_sync_color: Color,
    pub local_only_color: Color,
    pub remote_only_color: Color,
    pub ignored_color: Color,
    pub syncing_color: Color,
    pub scanning_color: Color,
    pub unknown_color: Color,
    pub error_color: Color,
    pub paused_color: Color,
}

impl Default for IconTheme {
    fn default() -> Self {
        Self {
            // File types
            folder_color: Color::Rgb(180, 140, 210), // Pastel purple
            file_color: Color::Rgb(150, 180, 220),   // Pastel blue

            // Status colors (pastels)
            synced_color: Color::Rgb(150, 220, 180),      // Pastel green
            out_of_sync_color: Color::Rgb(230, 220, 150), // Pastel yellow
            local_only_color: Color::Rgb(180, 180, 180),  // Pastel grey
            remote_only_color: Color::Rgb(220, 220, 220), // Pastel white
            ignored_color: Color::Rgb(220, 150, 150),     // Pastel red
            syncing_color: Color::Rgb(230, 180, 140),     // Pastel orange
            scanning_color: Color::Rgb(200, 170, 220),    // Pastel purple
            unknown_color: Color::Rgb(220, 150, 150),     // Pastel red
            error_color: Color::Rgb(220, 150, 150),       // Pastel red
            paused_color: Color::Rgb(180, 180, 180),      // Pastel grey
        }
    }
}

/// Internal status type for icon selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusType {
    Synced,
    OutOfSync,
    LocalOnly,
    RemoteOnly,
    Ignored,
    Syncing,
    Scanning,
    Unknown,
    Error,
    Paused,
}

/// Icon renderer that handles both emoji and Nerd Font modes
pub struct IconRenderer {
    mode: IconMode,
    theme: IconTheme,
}

impl IconRenderer {
    pub fn new(mode: IconMode, theme: IconTheme) -> Self {
        Self { mode, theme }
    }

    /// Render a folder with its status
    pub fn folder_with_status(&self, state: FolderState) -> Vec<Span<'static>> {
        let mut spans = vec![self.folder_icon()];

        let status_span = match state {
            FolderState::Loading => self.status_icon(StatusType::Scanning),
            FolderState::Paused => self.status_icon(StatusType::Paused),
            FolderState::Syncing => self.status_icon(StatusType::Syncing),
            FolderState::OutOfSync => self.status_icon(StatusType::OutOfSync),
            FolderState::Synced => self.status_icon(StatusType::Synced),
            FolderState::Scanning => self.status_icon(StatusType::Scanning),
            FolderState::Unknown => self.status_icon(StatusType::Unknown),
            FolderState::Error => self.status_icon(StatusType::Error),
        };

        spans.push(status_span);
        spans
    }

    /// Render a file or folder item with sync state
    pub fn item_with_sync_state(&self, is_dir: bool, state: SyncState) -> Vec<Span<'static>> {
        let mut spans = vec![];

        // Add folder or file icon
        if is_dir {
            spans.push(self.folder_icon());
        } else {
            spans.push(self.file_icon());
        }

        // Add sync state icon
        let status_type = match state {
            SyncState::Synced => StatusType::Synced,
            SyncState::OutOfSync => StatusType::OutOfSync,
            SyncState::LocalOnly => StatusType::LocalOnly,
            SyncState::RemoteOnly => StatusType::RemoteOnly,
            SyncState::Ignored => StatusType::Ignored,
            SyncState::Unknown => StatusType::Unknown,
        };

        spans.push(self.status_icon(status_type));
        spans
    }

    /// Render an ignored item (shows special indicator based on existence)
    pub fn ignored_item(&self, exists: bool) -> Vec<Span<'static>> {
        if exists {
            // Ignored + exists: show warning icon
            match self.mode {
                IconMode::Emoji => {
                    vec![Span::styled(
                        "🚫⚠️ ",
                        Style::default().fg(self.theme.ignored_color),
                    )]
                }
                IconMode::NerdFont => {
                    vec![
                        Span::styled("\u{F05E}", Style::default().fg(self.theme.ignored_color)),
                        Span::styled("\u{F071} ", Style::default().fg(self.theme.out_of_sync_color)),
                    ]
                }
            }
        } else {
            // Ignored + doesn't exist: just ban icon + double space for alignment
            match self.mode {
                IconMode::Emoji => {
                    vec![Span::styled(
                        "🚫  ",
                        Style::default().fg(self.theme.ignored_color),
                    )]
                }
                IconMode::NerdFont => {
                    vec![Span::styled(
                        "\u{F05E}  ",
                        Style::default().fg(self.theme.ignored_color),
                    )]
                }
            }
        }
    }

    /// Get folder icon span
    fn folder_icon(&self) -> Span<'static> {
        match self.mode {
            IconMode::Emoji => {
                Span::styled("📁", Style::default().fg(self.theme.folder_color))
            }
            IconMode::NerdFont => {
                Span::styled("\u{E5FF}", Style::default().fg(self.theme.folder_color))
            }
        }
    }

    /// Get file icon span
    fn file_icon(&self) -> Span<'static> {
        match self.mode {
            IconMode::Emoji => {
                Span::styled("📄", Style::default().fg(self.theme.file_color))
            }
            IconMode::NerdFont => {
                Span::styled("\u{F15B}", Style::default().fg(self.theme.file_color))
            }
        }
    }

    /// Get status icon span
    fn status_icon(&self, status: StatusType) -> Span<'static> {
        let (emoji_icon, nerd_icon, color) = match status {
            StatusType::Synced => ("✅ ", "\u{F00C} ", self.theme.synced_color),
            StatusType::OutOfSync => ("⚠️ ", "\u{F071} ", self.theme.out_of_sync_color),
            StatusType::LocalOnly => ("💻 ", "\u{F109} ", self.theme.local_only_color),
            StatusType::RemoteOnly => ("☁️ ", "\u{F0C2} ", self.theme.remote_only_color),
            StatusType::Ignored => ("🚫 ", "\u{F05E} ", self.theme.ignored_color),
            StatusType::Syncing => ("🔄 ", "\u{F021} ", self.theme.syncing_color),
            StatusType::Scanning => ("🔍 ", "\u{F002} ", self.theme.scanning_color),
            StatusType::Unknown => ("❓ ", "\u{F128} ", self.theme.unknown_color),
            StatusType::Error => ("❌ ", "\u{F00D} ", self.theme.error_color),
            StatusType::Paused => ("⏸  ", "\u{F04C}  ", self.theme.paused_color),
        };

        let icon = match self.mode {
            IconMode::Emoji => emoji_icon,
            IconMode::NerdFont => nerd_icon,
        };

        Span::styled(icon, Style::default().fg(color))
    }
}
