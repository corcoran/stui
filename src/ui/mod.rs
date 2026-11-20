// UI module - handles all TUI rendering using Ratatui
//
// Architecture:
// - icons: Icon rendering (emoji and Nerd Fonts) with themes
// - layout: Calculates screen layout (panes, splits, areas)
// - render: Main orchestration function that coordinates all rendering
// - system_bar: Renders top system info bar (device name, uptime, transfer rates)
// - folder_list: Renders the left folder panel
// - breadcrumb: Renders breadcrumb navigation panels
// - legend: Renders hotkey legend
// - search: Renders search input box with query and match count
// - status_bar: Renders bottom status bar with metrics
// - dialogs: Renders confirmation dialogs (revert, delete, pattern selection)
// - toast: Renders toast notifications (brief pop-up messages)

pub mod breadcrumb;
pub mod dialogs;
pub mod folder_history;
pub mod folder_list;
pub mod icons;
pub mod layout;
pub mod legend;
pub mod render;
pub mod search;
pub mod status_bar;
pub mod system_bar;
pub mod toast;

// Re-export main render function for convenience
pub use render::render;

#[cfg(test)]
mod tests {
    /// U+FE0F (emoji presentation) after a narrow symbol like ⚠ or ☁ makes Konsole
    /// shift the previous row by one cell, breaking borders. Emoji icons must be
    /// ones that are wide by default.
    #[test]
    fn test_ui_text_has_no_emoji_presentation_selector() {
        let files = [
            ("dialogs.rs", include_str!("dialogs.rs")),
            ("icons.rs", include_str!("icons.rs")),
        ];
        for (name, src) in files {
            for (n, line) in src.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                assert!(
                    !code.contains('\u{FE0F}'),
                    "{name}:{} contains U+FE0F: {}",
                    n + 1,
                    line.trim()
                );
            }
        }
    }
}
