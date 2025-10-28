// UI module - handles all TUI rendering using Ratatui
//
// Architecture:
// - icons: Icon rendering (emoji and Nerd Fonts) with themes
// - layout: Calculates screen layout (panes, splits, areas)
// - render: Main orchestration function that coordinates all rendering
// - folder_list: Renders the left folder panel with device status
// - breadcrumb: Renders breadcrumb navigation panels
// - status_bar: Renders bottom status bar with metrics
// - legend: Renders hotkey legend
// - dialogs: Renders confirmation dialogs (revert, delete, pattern selection)

pub mod icons;
pub mod dialogs;
pub mod legend;
pub mod status_bar;
pub mod folder_list;
pub mod breadcrumb;
pub mod layout;
pub mod render;

// Re-export main render function for convenience
pub use render::render;
