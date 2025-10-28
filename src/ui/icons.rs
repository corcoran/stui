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

/// Icon theme using terminal colors (respects user's terminal theme)
#[derive(Debug, Clone)]
pub struct IconTheme {
    // File type colors
    pub sync_folder_color: Color,  // Syncthing folders (left panel)
    pub folder_color: Color,        // Subdirectories
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
            // File types - use terminal colors that respect user's theme
            sync_folder_color: Color::Magenta,    // Syncthing folders
            folder_color: Color::Blue,            // Subdirectories
            file_color: Color::Cyan,              // Files

            // Status colors - use terminal colors
            synced_color: Color::Green,           // Successfully synced
            out_of_sync_color: Color::Yellow,     // Needs syncing
            local_only_color: Color::Gray,        // Only on this device
            remote_only_color: Color::White,      // Only on remote
            ignored_color: Color::Red,            // Ignored files
            syncing_color: Color::Yellow,         // Currently syncing
            scanning_color: Color::Magenta,       // Scanning for changes
            unknown_color: Color::Red,            // Unknown state
            error_color: Color::Red,              // Error state
            paused_color: Color::Gray,            // Paused folder
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
        let mut spans = vec![self.sync_folder_icon()];

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
            SyncState::Syncing => StatusType::Syncing,
            SyncState::Unknown => StatusType::Unknown,
        };

        spans.push(self.status_icon(status_type));
        spans
    }

    /// Render an ignored item (shows special indicator based on existence)
    /// Follows pattern: <file|dir><status>
    /// - exists: <file|dir>⚠️  (e.g., 📄⚠️ or 📁⚠️)
    /// - deleted: <file|dir>🚫  (e.g., 📄🚫 or 📁🚫)
    pub fn ignored_item(&self, is_dir: bool, exists: bool) -> Vec<Span<'static>> {
        let mut spans = vec![];

        // Add folder or file icon
        if is_dir {
            spans.push(self.folder_icon());
        } else {
            spans.push(self.file_icon());
        }

        // Add status icon based on existence
        if exists {
            // Ignored + exists: show warning icon
            let warning_span = match self.mode {
                IconMode::Emoji => Span::styled("⚠️ ", Style::default().fg(self.theme.out_of_sync_color)),
                IconMode::NerdFont => Span::styled("\u{F071} ", Style::default().fg(self.theme.out_of_sync_color)),
            };
            spans.push(warning_span);
        } else {
            // Ignored + deleted: show block icon
            let block_span = match self.mode {
                IconMode::Emoji => Span::styled("🚫 ", Style::default().fg(self.theme.ignored_color)),
                IconMode::NerdFont => Span::styled("\u{F05E} ", Style::default().fg(self.theme.ignored_color)),
            };
            spans.push(block_span);
        }

        spans
    }

    /// Get sync folder icon span (for Syncthing folders in left panel)
    fn sync_folder_icon(&self) -> Span<'static> {
        match self.mode {
            IconMode::Emoji => {
                Span::styled("📂", Style::default().fg(self.theme.sync_folder_color))
            }
            IconMode::NerdFont => {
                Span::styled("\u{F07C}", Style::default().fg(self.theme.sync_folder_color))
            }
        }
    }

    /// Get folder icon span (for subdirectories)
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
