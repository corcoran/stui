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
