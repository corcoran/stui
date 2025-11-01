//! UI state transition logic
//!
//! Pure functions for UI state cycling and transitions.

use crate::DisplayMode;

/// Cycle to the next display mode: Off → TimestampOnly → TimestampAndSize → Off
///
/// Display modes control what information is shown for files and directories:
/// - Off: No additional info
/// - TimestampOnly: Show modification time
/// - TimestampAndSize: Show size and modification time
///
/// # Arguments
/// * `current` - The current display mode
///
/// # Returns
/// The next display mode in the cycle
///
/// # Examples
/// ```
/// use synctui::DisplayMode;
/// use synctui::logic::ui::cycle_display_mode;
///
/// assert_eq!(cycle_display_mode(DisplayMode::Off), DisplayMode::TimestampOnly);
/// assert_eq!(cycle_display_mode(DisplayMode::TimestampOnly), DisplayMode::TimestampAndSize);
/// assert_eq!(cycle_display_mode(DisplayMode::TimestampAndSize), DisplayMode::Off);
/// ```
pub fn cycle_display_mode(current: DisplayMode) -> DisplayMode {
    match current {
        DisplayMode::Off => DisplayMode::TimestampOnly,
        DisplayMode::TimestampOnly => DisplayMode::TimestampAndSize,
        DisplayMode::TimestampAndSize => DisplayMode::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_display_mode_from_off() {
        assert_eq!(cycle_display_mode(DisplayMode::Off), DisplayMode::TimestampOnly);
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_only() {
        assert_eq!(cycle_display_mode(DisplayMode::TimestampOnly), DisplayMode::TimestampAndSize);
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_and_size() {
        assert_eq!(cycle_display_mode(DisplayMode::TimestampAndSize), DisplayMode::Off);
    }
}
